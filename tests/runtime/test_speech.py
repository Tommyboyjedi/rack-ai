import hashlib
import json
import tempfile
import unittest
import urllib.request
import urllib.error
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from support import Rack, ROOT, VERSION

def configure(c):
    p=c['profiles'][2]
    p.update(tag='local-tts', model='ResembleAI/chatterbox-turbo', backend='chatterbox',
        capabilities=['audio'], protocols=['speech'], resources=['gpu-2060'], device_mib={'gpu-2060':16})
    p['args'][0]=str(ROOT/'tests/runtime/speech_backend.py')
    p['args'][2]=p['model']
    p['artifact']=p['args'][0]
    p['artifact_sha256']=hashlib.sha256(Path(p['artifact']).read_bytes()).hexdigest()

def speech(r,d,key='one',body=None,owner='cb',path='speech'):
    req=urllib.request.Request('http://'+r.address+d['gateway_path']+'/'+path,
        data=json.dumps(body if body is not None else {'text':'Really? [gasp] It worked!', 'voice':'approved'}).encode(),
        headers={'Content-Type':'application/json','Authorization':'Bearer '+owner,'Idempotency-Key':key})
    try: response=urllib.request.urlopen(req,timeout=10)
    except urllib.error.HTTPError as e: response=e
    return response.status,response.read()

class SpeechTests(unittest.TestCase):
    def test_resident_speech_preemption_replay_and_explicit_reacquisition(self):
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,configure)
            try:
                coder=r.wait(r.acquire('athba','local-coder','low'))
                tts=r.wait(r.acquire('cb','local-tts','paramount'))
                r.wait(coder,'preempted')
                raw=r.call('cb','infer',status=400,request=dict(schema=VERSION,
                    submission_id='raw-speech',reservation_id=tts['id'],
                    generation=tts['generation'],profile_hash=tts['profile_hash'],
                    prompt='',max_tokens=1,timeout_seconds=5,
                    payload={'protocol':'speech','body':{'text':'hello','voice':'approved'}}))
                self.assertEqual(raw['error'],'use_scoped_speech_gateway')
                self.assertEqual(r.acquire('other','local-tts','paramount')['state'],'unavailable')
                self.assertEqual(speech(r,tts,owner='athba')[0],409)
                self.assertEqual(speech(r,tts,body={'text':'hello','voice':'../escape'})[0],409)
                self.assertEqual(speech(r,tts,body={'text':'hello','voice':'missing'})[0],409)
                self.assertEqual(speech(r,tts,body={'text':'x'*1001,'voice':'approved'})[0],409)
                self.assertEqual(speech(r,tts,body={'text':'hello','voice':'approved','audio_prompt_path':'/etc/passwd'})[0],409)
                status,a=speech(r,tts)
                self.assertEqual(status,200); self.assertEqual(a[:4],b'RIFF')
                self.assertEqual(speech(r,tts),(200,a))
                self.assertEqual(speech(r,tts,body={'text':'changed','voice':'approved'})[0],409)
                self.assertEqual(speech(r,tts,'two')[0],200)
                self.assertEqual(r.counts('dispatch')[tts['model']],2)
                r.release(tts); r.wait(tts,'released')
                self.assertEqual(r.inspect(coder)['state'],'preempted')
                replacement=r.wait(r.acquire('athba','local-coder','low',identity='speech-explicit-reacquire'))
                self.assertEqual(r.result(r.infer(replacement))['state'],'completed')
                self.assertEqual(speech(r,tts,'three')[0],409)
                r.release(replacement);r.wait(replacement,'released')
            finally:r.close()

    def test_one_active_and_invalid_wav_stays_uncertain(self):
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,configure)
            try:
                tts=r.wait(r.acquire('cb','local-tts','paramount'))
                r.controls('local-fun-chat',delay=1)
                with ThreadPoolExecutor() as pool:
                    first=pool.submit(speech,r,tts)
                    import time
                    deadline=time.monotonic()+3
                    while r.counts('dispatch')[tts['model']]<1 and time.monotonic()<deadline:time.sleep(.02)
                    self.assertEqual(speech(r,tts,'two')[0],429)
                    self.assertEqual(first.result()[0],200)
                r.controls('local-fun-chat',invalid_wav=True)
                self.assertEqual(speech(r,tts,'bad')[0],409)
                self.assertEqual(speech(r,tts,'bad')[0],409)
                self.assertEqual(r.counts('dispatch')[tts['model']],2)
                r.release(tts);r.wait(tts,'released')
            finally:r.close()

    def test_startup_failure_does_not_become_ready(self):
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,configure)
            try:
                r.controls('local-fun-chat',startup_delay=8)
                d=r.acquire('cb','local-tts','paramount')
                failed=r.wait(d,'released',seconds=10)
                self.assertNotEqual(failed['state'],'ready')
                self.assertEqual(speech(r,failed)[0],409)
                r.release(failed);r.wait(failed,'released')
            finally:r.close()

    def test_dispatch_wait_and_legacy_expiry_reconciliation(self):
        import fcntl
        import os
        import time
        from contextlib import ExitStack
        def settings(c):
            configure(c)
            c['limits']={'max_wait_seconds':5}
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,settings)
            try:
                tts=r.wait(r.acquire('cb','local-tts','paramount'))
                authority=r.root/'authority'
                state_path=authority/'managed.json'
                for key in ('queued','legacy'):
                    with ThreadPoolExecutor() as pool:
                        with ExitStack() as locks:
                            # Occupy dispatch slots without changing reservation ownership.
                            for index in range(4):
                                f=locks.enter_context((authority/'worker-slots'/f'dispatch-{index}').open('a'))
                                fcntl.flock(f,fcntl.LOCK_EX)
                            pending=pool.submit(speech,r,tts,key)
                            deadline=time.monotonic()+3
                            record=None
                            while time.monotonic()<deadline:
                                doc=json.loads(state_path.read_text())
                                record=next((i for i in doc['data']['invocations'].values()
                                    if i['request']['submission_id']==key),None)
                                if record:break
                                time.sleep(.02)
                            self.assertIsNotNone(record)
                            self.assertEqual(record['state'],'queued')
                            if key=='queued':
                                time.sleep(1.2)
                                current=json.loads(state_path.read_text())['data']['invocations'][record['id']]
                                self.assertEqual(current['state'],'queued')
                            else:
                                # Persist an old one-second request that expired before dispatch.
                                with (authority/'authority.lock').open('a') as lock:
                                    fcntl.flock(lock,fcntl.LOCK_EX)
                                    doc=json.loads(state_path.read_text())
                                    old=doc['data']['invocations'][record['id']]
                                    old['request']['wait_seconds']=1
                                    old['waiting_deadline']=0
                                    replacement=state_path.with_suffix('.fixture')
                                    replacement.write_text(json.dumps(doc))
                                    os.replace(replacement,state_path)
                        status,body=pending.result()
                        if key=='queued':
                            self.assertEqual(status,200,body)
                            self.assertEqual(speech(r,tts,key),(status,body))
                        else:
                            self.assertEqual(status,409,body)
                            self.assertIn(b'speech_Expired:',body)
                            self.assertEqual(speech(r,tts,key),(status,body))
                self.assertEqual(r.counts('dispatch')[tts['model']],1)
                r.release(tts);r.wait(tts,'released')
            finally:r.close()

    def test_multi_service_reservation_is_atomic_and_preempts_without_restoration(self):
        def reserve(r,owner,services,priority):
            return r.call(owner,'reserve',request=dict(acquisition_id=__import__('uuid').uuid4().hex,
                work_id='speech-reservation',services=services,priority=priority,ttl_seconds=60))
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,configure)
            try:
                coder=reserve(r,'athba',['local-coder'],'low')
                before=r.wait(coder['services']['local-coder'])
                combined=reserve(r,'cb',['local-primary','local-tts'],'paramount')
                primary=r.wait(combined['services']['local-primary'])
                tts=r.wait(combined['services']['local-tts'])
                self.assertEqual(r.wait(before,'preempted')['state'],'preempted')
                self.assertEqual(speech(r,tts,'preempt')[0],200)
                work=r.call('cb','submit_work',request=dict(reservation_id=combined['id'],
                    service='local-primary',work_id='ready-peer',payload=dict(kind='inference',
                    prompt='hello',max_tokens=8,timeout_seconds=5)))
                self.assertEqual(r.result(dict(id=work['invocation_id'],owner='cb'))['state'],'completed')
                r.call('cb','release_reservation',reservation_id=combined['id'])
                for member in combined['services'].values():r.wait(member,'released')
                self.assertEqual(r.inspect(before)['state'],'preempted')
                replacement=reserve(r,'athba',['local-coder'],'low')
                replacement=r.wait(replacement['services']['local-coder'])
                self.assertNotEqual(replacement['id'],before['id'])
                r.call('athba','release_reservation',reservation_id=replacement['reservation_id'] or replacement['id'])
                r.wait(replacement,'released')
            finally:r.close()

if __name__=='__main__':unittest.main()
