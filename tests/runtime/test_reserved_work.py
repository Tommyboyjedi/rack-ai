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
    def test_preempted_group_work_rejects_only_displaced_service(self):
        r=self.rack
        reservation,_=self.reserve(priority='low');reservation=self.wait(reservation)
        contender=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        displaced=self.wait(reservation,'partial')
        self.assertEqual(displaced['services']['local-primary']['state'],'preempted')
        self.assertEqual(displaced['services']['local-coder']['state'],'ready')
        r.call('athba','submit_work',status=409,request=self.work(displaced,'preempted-primary'))
        coder=self.work(displaced,'ready-coder','local-coder')
        r.call('athba','submit_work',request=coder)
        self.result('ready-coder')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        self.assertEqual(r.counts('dispatch')['local-coder'],1)
        r.release(contender);r.wait(contender,'released')
        self.assertEqual(r.call('athba','inspect_reservation',reservation_id=reservation['id'])['services']['local-primary']['state'],'preempted')
        r.call('athba','release_reservation',reservation_id=reservation['id'])
        self.wait(reservation,'released')
    def test_atomic_unavailable_group_does_not_create_partial_record(self):
        r=self.rack
        r.process.terminate();r.process.wait(timeout=5);r.log.close()
        big=next(p for p in r.config['profiles'] if p['tag']=='big-brain')
        big['resources']=['gpu-4080-super'];big['device_mib']={'gpu-4080-super':256}
        r.start()
        blocker=r.wait(r.acquire('cb','local-coder','paramount'))
        request=dict(acquisition_id='three-service-atomic',work_id='logical-work',
                     services=['big-brain','local-primary','local-coder'],priority='medium',ttl_seconds=60)
        before=len(json.loads((r.root/'authority/managed.json').read_text())['data']['demands'])
        unavailable=r.call('athba','reserve',request=request)
        self.assertEqual(unavailable['state'],'unavailable')
        self.assertEqual({member['state'] for member in unavailable['services'].values()},{'unavailable'})
        self.assertEqual(len(json.loads((r.root/'authority/managed.json').read_text())['data']['demands']),before)
        r.release(blocker);r.wait(blocker,'released')
        ready=self.wait(r.call('athba','reserve',request=request))
        self.assertEqual({member['state'] for member in ready['services'].values()},{'ready'})
        r.call('athba','release_reservation',reservation_id=ready['id'])
        self.wait(ready,'released')

def test_retired_work_unit_cli_is_unavailable():
    import subprocess
    result=subprocess.run([str(ROOT/'target/debug/rack_ai_cli'),'work-unit'],capture_output=True,text=True,timeout=5)
    assert result.returncode != 0
    assert 'unsupported command' in result.stderr
