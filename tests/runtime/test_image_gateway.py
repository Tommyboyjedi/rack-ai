"""Image-capable local-primary gateway regressions."""
import base64
import json
import struct
import tempfile
import unittest
import urllib.error
import urllib.request

from support import Rack


PNG_1X1 = base64.b64decode(
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+/p9sAAAAASUVORK5CYII="
)


def bmp_1x1(size=128):
    if size < 58:
        raise ValueError("BMP fixture must leave room for header and one pixel")
    header = bytearray()
    header.extend(b"BM")
    header.extend(struct.pack("<I", size))
    header.extend(b"\0\0\0\0")
    header.extend(struct.pack("<I", 54))
    header.extend(struct.pack("<I", 40))
    header.extend(struct.pack("<i", 1))
    header.extend(struct.pack("<i", 1))
    header.extend(struct.pack("<H", 1))
    header.extend(struct.pack("<H", 24))
    header.extend(struct.pack("<I", 0))
    header.extend(struct.pack("<I", size - 54))
    header.extend(struct.pack("<i", 2835))
    header.extend(struct.pack("<i", 2835))
    header.extend(struct.pack("<I", 0))
    header.extend(struct.pack("<I", 0))
    header.extend(b"\0" * (size - len(header)))
    return bytes(header)


def data_url(mime, data):
    return f"data:{mime};base64," + base64.b64encode(data).decode()


PNG_URL = data_url("image/png", PNG_1X1)
BMP_URL = data_url("image/bmp", bmp_1x1(128))
BMP_TOO_LARGE_URL = data_url("image/bmp", bmp_1x1(129))
PNG_AS_JPEG_URL = data_url("image/jpeg", PNG_1X1)


def visual_primary(config):
    for profile in config["profiles"]:
        if profile["tag"] == "local-primary":
            profile["capabilities"] = ["reasoning", "visual"]
            profile["protocols"] = ["chat_completions", "responses"]
            profile["max_images_per_request"] = 2
            profile["max_image_bytes"] = 128
            profile["max_image_pixels"] = 4096


class ImageGatewayRegressions(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="rack-image-gateway-")
        self.rack = Rack(self.directory.name, configure=visual_primary)

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def gateway(self, demand, body, key=None, path="/chat/completions"):
        headers = {"Content-Type": "application/json"}
        if key:
            headers["Idempotency-Key"] = key
        request = urllib.request.Request(
            f"http://{self.rack.address}" + demand["gateway_path"] + path,
            data=json.dumps(body).encode(),
            headers=headers,
        )
        try:
            response = urllib.request.urlopen(request, timeout=15)
        except urllib.error.HTTPError as error:
            response = error
        with response:
            return response.status, json.loads(response.read())

    def body(self, content, max_tokens=16):
        return {
            "model": "local-primary",
            "messages": [{"role": "user", "content": content}],
            "max_tokens": max_tokens,
        }

    def image_part(self, url=PNG_URL):
        return {"type": "image_url", "image_url": {"url": url}}

    def canonical_invocation(self, rack, submission_id):
        authority = json.loads((rack.root / "authority" / "managed.json").read_text())
        for item in authority["data"].get("invocations", {}).values():
            if item["request"]["submission_id"] == submission_id:
                return item
        for path in (rack.root / "authority" / "archive" / "owners").glob("*/invocations/*.json"):
            archived = json.loads(path.read_text())
            if archived.get("submission_id") == submission_id:
                return archived["record"]
        raise AssertionError(f"canonical invocation record not found: {submission_id}")

    def test_gateway_accepts_bounded_images_and_records_protocol_invocation(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        status, value = self.gateway(
            demand,
            self.body([
                {"type": "text", "text": "Name the marker."},
                self.image_part(),
            ]),
            "image-one",
        )
        self.assertEqual(status, 200, value)
        text_status, text_value = self.gateway(demand, self.body("text only"), "text-one")
        self.assertEqual(text_status, 200, text_value)

        invocation = self.canonical_invocation(r, "image-one")
        request = invocation["request"]
        if "payload" not in request and invocation.get("request_ref"):
            request = json.loads((r.root / "authority" / invocation["request_ref"]["path"]).read_text())
        self.assertEqual(request["reservation_id"], demand["id"])
        self.assertEqual(request["payload"]["protocol"], "chat_completions")
        sent = json.loads((r.events.with_suffix(r.events.suffix + ".requests")).read_text().splitlines()[-2])
        self.assertEqual(sent["messages"][0]["content"][1]["image_url"]["url"], PNG_URL)
        self.assertEqual(r.counts("dispatch")["local-primary"], 2)

    def test_near_limit_image_is_accepted_and_over_limit_rejected(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        status, value = self.gateway(
            demand,
            self.body([{"type": "text", "text": "limit"}, self.image_part(BMP_URL)]),
            "exact-byte-limit",
        )
        self.assertEqual(status, 200, value)

        status, value = self.gateway(
            demand,
            self.body([{"type": "text", "text": "limit"}, self.image_part(BMP_TOO_LARGE_URL)]),
            "over-byte-limit",
        )
        self.assertEqual(status, 409, value)
        self.assertEqual(value["error"], "image_bytes_exceeded")
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)

    def test_max_image_count_is_enforced(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        status, value = self.gateway(
            demand,
            self.body([
                {"type": "text", "text": "two"},
                self.image_part(),
                self.image_part(BMP_URL),
            ]),
            "two-images",
        )
        self.assertEqual(status, 200, value)

        status, value = self.gateway(
            demand,
            self.body([
                {"type": "text", "text": "three"},
                self.image_part(),
                self.image_part(BMP_URL),
                self.image_part(),
            ]),
            "three-images",
        )
        self.assertEqual(status, 409, value)
        self.assertEqual(value["error"], "image_count_exceeded")
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)

    def test_invalid_images_are_rejected_before_dispatch(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        cases = [
            ("remote", "https://example.test/image.png", "image_url_unqualified"),
            ("mime", data_url("text/plain", b"abc"), "unsupported_image_type"),
            ("malformed", "data:image/png;base64,%%%", "invalid_image_payload"),
            ("corrupt", data_url("image/png", b"not-a-png"), "invalid_image_payload"),
            ("mismatch", PNG_AS_JPEG_URL, "image_format_mismatch"),
            ("bytes", BMP_TOO_LARGE_URL, "image_bytes_exceeded"),
        ]
        for key, url, error in cases:
            status, value = self.gateway(
                demand,
                self.body([
                    {"type": "text", "text": "blocked"},
                    self.image_part(url),
                ]),
                key,
            )
            self.assertEqual(status, 409, value)
            self.assertEqual(value["error"], error)
        self.assertEqual(r.counts("dispatch")["local-primary"], 0)

    def test_responses_image_input_is_not_accepted(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        body = {
            "model": "local-primary",
            "input": [{"role": "user", "content": [
                {"type": "input_text", "text": "blocked"},
                {"type": "input_image", "image_url": PNG_URL},
            ]}],
            "max_output_tokens": 16,
        }
        status, value = self.gateway(demand, body, "responses-image", path="/responses")
        self.assertEqual(status, 409, value)
        self.assertEqual(value["error"], "image_input_unqualified")
        self.assertEqual(r.counts("dispatch")["local-primary"], 0)

    def test_image_idempotency_scope_preserves_canonical_invocation(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        payload = self.body([{"type": "text", "text": "same"}, self.image_part()])
        status, value = self.gateway(demand, payload, "image-stable")
        self.assertEqual(status, 200, value)
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)

        status, retry = self.gateway(demand, payload, "image-stable")
        self.assertEqual(status, 200, retry)
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)

        changed = self.body([{"type": "text", "text": "same"}, self.image_part(BMP_URL)])
        status, conflict = self.gateway(demand, changed, "image-stable")
        self.assertEqual(status, 409, conflict)
        self.assertEqual(conflict["error"], "identity_conflict")
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)

        status, value = self.gateway(demand, payload, "image-distinct")
        self.assertEqual(status, 200, value)
        self.assertEqual(r.counts("dispatch")["local-primary"], 2)

    def test_stale_released_scoped_gateway_rejects_image_submission(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        r.release(demand)
        released = r.wait(demand, "released")
        status, value = self.gateway(
            released,
            self.body([
                {"type": "text", "text": "blocked"},
                self.image_part(),
            ]),
            "after-release",
        )
        self.assertEqual(status, 409, value)
        self.assertEqual(value["error"], "reservation_not_dispatchable")
        self.assertEqual(r.counts("dispatch")["local-primary"], 0)

    def test_preempted_scoped_gateway_rejects_image_submission(self):
        r = self.rack
        demand = r.wait(r.acquire("athba", "local-primary", "low"))
        incumbent = r.wait(r.acquire("cb", "local-fun-chat", "paramount"))
        stale = r.wait(demand, "preempted")
        status, value = self.gateway(
            stale,
            self.body([
                {"type": "text", "text": "blocked"},
                self.image_part(),
            ]),
            "after-preempt",
        )
        self.assertEqual(status, 409, value)
        self.assertEqual(value["error"], "reservation_preempted")
        self.assertEqual(r.counts("dispatch")["local-primary"], 0)
        r.release(incumbent)


class TextOnlyProfileImageRejection(unittest.TestCase):
    def test_image_payload_requires_visual_profile(self):
        with tempfile.TemporaryDirectory(prefix="rack-image-unqualified-") as root:
            rack = Rack(root)
            try:
                demand = rack.wait(rack.acquire("athba", "local-primary", "low"))
                body = {"model": "local-primary", "messages": [{"role": "user", "content": [
                    {"type": "text", "text": "blocked"},
                    {"type": "image_url", "image_url": {"url": PNG_URL}},
                ]}], "max_tokens": 16}
                request = urllib.request.Request(
                    f"http://{rack.address}" + demand["gateway_path"] + "/chat/completions",
                    data=json.dumps(body).encode(),
                    headers={"Content-Type": "application/json", "Idempotency-Key": "no-visual"},
                )
                try:
                    response = urllib.request.urlopen(request, timeout=15)
                except urllib.error.HTTPError as error:
                    response = error
                with response:
                    value = json.loads(response.read())
                self.assertEqual(response.status, 409, value)
                self.assertEqual(value["error"], "image_input_unqualified")
                self.assertEqual(rack.counts("dispatch")["local-primary"], 0)
            finally:
                rack.close()


if __name__ == "__main__":
    unittest.main()
