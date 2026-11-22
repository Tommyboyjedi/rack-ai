#!/usr/bin/env python3
"""Compatibility tests for the Honcho -> Shared-Bridge adapter."""

import json
import socket
import threading
import unittest
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import proxy


def free_port():
    sock = socket.socket()
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]
    sock.close()
    return port


class MockBridge:
    def __init__(self):
        self.sequence = []
        self.calls = 0
        self.server = None

    def pop(self):
        self.calls += 1
        if not self.sequence:
            return self.sequence[-1]
        item = self.sequence.pop(0)
        if len(self.sequence) == 0:
            self.sequence.append(item)
        return item


class MockUpstream:
    def __init__(self, responses):
        self.responses = list(responses)
        self.requests = []

    def pop(self):
        if not self.responses:
            return 200, "application/json", b'{"ok":true}'
        item = self.responses.pop(0)
        if len(self.responses) == 0:
            self.responses.append(item)
        return item


def start_mock(handler_factory):
    port = free_port()
    server = ThreadingHTTPServer(("127.0.0.1", port), handler_factory())
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, port, thread


def bridge_handler(bridge):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            return

        def do_GET(self):
            payload = bridge.pop()
            body = json.dumps(payload).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

    return Handler


def upstream_handler(upstream):
    class Handler(BaseHTTPRequestHandler):
        protocol_version = "HTTP/1.1"

        def log_message(self, *args):
            return

        def do_POST(self):
            length = int(self.headers.get("Content-Length", "0"))
            body = self.rfile.read(length)
            upstream.requests.append((body, dict(self.headers)))
            status, content_type, response = upstream.pop()
            self.send_response(status)
            self.send_header("Content-Type", content_type)
            self.send_header("Content-Length", str(len(response)))
            self.end_headers()
            self.wfile.write(response)

    return Handler


def ready_payload(upstream_port, reservation="res-1", generation="gen-1"):
    return {
        "state": "ready",
        "service": "maxi-brain",
        "reservation_id": reservation,
        "generation": generation,
        "model": "qwen3.6-35b-a3b",
        "chat_completions_url": f"http://127.0.0.1:{upstream_port}/v1/chat/completions",
        "responses_url": f"http://127.0.0.1:{upstream_port}/v1/responses",
        "expires_at": 9999999999,
        "limits": {"context_tokens": 131072},
    }


class AdapterTest(unittest.TestCase):
    def setUp(self):
        self.bridge = MockBridge()
        self.upstream = MockUpstream([])
        self.bridge_server, self.bridge_port, _ = start_mock(
            lambda: bridge_handler(self.bridge)
        )
        self.upstream_server, self.upstream_port, _ = start_mock(
            lambda: upstream_handler(self.upstream)
        )
        self.proxy_port = free_port()
        self.proxy = proxy.build_server(
            "127.0.0.1",
            self.proxy_port,
            f"http://127.0.0.1:{self.bridge_port}/reservation",
            max_wait=10,
            timeout=30,
        )
        threading.Thread(target=self.proxy.serve_forever, daemon=True).start()

    def tearDown(self):
        self.proxy.shutdown()
        self.proxy.server_close()
        self.bridge_server.shutdown()
        self.bridge_server.server_close()
        self.upstream_server.shutdown()
        self.upstream_server.server_close()

    def post(self, body, path="/v1/chat/completions"):
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.proxy_port}{path}",
            data=json.dumps(body).encode(),
            headers={"Content-Type": "application/json"},
            method="POST",
        )
        try:
            with urllib.request.urlopen(request, timeout=30) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as error:
            return error.code, json.loads(error.read())

    def test_ready_forwards_model_and_idempotency(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        self.upstream.responses = [
            (200, "application/json", b'{"choices":[{"message":{"content":"ok"}}]}')
        ]
        status, payload = self.post({"model": "gpt-5.4-mini", "messages": []})
        self.assertEqual(status, 200)
        self.assertEqual(payload["choices"][0]["message"]["content"], "ok")
        body, headers = self.upstream.requests[0]
        decoded = json.loads(body)
        self.assertEqual(decoded["model"], "qwen3.6-35b-a3b")
        key = next(
            (value for name, value in headers.items() if name.lower() == "idempotency-key"),
            None,
        )
        self.assertTrue(key and key.startswith("honcho-bridge-"))
        self.assertEqual(
            {name.lower(): value for name, value in headers.items()}["rackai-execution-timeout-seconds"],
            "120",
        )

    def test_execution_timeout_override_on_responses(self):
        self.proxy.RequestHandlerClass.execution_timeout = 45
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        self.upstream.responses = [(200, "application/json", b'{"output":[]}')]
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.proxy_port}/v1/responses",
            data=json.dumps({"model": "rackai-service", "input": "test"}).encode(),
            headers={"Content-Type": "application/json", "RackAI-Execution-Timeout-Seconds": "600"},
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            self.assertEqual(response.status, 200)
        headers = {k.lower(): v for k, v in self.upstream.requests[0][1].items()}
        self.assertEqual(headers["rackai-execution-timeout-seconds"], "45")

    def test_pending_then_ready(self):
        self.bridge.sequence = [
            {
                "state": "pending",
                "service": "maxi-brain",
                "reason": "preparing",
                "retry_after": 1,
            },
            ready_payload(self.upstream_port),
        ]
        status, _ = self.post({"model": "x", "messages": []})
        self.assertEqual(status, 200)
        self.assertEqual(self.bridge.calls, 2)

    def test_unavailable_returns_503(self):
        self.bridge.sequence = [
            {
                "state": "unavailable",
                "service": "maxi-brain",
                "reason": "reservation_preempted",
                "retry_after": 2,
            }
        ]
        status, payload = self.post({"model": "x", "messages": []})
        self.assertEqual(status, 503)
        self.assertEqual(payload["error"], "rackai_backend_unavailable")

    def test_invalidation_retries_on_new_endpoint(self):
        other_upstream = MockUpstream(
            [(200, "application/json", b'{"retried":true}')]
        )
        other_server, other_port, _ = start_mock(
            lambda: upstream_handler(other_upstream)
        )
        self.addCleanup(other_server.shutdown)
        self.addCleanup(other_server.server_close)
        self.bridge.sequence = [
            ready_payload(self.upstream_port),
            ready_payload(other_port, reservation="res-2", generation="gen-2"),
        ]
        self.upstream.responses = [
            (409, "application/json", b'{"error":"reservation_preempted"}')
        ]
        status, payload = self.post({"model": "x", "messages": []})
        self.assertEqual(status, 200)
        self.assertEqual(payload["retried"], True)
        self.assertEqual(len(other_upstream.requests), 1)
        for upstream in (self.upstream, other_upstream):
            headers = {k.lower(): v for k, v in upstream.requests[0][1].items()}
            self.assertEqual(headers["rackai-execution-timeout-seconds"], "120")

    def test_429_is_not_retried(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        self.upstream.responses = [
            (429, "application/json", b'{"error":"capacity"}')
        ]
        status, payload = self.post({"model": "x", "messages": []})
        self.assertEqual(status, 429)
        self.assertEqual(payload["error"], "capacity")
        self.assertEqual(self.bridge.calls, 1)

    def test_required_tool_choice_becomes_named_function(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        tools = [
            {"type": "function", "function": {"name": "search_memory", "parameters": {}}},
            {"type": "function", "function": {"name": "search_messages", "parameters": {}}},
        ]
        status, _ = self.post(
            {"model": "x", "messages": [], "tools": tools, "tool_choice": "required"}
        )
        self.assertEqual(status, 200)
        body, _ = self.upstream.requests[0]
        decoded = json.loads(body)
        self.assertEqual(
            decoded["tool_choice"],
            {"type": "function", "function": {"name": "search_memory"}},
        )

    def test_named_tool_choice_becomes_function_choice(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        tools = [
            {"type": "function", "function": {"name": "search_memory", "parameters": {}}},
        ]
        status, _ = self.post(
            {"model": "x", "messages": [], "tools": tools, "tool_choice": "search_memory"}
        )
        self.assertEqual(status, 200)
        body, _ = self.upstream.requests[0]
        decoded = json.loads(body)
        self.assertEqual(
            decoded["tool_choice"],
            {"type": "function", "function": {"name": "search_memory"}},
        )

    def test_auto_tool_choice_is_unchanged(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        tools = [
            {"type": "function", "function": {"name": "search_memory", "parameters": {}}},
        ]
        status, _ = self.post(
            {"model": "x", "messages": [], "tools": tools, "tool_choice": "auto"}
        )
        self.assertEqual(status, 200)
        body, _ = self.upstream.requests[0]
        decoded = json.loads(body)
        self.assertEqual(decoded["tool_choice"], "auto")

    def test_response_format_is_dropped(self):
        self.bridge.sequence = [ready_payload(self.upstream_port)]
        status, _ = self.post(
            {
                "model": "x",
                "messages": [],
                "response_format": {"type": "json_object"},
            }
        )
        self.assertEqual(status, 200)
        body, _ = self.upstream.requests[0]
        decoded = json.loads(body)
        self.assertNotIn("response_format", decoded)

    def test_models_endpoint_is_static(self):
        request = urllib.request.Request(
            f"http://127.0.0.1:{self.proxy_port}/v1/models"
        )
        with urllib.request.urlopen(request, timeout=10) as response:
            payload = json.load(response)
        self.assertEqual(payload["data"][0]["id"], "qwen3.6-35b-a3b")
        self.assertEqual(self.bridge.calls, 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
