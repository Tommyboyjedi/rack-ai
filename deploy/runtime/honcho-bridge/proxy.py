#!/usr/bin/env python3
"""Honcho -> Shared-Bridge V2 inference adapter.

Loopback OpenAI-compatible surface for Honcho.  Every request resolves the
current reservation from the existing Shared Bridge (GET /reservation), then
forwards directly to the bridge-supplied chat_completions_url / responses_url.

The bridge owns service selection, reservation lifetime, expiry and
preemption.  This adapter never selects a service, never reserves, releases or
renews, and never contacts NInfer directly.

Retry policy: a 409 reservation-invalidation or a transport failure is retried
exactly once after re-resolving the reservation, with the same idempotency key.
Other 4xx/5xx responses are returned unchanged.
"""

import argparse
import hashlib
import json
import logging
import os
import sys
import time
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_BODY_BYTES = 16 * 1024 * 1024
DEFAULT_BRIDGE_URL = "http://127.0.0.1:18098/reservation"
DEFAULT_MODEL = "qwen3.6-35b-a3b"
DEFAULT_EXECUTION_TIMEOUT_SECONDS = 120
EXECUTION_TIMEOUT_HEADER = "RackAI-Execution-Timeout-Seconds"

INVALIDATION_REASONS = frozenset(
    {
        "reservation_not_dispatchable",
        "reservation_preempting",
        "reservation_preempted",
        "stale_generation_or_profile",
        "invalid_scoped_capability",
        "reservation_terminal",
    }
)

LOG = logging.getLogger("honcho-rackai-bridge")


def _function_choice(name):
    return {"type": "function", "function": {"name": name}}


def _normalize_tool_choice(payload):
    """Translate Honcho's tool-choice forms into NInfer's accepted form.

    NInfer rejects ``tool_choice="required"`` when more than one tool is
    offered, and rejects bare function names such as ``"search_memory"``.
    Honcho uses both forms (the workspace dialectic forces ``"required"``;
    the low reasoning level names ``"search_memory"``).  A named function
    choice preserves the intended forced recall while remaining valid
    OpenAI/NInfer syntax.
    """
    choice = payload.get("tool_choice")
    tools = payload.get("tools") or []
    names = []
    for tool in tools:
        if not isinstance(tool, dict):
            continue
        function = tool.get("function")
        if isinstance(function, dict) and isinstance(function.get("name"), str):
            names.append(function["name"])
    if choice in ("required", "any"):
        if not names:
            return payload
        name = "search_memory" if "search_memory" in names else names[0]
        payload["tool_choice"] = _function_choice(name)
    elif isinstance(choice, str) and choice not in ("auto", "none") and choice in names:
        payload["tool_choice"] = _function_choice(choice)
    return payload


class BridgeUnavailable(RuntimeError):
    """The bridge cannot currently provide a usable reservation."""


def resolve_reservation(bridge_url, max_wait_seconds, sleep=time.sleep):
    """Poll the bridge until ready; return the ready payload."""
    deadline = time.monotonic() + max_wait_seconds
    while True:
        request = urllib.request.Request(
            bridge_url, headers={"accept": "application/json"}
        )
        try:
            with urllib.request.urlopen(request, timeout=15) as response:
                payload = json.load(response)
        except Exception as error:  # bridge unreachable / malformed
            raise BridgeUnavailable("bridge_unreachable") from error

        state = payload.get("state")
        if state == "ready":
            missing = [
                key
                for key in ("service", "model", "chat_completions_url")
                if not payload.get(key)
            ]
            if missing:
                raise BridgeUnavailable("bridge_ready_incomplete:" + ",".join(missing))
            return payload
        if state == "pending":
            delay = min(15, max(1, int(payload.get("retry_after") or 2)))
            if time.monotonic() + delay >= deadline:
                reason = payload.get("reason") or "pending"
                raise BridgeUnavailable("bridge_pending_timeout:" + str(reason))
            sleep(delay)
            continue
        reason = payload.get("reason") or state or "unavailable"
        raise BridgeUnavailable("bridge_" + str(reason))


def forward(url, body, headers, timeout):
    """POST to the RackAI gateway and return (status, content_type, body)."""
    request = urllib.request.Request(url, data=body, method="POST", headers=headers)
    try:
        with urllib.request.urlopen(request, timeout=timeout) as response:
            return (
                response.status,
                response.headers.get("Content-Type", "application/json"),
                response.read(),
            )
    except urllib.error.HTTPError as error:
        return (
            error.code,
            error.headers.get("Content-Type", "application/json"),
            error.read(),
        )


def _invalidation(status, body):
    if status != 409:
        return None
    try:
        payload = json.loads(body)
    except ValueError:
        return None
    reason = payload.get("error") if isinstance(payload, dict) else None
    return reason if reason in INVALIDATION_REASONS else None


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    bridge_url = DEFAULT_BRIDGE_URL
    max_wait = 90
    timeout = 600
    execution_timeout = DEFAULT_EXECUTION_TIMEOUT_SECONDS
    static_model = DEFAULT_MODEL
    dump_request = None

    def log_message(self, fmt, *args):
        # Keep message contents and capability URLs out of logs.
        return

    def _json(self, status, payload):
        body = json.dumps(payload).encode("utf-8")
        self._send(status, "application/json", body)

    def _send(self, status, content_type, body):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        if path in ("/health", "/healthz"):
            return self._json(200, {"status": "ok", "backend": "shared-bridge"})
        if path in ("/v1/models", "/models"):
            return self._json(
                200,
                {
                    "object": "list",
                    "data": [
                        {
                            "id": self.static_model,
                            "object": "model",
                            "owned_by": "rackai",
                        }
                    ],
                },
            )
        return self._json(404, {"error": "not_found"})

    def do_POST(self):
        path = self.path.split("?", 1)[0]
        if path in ("/v1/chat/completions", "/chat/completions"):
            endpoint = "chat_completions_url"
        elif path in ("/v1/responses", "/responses"):
            endpoint = "responses_url"
        else:
            return self._json(404, {"error": "not_found"})

        try:
            length = int(self.headers.get("Content-Length", "0"))
        except ValueError:
            length = 0
        if length <= 0 or length > MAX_BODY_BYTES:
            return self._json(400, {"error": "invalid_request_size"})
        raw = self.rfile.read(length)
        try:
            payload = json.loads(raw)
        except ValueError:
            return self._json(400, {"error": "invalid_json"})
        if not isinstance(payload, dict):
            return self._json(400, {"error": "invalid_json"})

        request_id = hashlib.sha256(raw).hexdigest()[:12]
        # Non-content diagnostics: never log message text or capability URLs.
        LOG.info(
            "request id=%s bytes=%d keys=%s tools=%d",
            request_id,
            len(raw),
            ",".join(sorted(payload)),
            len(payload.get("tools") or []),
        )
        if self.dump_request:
            try:
                os.makedirs(self.dump_request, exist_ok=True)
                path = os.path.join(
                    self.dump_request, "honcho-request-" + request_id + ".json"
                )
                descriptor = os.open(
                    path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600
                )
                with os.fdopen(descriptor, "wb") as handle:
                    handle.write(raw)
            except OSError:
                LOG.warning("request id=%s body dump failed", request_id)
        try:
            reservation = resolve_reservation(self.bridge_url, self.max_wait)
        except BridgeUnavailable as error:
            LOG.warning(
                "request id=%s bridge=%s", request_id, str(error)[:120]
            )
            return self._json(
                503,
                {
                    "error": "rackai_backend_unavailable",
                    "message": str(error),
                },
            )

        body, key = self._encode(payload, reservation)
        url = reservation.get(endpoint)
        if not url:
            return self._json(
                503,
                {
                    "error": "rackai_backend_unavailable",
                    "message": "bridge_ready_missing_" + endpoint,
                },
            )
        headers = {
            "Content-Type": "application/json",
            "Accept": self.headers.get("Accept", "application/json"),
            "idempotency-key": key,
            EXECUTION_TIMEOUT_HEADER: str(self.execution_timeout),
        }
        LOG.info(
            "request id=%s service=%s reservation=%s generation=%s endpoint=%s",
            request_id,
            reservation.get("service"),
            reservation.get("reservation_id"),
            reservation.get("generation"),
            path,
        )
        try:
            status, content_type, response_body = forward(
                url, body, headers, self.timeout
            )
        except Exception:
            status, content_type, response_body = 502, "application/json", (
                b'{"error":"rackai_transport_failed"}'
            )

        reason = _invalidation(status, response_body)
        if reason:
            LOG.warning(
                "request id=%s invalidated reason=%s; re-resolving",
                request_id,
                reason,
            )
            try:
                reservation = resolve_reservation(self.bridge_url, self.max_wait)
            except BridgeUnavailable as error:
                return self._json(
                    503,
                    {
                        "error": "rackai_backend_unavailable",
                        "message": str(error),
                    },
                )
            body, key = self._encode(payload, reservation)
            url = reservation.get(endpoint)
            if not url:
                return self._json(503, {"error": "rackai_backend_unavailable"})
            headers["idempotency-key"] = key
            try:
                status, content_type, response_body = forward(
                    url, body, headers, self.timeout
                )
            except Exception:
                status, content_type, response_body = 502, "application/json", (
                    b'{"error":"rackai_transport_failed"}'
                )
            LOG.info(
                "request id=%s retry service=%s reservation=%s generation=%s status=%s",
                request_id,
                reservation.get("service"),
                reservation.get("reservation_id"),
                reservation.get("generation"),
                status,
            )

        self._send(status, content_type, response_body)

    @staticmethod
    def _encode(payload, reservation):
        payload = _normalize_tool_choice(dict(payload))
        # NInfer rejects response_format ("response format not supported").
        # Honcho's prompts already carry the required JSON shape, so drop the
        # provider-side constraint rather than failing the whole derivation.
        payload.pop("response_format", None)
        payload["model"] = reservation["model"]
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        key = "honcho-bridge-" + hashlib.sha256(body).hexdigest()[:32]
        return body, key


def build_server(
    host, port, bridge_url, max_wait, timeout, dump_request=None, *,
    execution_timeout=DEFAULT_EXECUTION_TIMEOUT_SECONDS,
):
    if type(execution_timeout) is not int or execution_timeout <= 0:
        raise ValueError("execution timeout must be a positive integer")
    handler = type(
        "HonchoBridgeHandler",
        (Handler,),
        {
            "bridge_url": bridge_url,
            "max_wait": max_wait,
            "timeout": timeout,
            "execution_timeout": execution_timeout,
            "dump_request": dump_request,
        },
    )
    return ThreadingHTTPServer((host, port), handler)


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--listen", default="127.0.0.1:18099")
    parser.add_argument("--bridge-url", default=DEFAULT_BRIDGE_URL)
    parser.add_argument("--max-wait", type=int, default=90)
    parser.add_argument("--timeout", type=int, default=600,
                        help="HTTP response wait, including RackAI queue time")
    parser.add_argument("--execution-timeout", type=int, default=DEFAULT_EXECUTION_TIMEOUT_SECONDS,
                        help="RackAI generation budget in seconds after dispatch")
    parser.add_argument("--log-level", default="INFO")
    parser.add_argument(
        "--dump-request",
        default=None,
        help="write received request bodies (mode 0600) to this directory",
    )
    arguments = parser.parse_args(argv)
    if arguments.execution_timeout <= 0:
        parser.error("--execution-timeout must be positive")
    logging.basicConfig(
        level=getattr(logging, arguments.log_level.upper(), logging.INFO),
        format="%(asctime)s %(levelname)s %(message)s",
    )
    host, port = arguments.listen.rsplit(":", 1)
    server = build_server(
        host,
        int(port),
        arguments.bridge_url,
        arguments.max_wait,
        arguments.timeout,
        arguments.dump_request,
        execution_timeout=arguments.execution_timeout,
    )
    LOG.info("listening on %s bridge=%s", arguments.listen, arguments.bridge_url)
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    sys.exit(main())
