"""Automatic recovery of attributable effects on disposable process authorities."""
import copy
import json
import tempfile
import time
import unittest
from pathlib import Path

from support import Rack


def alive(process):
    try:
        fields = Path(f'/proc/{process["pid"]}/stat').read_text().rsplit(') ', 1)[1].split()
        return fields[0] != 'Z' and fields[19] == process['start']
    except FileNotFoundError:
        return False


def stop_receiver(rack):
    rack.process.kill()
    rack.process.wait(timeout=5)
    rack.log.close()


def document(rack):
    return json.loads((rack.root / 'authority' / 'managed.json').read_text())


def wait_until(observe, seconds=8):
    deadline = time.monotonic() + seconds
    last = None
    while time.monotonic() < deadline:
        last = observe()
        if last:
            return last
        time.sleep(.03)
    raise AssertionError(f'condition did not become true: {last!r}')


def seed_recovery(rack, demand, reason='start_outcome_unknown'):
    """Only edit the disposable authority after its receiver has stopped."""
    stop_receiver(rack)
    state = document(rack)
    saved = state['data']['demands'][demand['id']]
    saved.update(state='recovery_required', reason=reason, effect_started=True)
    return state


def save_and_restart(rack, state):
    assert rack.process.poll() is not None
    (rack.root / 'authority' / 'managed.json').write_text(json.dumps(state))
    rack.start()


def wait_released(rack, demand):
    last = {}
    def released():
        current = rack.inspect(demand)
        last.update({key: current.get(key) for key in (
            'state', 'reason', 'recovery_error', 'released', 'process')})
        return current if current['state'] in ('released', 'expired', 'cancelled') else None
    try:
        return wait_until(released)
    except AssertionError as error:
        raise AssertionError(last) from error


class OwnedProcessRecovery(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rack-owned-recovery-')
        self.rack = Rack(Path(self.directory.name))

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def test_owned_effect_is_cleaned_uncertainty_retained_and_fresh_request_acquires(self):
        rack = self.rack
        old = rack.wait(rack.acquire('athba', 'local-primary', 'low', identity='old-owner'))
        invocation = rack.result(rack.infer(old, identity='historical-result-lost'))
        before = rack.counts('dispatch')['local-primary']
        state = seed_recovery(rack, old)
        uncertain = state['data']['invocations'][invocation['id']]
        uncertain.update(state='uncertain', result=None, error='simulated_lost_terminal_result')
        queued = copy.deepcopy(uncertain)
        queued.update(id='queued-before-recovery', state='queued', started=None,
                      activation=None, error=None, queue_order=uncertain['queue_order'] + 1)
        queued['request']['submission_id'] = queued['id']
        state['data']['invocations'][queued['id']] = queued
        save_and_restart(rack, state)
        released = wait_released(rack, old)
        self.assertEqual(released['reason'], 'start_outcome_unknown')
        self.assertFalse(alive(old['process']))
        after = document(rack)
        retained = after['data']['demands'][old['id']]
        proof = retained['recovery_reconciliation']
        self.assertEqual(proof['historical_outcome'], 'start_outcome_unknown')
        self.assertEqual(proof['current_effect'], 'proven_absent')
        self.assertTrue(all(proof['checks'].values()))
        self.assertFalse(retained['effect_started'])
        self.assertIsNone(retained['process'])
        self.assertEqual(after['claims'], {})
        self.assertEqual(rack.result(invocation, 'uncertain')['error'], 'simulated_lost_terminal_result')
        self.assertIsNone(rack.result(queued, 'cancelled')['started'])
        self.assertEqual(rack.counts('dispatch')['local-primary'], before)
        rack.release(old)
        repeated_release = rack.inspect(old)
        self.assertEqual(repeated_release['reason'], 'start_outcome_unknown')
        self.assertEqual(repeated_release['recovery_reconciliation'], proof)
        stop_receiver(rack)
        rack.start()
        time.sleep(.25)
        self.assertEqual(document(rack)['claims'], {})
        self.assertEqual(rack.counts('start')['local-primary'], 1)
        self.assertEqual(rack.counts('dispatch')['local-primary'], before)
        self.assertEqual(document(rack)['data']['demands'][old['id']]['recovery_reconciliation'], proof)
        fresh = rack.wait(rack.acquire('athba', 'local-primary', 'low', identity='fresh-owner'))
        self.assertNotEqual(fresh['id'], old['id'])
        self.assertNotEqual(fresh['generation'], old['generation'])
        rack.release(fresh)
        rack.wait(fresh, 'released')

    def test_changed_activation_stays_fenced_without_stopping_process(self):
        self.assert_changed_identity_fenced('activation')

    def test_changed_process_generation_stays_fenced_without_stopping_process(self):
        self.assert_changed_identity_fenced('start')

    def assert_changed_identity_fenced(self, field):
        rack = self.rack
        old = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
        state = seed_recovery(rack, old)
        state['data']['demands'][old['id']]['process'][field] = 'wrong-identity'
        save_and_restart(rack, state)
        rack.wait(old, 'recovery_required')
        wait_until(lambda: rack.inspect(old).get('recovery_error'))
        after = document(rack)
        self.assertEqual(after['claims']['gpu-4060ti'], old['id'])
        self.assertNotIn('recovery_reconciliation', after['data']['demands'][old['id']])
        self.assertTrue(alive(old['process']))
        self.assertEqual(rack.counts('stop')['local-primary'], 0)
        denied = rack.acquire('cb', 'local-fun-chat', 'paramount')
        self.assertEqual(denied['state'], 'unavailable')

    def test_active_call_drains_before_owned_recovery_cleanup(self):
        rack = self.rack
        old = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
        rack.controls('local-primary', delay=2)
        running = rack.infer(old)
        wait_until(lambda: rack.counts('dispatch')['local-primary'] == 1)
        queued = rack.infer(old, identity='waiting-when-backend-fails')
        rack.controls('local-primary', delay=2, model='unexpected-model')
        rack.wait(old, 'recovery_required')
        self.assertTrue(alive(old['process']))
        self.assertEqual(rack.call('athba', 'result', invocation_id=running['id'])['state'], 'running')
        self.assertEqual(document(rack)['claims']['gpu-4060ti'], old['id'])
        rejected = copy.deepcopy(running['request'])
        rejected['submission_id'] = 'no-admission-during-recovery'
        error = rack.call('athba', 'infer', status=409, request=rejected)
        self.assertEqual(error['error'], 'reservation_not_dispatchable')
        self.assertIsNone(rack.result(queued, 'cancelled')['started'])
        self.assertEqual(rack.result(running)['state'], 'completed')
        wait_released(rack, old)
        self.assertFalse(alive(old['process']))
        self.assertEqual(rack.counts('complete')['local-primary'], 1)
        self.assertEqual(document(rack)['claims'], {})


class UncertainInvocationRecovery(unittest.TestCase):
    def test_healthy_backend_with_uncertain_invocation_is_cleaned_without_replay(self):
        with tempfile.TemporaryDirectory(prefix='rack-uncertain-recovery-') as directory:
            rack = Rack(Path(directory))
            try:
                old = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                rack.controls('local-primary', uncertain=True)
                invocation = rack.infer(old)
                uncertain = rack.result(invocation, 'uncertain')
                self.assertIsNone(uncertain['result'])
                self.assertTrue(uncertain['error'])
                wait_released(rack, old)
                self.assertFalse(alive(old['process']))
                self.assertEqual(document(rack)['claims'], {})
                self.assertEqual(rack.result(invocation, 'uncertain')['error'], uncertain['error'])
                self.assertEqual(rack.counts('dispatch')['local-primary'], 1)
                self.assertEqual(rack.counts('start')['local-primary'], 1)
                rack.controls('local-primary', uncertain=False)
                fresh = rack.wait(rack.acquire('athba', 'local-primary', 'low'))
                self.assertNotEqual(fresh['id'], old['id'])
                self.assertEqual(rack.result(rack.infer(fresh))['state'], 'completed')
                rack.release(fresh)
                rack.wait(fresh, 'released')
            finally:
                rack.close()


if __name__ == '__main__':
    unittest.main(verbosity=2)
