"""Controllable RackAI double for deterministic bridge tests.

Only the five public operations the bridge may use are implemented. Every call
requires the bridge's bearer token, and the call order is recorded so that
release-before-acquire sequencing can be asserted.
"""
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import time

TOKEN = "test-bridge-token"
CAPABILITY = "cap-secret-value"


def member(service, state, reservation_id, generation="gen-1"):
    entry = {
        "id": reservation_id,
        "state": state,
        "generation": generation,
        "model": service + "-model",
        "deadline": int(time.time()) + 1800,
        "context_tokens": 131072,
        "max_input_tokens": 65536,
        "max_output_tokens": 65536,
        "profile_version": "v1",
    }
    if state == "ready":
        entry["gateway_path"] = f"/scoped/{reservation_id}/{CAPABILITY}/v1"
    if state in ("preempting", "preempted"):
        entry["reason"] = "preempted_by_higher_priority"
    return entry


def reservation_view(service, state, reservation_id, acquisition_id=None, reason=None, retry_after=None):
    view = {
        "id": reservation_id,
        "state": state,
        "priority": "medium",
        "acquisition_id": acquisition_id,
        "requested_services": [service],
        "services": {service: member(service, state, reservation_id)},
    }
    if reason is not None:
        view["reason"] = reason
        view["services"][service]["reason"] = reason
    if retry_after is not None:
        view["retry_after"] = retry_after
        view["services"][service]["retry_after"] = retry_after
    return view


DISCOVERY = {
    "tags": [
        {"tag": "mini-brain", "version": "v1", "qualified": True,
         "capabilities": ["reasoning", "coding"], "context_tokens": 131072,
         "max_input_tokens": 65536, "max_output_tokens": 65536},
        {"tag": "local-primary", "version": "v1", "qualified": True,
         "capabilities": ["reasoning", "coding"], "context_tokens": 65536,
         "max_input_tokens": 65536, "max_output_tokens": 65536},
        {"tag": "local-image", "version": "v1", "qualified": True,
         "capabilities": ["visual"], "context_tokens": 0,
         "max_input_tokens": 0, "max_output_tokens": 0},
        {"tag": "unqualified", "version": "v1", "qualified": False,
         "capabilities": ["reasoning"], "context_tokens": 1,
         "max_input_tokens": 1, "max_output_tokens": 1},
    ],
    "priorities": ["low", "medium", "high", "paramount"],
}


class FakeRackAI:
    def __init__(self):
        self.lock = threading.Lock()
        self.calls = []
        self.counter = 0
        self.states = {}
        self.services = {}
        self.release_calls = []
        self.reserve_error = None
        self.reserve_view = None
        self.discover_error = None
        server = self

        class Handler(BaseHTTPRequestHandler):
            protocol_version = "HTTP/1.1"

            def log_message(self, *_args):
                pass

            def do_POST(self):
                length = int(self.headers.get("Content-Length", "0"))
                body = json.loads(self.rfile.read(length))
                status, payload = server.handle(body, self.headers.get("Authorization"))
                encoded = json.dumps(payload).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(encoded)))
                self.end_headers()
                self.wfile.write(encoded)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}/runtime/v1"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    def operations(self):
        with self.lock:
            return [call[1] for call in self.calls]

    def sequence(self):
        with self.lock:
            return [(call[1], call[2].get("reservation_id") or
                     (call[2].get("request") or {}).get("services")) for call in self.calls]

    def acquisition_ids(self):
        with self.lock:
            return [call[2]["request"]["acquisition_id"] for call in self.calls if call[1] == "reserve"]

    def reserve_count(self):
        return len(self.acquisition_ids())

    def handle(self, body, authorization):
        operation = body.get("operation")
        if authorization != "Bearer " + TOKEN:
            return 401, {"schema": "rack-ai/runtime/v1", "error": "unauthorized"}
        with self.lock:
            self.calls.append((len(self.calls), operation, body))
        if operation == "discover":
            if self.discover_error:
                return self.discover_error, {"error": "discover_failed"}
            return 200, {"schema": "rack-ai/runtime/v1", "result": DISCOVERY}
        if operation == "reserve":
            return self._reserve(body["request"])
        if operation == "inspect_reservation":
            return self._inspect(body["reservation_id"])
        if operation == "release_reservation":
            return self._release(body["reservation_id"])
        return 400, {"error": "unknown_operation"}

    def _reserve(self, request):
        service = request["services"][0]
        if self.reserve_error:
            status, error = self.reserve_error
            return status, {"error": error}
        with self.lock:
            self.counter += 1
            reservation_id = f"res-{self.counter}"
            self.services[reservation_id] = service
        view = None
        if self.reserve_view is not None:
            view = self.reserve_view(service, request, reservation_id)
        if view is None:
            view = reservation_view(service, "ready", reservation_id, request["acquisition_id"])
        with self.lock:
            self.states.setdefault(reservation_id, [view["state"]])
        return 200, {"schema": "rack-ai/runtime/v1", "result": view}

    def _inspect(self, reservation_id):
        with self.lock:
            service = self.services.get(reservation_id, "mini-brain")
            states = self.states.get(reservation_id)
            if not states:
                return 404, {"error": "not_found"}
            state = states[0] if len(states) == 1 else states.pop(0)
        return 200, {
            "schema": "rack-ai/runtime/v1",
            "result": reservation_view(service, state, reservation_id),
        }

    def _release(self, reservation_id):
        with self.lock:
            self.release_calls.append(reservation_id)
            service = self.services.get(reservation_id, "mini-brain")
        return 200, {
            "schema": "rack-ai/runtime/v1",
            "result": reservation_view(service, "releasing", reservation_id),
        }
