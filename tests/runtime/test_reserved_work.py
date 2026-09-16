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
        self.wait(reservation,'held')
        work=self.work(reservation)
        waiting=r.call('athba','submit_work',request=work)
        self.assertEqual(waiting['state'],'held')
        self.assertIsNone(waiting['started'])
        cancelled=self.work(reservation,'cancelled','local-coder')
        r.call('athba','submit_work',request=cancelled)
        self.assertEqual(r.call('athba','cancel_work',work_id='cancelled')['state'],'cancelled')
        self.assertEqual(r.counts('dispatch')['local-primary'],0)
        r.release(contender);self.wait(reservation)
        completed=self.result('one')
        self.assertEqual(completed['invocation_id'],waiting['invocation_id'])
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        self.assertEqual(r.counts('dispatch')['local-coder'],0)
    def test_whole_reservation_denial_and_multi_resource_preemption(self):
        r=self.rack
        incumbent=r.wait(r.acquire('cb','local-coder','paramount'))
        denied,request=self.reserve(priority='paramount')
        self.assertEqual(denied['state'],'denied')
        self.assertEqual(r.counts('start')['local-primary'],0)
        r.release(incumbent);r.wait(incumbent,'released')
        self.assertEqual(r.call('athba','reserve',request=request)['state'],'denied')
        big=r.wait(r.acquire('other','big-brain','low'))
        replacement,_=self.reserve('fresh','high')
        replacement=self.wait(replacement)
        self.assertEqual(r.wait(big,'held')['id'],big['id'])
        self.assertEqual(replacement['priority'],'high')
