"""Historical archive retirement and owner-scoped replay behavior."""
import json
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack, VERSION

class HistoryArchiveTests(unittest.TestCase):
    def managed(self, rack):
        return json.loads((rack.root/'authority'/'managed.json').read_text())

    def archive_files(self, rack):
        root = rack.root/'authority'/'archive'
        return sorted(path for path in root.rglob('*.json')) if root.exists() else []

    def wait_archived(self, rack, invocation_id):
        deadline = time.monotonic() + 5
        active = self.managed(rack)
        while time.monotonic() < deadline:
            active = self.managed(rack)
            if invocation_id not in active['data']['invocations'] and self.archive_files(rack):
                return active
            time.sleep(.05)
        self.fail(f'invocation {invocation_id} was not retired from active state: {active}')

    def wait_active_empty(self, rack):
        deadline = time.monotonic() + 5
        active = self.managed(rack)
        while time.monotonic() < deadline:
            active = self.managed(rack)
            if active['data']['demands'] == {} and active['data']['invocations'] == {}:
                return active
            time.sleep(.05)
        self.fail(f'active state did not retire after release: {active}')

    def test_completed_call_retires_during_live_reservation_and_replays_from_archive(self):
        with tempfile.TemporaryDirectory(prefix='rack-history-live-reservation-') as root:
            r = Rack(root)
            try:
                d = r.wait(r.acquire('athba', 'local-primary', 'low'))
                invocation = r.infer(d, identity='historical-live-call')
                terminal = r.result(invocation)
                self.assertEqual(terminal['state'], 'completed')

                self.wait_archived(r, invocation['id'])

                before = r.counts('dispatch')['local-primary']
                replay = r.infer(d, identity='historical-live-call')
                after = r.counts('dispatch')['local-primary']
                self.assertEqual(replay['id'], invocation['id'])
                self.assertEqual(before, after)

                changed = dict(schema=VERSION, submission_id='historical-live-call',
                    reservation_id=d['id'], generation=d['generation'],
                    profile_hash=d['profile_hash'], prompt='changed',
                    max_tokens=16, timeout_seconds=5)
                conflict = r.call('athba', 'infer', status=409, request=changed)
                self.assertEqual(conflict['error'], 'identity_conflict')
                self.assertEqual(r.call('other', 'result', status=404,
                    invocation_id=invocation['id'])['error'], 'not_found')

                r.release(d)
                released = r.wait(d, 'released')
                self.assertEqual(released['state'], 'released')
                self.wait_active_empty(r)
                self.assertEqual(r.result(invocation)['id'], invocation['id'])
            finally:
                r.close()

    def test_repeated_closed_history_returns_active_state_to_bounded_baseline(self):
        sizes = []
        with tempfile.TemporaryDirectory(prefix='rack-history-bounded-') as root:
            r = Rack(root)
            try:
                for index, owner in enumerate(['athba', 'cb', 'other']):
                    d = r.wait(r.acquire(owner, 'local-primary', 'low',
                        identity=f'history-cycle-{index}'))
                    invocation = r.infer(d, identity=f'cycle-call-{index}')
                    self.assertEqual(r.result(invocation)['state'], 'completed')
                    r.release(d)
                    self.assertEqual(r.wait(d, 'released')['state'], 'released')
                    managed_path = r.root/'authority'/'managed.json'
                    active = self.wait_active_empty(r)
                    self.assertEqual(active['claims'], {})
                    sizes.append(managed_path.stat().st_size)
                self.assertLess(max(sizes) - min(sizes), 2048)
            finally:
                r.close()

    def test_expired_archive_is_non_executing_and_swept_without_refresh(self):
        with tempfile.TemporaryDirectory(prefix='rack-history-expiry-') as root:
            r = Rack(root)
            try:
                d = r.wait(r.acquire('athba', 'local-primary', 'low'))
                invocation = r.infer(d, identity='expires')
                self.assertEqual(r.result(invocation)['state'], 'completed')
                r.release(d)
                self.assertEqual(r.wait(d, 'released')['state'], 'released')
                self.wait_active_empty(r)
                files = self.archive_files(r)
                self.assertTrue(files)
                for path in files:
                    data = json.loads(path.read_text())
                    if 'expires_at' in data:
                        data['closed_at'] = 1
                        data['expires_at'] = 1
                        path.write_text(json.dumps(data))
                self.assertEqual(r.call('athba', 'result', status=404,
                    invocation_id=invocation['id'])['error'], 'not_found')
                # A new admission wakes the bounded maintenance sweep; it must not
                # use the expired identity as execution authority.
                fresh = r.wait(r.acquire('athba', 'local-primary', 'low', identity='after-expiry'))
                deadline = time.monotonic() + 5
                while time.monotonic() < deadline and self.archive_files(r):
                    time.sleep(.05)
                self.assertFalse(self.archive_files(r))
                r.release(fresh)
            finally:
                r.close()

if __name__ == '__main__':
    unittest.main(verbosity=2)
