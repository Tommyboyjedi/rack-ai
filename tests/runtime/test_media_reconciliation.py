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
from test_recovery import fault
from test_shared_media import native_config_hash


@contextmanager
def shared(root):
    with receiver(root/'media') as media:
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
    with shared(tmp_path) as (rack, media):
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
