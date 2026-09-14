"""PR35 review regressions exercise real HTTP and disposable backend calls."""
import json,time,tempfile,unittest,urllib.request,urllib.error
from pathlib import Path
from support import Rack

class ReviewRegressions(unittest.TestCase):
    def setUp(self):
        self.directory=tempfile.TemporaryDirectory(prefix='rack-pr35-review-')
        self.rack=Rack(self.directory.name)
    def tearDown(self):
        self.rack.close();self.directory.cleanup()
    def wait_dispatch(self,model,count=1):
        deadline=time.monotonic()+3
        while self.rack.counts('dispatch')[model]<count and time.monotonic()<deadline:time.sleep(.01)
        self.assertEqual(self.rack.counts('dispatch')[model],count)
    def gateway(self,d,body,key=None):
        headers={'Content-Type':'application/json'}
        if key:headers['Idempotency-Key']=key
        request=urllib.request.Request(f'http://{self.rack.address}'+d['gateway_path']+'/chat/completions',data=json.dumps(body).encode(),headers=headers)
        try:response=urllib.request.urlopen(request,timeout=15)
        except urllib.error.HTTPError as error:response=error
        with response:return response.status,json.loads(response.read())
    def test_started_cancellation_preserves_late_output_without_success(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',delay=1)
        i=r.infer(d);self.wait_dispatch('local-primary')
        first=r.call('athba','cancel',invocation_id=i['id'])
        second=r.call('athba','cancel',invocation_id=i['id'])
        self.assertEqual(first,second)
        time.sleep(1.3)
        actual=r.call('athba','result',invocation_id=i['id'])
        self.assertEqual(actual['state'],'cancelled',actual)
        self.assertIsNotNone(actual['cancellation'])
        self.assertIsNone(actual['result'])
        self.assertIsNotNone(actual['late_result'])
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
    def test_held_wait_does_not_spend_execution_timeout(self):
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'))
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        held=r.wait(p,'held');i=r.infer(held,timeout_seconds=1)
        time.sleep(2)
        r.release(chat);r.wait(p)
        actual=r.result(i)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        self.assertEqual(actual['execution_deadline']-actual['started'],1)
    def test_fallback_identity_does_not_collide_after_restoration(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'))
        body=dict(model='local-primary',messages=[dict(role='user',content='same')],max_tokens=16)
        self.assertEqual(self.gateway(d,body)[0],200)
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        r.release(chat);d=r.wait(d)
        status,value=self.gateway(d,body)
        self.assertEqual(status,200,value)
        self.assertEqual(r.counts('dispatch')['local-primary'],2)
    def test_status_read_does_not_rewrite_authority(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'));r.release(d);d=r.wait(d,'released')
        # Terminal reservation has no lifecycle work left; inspection must be read-only.
        path=r.root/'authority/managed.json'
        before=path.stat().st_ino
        r.inspect(d)
        self.assertEqual(path.stat().st_ino,before,'status rewrote canonical document')

    def restart(self,limits=None):
        r=self.rack;r.process.kill();r.process.wait();r.log.close()
        if limits:r.config['limits']=limits
        r.start()

    def test_cancel_restart_retains_intent_and_never_replays_uncertain_dispatch(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'))
        r.controls('local-primary',delay=2)
        i=r.infer(d);self.wait_dispatch('local-primary')
        cancelled=r.call('athba','cancel',invocation_id=i['id'])
        self.restart()
        actual=r.result(i,'uncertain')
        self.assertEqual(actual['cancellation'],cancelled['cancellation'])
        self.assertIsNone(actual['result']);time.sleep(2.2)
        replay=r.call('athba','infer',request=i['request'])
        self.assertEqual(replay['id'],i['id']);self.assertEqual(replay['state'],'uncertain')
        self.assertEqual(r.call('athba','cancel',invocation_id=i['id']),replay)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)

    def test_waiting_expiry_and_cancellation_never_dispatch(self):
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'))
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));held=r.wait(p,'held')
        expired=r.infer(held,wait_seconds=1);cancelled=r.infer(held,wait_seconds=10)
        r.call('athba','cancel',invocation_id=cancelled['id'])
        self.assertEqual(r.result(expired,'expired')['started'],None)
        result=r.result(cancelled,'cancelled');self.assertIsNotNone(result['cancellation'])
        r.release(chat);r.wait(p);time.sleep(.3)
        self.assertEqual(r.counts('dispatch')['local-primary'],0)

    def test_submission_during_bounded_preemption_drain_is_held(self):
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'));r.controls('local-primary',delay=1)
        running=r.infer(p);self.wait_dispatch('local-primary')
        chat=r.acquire('cb','local-fun-chat','paramount')
        draining=r.inspect(p);self.assertEqual(draining['state'],'draining')
        pending=r.infer(draining);self.assertEqual(pending['state'],'accepted')
        r.result(running);chat=r.wait(chat);r.wait(p,'held');r.controls('local-primary',delay=0)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)
        r.release(chat);r.wait(p);r.result(pending)
        self.assertEqual(r.counts('dispatch')['local-primary'],2)

    def test_explicit_identity_distinct_replay_conflict_restore_and_reacquire(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'))
        body=dict(model='local-primary',messages=[dict(role='user',content='same')],max_tokens=16)
        first=self.gateway(d,body,'one');self.assertEqual(first[0],200)
        self.assertEqual(self.gateway(d,body,'one'),first)
        self.assertEqual(self.gateway(d,body,'two')[0],200)
        changed=dict(body,messages=[dict(role='user',content='changed')])
        self.assertEqual(self.gateway(d,changed,'one'),(409,dict(schema='rack-ai/runtime/v1',error='identity_conflict')))
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));r.release(chat);restored=r.wait(d)
        self.assertEqual(self.gateway(restored,body,'one'),first)
        self.assertEqual(r.counts('dispatch')['local-primary'],2)
        self.assertEqual(self.gateway(d,body,'three')[0],409)
        r.release(restored);r.wait(restored,'released');fresh=r.wait(r.acquire('athba','local-primary','low'))
        self.assertEqual(self.gateway(fresh,body,'one')[0],200)
        self.assertEqual(r.counts('dispatch')['local-primary'],3)

    def test_gateway_uncertain_response_retry_reconciles_one_dispatch(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'));r.controls('local-primary',uncertain=True)
        body=dict(model='local-primary',messages=[dict(role='user',content='uncertain')],max_tokens=16)
        first=self.gateway(d,body,'one');self.assertEqual(first[0],409)
        self.assertIn('Uncertain',first[1]['error'])
        r.controls('local-primary',uncertain=False)
        self.assertEqual(self.gateway(d,body,'one'),first)
        self.restart();self.assertEqual(self.gateway(d,body,'one'),first)
        self.assertEqual(r.counts('dispatch')['local-primary'],1)

    def test_queue_refuses_precisely_and_held_work_creates_no_dispatch_workers(self):
        self.restart(dict(max_pending=3,max_pending_per_reservation=2,max_dispatch_workers=1))
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'));chat=r.wait(r.acquire('cb','local-fun-chat','paramount'))
        held=r.wait(p,'held');one=r.infer(held);two=r.infer(held)
        request=dict(one['request'],submission_id='saturated')
        failure=r.call('athba','infer',request=request,status=429)
        self.assertEqual(failure['error'],'capacity_pending_reservation')
        time.sleep(.5)
        self.assertFalse((r.root/'authority/workers'/one['id']).exists())
        self.assertFalse((r.root/'authority/workers'/two['id']).exists())
        start=time.monotonic();r.inspect(p);r.call('athba','cancel',invocation_id=one['id']);r.release(chat)
        self.assertLess(time.monotonic()-start,2)
        r.wait(p);self.assertEqual(r.result(two)['state'],'completed')
        self.assertEqual(r.counts('dispatch')['local-primary'],1)

    def test_global_queue_limit_and_reservation_expiry_without_dispatch(self):
        self.restart(dict(max_pending=3,max_pending_per_reservation=2))
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'));c=r.wait(r.acquire('athba','local-coder','low'))
        brain=r.wait(r.acquire('other','big-brain','paramount'));p=r.wait(p,'held');c=r.wait(c,'held')
        one=r.infer(p);two=r.infer(p);three=r.infer(c)
        failure=r.call('athba','infer',request=dict(three['request'],submission_id='global-full'),status=429)
        self.assertEqual(failure['error'],'capacity_pending_global')
        r.call('athba','control',reservation_id=p['id'],request=dict(generation=p['generation'],action=dict(kind='renew',ttl_seconds=1)))
        self.assertIsNone(r.result(one,'expired')['started']);self.assertIsNone(r.result(two,'expired')['started'])
        r.call('athba','cancel',invocation_id=three['id']);r.release(brain);r.wait(c)
        self.assertEqual(sum(r.counts('dispatch').values()),0)

    def test_global_worker_limit_serializes_actual_backend_calls(self):
        self.restart(dict(max_dispatch_workers=1))
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'));c=r.wait(r.acquire('athba','local-coder','low'))
        r.controls('local-primary',delay=1);r.controls('local-coder',delay=1)
        one=r.infer(p);two=r.infer(c);r.result(one);r.result(two)
        active=0;peak=0
        for event in map(json.loads,r.events.read_text().splitlines()):
            if event['kind']=='dispatch':active+=1;peak=max(peak,active)
            if event['kind']=='complete':active-=1
        self.assertEqual(peak,1);self.assertEqual(active,0)

    def test_retention_refusal_leaves_accepted_output_and_release_headroom(self):
        self.restart(dict(max_response_bytes=16384,retention_admission_bytes=512*1024))
        r=self.rack;p=r.wait(r.acquire('athba','local-primary','low'));c=r.wait(r.acquire('athba','local-coder','low'))
        chat=r.wait(r.acquire('cb','local-fun-chat','paramount'));held=r.wait(p,'held');pending=r.infer(held)
        r.controls('local-coder',content='x'*12000)
        refused=False
        for index in range(35):
            request=dict(pending['request'],reservation_id=c['id'],generation=c['generation'],profile_hash=c['profile_hash'],submission_id=f'fill-{index}')
            # Inspect the actual status instead of accepting any generic storage error.
            req=urllib.request.Request(f'http://{r.address}/runtime/v1',data=json.dumps(dict(operation='infer',request=request)).encode(),headers={'Authorization':'Bearer athba','Content-Type':'application/json'})
            try:response=urllib.request.urlopen(req,timeout=4)
            except urllib.error.HTTPError as e:response=e
            with response:status=response.status;value=json.loads(response.read())
            if status==429:
                self.assertEqual(value['error'],'capacity_retained_evidence');refused=True;break
            self.assertEqual(status,200,value);r.result(value['result'])
        self.assertTrue(refused,'did not reach admission threshold')
        r.release(chat);r.wait(p);self.assertEqual(r.result(pending)['state'],'completed')
        r.release(p);r.release(c);r.wait(p,'released');r.wait(c,'released')
        document=json.loads((r.root/'authority/managed.json').read_text())
        self.assertEqual(document['claims'],{})
        self.assertGreater(len(document['data']['invocations']),1)
        self.assertEqual(document['data']['invocations'][pending['id']]['state'],'completed')

    def test_large_retained_history_idle_reads_do_not_starve_control_lock(self):
        import copy,fcntl
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'));seed=r.result(r.infer(d))
        r.release(d);r.wait(d,'released');r.process.kill();r.process.wait();r.log.close()
        path=r.root/'authority/managed.json';document=json.loads(path.read_text())
        # Seed explicitly synthetic terminal history; these are not claimed backend calls.
        document['data']['invocations']={}
        for index in range(8):
            item=copy.deepcopy(seed);item['id']=f'seeded-history-{index}';item['request']['submission_id']=item['id'];item['response_bytes']=4*1024*1024
            item['result']['choices'][0]['message']['content']='x'*(3*1024*1024)
            document['data']['invocations'][item['id']]=item
        current=len(json.dumps(document,separators=(',',':')).encode())
        extra=30*1024*1024-65536-current
        for item in document['data']['invocations'].values():item['result']['choices'][0]['message']['content']+='x'*(extra//8)
        path.write_text(json.dumps(document,separators=(',',':')));r.start()
        blocked=0;inode=path.stat().st_ino
        with (r.root/'authority/authority.lock').open('r+') as lock:
            for _ in range(20):
                try:fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
                except BlockingIOError:blocked+=1
                else:fcntl.flock(lock,fcntl.LOCK_UN)
                time.sleep(.05)
        self.assertEqual(blocked,0,'idle inspection took the durable mutation lock')
        self.assertEqual(r.inspect(d)['state'],'released');self.assertEqual(path.stat().st_ino,inode)
        request=dict(d['request'],acquisition_id='retained-full')
        failure=r.call('athba','acquire',request=request,status=429)
        self.assertEqual(failure['error'],'capacity_retained_evidence')
        self.assertEqual(json.loads(path.read_text())['claims'],{})

    def test_actual_storage_failure_keeps_started_uncertain_and_fail_closed(self):
        r=self.rack;d=r.wait(r.acquire('athba','local-primary','low'));r.controls('local-primary',delay=1)
        i=r.infer(d);self.wait_dispatch('local-primary');authority=r.root/'authority'
        authority.chmod(0o500)
        try:
            time.sleep(1.4)
            self.assertEqual(r.call('athba','result',invocation_id=i['id'])['state'],'started')
            failure=r.call('athba','cancel',invocation_id=i['id'],status=400)
            self.assertNotIn('capacity_',failure['error'])
            self.assertIn('Permission denied',failure['error'])
        finally:authority.chmod(0o700)
        self.restart();self.assertEqual(r.result(i,'uncertain')['result'],None)
        self.assertEqual(r.call('athba','infer',request=i['request'])['state'],'uncertain')
        self.assertEqual(r.counts('dispatch')['local-primary'],1)

if __name__=='__main__':unittest.main(verbosity=2)
