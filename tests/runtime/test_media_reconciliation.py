"""Shared ComfyUI recovery uses real receiver transitions and isolated OS fixtures."""
import json
import subprocess
import sys
import time
from contextlib import contextmanager
from pathlib import Path

import pytest
from support import Rack, ROOT
sys.path.insert(0, str(ROOT/'tests/media'))
from fixture import receiver, wait_for
from test_recovery import fault, status
from test_shared_media import native_config_hash


@contextmanager
def shared(root, legacy=False):
    def historical(c):
        if legacy:
            for principal in c['principals']:
                principal['ceiling'] = 'low'
    with receiver(root/'media', historical) as media:
        fault(media, machine={'new_process': True})
        def configure(c):
            c['authority_root'] = media['config']['resource_root']
            c['devices']['gpu-4080-super']['uuid'] = media['config']['media_uuid']
            p = next(p for p in c['profiles'] if p['tag'] == 'comfyui')
            p.update(backend='comfyui', driver='systemd', media_mode='interactive',
                media_config=str(media['root']/'config.json'),
                media_config_sha256=native_config_hash(media), endpoint=media['backend'],
                model='', startup_seconds=10)
        rack = Rack(root/'runtime', configure, media['environment'])
        try:
            yield rack, media
        finally:
            rack.close()


def reserve(rack, key):
    result = rack.call('cb', 'reserve', request=dict(acquisition_id=key,
        work_id=key, services=['comfyui'], priority='paramount', ttl_seconds=60))
    return result, result['services']['comfyui']


def stop_owned(media):
    subprocess.run(['systemctl', '--user', 'stop', media['config']['unit']],
        env=media['environment'], check=True, capture_output=True)


def terminal(rack, demand):
    def observed():
        value = rack.inspect(demand)
        return value if value['state'] in ('released', 'expired') else None
    return wait_for(observed, timeout=20)



def mark_start_outcome_unknown(rack, media, demand, *, released=True,
    stop_effect=True, retain_uncertain=False):
    rack.process.kill()
    rack.process.wait(timeout=5)
    rack.log.close()
    if stop_effect:
        stop_owned(media)
        wait_for(lambda: status(media)['state'] == 'stopped')
    path = Path(rack.config['authority_root'])/'managed.json'
    doc = json.loads(path.read_text())
    saved = doc['data']['demands'][demand['id']]
    saved.update(state='recovery_required', reason='start_outcome_unknown',
        process=None, effect_started=True, released=released)
    root = doc['data']['demands'][saved.get('reservation_id') or demand['id']]
    root['reservation_closed'] = 'released' if released else None
    if retain_uncertain:
        doc['data']['invocations']['historical-uncertain'] = dict(
            id='historical-uncertain', owner='cb', state='uncertain', queue_order=1,
            waiting_deadline=int(time.time())+60, execution_deadline=None,
            response_bytes=0, cancellation=None, late_result=None, result_digest=None,
            late_result_digest=None, started=int(time.time()), activation=demand['generation'],
            result=None, error='simulated_lost_terminal_result', request=dict(
                schema='rack-ai/runtime/v1', submission_id='historical-uncertain',
                reservation_id=demand['id'], generation=demand['generation'],
                profile_hash=demand['profile_hash'], prompt='retained historical evidence',
                max_tokens=16, timeout_seconds=5))
    path.write_text(json.dumps(doc))
    return path


def assert_fenced(rack, demand, path):
    rack.start()
    rack.wait(demand, 'recovery_required')
    time.sleep(.3)
    document = json.loads(path.read_text())
    saved = document['data']['demands'][demand['id']]
    assert document['claims']['gpu-4080-super'] == demand['id']
    assert 'recovery_reconciliation' not in saved
    return document


def test_start_outcome_unknown_live_owned_process_keeps_claim_fenced(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'live-process')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, stop_effect=False)
        assert_fenced(rack, demand, path)
        assert json.loads((media['root']/'machine.json').read_text())['active']


def test_start_outcome_unknown_gpu_process_keeps_claim_fenced(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'foreign-gpu')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand)
        fault(media, machine={'foreign': True})
        assert_fenced(rack, demand, path)


def test_start_outcome_unknown_unreadable_gpu_probe_keeps_claim_fenced(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'unreadable-gpu')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand)
        fault(media, machine={'gpu_probe_error': True})
        assert_fenced(rack, demand, path)


def test_start_outcome_unknown_active_reservation_keeps_claim_fenced(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'active-reservation')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, released=False)
        assert_fenced(rack, demand, path)

def test_start_outcome_unknown_releases_only_proven_absent_current_effect(tmp_path):
    with shared(tmp_path, legacy=True) as (rack, media):
        _, demand = reserve(rack, 'effect-absence')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, retain_uncertain=True)
        rack.start()
        terminal(rack, demand)
        after = json.loads(path.read_text())
        reconciled = after['data']['demands'][demand['id']]
        assert reconciled['state'] == 'released'
        assert reconciled['reason'] == 'start_outcome_unknown'
        assert reconciled['effect_started'] is False
        assert reconciled['process'] is None
        assert reconciled['recovery_reconciliation'] == dict(
            historical_outcome='start_outcome_unknown', current_effect='proven_absent',
            reconciled_at=reconciled['recovery_reconciliation']['reconciled_at'],
            checks=dict(reservation_inactive=True, active_invocations_absent=True,
                recorded_process_absent=True, systemd_activation_absent=True,
                gpu_allocation_absent=True, media_session_absent=True,
                lifecycle_transition_absent=True, ownership_fence_intact=True))
        assert after['data']['invocations']['historical-uncertain']['state'] == 'uncertain'
        assert 'gpu-4080-super' not in after['claims']
        starts_before_restart = sum('start' in args for args in json.loads(
            (media['root']/'machine.json').read_text()).get('mutations', []))
        rack.process.kill()
        rack.process.wait(timeout=5)
        rack.log.close()
        rack.start()
        assert rack.inspect(demand)['state'] == 'released'
        restarted = json.loads(path.read_text())
        assert restarted['data']['demands'][demand['id']]['recovery_reconciliation'] == reconciled['recovery_reconciliation']
        assert restarted['data']['invocations']['historical-uncertain']['state'] == 'uncertain'
        assert 'gpu-4080-super' not in restarted['claims']
        starts_after_restart = sum('start' in args for args in json.loads(
            (media['root']/'machine.json').read_text()).get('mutations', []))
        assert starts_after_restart == starts_before_restart
        fresh, next_demand = reserve(rack, 'fresh-after-reconciliation')
        rack.wait(next_demand)
        rack.call('cb', 'release_reservation', reservation_id=fresh['id'])
        terminal(rack, next_demand)
def test_transport_failure_then_clean_stop_releases_claim(tmp_path):
    with shared(tmp_path) as (rack, media):
        reservation, demand = reserve(rack, 'transport-stop')
        demand = rack.wait(demand)
        fault(media, gate_disconnect=True)
        wait_for(lambda: rack.inspect(demand)['state'] == 'preparing')
        stop_owned(media)
        terminal(rack, demand)
        doc = json.loads((Path(rack.config['authority_root'])/'managed.json').read_text())
        assert 'gpu-4080-super' not in doc['claims']
        fault(media, gate_disconnect=False)
        renewed, next_demand = reserve(rack, 'after-stop')
        rack.wait(next_demand)
        rack.call('cb', 'release_reservation', reservation_id=renewed['id'])
        terminal(rack, next_demand)


def test_stale_recovery_self_reconciles_after_receiver_restart(tmp_path):
    with shared(tmp_path, legacy=True) as (rack, media):
        reservation, demand = reserve(rack, 'old-generation')
        demand = rack.wait(demand)
        rack.process.kill()
        rack.process.wait(timeout=5)
        rack.log.close()
        stop_owned(media)
        path = Path(rack.config['authority_root'])/'managed.json'
        doc = json.loads(path.read_text())
        doc['data']['demands'][demand['id']].update(
            state='recovery_required', reason='backend transport uncertain')
        history = doc['data']['invocations']
        path.write_text(json.dumps(doc))
        rack.start()
        terminal(rack, demand)
        after = json.loads(path.read_text())
        assert after['data']['invocations'] == history
        assert 'gpu-4080-super' not in after['claims']
        renewed, next_demand = reserve(rack, 'after-restart')
        rack.wait(next_demand)
        rack.call('cb', 'release_reservation', reservation_id=renewed['id'])
        terminal(rack, next_demand)


@pytest.mark.parametrize('ambiguity', ['changed_invocation', 'foreign_gpu'])
def test_ambiguous_ownership_keeps_claim(tmp_path, ambiguity):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'ambiguous')
        demand = rack.wait(demand)
        if ambiguity == 'changed_invocation':
            fault(media, machine={'invocation': 'foreign-invocation'})
        else:
            fault(media, machine={'foreign': True})
            stop_owned(media)
        rack.wait(demand, 'recovery_required')
        time.sleep(.6)
        assert rack.inspect(demand)['state'] == 'recovery_required'
        doc = json.loads((Path(rack.config['authority_root'])/'managed.json').read_text())
        assert doc['claims']['gpu-4080-super'] == demand['id']
        _, denied = reserve(rack, 'still-blocked')
        assert denied['state'] == 'unavailable'
        if ambiguity == 'changed_invocation':
            machine = json.loads((media['root']/'machine.json').read_text())
            assert machine['active']
            assert not any('stop' in args for args in machine['mutations'])


def test_owned_transient_transport_recovers_same_generation(tmp_path):
    with shared(tmp_path) as (rack, media):
        reservation, demand = reserve(rack, 'transient')
        demand = rack.wait(demand)
        fault(media, gate_disconnect=True)
        wait_for(lambda: rack.inspect(demand)['state'] == 'preparing')
        # Let the independent media supervisor see the same transport outage.
        time.sleep(3)
        assert rack.inspect(demand)['state'] == 'preparing'
        fault(media, gate_disconnect=False)
        restored = rack.wait(demand, seconds=15)
        assert restored['generation'] == demand['generation']
        assert restored['process']['pid'] == demand['process']['pid']
        rack.call('cb', 'release_reservation', reservation_id=reservation['id'])
        terminal(rack, demand)


def test_owned_unreachable_backend_is_cleaned_up_at_recovery_deadline(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'unreachable')
        demand = rack.wait(demand)
        fault(media, gate_disconnect=True)
        terminal(rack, demand)
        machine = json.loads((media['root']/'machine.json').read_text())
        assert not machine['active']
        assert any('stop' in args for args in machine['mutations'])
        doc = json.loads((Path(rack.config['authority_root'])/'managed.json').read_text())
        assert 'gpu-4080-super' not in doc['claims']


def test_initial_start_waits_for_gate_identity_without_quarantine(tmp_path):
    with shared(tmp_path) as (rack, media):
        fault(media, gate_unavailable=True)
        reservation, demand = reserve(rack, 'initial-start')
        time.sleep(2)
        assert rack.inspect(demand)['state'] == 'preparing'
        fault(media, gate_unavailable=False)
        rack.wait(demand, seconds=15)
        rack.call('cb', 'release_reservation', reservation_id=reservation['id'])
        terminal(rack, demand)
