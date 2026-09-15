import hashlib
import json
import tempfile
import unittest
import urllib.request
import urllib.error
from pathlib import Path
from concurrent.futures import ThreadPoolExecutor
from support import Rack, ROOT

def configure(c):
    p=c['profiles'][2]
    p.update(tag='local-tts', model='ResembleAI/chatterbox-turbo', backend='chatterbox',
        capabilities=['audio'], protocols=['speech'], resources=['gpu-2060'], device_mib={'gpu-2060':16})
    p['args'][0]=str(ROOT/'tests/runtime/speech_backend.py')
    p['args'][2]=p['model']
    p['artifact']=p['args'][0]
    p['artifact_sha256']=hashlib.sha256(Path(p['artifact']).read_bytes()).hexdigest()
    for s in c['sources']:
        s['tags'].remove('local-fun-chat')
        if s['source'] in ('cb','other'): s['tags'].append('local-tts')

def speech(r,d,key='one',body=None,owner='cb',path='speech'):
    req=urllib.request.Request('http://'+r.address+d['gateway_path']+'/'+path,
        data=json.dumps(body if body is not None else {'text':'Really? [gasp] It worked!', 'voice':'approved'}).encode(),
        headers={'Content-Type':'application/json','Authorization':'Bearer '+owner,'Idempotency-Key':key})
    try: response=urllib.request.urlopen(req,timeout=10)
    except urllib.error.HTTPError as e: response=e
    return response.status,response.read()

class SpeechTests(unittest.TestCase):
    def test_resident_speech_preemption_replay_and_restoration(self):
        with tempfile.TemporaryDirectory() as tmp:
            r=Rack(tmp,configure)
            try:
                coder=r.wait(r.acquire('athba','local-coder','low'))
                tts=r.wait(r.acquire('cb','local-tts','paramount'))
                r.wait(coder,'held')
                self.assertEqual(r.acquire('other','local-tts','paramount')['state'],'denied')
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
                self.assertEqual(r.counts('start')[tts['model']],1)
                r.release(tts); r.wait(tts,'released')
                restored=r.wait(coder)
                self.assertNotEqual(restored['generation'],coder['generation'])
                self.assertEqual(r.result(r.infer(restored))['state'],'completed')
                self.assertEqual(speech(r,tts,'three')[0],409)
                r.release(restored);r.wait(restored,'released')
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

if __name__=='__main__':unittest.main()
