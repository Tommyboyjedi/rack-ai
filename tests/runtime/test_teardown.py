"""Actual disposable backend with controlled systemd teardown observations."""
import fcntl
import hashlib
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack, ROOT

class TeardownFixture(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rack-teardown-')
        self.root = Path(self.directory.name)
        binary = self.root/'bin'
        binary.mkdir()
        for name in ('systemd-run','systemctl','nvidia-smi'):
            path = binary/name
            path.write_text('#!' + sys.executable + '\n' +
                            (ROOT/'tests/runtime/fake_host.py').read_text().split('\n',1)[1])
            path.chmod(0o700)
        artifact = self.root/'fixture.gguf'
        artifact.write_bytes(b'disposable fixture only')
        def configure(config):
            profile = config['profiles'][0]
            profile.update(driver='systemd',backend='llama_cpp',stop_seconds=2,
                           artifact=str(artifact),artifact_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest())
        environment = dict(os.environ,RACK_HOST_FIXTURE=str(self.root),
                           PATH=str(binary)+':'+os.environ['PATH'])
        self.rack = Rack(self.root/'rack', configure, environment)
        self.demand = self.rack.wait(self.rack.acquire('other','local-primary','medium'))

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def faults(self, values):
        path = self.root/'faults.json'
        temporary = self.root/'faults.next'
        temporary.write_text(json.dumps(values))
        temporary.replace(path)

    def claims(self):
        return json.loads((self.rack.root/'authority/managed.json').read_text())['claims']

class TeardownTests(TeardownFixture):
    def test_process_exits_before_transient_unit_then_cleanup_succeeds(self):
        self.faults(dict(teardown_seconds=.8,teardown_cgroup=True))
        self.rack.release(self.demand,'cancel')
        deadline = time.monotonic()+4
        observed_draining = False
        while time.monotonic()<deadline:
            current = self.rack.inspect(self.demand)
            self.assertNotEqual(current['state'],'recovery_required',current.get('reason'))
            if current['state']=='cancelled':
                break
            if self.rack.counts('stop')['local-primary']:
                observed_draining = True
                self.assertTrue(self.claims())
            time.sleep(.03)
        self.assertTrue(observed_draining)
        self.assertEqual(current['state'],'cancelled')
        self.assertEqual(self.claims(),{})
        self.assertIsNone(current['process'])
        self.rack.release(current,'cancel')
        self.rack.wait(current,'cancelled')
        self.assertEqual(self.rack.counts('stop')['local-primary'],1)

    def test_persistent_teardown_blocks_until_repeated_valid_cancel(self):
        self.faults(dict(teardown_seconds=30))
        start=time.monotonic()
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertGreaterEqual(time.monotonic()-start,1.8)
        self.assertIn('cleanup_unproven',failure['reason'])
        self.assertTrue(self.claims())
        self.faults({})
        self.rack.release(failure,'cancel')
        self.rack.wait(failure,'cancelled')
        self.assertEqual(self.claims(),{})

    def test_changed_invocation_after_stop_keeps_claims(self):
        self.faults(dict(changed_invocation=True))
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertIn('invocation',failure['reason'])
        self.assertTrue(self.claims())

    def test_failed_observation_keeps_claims(self):
        self.faults(dict(observe_failure=True))
        self.rack.release(self.demand,'cancel')
        self.rack.wait(self.demand,'recovery_required')
        self.assertTrue(self.claims())

    def test_foreign_gpu_after_teardown_keeps_claims(self):
        self.faults(dict(teardown_seconds=.5,foreign_pid=os.getpid()))
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertIn('gpu_cleanup',failure['reason'])
        self.assertTrue(self.claims())

class OwnershipTests(TeardownFixture):
    def test_persistent_cgroup_with_inactive_unit_still_blocks(self):
        self.faults(dict(persistent_cgroup=True))
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertIn('cleanup_unproven',failure['reason'])
        self.assertTrue(self.claims())

    def test_changed_recorded_process_generation_never_receives_stop(self):
        authority=self.rack.root/'authority'
        with (authority/'authority.lock').open('a') as lock:
            fcntl.flock(lock,fcntl.LOCK_EX)
            path=authority/'managed.json'
            state=json.loads(path.read_text())
            state['data']['demands'][self.demand['id']]['process']['start']='0'
            temporary=authority/'fixture.next'
            temporary.write_text(json.dumps(state))
            temporary.replace(path)
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertIn('process_generation_changed',failure['reason'])
        self.assertEqual(self.rack.counts('stop')['local-primary'],0)
        self.assertTrue(self.claims())

    def test_unknown_invocation_during_teardown_blocks(self):
        self.faults(dict(unknown_invocation=True))
        self.rack.release(self.demand,'cancel')
        failure=self.rack.wait(self.demand,'recovery_required')
        self.assertIn('ownership_uncertain',failure['reason'])
        self.assertTrue(self.claims())

if __name__=='__main__':
    unittest.main(verbosity=2)
