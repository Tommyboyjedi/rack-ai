"""Loopback HTTP surface for the operator UI and consumer reservation lookups."""
import json
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import urlsplit

from .manager import SwitchInProgress, UnknownService
from .rack import RackAIError
from .ui import PAGE

MAX_BODY_BYTES = 4096


def build_server(config, manager):
    handler = type("BoundBridgeHandler", (BridgeHandler,), {"manager": manager})
    return ThreadingHTTPServer((config.listen_host, config.listen_port), handler)


class BridgeHandler(BaseHTTPRequestHandler):
    manager = None
    server_version = "shared-bridge-v2"
    protocol_version = "HTTP/1.1"

    def log_message(self, *_args):
        """Never log request paths or bodies; nginx logging is disabled too."""

    def do_GET(self):
        path = urlsplit(self.path).path
        if path in ("/", "/index.html"):
            return self._bytes(200, "text/html; charset=utf-8", PAGE.encode("utf-8"))
        if path == "/healthz":
            return self._json(200, {"status": "ok"})
        if path == "/status":
            return self._guarded(lambda: self._json(200, self.manager.operator_status()))
        if path == "/services":
            return self._guarded(
                lambda: self._json(200, {"services": self.manager.services()})
            )
        if path == "/reservation":
            return self._guarded(lambda: self._json(200, self.manager.consumer_outcome()))
        return self._json(404, {"error": "not_found"})

    def do_POST(self):
        path = urlsplit(self.path).path
        if path not in ("/select", "/release"):
            return self._json(404, {"error": "not_found"})
        try:
            body = json.loads(self._body())
            status = self.manager.release() if path == "/release" else self.manager.select(body.get("service"))
        except UnknownService:
            return self._json(400, {"error": "unknown_service"})
        except SwitchInProgress:
            return self._json(409, {"error": "switch_in_progress"})
        except (ValueError, TypeError, json.JSONDecodeError):
            return self._json(400, {"error": "invalid_request"})
        except RackAIError as error:
            return self._unavailable(error)
        return self._json(200, status)

    def _guarded(self, action):
        try:
            return action()
        except RackAIError as error:
            return self._unavailable(error)

    def _unavailable(self, error):
        self._json(
            503,
            {
                "state": "pending",
                "reason": "rackai_temporarily_unavailable",
                "retry_after": 2,
                "detail": error.reason,
            },
        )

    def _body(self):
        length = int(self.headers.get("Content-Length", "0"))
        if length <= 0 or length > MAX_BODY_BYTES:
            raise ValueError("invalid request size")
        return self.rfile.read(length)

    def _json(self, status, value):
        payload = json.dumps(value).encode("utf-8")
        self._bytes(status, "application/json", payload)

    def _bytes(self, status, content_type, payload):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(payload)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()
        self.wfile.write(payload)
