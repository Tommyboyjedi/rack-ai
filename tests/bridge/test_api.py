import json
import socket
import sys
import threading
import unittest
from pathlib import Path
from urllib.error import HTTPError
from urllib.request import Request, urlopen

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from shared_bridge.api import build_server
from support import BridgeCase


def free_port():
    probe = socket.socket()
    probe.bind(("127.0.0.1", 0))
    port = probe.getsockname()[1]
    probe.close()
    return port


class HttpSurfaceTests(BridgeCase):
    def setUp(self):
        super().setUp()
        self.config = type(self.config)(
            **{**self.config.__dict__, "listen_port": free_port()}
        )
        self.server = build_server(self.config, self.manager)
        threading.Thread(target=self.server.serve_forever, daemon=True).start()
        self.base = f"http://127.0.0.1:{self.config.listen_port}"

    def tearDown(self):
        self.server.shutdown()
        self.server.server_close()
        super().tearDown()

    def get(self, path, status=200):
        try:
            with urlopen(self.base + path, timeout=5) as response:
                return response.status, response.read(), dict(response.headers)
        except HTTPError as error:
            body = error.read()
            error.close()
            self.assertEqual(error.code, status)
            return error.code, body, {}

    def post(self, path, payload, status=200):
        request = Request(
            self.base + path,
            data=json.dumps(payload).encode(),
            headers={"Content-Type": "application/json"},
        )
        try:
            with urlopen(request, timeout=5) as response:
                return response.status, json.load(response)
        except HTTPError as error:
            body = json.loads(error.read() or b"{}")
            error.close()
            self.assertEqual(error.code, status)
            return error.code, body

    def test_consumer_lookup_needs_no_bridge_credentials(self):
        status, body, headers = self.get("/reservation")
        self.assertEqual(status, 200)
        payload = json.loads(body)
        self.assertEqual(payload["state"], "ready")
        self.assertEqual(payload["service"], "mini-brain")
        self.assertEqual(headers["Cache-Control"], "no-store")
        self.assertTrue(payload["chat_completions_url"].endswith("/chat/completions"))
        self.assertNotIn("test-bridge-token", body.decode())

    def test_bridge_has_no_inference_proxy(self):
        status, _ = self.post("/chat/completions", {"messages": []}, status=404)
        self.assertEqual(status, 404)

    def test_ui_and_status_are_served(self):
        status, body, _ = self.get("/")
        self.assertEqual(status, 200)
        self.assertIn(b"Shared Service", body)
        status, body, _ = self.get("/status")
        self.assertEqual(json.loads(body)["selected_service"], "mini-brain")
        self.assertEqual([s["service"] for s in json.loads(body)["services"]], ["local-primary", "mini-brain"])

    def test_service_selection_rejects_unknown_services(self):
        status, payload = self.post("/select", {"service": "not-a-service"}, status=400)
        self.assertEqual(payload["error"], "unknown_service")

    def test_service_selection_is_accepted_for_published_services(self):
        status, payload = self.post("/select", {"service": "local-primary"})
        self.assertEqual(status, 200)
        self.assertEqual(payload["selected_service"], "local-primary")

    def test_health_is_minimal(self):
        status, body, _ = self.get("/healthz")
        self.assertEqual((status, json.loads(body)), (200, {"status": "ok"}))


if __name__ == "__main__":
    unittest.main()
