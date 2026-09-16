"""Focused proof of reservation-owned priority, service membership and work identity."""
import copy
import tempfile
import time
import unittest
from pathlib import Path
from support import Rack, ROOT, VERSION
import jsonschema
import json

class ReservedWork(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='rack-reserved-work-')
        self.rack = Rack(Path(self.directory.name))
    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()
    def reserve(self, identity='group', priority='medium', services=None):
        request=dict(acquisition_id=identity,work_id='logical-work',services=services or ['local-primary','local-coder'],priority=priority,ttl_seconds=60)
        return self.rack.call('athba','reserve',request=request), request
    def wait(self, reservation, expected='ready'):
        end=time.monotonic()+12
        while time.monotonic()<end:
            reservation=self.rack.call('athba','inspect_reservation',reservation_id=reservation['id'])
            if reservation['state']==expected:return reservation
            time.sleep(.04)
        self.fail(reservation)
    def work(self, reservation, identity='one', service='local-primary'):
        return dict(reservation_id=reservation['id'],service=service,work_id=identity,
                    payload=dict(kind='inference',prompt='hello',max_tokens=16,timeout_seconds=5))
    def result(self, identity, state='completed'):
        end=time.monotonic()+10
        while time.monotonic()<end:
            result=self.rack.call('athba','inspect_work',work_id=identity)
            if result['state']==state:return result
            time.sleep(.04)
        self.fail(result)
    def test_services_priority_and_work_reconciliation(self):
        r=self.rack
        reservation,request=self.reserve(priority='paramount')
        reservation=self.wait(reservation)
        self.assertEqual(set(reservation['services']),{'local-primary','local-coder'})
        schema=json.loads((ROOT/'config/runtime/response.schema.json').read_text())
        jsonschema.validate(dict(schema=VERSION,result=reservation),schema)
        for service in reservation['services']:
            work=self.work(reservation,service,service)
            accepted=r.call('athba','submit_work',request=work)
            result=self.result(service)
            jsonschema.validate(dict(schema=VERSION,result=result),schema)
            self.assertEqual(r.call('athba','submit_work',request=work)['invocation_id'],accepted['invocation_id'])
            self.assertEqual(result['result']['model'],service)
            r.call('athba','submit_work',status=422,request=dict(work,priority='low'))
            changed=copy.deepcopy(work);changed['payload']['prompt']='different'
            r.call('athba','submit_work',status=409,request=changed)
        r.call('athba','submit_work',status=400,request=self.work(reservation,'wrong','big-brain'))
        r.call('cb','submit_work',status=404,request=self.work(reservation,'foreign'))
        r.call('cb','inspect_work',status=404,work_id='local-primary')
        self.assertEqual(r.call('athba','reserve',request=request)['id'],reservation['id'])
        r.call('athba','release_reservation',reservation_id=reservation['id'])
        self.wait(reservation,'released')
        self.assertFalse(__import__('json').loads((r.root/'authority/managed.json').read_text())['claims'])
    def test_held_group_work_waits_and_cancel_prevents_dispatch(self):
        r=self.rack
        reservation,_=self.reserve(priority='low');reservation=self.wait(reservation)
        contender=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        self.wait(reservation,'partial')
        work=self.work(reservation)
        waiting=r.call('athba','submit_work',request=work)
        self.assertEqual(waiting['state'],'held')
        self.assertIsNone(waiting['started'])
        coder=self.work(reservation,'ready-coder','local-coder')
        r.call('athba','submit_work',request=coder)
        self.result('ready-coder')
        cancelled=self.work(reservation,'cancelled','local-primary')
        r.call('athba','submit_work',request=cancelled)
        self.assertEqual(r.call('athba','cancel_work',work_id='cancelled')['state'],'cancelled')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        r.release(contender);self.wait(reservation)
        completed=self.result('one')
        self.assertEqual(completed['invocation_id'],waiting['invocation_id'])
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        self.assertEqual(r.counts('dispatch')['local-coder'],1)
    def test_partial_refresh_is_explicit_and_preserves_ready_and_held_members(self):
        r=self.rack
        r.process.terminate();r.process.wait(timeout=5);r.log.close()
        big=next(p for p in r.config['profiles'] if p['tag']=='big-brain')
        big['resources']=['gpu-4080-super'];big['device_mib']={'gpu-4080-super':256}
        r.start()
        blocker=r.wait(r.acquire('cb','comfyui','paramount'))
        original,request=self.reserve(priority='medium',services=['big-brain','local-primary','local-coder'])
        current=self.wait(original,'partial')
        end=time.monotonic()+10
        while any(current['services'][tag]['state']!='ready' for tag in ['local-primary','local-coder']):
            self.assertLess(time.monotonic(),end)
            time.sleep(.04);current=self.wait(original,'partial')
        self.assertEqual(current['services']['big-brain']['state'],'unavailable')
        self.assertEqual(current['requested_services'],request['services'])
        jsonschema.validate(dict(schema=VERSION,result=current),json.loads((ROOT/'config/runtime/response.schema.json').read_text()))
        snapshots={tag:(current['services'][tag]['id'],current['services'][tag]['generation']) for tag in ['local-primary','local-coder']}
        work=self.work(current,'partial-coder','local-coder');r.call('athba','submit_work',request=work);self.result('partial-coder')
        r.call('athba','submit_work',status=400,request=self.work(current,'missing','big-brain'))
        r.call('cb','refresh_reservation',status=404,reservation_id=current['id'])
        for _ in range(2):
            same=r.call('athba','refresh_reservation',reservation_id=current['id'])
            self.assertEqual(same['services']['big-brain']['state'],'unavailable')
        r.release(blocker);r.wait(blocker,'released');time.sleep(.3)
        replay=r.call('athba','reserve',request=request)
        self.assertEqual(replay,original)
        self.assertEqual(r.counts('start')['big-brain'],0)
        contender=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        r.wait(current['services']['local-primary'],'held')
        refreshed=r.call('athba','refresh_reservation',reservation_id=current['id'])
        self.assertEqual(refreshed['id'],current['id']);self.assertEqual(refreshed['priority'],'medium')
        self.assertEqual(refreshed['services']['local-primary']['state'],'held')
        r.wait(refreshed['services']['big-brain'])
        for tag in ['local-primary','local-coder']:
            self.assertEqual((refreshed['services'][tag]['id'],refreshed['services'][tag]['generation']),snapshots[tag])
            self.assertEqual(r.counts('start')[tag],1)
        r.call('athba','submit_work',request=self.work(current,'held-peer-coder','local-coder'));self.result('held-peer-coder')
        r.release(contender);restored=self.wait(current)
        self.assertEqual(restored['services']['local-primary']['id'],snapshots['local-primary'][0])
        self.assertEqual(r.counts('start')['big-brain'],1)
        self.assertEqual(r.call('athba','reserve',request=request),original)
        r.call('athba','release_reservation',reservation_id=current['id']);self.wait(current,'released')
        r.call('athba','refresh_reservation',status=409,reservation_id=current['id'])


def test_retired_work_unit_cli_is_unavailable():
    import subprocess
    result=subprocess.run([str(ROOT/'target/debug/rack_ai_cli'),'work-unit'],capture_output=True,text=True,timeout=5)
    assert result.returncode != 0
    assert 'unsupported command' in result.stderr
