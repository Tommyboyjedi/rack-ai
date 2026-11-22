"""The deployed ingress must expose inference only, never RackAI management routes."""
import re
import unittest
from pathlib import Path

CONFIG = Path(__file__).resolve().parents[2] / "config/media/https/nginx.conf"


def text():
    return CONFIG.read_text(encoding="utf-8")


def block(name):
    source = text()
    start = source.index(name)
    depth = 0
    for index in range(start, len(source)):
        if source[index] == "{":
            depth += 1
        elif source[index] == "}":
            depth -= 1
            if depth == 0:
                return source[start:index + 1]
    raise AssertionError("unbalanced block " + name)


class IngressTests(unittest.TestCase):
    def test_scoped_inference_route_is_narrow_and_unlogged(self):
        scoped = block("location ~ ^/scoped/")
        self.assertIn("(chat/completions|responses)", scoped)
        self.assertIn("access_log off;", scoped)
        self.assertIn("limit_except POST", scoped)
        self.assertIn("client_max_body_size 1m;", scoped)
        self.assertIn("proxy_pass http://127.0.0.1:8095$request_uri;", scoped)

    def test_privileged_runtime_api_is_not_routed(self):
        source = text()
        self.assertNotIn("runtime/v1", source)
        for suffix in ("/voices", "/scopes/", "preflight"):
            self.assertNotIn("location " + suffix, source)

    def test_no_http_authentication_is_configured(self):
        source = text()
        self.assertNotIn("auth_basic", source)
        self.assertNotIn("htpasswd", source)
        reservation = block("location = /bridge/reservation")
        self.assertIn("limit_except GET", reservation)
        ui = block("location /bridge/")
        self.assertNotIn("auth_basic", ui)
        self.assertIn("client_max_body_size 16k;", ui)

    def test_legacy_control_proxy_is_gone(self):
        source = text()
        self.assertNotIn("/bridge/control/", source)
        self.assertNotIn("18099", source)
        self.assertIn("127.0.0.1:18098", source)

    def test_capability_is_never_logged_or_put_in_a_query(self):
        source = text()
        self.assertIn("access_log off;", source)
        self.assertIsNone(re.search(r"proxy_pass[^;]*\?.*scoped", source))


if __name__ == "__main__":
    unittest.main()
