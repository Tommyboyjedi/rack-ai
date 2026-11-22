import sys
import threading
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from fake_rackai import reservation_view
from shared_bridge import projection
from shared_bridge.manager import SwitchInProgress, UnknownService
from support import BridgeCase


class AcquisitionTests(BridgeCase):
    def test_initial_acquisition_is_demand_driven_and_starts_pending(self):
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "preparing", rid, request["acquisition_id"]
        )
        self.assertEqual(self.fake.operations(), [])
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.PENDING)
        self.assertEqual(self.fake.operations().count("reserve"), 1)
        self.assertEqual(self.persisted()["reservation"]["id"], "res-1")

    def test_pending_becomes_ready_without_a_second_reservation(self):
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "preparing", rid, request["acquisition_id"]
        )
        self.assertEqual(self.manager.consumer_outcome()["state"], projection.PENDING)
        self.fake.states["res-1"] = ["ready"]
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.READY)
        self.assertEqual(outcome["reservation_id"], "res-1")
        self.assertEqual(self.fake.reserve_count(), 1)

    def test_polling_does_not_acquire_or_renew(self):
        self.manager.consumer_outcome()
        for _ in range(5):
            self.manager.operator_status()
        self.assertEqual(self.fake.reserve_count(), 1)
        self.assertNotIn("control", self.fake.operations())
        self.assertNotIn("renew", self.fake.operations())

    def test_two_consumers_receive_identical_details(self):
        first = self.manager.consumer_outcome()
        second = self.manager.consumer_outcome()
        self.assertEqual(first, second)
        self.assertEqual(first["chat_completions_url"], second["chat_completions_url"])
        self.assertEqual(self.fake.reserve_count(), 1)

    def test_concurrent_lookups_produce_one_acquisition(self):
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "preparing", rid, request["acquisition_id"]
        )
        barrier = threading.Barrier(8)
        results = []

        def lookup():
            barrier.wait()
            results.append(self.manager.consumer_outcome()["state"])

        threads = [threading.Thread(target=lookup) for _ in range(8)]
        for thread in threads:
            thread.start()
        for thread in threads:
            thread.join()
        self.assertEqual(results, [projection.PENDING] * 8)
        self.assertEqual(self.fake.reserve_count(), 1)

    def test_unknown_service_is_rejected_and_media_is_not_selectable(self):
        self.assertEqual([s["service"] for s in self.manager.services()], ["local-primary", "mini-brain"])
        with self.assertRaises(UnknownService):
            self.manager.select("local-image")
        with self.assertRaises(UnknownService):
            self.manager.select("missing-service")

    def test_bridge_never_calls_any_other_rackai_operation(self):
        self.manager.consumer_outcome()
        self.manager.operator_status()
        self.assertTrue(set(self.fake.operations()) <= {"discover", "reserve", "inspect_reservation"})


class InvalidationTests(BridgeCase):
    def test_expiry_triggers_reacquisition_with_a_new_identity(self):
        self.assertEqual(self.manager.consumer_outcome()["reservation_id"], "res-1")
        self.fake.states["res-1"] = ["expired"]
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.READY)
        self.assertEqual(outcome["reservation_id"], "res-2")
        acquisitions = self.fake.acquisition_ids()
        self.assertEqual(len(acquisitions), 2)
        self.assertEqual(len(set(acquisitions)), 2)

    def test_missing_reservation_is_never_reported_ready(self):
        self.manager.consumer_outcome()
        self.fake.states.pop("res-1")
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "preparing", rid, request["acquisition_id"]
        )
        self.assertEqual(self.manager.consumer_outcome()["state"], projection.PENDING)
        self.assertEqual(self.fake.reserve_count(), 2)

    def test_released_reservation_is_replaced_not_reused(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["released"]
        outcome = self.manager.consumer_outcome()
        self.assertNotEqual(outcome.get("reservation_id"), "res-1")
        self.assertEqual(self.fake.reserve_count(), 2)

    def test_preemption_reacquires_and_reports_the_rackai_conflict(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["preempting"]
        blocked = self.manager.consumer_outcome()
        self.assertEqual(blocked["state"], projection.UNAVAILABLE)
        self.assertEqual(blocked["reason"], "preempted_by_higher_priority")
        self.assertEqual(blocked["conflict"], "higher_priority")

        self.fake.states["res-1"] = ["preempted"]
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "unavailable", rid, request["acquisition_id"],
            reason="incumbent_priority:res-9", retry_after=3,
        )
        self.advance(2)
        refused = self.manager.consumer_outcome()
        self.assertEqual(refused["state"], projection.UNAVAILABLE)
        self.assertEqual(refused["reason"], "incumbent_priority:res-9")
        self.assertEqual(refused["conflict"], "higher_priority")
        self.assertEqual(refused["retry_after"], 3)


class RefusalTests(BridgeCase):
    def test_retry_guidance_prevents_a_busy_retry_loop(self):
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "unavailable", rid, request["acquisition_id"],
            reason="incumbent_priority:res-9", retry_after=7,
        )
        self.assertEqual(self.manager.consumer_outcome()["retry_after"], 7)
        self.assertEqual(self.manager.consumer_outcome()["state"], projection.UNAVAILABLE)
        self.assertEqual(self.fake.reserve_count(), 1)
        self.advance(7)
        self.manager.consumer_outcome()
        self.assertEqual(self.fake.reserve_count(), 2)

    def test_capacity_error_is_pending_and_conflict_is_unavailable(self):
        self.fake.reserve_error = (429, "capacity_api_workers")
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.PENDING)
        self.assertEqual(outcome["retry_after"], 5)

        self.fake.reserve_error = (409, "identity_conflict")
        self.manager.select("local-primary")
        self.advance(10)
        self.assertEqual(self.manager.consumer_outcome()["state"], projection.UNAVAILABLE)

    def test_rackai_authorization_failure_is_never_ready(self):
        self.fake.reserve_error = (401, "unauthorized")
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.UNAVAILABLE)
        self.assertEqual(outcome["reason"], "bridge_not_authorized")


class SwitchingTests(BridgeCase):
    def test_switch_releases_before_acquiring(self):
        self.manager.consumer_outcome()
        self.assertEqual(self.persisted()["reservation"]["id"], "res-1")
        self.fake.states["res-1"] = ["releasing", "released"]
        status = self.manager.select("local-primary")
        self.assertEqual(status["switch"]["to_service"], "local-primary")
        self.assertTrue(self.settle(lambda: self.fake.reserve_count() == 2))
        self.assertTrue(self.settle(lambda: self.persisted()["switch"] is None))

        released = [entry for entry in self.fake.sequence() if entry[0] == "release_reservation"]
        self.assertTrue(released and all(entry[1] == "res-1" for entry in released))
        order = [entry[0] for entry in self.fake.sequence()]
        replacement = order.index("reserve", order.index("release_reservation"))
        self.assertGreater(replacement, order.index("release_reservation"))
        self.assertEqual(self.persisted()["reservation"]["service"], "local-primary")

    def test_no_ready_details_while_the_old_reservation_drains(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["releasing"] * 20
        self.manager.select("local-primary")
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.PENDING)
        self.assertEqual(outcome["reason"], "service_switch_in_progress")
        self.assertEqual(self.fake.reserve_count(), 1)
        self.fake.states["res-1"] = ["released"]
        self.assertTrue(self.settle(lambda: self.fake.reserve_count() == 2))

    def test_recovery_required_blocks_replacement(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["recovery_required"]
        self.manager.select("local-primary")
        self.assertTrue(self.settle(lambda: self.persisted()["switch"]["phase"] == "recovery_required"))
        outcome = self.manager.consumer_outcome()
        self.assertEqual(outcome["state"], projection.PENDING)
        self.assertEqual(outcome["reason"], "previous_reservation_recovery_required")
        self.assertEqual(self.fake.reserve_count(), 1)
        self.fake.states["res-1"] = ["released"]
        self.assertTrue(self.settle(lambda: self.fake.reserve_count() == 2))

    def test_a_second_switch_cannot_start_while_one_is_running(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["releasing"] * 20
        self.manager.select("local-primary")
        with self.assertRaises(SwitchInProgress):
            self.manager.select("mini-brain")
        self.fake.states["res-1"] = ["released"]
        self.assertTrue(self.settle(lambda: self.fake.reserve_count() == 2))

    def test_selecting_the_ready_service_is_a_no_op(self):
        self.manager.consumer_outcome()
        status = self.manager.select("mini-brain")
        self.assertIsNone(status["switch"])
        self.assertEqual(self.fake.reserve_count(), 1)


class RestartTests(BridgeCase):
    def test_restart_reconciles_the_existing_reservation_without_duplicating(self):
        self.fake.reserve_view = lambda service, request, rid: reservation_view(
            service, "preparing", rid, request["acquisition_id"]
        )
        self.manager.consumer_outcome()
        restarted = self.new_manager()
        restarted.reconcile()
        self.assertEqual(restarted.consumer_outcome()["state"], projection.PENDING)
        self.assertEqual(self.fake.reserve_count(), 1)
        self.assertEqual(restarted.operator_status()["reservation"]["id"], "res-1")

    def test_restart_drops_a_terminal_reservation_and_reacquires_on_demand(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["expired"]
        restarted = self.new_manager()
        restarted.reconcile()
        self.assertIsNone(self.persisted().get("reservation"))
        outcome = restarted.consumer_outcome()
        self.assertEqual(outcome["reservation_id"], "res-2")
        self.assertEqual(self.fake.reserve_count(), 2)

    def test_restart_resumes_an_interrupted_switch(self):
        self.manager.consumer_outcome()
        self.fake.states["res-1"] = ["releasing"] * 20
        self.manager.select("local-primary")
        self.assertTrue(self.settle(lambda: self.persisted()["switch"]["to_service"] == "local-primary"))
        self.manager.close()
        restarted = self.new_manager()
        restarted.reconcile()
        self.fake.states["res-1"] = ["released"]
        self.assertTrue(self.settle(lambda: self.fake.reserve_count() >= 2))
        self.assertTrue(self.settle(lambda: (self.persisted().get("reservation") or {}).get("service") == "local-primary"))


if __name__ == "__main__":
    unittest.main()
