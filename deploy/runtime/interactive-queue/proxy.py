#!/usr/bin/env python3
"""Hermes → Shared-Bridge V2 inference adapter.

Loopback OpenAI-compatible surface for Hermes.  Every request resolves the
current reservation from the Shared Bridge (GET /reservation), then forwards
directly to the bridge-supplied chat_completions_url.

The bridge owns service selection, reservation lifetime, expiry and
preemption.  This adapter never selects a service, never reserves, releases or
renews, and never contacts NInfer directly.

Retry policy: a 409 reservation-invalidation is retried once after re-resolving
the reservation, with the same idempotency key.  Other 4xx/5xx responses are
returned unchanged.
"""

import argparse
from collections import OrderedDict
from threading import Lock
import re
import hashlib
import http.client
import json
import logging
import os
import sys
import time
import urllib.error
import urllib.request
from urllib.parse import urlsplit
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

MAX_BODY_BYTES = 16 * 1024 * 1024
DEFAULT_BRIDGE_URL = "http://127.0.0.1:18098/reservation"
DEFAULT_MODEL = "qwen3.6-35b-a3b"
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

LOG = logging.getLogger("hermes-rackai-bridge")


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
                raise BridgeUnavailable(
                    "bridge_ready_incomplete:" + ",".join(missing)
                )
            return payload
        if state == "pending":
            delay = min(15, max(1, int(payload.get("retry_after") or 2)))
            if time.monotonic() + delay >= deadline:
                reason = payload.get("reason") or "pending"
                raise BridgeUnavailable(
                    "bridge_pending_timeout:" + str(reason)
                )
            sleep(delay)
            continue
        reason = payload.get("reason") or state or "unavailable"
        raise BridgeUnavailable("bridge_" + str(reason))


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def forward(url, body, headers, timeout, stream_handler=None):
    """Request the selected RackAI gateway and return status, content type, body."""
    request = urllib.request.Request(url, data=body, headers=headers)
    handlers = [NoRedirect()]
    if urlsplit(url).hostname == "127.0.0.1":
        handlers.append(urllib.request.ProxyHandler({}))
    opener = urllib.request.build_opener(*handlers)
    try:
        with opener.open(request, timeout=timeout) as response:
            content_type = response.headers.get("Content-Type", "application/json")
            if stream_handler and content_type.split(";", 1)[0].strip() == "text/event-stream":
                stream_handler(response)
                return response.status, content_type, None
            return response.status, content_type, response.read()
    except urllib.error.HTTPError as error:
        with error:
            return error.code, error.headers.get("Content-Type", "application/json"), error.read()


def _invalidation(status, body):
    if status != 409:
        return None
    try:
        payload = json.loads(body)
    except ValueError:
        return None
    reason = payload.get("error") if isinstance(payload, dict) else None
    return reason if reason in INVALIDATION_REASONS else None


# Only opaque turn IDs and fixed state names are retained in memory.
_progress = OrderedDict()
_progress_lock = Lock()
MAX_PROGRESS_RECORDS = 256

def report(trace, state):
    if not trace:
        return
    with _progress_lock:
        _progress[trace] = state
        _progress.move_to_end(trace)
        while len(_progress) > MAX_PROGRESS_RECORDS:
            _progress.popitem(last=False)

def forward_interactive(url, body, headers, timeout, trace, stream_handler):
    path = urlsplit(url).path
    if not re.fullmatch(r"/scoped/[A-Za-z0-9_-]+/[A-Za-z0-9_-]+/v1/(chat/completions|responses)", path):
        return 502, "application/json", b'{"error":"rackai_invalid_progress_url"}'
    # The public ingress only accepts completion POSTs. Use the existing
    # authenticated SSH tunnel for both admission and status.
    url = "http://127.0.0.1:8095" + path
    root = url.rsplit("/v1/", 1)[0]
    deadline = time.monotonic() + timeout
    result = forward(url, body, headers, timeout, stream_handler)
    while result[0] == 202:
        progress = json.loads(result[2])
        state, invocation = progress.get("state"), progress.get("invocation_id")
        if state not in ("queued", "running") or not isinstance(invocation, str) or not re.fullmatch(r"[a-f0-9]{32}", invocation):
            return 502, "application/json", b'{"error":"rackai_invalid_progress"}'
        report(trace, state)
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return 504, "application/json", b'{"error":"rackai_progress_wait_timeout"}'
        time.sleep(min(1, remaining))
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return 504, "application/json", b'{"error":"rackai_progress_wait_timeout"}'
        result = forward(root + "/v1/invocations/" + invocation, None, {}, remaining, stream_handler)
    return result


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"
    bridge_url = DEFAULT_BRIDGE_URL
    max_wait = 90
    timeout = 600
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

    def _stream(self, response):
        self.send_response(response.status)
        self.send_header("Content-Type", response.headers.get("Content-Type"))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Accel-Buffering", "no")
        self.send_header("Connection", "close")
        self.end_headers()
        self.close_connection = True
        try:
            while True:
                chunk = response.read1(65536)
                if not chunk:
                    break
                self.wfile.write(chunk)
                self.wfile.flush()
        except (OSError, http.client.HTTPException):
            LOG.warning("Streaming connection closed before completion")

    def do_GET(self):
        path = self.path.split("?", 1)[0]
        trace = path.removeprefix("/queue-status/")
        if path.startswith("/queue-status/") and re.fullmatch(r"[0-9a-f]{32}", trace):
            with _progress_lock:
                state = _progress.get(trace)
            return self._json(200, {"state": state})
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
            "request id=%s bytes=%d keys=%s tools=%d stream=%s",
            request_id,
            len(raw),
            ",".join(sorted(payload)),
            len(payload.get("tools") or []),
            payload.get("stream"),
        )
        if self.dump_request:
            try:
                os.makedirs(self.dump_request, exist_ok=True)
                dump_path = os.path.join(
                    self.dump_request, "hermes-request-" + request_id + ".json"
                )
                descriptor = os.open(
                    dump_path, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600
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

        requested_trace = self.headers.get("x-rackai-interactive-turn", "")
        queue_key = os.environ.get("RACKAI_INTERACTIVE_QUEUE_KEY")
        trace = requested_trace if queue_key and re.fullmatch(r"[0-9a-f]{32}", requested_trace) else None
        report(trace, "submitted")
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
        }
        if trace:
            headers["idempotency-key"] = trace + "-" + key
            headers["rackai-interactive-queue-key"] = queue_key
            headers["Prefer"] = "respond-async"

        def send_upstream():
            handler = self._stream if payload.get("stream") else None
            if trace:
                return forward_interactive(url, body, headers, self.timeout, trace, handler)
            return forward(url, body, headers, self.timeout, handler)

        LOG.info(
            "request id=%s service=%s reservation=%s generation=%s endpoint=%s",
            request_id,
            reservation.get("service"),
            reservation.get("reservation_id"),
            reservation.get("generation"),
            path,
        )
        try:
            status, content_type, response_body = send_upstream()
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
            headers["idempotency-key"] = trace + "-" + key if trace else key
            try:
                status, content_type, response_body = send_upstream()
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

        report(trace, "finished")
        if response_body is not None:
            self._send(status, content_type, response_body)

    @staticmethod
    def _encode(payload, reservation):
        """Rewrite the request body for the reservation's model and endpoint."""
        payload = dict(payload)
        # NInfer rejects response_format ("response format not supported").
        # Drop it rather than failing the whole derivation.
        payload.pop("response_format", None)
        payload["model"] = reservation["model"]
        body = json.dumps(payload, separators=(",", ":")).encode("utf-8")
        key = "hermes-bridge-" + hashlib.sha256(body).hexdigest()[:32]
        return body, key


def build_server(host, port, bridge_url, max_wait, timeout, dump_request=None):
    handler = type(
        "HermesBridgeHandler",
        (Handler,),
        {
            "bridge_url": bridge_url,
            "max_wait": max_wait,
            "timeout": timeout,
            "dump_request": dump_request,
        },
    )
    return ThreadingHTTPServer((host, port), handler)


def main(argv=None):
    parser = argparse.ArgumentParser(
        description=(
            "Hermes bridge adapter — resolves Shared Bridge reservations and "
            "forwards inference requests."
        )
    )
    parser.add_argument(
        "--listen",
        default="127.0.0.1:18102",
        help="Address to listen on (default: 127.0.0.1:18102)",
    )
    parser.add_argument(
        "--bridge-url",
        default=DEFAULT_BRIDGE_URL,
        help="Shared Bridge reservation URL "
        "(default: http://127.0.0.1:18098/reservation)",
    )
    parser.add_argument(
        "--max-wait",
        type=int,
        default=90,
        help="Max seconds to wait for bridge readiness (default: 90)",
    )
    parser.add_argument(
        "--timeout",
        type=int,
        default=600,
        help="Forward request timeout in seconds (default: 600)",
    )
    parser.add_argument(
        "--log-level",
        default="INFO",
        help="Logging level (default: INFO)",
    )
    parser.add_argument(
        "--dump-request",
        default=None,
        help="Write received request bodies (mode 0600) to this directory",
    )
    arguments = parser.parse_args(argv)
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
