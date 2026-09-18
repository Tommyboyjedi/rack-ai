"""Disposable real processes behind Docker/systemd transports; no production state."""
from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from support import Rack, ROOT


@contextmanager
def host(driver):
    with tempfile.TemporaryDirectory(prefix='rack-recovery-host-') as directory:
        root = Path(directory)
        binary = root/'bin'
        binary.mkdir()
        for name in ('systemd-run', 'systemctl', 'docker', 'nvidia-smi'):
            path = binary/name
            path.write_text('#!'+sys.executable+'\n'+(ROOT/'tests/runtime/fake_host.py').read_text().split('\n', 1)[1])
            path.chmod(0o700)
        def configure(config):
            profile = config['profiles'][0]
            profile['driver'] = driver
            profile['args'] += ['--host', '127.0.0.1']
            if driver == 'docker':
                profile.update(container_image='sha256:'+'a'*64, executable=str(binary/'docker'),
                    executable_sha256=hashlib.sha256((binary/'docker').read_bytes()).hexdigest())
            else:
                artifact = root/'synthetic.gguf'
                artifact.write_bytes(b'fixture only')
                profile.update(backend='llama_cpp', artifact=str(artifact),
                    artifact_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest())
        environment = dict(os.environ, RACK_HOST_FIXTURE=str(root), PATH=str(binary)+':'+os.environ['PATH'])
        rack = Rack(root/'rack', configure, environment)
        try:
            yield root, rack
        finally:
            rack.close()


def lose_start_receipt(rack, demand):
    rack.process.terminate()
    rack.process.wait(timeout=5)
    rack.log.close()
    path = rack.root/'authority/managed.json'
    state = json.loads(path.read_text())
    record = state['data']['demands'][demand['id']]
    record.update(state='recovery_required', reason='start_outcome_unknown', process=None, effect_started=True)
    temporary = path.with_name('fixture.next')
    temporary.write_text(json.dumps(state))
    temporary.replace(path)


def wait_cleanup(rack, demand):
    deadline = time.monotonic()+12
    while time.monotonic() < deadline:
        current = rack.inspect(demand)
        if current['state'] == 'released':
            return current
        time.sleep(.05)
    raise AssertionError(current)


class HostRecoveryTests(unittest.TestCase):
    def test_missing_start_receipt_owned_process_is_cleaned_then_fresh_acquires(self):
        for driver in ('docker', 'systemd'):
            with self.subTest(driver=driver), host(driver) as (root, rack):
                old = rack.wait(rack.acquire('other', 'local-primary', 'low'))
                lose_start_receipt(rack, old)
                rack.start()
                cleaned = wait_cleanup(rack, old)
                self.assertEqual(cleaned['reason'], 'start_outcome_unknown')
                self.assertEqual(cleaned['recovery_reconciliation']['current_effect'], 'proven_absent')
                state = json.loads((rack.root/'authority/managed.json').read_text())
                self.assertEqual(state['claims'], {})
                self.assertEqual(rack.counts('start')['local-primary'], 1)
                self.assertEqual(rack.counts('dispatch')['local-primary'], 0)
                commands = [json.loads(line) for line in (root/'commands.jsonl').read_text().splitlines()]
                self.assertTrue(any('stop' in command['args'] for command in commands))
                if driver == 'docker':
                    self.assertTrue(any(command['args'][0] == 'rm' for command in commands))
                fresh = rack.wait(rack.acquire('other', 'local-primary', 'low'))
                self.assertNotEqual(fresh['id'], old['id'])
                rack.release(fresh)
                rack.wait(fresh, 'released')
                print(json.dumps(dict(driver=driver, old_reservation=old['id'], generation=old['generation'],
                    terminal=cleaned['state'], fresh_reservation=fresh['id'], no_replay=True, cleaned=True)))

    def test_missing_start_receipt_with_foreign_generation_remains_fenced(self):
        for driver in ('docker', 'systemd'):
            with self.subTest(driver=driver), host(driver) as (root, rack):
                old = rack.wait(rack.acquire('other', 'local-primary', 'low'))
                lose_start_receipt(rack, old)
                fault = 'container_activation' if driver == 'docker' else 'systemd_activation'
                (root/'faults.json').write_text(json.dumps({fault: 'foreign'}))
                rack.start()
                deadline = time.monotonic()+5
                while time.monotonic() < deadline:
                    current = rack.inspect(old)
                    if current.get('recovery_error'):
                        break
                    time.sleep(.05)
                self.assertEqual(current['state'], 'recovery_required')
                self.assertIsNotNone(current.get('recovery_error'))
                state = json.loads((rack.root/'authority/managed.json').read_text())
                self.assertEqual(set(state['claims'].values()), {old['id']})
                self.assertEqual(rack.counts('stop')['local-primary'], 0)
                self.assertEqual(rack.acquire('cb', 'local-fun-chat', 'paramount')['state'], 'unavailable')
                print(json.dumps(dict(driver=driver, reservation=old['id'], state=current['state'],
                    recovery_error=current['recovery_error'], foreign_process_stopped=False)))


if __name__ == '__main__':
    unittest.main(verbosity=2)
