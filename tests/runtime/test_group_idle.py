"""Grouped model inactivity through the real receiver with disposable CPU backends."""
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack


class GroupIdle(unittest.TestCase):
    def test_one_active_model_retains_both_then_group_expires(self):
        with tempfile.TemporaryDirectory(prefix="rack-group-idle-") as directory:
            rack = Rack(Path(directory), configure=lambda c: c.update(idle_timeout_seconds=3))
            try:
                group = rack.call("athba", "reserve", request=dict(
                    acquisition_id="group", work_id="group", services=["local-primary", "local-coder"],
                    priority="low", ttl_seconds=60))
                for member in group["services"].values():
                    rack.wait(member)
                deadline = time.monotonic() + 6
                sequence = 0
                while time.monotonic() < deadline:
                    sequence += 1
                    rack.call("athba", "submit_work", request=dict(
                        reservation_id=group["id"], service="local-primary", work_id=f"call-{sequence}",
                        payload=dict(kind="inference", prompt="hello", max_tokens=16, timeout_seconds=5)))
                    current = rack.call("athba", "inspect_reservation", reservation_id=group["id"])
                    self.assertTrue(all(member["state"] == "ready" for member in current["services"].values()), current)
                    time.sleep(.5)
                for member in group["services"].values():
                    expired = rack.wait(member, "expired")
                    self.assertEqual(expired["reason"], "idle_timeout")
            finally:
                rack.close()
