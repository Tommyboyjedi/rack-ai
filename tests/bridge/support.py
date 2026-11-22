"""Shared fixtures for the isolated Shared-Bridge V2 test suite."""
import json
import os
from pathlib import Path
import tempfile
import unittest

from fake_rackai import FakeRackAI, TOKEN

from shared_bridge import manager as manager_module
from shared_bridge.config import Config
from shared_bridge.rack import RackAI
from shared_bridge.switching import SwitchRunner


class BridgeCase(unittest.TestCase):
    """One disposable bridge against one controllable RackAI double."""

    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="shared-bridge-")
        self.root = Path(self.directory.name)
        self.token_path = self.root / "token"
        self.token_path.write_text(TOKEN + "\n")
        os.chmod(self.token_path, 0o600)
        self.fake = FakeRackAI()
        self.config = Config(
            runtime_url=self.fake.url,
            token_path=self.token_path,
            state_path=self.root / "state.json",
            public_base_url="https://gpurack.example.test",
            listen_host="127.0.0.1",
            listen_port=18098,
        )
        self.clock_value = 1_000
        self.manager = self.new_manager()

    def tearDown(self):
        for manager in getattr(self, "_managers", []):
            manager.close()
        self.fake.close()
        self.directory.cleanup()

    def new_manager(self):
        core = manager_module.Core(
            self.config, rack=RackAI(self.config), clock=lambda: self.clock_value
        )
        manager = manager_module.ReservationManager(
            self.config, core=core, switcher_factory=self.switcher
        )
        self._managers = getattr(self, "_managers", []) + [manager]
        return manager

    def switcher(self, core, acquire):
        return SwitchRunner(core, acquire, sleep=lambda _seconds: None)

    def advance(self, seconds):
        self.clock_value += seconds

    def settle(self, predicate, timeout=5.0):
        import time

        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if predicate():
                return True
            time.sleep(0.01)
        return False

    def persisted(self):
        return json.loads(self.config.state_path.read_text())
