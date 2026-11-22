import json
import os
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from shared_bridge import projection


def view(state, service="mini-brain", reason=None, retry_after=None, gateway=True):
    entry = {
        "id": "res-1",
        "state": state,
        "generation": "gen-1",
        "model": "model",
        "deadline": 2000,
        "context_tokens": 100,
        "max_input_tokens": 80,
        "max_output_tokens": 20,
        "profile_version": "v1",
    }
    if gateway and state == "ready":
        entry["gateway_path"] = "/scoped/res-1/cap/v1"
    if reason:
        entry["reason"] = reason
    if retry_after:
        entry["retry_after"] = retry_after
    return {"id": "res-1", "state": state, "services": {service: entry}}


class ProjectionTests(unittest.TestCase):
    def test_ready_exposes_direct_rackai_urls_and_never_the_owner_token(self):
        kind, payload = projection.classify(view("ready"), "mini-brain", "https://gpurack.test")
        self.assertEqual(kind, projection.READY)
        self.assertEqual(payload["reservation_id"], "res-1")
        self.assertEqual(payload["service"], "mini-brain")
        self.assertEqual(
            payload["chat_completions_url"],
            "https://gpurack.test/scoped/res-1/cap/v1/chat/completions",
        )
        self.assertTrue(payload["idempotency_key_required"])
        self.assertNotIn("authorization", json.dumps(payload).lower().replace("authentication", ""))

    def test_preparing_is_pending_and_preempting_is_unavailable(self):
        self.assertEqual(projection.classify(view("preparing"), "mini-brain", "https://x")[0], projection.PENDING)
        kind, payload = projection.classify(
            view("preempting", reason="preempted_by_higher_priority"), "mini-brain", "https://x"
        )
        self.assertEqual(kind, projection.UNAVAILABLE)
        self.assertEqual(payload["reason"], "preempted_by_higher_priority")
        self.assertEqual(payload["conflict"], "higher_priority")

    def test_refusal_reason_and_retry_guidance_are_passed_through(self):
        kind, payload = projection.classify(
            view("unavailable", reason="incumbent_priority:res-9", retry_after=7),
            "mini-brain",
            "https://x",
        )
        self.assertEqual(kind, projection.UNAVAILABLE)
        self.assertEqual(payload["reason"], "incumbent_priority:res-9")
        self.assertEqual(payload["conflict"], "higher_priority")
        self.assertEqual(payload["retry_after"], 7)

    def test_non_priority_refusal_is_not_labelled_as_priority(self):
        _kind, payload = projection.classify(
            view("unavailable", reason="insufficient_host_memory"), "mini-brain", "https://x"
        )
        self.assertIsNone(payload["conflict"])
        self.assertEqual(payload["reason"], "insufficient_host_memory")

    def test_terminal_states_are_not_ready(self):
        for state in ("released", "cancelled", "expired", "preempted"):
            kind, _payload = projection.classify(view(state), "mini-brain", "https://x")
            self.assertEqual(kind, projection.TERMINAL, state)

    def test_ready_without_capability_is_not_published_as_ready(self):
        kind, _payload = projection.classify(view("ready", gateway=False), "mini-brain", "https://x")
        self.assertNotEqual(kind, projection.READY)

    def test_discovery_filters_to_qualified_inference_services(self):
        discovery = {
            "tags": [
                {"tag": "b", "qualified": True, "capabilities": ["coding"]},
                {"tag": "a", "qualified": True, "capabilities": ["reasoning"]},
                {"tag": "media", "qualified": True, "capabilities": ["visual"]},
                {"tag": "draft", "qualified": False, "capabilities": ["reasoning"]},
            ]
        }
        self.assertEqual([s["service"] for s in projection.compatible_services(discovery)], ["a", "b"])


if __name__ == "__main__":
    unittest.main()
