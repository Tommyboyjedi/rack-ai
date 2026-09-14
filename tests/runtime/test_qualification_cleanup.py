"""Qualification shutdown keeps unknown output while proving owned cleanup."""
import json
import tempfile
import time
import unittest
from support import Rack

class QualificationCleanup(unittest.TestCase):
    def exercise(self, restart):
        with tempfile.TemporaryDirectory() as directory:
            rack = Rack(directory)
            try:
                demand = rack.wait(rack.acquire('other', 'big-brain', 'medium'))
                rack.controls('big-brain', uncertain=True)
                invocation = rack.result(rack.infer(demand), 'uncertain')
                if restart:
                    rack.process.terminate()
                    rack.process.wait(timeout=5)
                    rack.log.close()
                    rack.start()
                before = time.monotonic()
                rack.release(demand)
                self.assertIsNotNone(rack.inspect(demand)['process'])
                terminal = rack.wait(demand, 'released')
                self.assertGreater(time.monotonic() - before, 1.5)
                self.assertIsNone(terminal['process'])
                actual = rack.call('other', 'reconcile', invocation_id=invocation['id'])
                self.assertEqual(actual, invocation)
                replay = rack.call('other', 'infer', request=invocation['request'])
                self.assertEqual(replay, invocation)
                self.assertEqual(rack.counts('dispatch')['big-brain'], 1)
                self.assertEqual(rack.counts('stop')['big-brain'], 1)
                state = json.loads((rack.root / 'authority/managed.json').read_text())
                self.assertEqual(state['claims'], {})
            finally:
                rack.close()

    def test_cancel_bounds_started_drain_and_preserves_uncertain_output(self):
        with tempfile.TemporaryDirectory() as directory:
            def configure(config):
                for profile in config['profiles']:
                    profile['drain_seconds'] = 1
            rack = Rack(directory, configure)
            try:
                demand = rack.wait(rack.acquire('other', 'big-brain', 'medium'))
                rack.controls('big-brain', delay=4)
                invocation = rack.infer(demand)
                deadline = time.monotonic() + 2
                while rack.counts('dispatch')['big-brain'] == 0 and time.monotonic() < deadline:
                    time.sleep(.01)
                self.assertEqual(rack.counts('dispatch')['big-brain'], 1)
                before = time.monotonic()
                rack.release(demand, 'cancel')
                terminal = rack.wait(demand, 'cancelled')
                self.assertLess(time.monotonic() - before, 3)
                result = rack.result(invocation, 'uncertain')
                self.assertIsNotNone(result['cancellation'])
                self.assertIsNone(result['result'])
                self.assertIsNone(terminal['process'])
                self.assertEqual(rack.counts('dispatch')['big-brain'], 1)
            finally:
                rack.close()

    def test_cleanup_persistence_failure_retains_claims_until_recovery(self):
        with tempfile.TemporaryDirectory() as directory:
            rack = Rack(directory)
            authority = rack.root/'authority'
            try:
                demand = rack.wait(rack.acquire('other', 'big-brain', 'medium'))
                rack.controls('big-brain', uncertain=True)
                invocation = rack.result(rack.infer(demand), 'uncertain')
                rack.release(demand)
                authority.chmod(0o500)
                time.sleep(3.5)
                state = json.loads((authority/'managed.json').read_text())
                self.assertTrue(state['claims'])
                self.assertEqual(state['data']['demands'][demand['id']]['state'], 'releasing')
                self.assertEqual(rack.counts('stop')['big-brain'], 1)
                authority.chmod(0o700)
                rack.wait(demand, 'released')
                self.assertEqual(rack.call('other', 'reconcile', invocation_id=invocation['id']), invocation)
            finally:
                authority.chmod(0o700)
                rack.close()

    def test_release_stops_uncertain_backend_after_drain_without_replay(self):
        self.exercise(False)

    def test_uncertain_release_survives_receiver_restart(self):
        self.exercise(True)

if __name__ == '__main__':
    unittest.main(verbosity=2)
