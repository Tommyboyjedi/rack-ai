"""Deterministic PR38 ownership/preemption proof against disposable fixtures."""
import json
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack, VERSION


class ReservationOwnershipV2(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="rack-pr38-")
        self.rack = Rack(Path(self.directory.name))

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def wait_reservation(self, owner, reservation, state, seconds=12):
        deadline = time.monotonic() + seconds
        current = reservation
        while time.monotonic() < deadline:
            current = self.rack.call(owner, "inspect_reservation", reservation_id=reservation["id"])
            if current["state"] == state:
                return current
            if current["state"] == "recovery_required":
                self.fail(current)
            time.sleep(.03)
        self.fail(current)

    def wait_dispatch(self, tag):
        deadline = time.monotonic() + 3
        while self.rack.counts("dispatch")[tag] < 1 and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertEqual(self.rack.counts("dispatch")[tag], 1)

    def inference(self, reservation, identity):
        return dict(
            schema=VERSION, submission_id=identity, reservation_id=reservation["id"],
            generation=reservation["generation"], profile_hash=reservation["profile_hash"],
            prompt=identity, max_tokens=16, timeout_seconds=5,
        )

    def test_running_drains_queued_calls_cancel_and_no_auto_reacquire(self):
        r = self.rack
        low = r.wait(r.acquire("athba", "local-primary", "low", identity="low-owner"))
        r.controls("local-primary", delay=1)
        running = r.infer(low, identity="running")
        self.wait_dispatch("local-primary")
        queued = [r.infer(low, identity=f"queued-{index}") for index in range(9)]

        high = r.acquire("cb", "local-fun-chat", "paramount", identity="paramount-owner")
        self.assertEqual(r.inspect(low)["state"], "preempting")
        blocked = r.call("athba", "infer", status=409, request=self.inference(low, "new-after-preempt"))
        self.assertEqual(blocked["error"], "reservation_preempting")
        for invocation in queued:
            actual = r.result(invocation, "cancelled")
            self.assertEqual(actual["error"], "reservation_superseded_by_higher_priority")
            self.assertIsNone(actual["started"])

        # The competing claim must not become Ready until the original backend
        # call has completed normally.
        self.assertEqual(r.result(running)["state"], "completed")
        high = r.wait(high)
        low_after = r.wait(low, "preempted")
        self.assertEqual(r.counts("dispatch")["local-primary"], 1)
        r.release(high)
        r.wait(high, "released")
        time.sleep(.15)
        self.assertEqual(r.inspect(low_after)["state"], "preempted")

        r.controls("local-primary", delay=0)
        reacquired = r.wait(r.acquire("athba", "local-primary", "low", identity="new-explicit-decision"))
        self.assertNotEqual(reacquired["id"], low["id"])
        r.release(reacquired)

    def test_restart_fences_preempting_uncertainty_and_never_replays_cancelled_work(self):
        r = self.rack
        low = r.wait(r.acquire("athba", "local-primary", "low", identity="restart-low"))
        r.controls("local-primary", delay=2)
        running = r.infer(low, identity="restart-running")
        self.wait_dispatch("local-primary")
        queued = [r.infer(low, identity=f"restart-queued-{index}") for index in range(3)]
        high = r.acquire("cb", "local-fun-chat", "paramount", identity="restart-high")
        self.assertEqual(r.inspect(low)["state"], "preempting")
        r.process.kill()
        r.process.wait()
        r.log.close()
        r.start()
        for invocation in queued:
            receipt = r.result(invocation, "cancelled")
            self.assertEqual(receipt["error"], "reservation_superseded_by_higher_priority")
            self.assertIsNone(receipt["started"])
        # Restart makes an in-flight backend outcome uncertain. The preemption
        # transfer therefore fences rather than guessing or replaying anything.
        self.assertEqual(r.result(running, "uncertain")["state"], "uncertain")
        self.assertEqual(r.wait(high, "recovery_required")["state"], "recovery_required")
        self.assertNotEqual(r.counts("dispatch")["local-fun-chat"], 1)

    def test_atomic_set_has_no_partial_ready_leakage(self):
        r = self.rack
        blocker = r.wait(r.acquire("cb", "local-coder", "paramount", identity="coder-blocker"))
        request = dict(acquisition_id="atomic-denied", work_id="bundle", services=["local-primary", "local-coder"], priority="low", ttl_seconds=60)
        denied = r.call("athba", "reserve", request=request)
        self.assertEqual(denied["state"], "unavailable")
        self.assertEqual({view["state"] for view in denied["services"].values()}, {"unavailable"})
        self.assertEqual(r.counts("start")["local-primary"], 0)
        r.release(blocker)
        r.wait(blocker, "released")
        # The claim environment changed, so the bounded unavailable decision
        # cache permits this explicit retry to make a fresh placement decision.
        ready = r.call("athba", "reserve", request=request)
        ready = self.wait_reservation("athba", ready, "ready")
        self.assertEqual({view["state"] for view in ready["services"].values()}, {"ready"})
        r.call("athba", "release_reservation", reservation_id=ready["id"])
        self.wait_reservation("athba", ready, "released")

    def test_partial_preemption_preserves_unaffected_logical_service(self):
        r = self.rack
        request = dict(acquisition_id="two-services", work_id="bundle", services=["local-primary", "local-coder"], priority="low", ttl_seconds=60)
        reservation = self.wait_reservation("athba", r.call("athba", "reserve", request=request), "ready")
        high = r.wait(r.acquire("cb", "local-fun-chat", "paramount", identity="only-primary"))
        displaced = self.wait_reservation("athba", reservation, "partial")
        self.assertEqual(displaced["services"]["local-primary"]["state"], "preempted")
        self.assertEqual(displaced["services"]["local-coder"]["state"], "ready")
        r.release(high)
        r.wait(high, "released")
        self.assertEqual(r.call("athba", "inspect_reservation", reservation_id=reservation["id"])["services"]["local-primary"]["state"], "preempted")
        r.call("athba", "release_reservation", reservation_id=reservation["id"])
        self.wait_reservation("athba", reservation, "released")

    def test_unavailable_decision_cache_does_not_retain_retry_demands(self):
        r = self.rack
        incumbent = r.wait(r.acquire("cb", "local-fun-chat", "paramount", identity="incumbent"))
        managed = r.root / "authority" / "managed.json"
        before = len(json.loads(managed.read_text())["data"]["demands"])
        one = r.acquire("athba", "local-primary", "low", identity="poll-one")
        two = r.acquire("athba", "local-primary", "low", identity="poll-two")
        self.assertEqual((one["state"], two["state"]), ("unavailable", "unavailable"))
        self.assertEqual(one["reason"], two["reason"])
        self.assertGreaterEqual(one["retry_after"], 1)
        self.assertEqual(len(json.loads(managed.read_text())["data"]["demands"]), before)
        r.release(incumbent)
        r.wait(incumbent, "released")
        fresh = r.wait(r.acquire("athba", "local-primary", "low", identity="poll-after-release"))
        r.release(fresh)

    def test_terminal_payloads_compact_without_rejecting_ready_calls(self):
        directory = self.directory
        self.rack.close()
        def configure(config):
            config["limits"] = dict(max_response_bytes=16384, retention_admission_bytes=512 * 1024, terminal_evidence_bytes=64 * 1024)
        self.rack = Rack(Path(directory.name), configure=configure)
        r = self.rack
        reservation = r.wait(r.acquire("athba", "local-primary", "low", identity="retention"))
        r.controls("local-primary", content="x" * 12000)
        for index in range(36):
            invocation = r.infer(reservation, identity=f"retained-{index}")
            self.assertEqual(r.result(invocation)["state"], "completed")
        document = json.loads((r.root / "authority" / "managed.json").read_text())
        results = document["data"]["invocations"].values()
        self.assertTrue(any(item.get("result_digest") for item in results))
        self.assertFalse(any(item.get("error") in {"capacity_active_control", "capacity_active_payload"} for item in results))


if __name__ == "__main__":
    unittest.main(verbosity=2)
