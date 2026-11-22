import json
import os
import sys
import unittest
from pathlib import Path
import tempfile

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))

from shared_bridge.config import Config
from shared_bridge.state import Reservation, State, StateStore


class StateTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="bridge-state-")
        self.path = Path(self.directory.name) / "state.json"

    def tearDown(self):
        self.directory.cleanup()

    def test_selected_service_survives_restart_with_private_permissions(self):
        store = StateStore(self.path)
        store.save(State("mini-brain", Reservation("res-1", "acq-1", "mini-brain", 10)))
        reloaded = StateStore(self.path).load("fallback")
        self.assertEqual(reloaded.selected_service, "mini-brain")
        self.assertEqual(reloaded.reservation.id, "res-1")
        self.assertEqual(os.stat(self.path).st_mode & 0o777, 0o600)

    def test_missing_state_uses_default_service(self):
        self.assertEqual(StateStore(self.path).load("local-primary").selected_service, "local-primary")

    def test_corrupt_state_is_quarantined_not_guessed(self):
        self.path.write_text("{not json")
        state = StateStore(self.path).load("mini-brain")
        self.assertEqual(state.selected_service, "mini-brain")
        self.assertIsNone(state.reservation)
        self.assertTrue(list(self.path.parent.glob("state.json.corrupt-*")))


class ConfigTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="bridge-config-")
        self.path = Path(self.directory.name) / "config.json"

    def tearDown(self):
        self.directory.cleanup()

    def write(self, **overrides):
        value = {
            "runtime_url": "http://127.0.0.1:8095/runtime/v1",
            "token_path": "/tmp/token",
            "state_path": "/tmp/state.json",
            "public_base_url": "https://gpurack.duckdns.org",
            "listen_port": 18098,
        }
        value.update(overrides)
        self.path.write_text(json.dumps(value))
        return Config.load(self.path)

    def test_defaults_are_conservative(self):
        config = self.write()
        self.assertEqual(config.priority, "medium")
        self.assertEqual(config.ttl_seconds, 1800)
        self.assertEqual(config.listen_host, "127.0.0.1")

    def test_non_https_public_origin_is_rejected(self):
        with self.assertRaises(ValueError):
            self.write(public_base_url="http://gpurack.duckdns.org")

    def test_public_runtime_endpoint_is_rejected(self):
        with self.assertRaises(ValueError):
            self.write(runtime_url="https://gpurack.duckdns.org/runtime/v1")

    def test_unknown_priority_is_rejected(self):
        with self.assertRaises(ValueError):
            self.write(priority="urgent")


if __name__ == "__main__":
    unittest.main()
