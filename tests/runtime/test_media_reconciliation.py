"""Shared ComfyUI recovery uses real receiver transitions and isolated OS fixtures."""
import hashlib
import json
import subprocess
import sys
import time
from contextlib import contextmanager
from pathlib import Path

import pytest
import requests
from support import Rack, ROOT
sys.path.insert(0, str(ROOT/'tests/media'))
from fixture import receiver, wait_for
from test_recovery import fault, status
from test_shared_media import native_config_hash


@contextmanager
def shared(root, legacy=False):
    def historical(c):
        c['principals'].append(dict(id='cb',
            token_sha256=hashlib.sha256(b'cb').hexdigest(), operator=False))
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
        if released:
            headers = {'Authorization': 'Bearer cb'}
            session = requests.get(media['api']+'/api/media/v1/status', headers=headers,
                timeout=3).json()['session_id']
            assert session
            response = requests.post(media['api']+f'/api/media/v1/sessions/{session}/release',
                json={}, headers=headers, timeout=3)
            assert response.status_code == 202, response.text
            wait_for(lambda: status(media)['state'] == 'stopped')
        else:
            stop_owned(media)
        assert not json.loads((media['root']/'machine.json').read_text())['active']
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
    doc['claims']['gpu-4080-super'] = demand['id']
    path.write_text(json.dumps(doc))
    return path


def assert_fenced(rack, demand, path):
    rack.start()
    rack.wait(demand, 'recovery_required')
    wait_for(lambda: rack.inspect(demand).get('recovery_error'), timeout=10)
    document = json.loads(path.read_text())
    saved = document['data']['demands'][demand['id']]
    assert document['claims']['gpu-4080-super'] == demand['id']
    assert 'recovery_reconciliation' not in saved
    return document


def test_start_outcome_unknown_recovers_unrecorded_owned_media_process(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'live-process')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, stop_effect=False)
        rack.start()
        terminal(rack, demand)
        after = json.loads(path.read_text())
        recovered = after['data']['demands'][demand['id']]
        assert recovered['reason'] == 'start_outcome_unknown'
        assert recovered['recovery_reconciliation']['current_effect'] == 'proven_absent'
        assert recovered['recovery_reconciliation']['cleanup_process']['pid'] == demand['process']['pid']
        assert 'gpu-4080-super' not in after['claims']
        machine = json.loads((media['root']/'machine.json').read_text())
        assert not machine['active']
        assert any('stop' in args for args in machine['mutations'])


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


def test_start_outcome_unknown_closes_active_reservation_and_cleans_owned_effect(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'active-reservation')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, released=False, stop_effect=False)
        rack.start()
        terminal(rack, demand)
        after = json.loads(path.read_text())
        saved = after['data']['demands'][demand['id']]
        assert saved['released'] is True
        assert saved['recovery_reconciliation']['current_effect'] == 'proven_absent'
        assert 'gpu-4080-super' not in after['claims']
        media_state = json.loads((media['root']/'state/state.json').read_text())
        assert media_state['service']['state'] == 'stopped'
        assert all(session['stopped'] for session in media_state['sessions'])

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


def stop_media_writer(media):
    media['process'].kill()
    media['process'].wait(timeout=5)


def test_blank_stale_media_recovery_self_heals_when_physical_effect_is_absent(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'stale-blank-media')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, retain_uncertain=True)
        stop_media_writer(media)
        media_path = media['root']/'state/state.json'
        retained = json.loads(media_path.read_text())
        retained['service'].update(state='recovery_required', error='start_outcome_unknown')
        assert retained['service']['lease'] is None
        assert retained['service']['generation'] is None
        assert not retained['service']['activation']
        media_path.write_text(json.dumps(retained))
        rack.start()
        terminal(rack, demand)
        after = json.loads(path.read_text())
        assert 'gpu-4080-super' not in after['claims']
        assert after['data']['invocations']['historical-uncertain']['state'] == 'uncertain'
        assert after['data']['demands'][demand['id']]['reason'] == 'start_outcome_unknown'
        assert json.loads(media_path.read_text())['service']['state'] == 'stopped'


def foreign_job():
    request = json.loads((ROOT/'config/media/fixtures/job-request.json').read_text())
    request.update(submission_id='foreign-work', idempotency_key='foreign-work')
    now = int(time.time())
    return dict(id='foreign-job', owner='foreign-owner', request=request,
        state='running', created_at=now, updated_at=now, deadline=now+60,
        cleanup_pending=True, cancel_requested=False, prompt_id='foreign-prompt',
        activation='foreign-activation', workflow={}, workflow_sha256='0'*64,
        model_identity='', dispatched_at=now, artifacts=[], error=None)


@pytest.mark.parametrize('foreign_kind', ['session', 'job'])
def test_foreign_media_ownership_fences_recovery_without_stop(tmp_path, foreign_kind):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'foreign-media-'+foreign_kind)
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, stop_effect=False)
        stop_media_writer(media)
        media_path = media['root']/'state/state.json'
        retained = json.loads(media_path.read_text())
        if foreign_kind == 'session':
            retained['sessions'].append(dict(id='foreign-session', owner='foreign-owner',
                request=dict(schema='rack-ai/media/v1', idempotency_key='foreign-session'),
                created_at=int(time.time()), release_requested=False, stopped=False,
                terminal_reason=None))
        else:
            retained['jobs'].append(foreign_job())
        media_path.write_text(json.dumps(retained))
        assert_fenced(rack, demand, path)
        machine = json.loads((media['root']/'machine.json').read_text())
        assert machine['active']
        assert not any('stop' in args for args in machine['mutations'])
        after = json.loads(media_path.read_text())
        assert after['sessions'] == retained['sessions']
        assert after['jobs'] == retained['jobs']


def test_refused_owned_stop_keeps_claim_until_retry_proves_cleanup(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'stop-refused')
        demand = rack.wait(demand)
        path = mark_start_outcome_unknown(rack, media, demand, stop_effect=False)
        fault(media, machine={'stop_ignored': True})
        rack.start()
        wait_for(lambda: rack.inspect(demand).get('recovery_error') == 'media_recovery_cleanup_deadline',
            timeout=12)
        blocked = json.loads(path.read_text())
        assert blocked['claims']['gpu-4080-super'] == demand['id']
        assert 'recovery_reconciliation' not in blocked['data']['demands'][demand['id']]
        machine = json.loads((media['root']/'machine.json').read_text())
        assert machine['active']
        assert any('stop' in args for args in machine['mutations'])
        fault(media, machine={'stop_ignored': False})
        terminal(rack, demand)
        assert 'gpu-4080-super' not in json.loads(path.read_text())['claims']
        assert not json.loads((media['root']/'machine.json').read_text())['active']


def test_changed_recorded_media_process_generation_never_stops_live_backend(tmp_path):
    with shared(tmp_path) as (rack, media):
        _, demand = reserve(rack, 'recorded-generation-changed')
        demand = rack.wait(demand)
        stop_media_writer(media)
        path = mark_start_outcome_unknown(rack, media, demand, stop_effect=False)
        retained = json.loads(path.read_text())
        recorded = dict(demand['process'], start='wrong-start-ticks')
        retained['data']['demands'][demand['id']]['process'] = recorded
        path.write_text(json.dumps(retained))
        assert_fenced(rack, demand, path)
        machine = json.loads((media['root']/'machine.json').read_text())
        assert machine['active']
        assert not any('stop' in args for args in machine['mutations'])
