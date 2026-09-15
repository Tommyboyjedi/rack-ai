import copy
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
import requests
from support import Rack, ROOT
sys.path.insert(0,str(ROOT/'tests/media'))
from fixture import receiver, CLIENT, TOKEN, wait_for

def native_config_hash(media):
    value=json.loads((media['root']/'config.json').read_text())
    value.pop('profile',None)
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()

class SharedMediaTests(unittest.TestCase):
    def test_paramount_managed_images_bind_owner_priority_and_replay(self):
        with tempfile.TemporaryDirectory(prefix='rack-pr35-media-') as directory:
            root = Path(directory)
            def media_config(c):
                c['principals'][1]['ceiling']='paramount'
            with receiver(root/'media',configure=media_config) as media:
                def configure(c):
                    c['authority_root']=media['config']['resource_root']
                    c['devices']['gpu-4080-super']['uuid']=media['config']['media_uuid']
                    p=next(p for p in c['profiles'] if p['tag']=='comfyui')
                    p.update(tag='local-image',backend='comfyui',driver='systemd',media_mode='managed',
                        media_config=str(media['root']/'config.json'),media_config_sha256=hashlib.sha256((media['root']/'config.json').read_bytes()).hexdigest(),endpoint=media['backend'],
                        model=media['config']['profile']['checkpoint_sha256'],capabilities=['visual'],startup_seconds=10)
                    for source in c['sources']:
                        source['tags']=['local-image' if t=='comfyui' else t for t in source['tags']]
                    c['sources'].append(dict(source='director',token_sha256=hashlib.sha256(b'director').hexdigest(),
                        permitted=['paramount'],default='paramount',maximum='paramount',tags=['local-image'],qualification=False))
                r=Rack(root/'runtime',configure=configure,environment=media['environment'])
                try:
                    d=r.wait(r.acquire('director','local-image','paramount'))
                    job=json.loads((ROOT/'config/media/fixtures/job-request.json').read_text())
                    job.update(priority='paramount',reservation=dict(id=d['id'],generation=d['generation']))
                    url=media['api']+'/api/media/v1/jobs'
                    response=requests.post(url,headers=media['headers'],json=job,timeout=3)
                    self.assertEqual(response.status_code,202,response.text)
                    accepted=response.json()
                    def result():
                        value=requests.get(url+'/'+accepted['id'],headers=media['headers'],timeout=3).json()
                        return value if value['state'] in ['completed','failed','interrupted'] else None
                    completed=wait_for(result)
                    self.assertEqual(completed['state'],'completed',completed)
                    altered=copy.deepcopy(job); altered.update(submission_id='forged',idempotency_key='forged')
                    response=requests.post(url,headers={'Authorization':'Bearer '+TOKEN},json=altered,timeout=3)
                    self.assertGreaterEqual(response.status_code,400,response.text)
                    r.release(d); r.wait(d,'released')
                    replay=requests.post(url,headers=media['headers'],json=job,timeout=3)
                    self.assertEqual(replay.status_code,202,replay.text)
                    self.assertEqual(replay.json()['id'],accepted['id'])
                    count=requests.get(media['backend']+'/fixture/control',timeout=3).json()['dispatches']
                    self.assertEqual(len(count),1)
                finally:
                    r.close()

    def test_shared_interactive_restart_retains_reservation_and_tracks_new_process(self):
        from test_recovery import fault
        from test_restart import restart, service
        with tempfile.TemporaryDirectory(prefix='rack-pr35-restart-') as directory:
            root=Path(directory)
            with receiver(root/'media') as media:
                def configure(c):
                    c['authority_root']=media['config']['resource_root']
                    c['devices']['gpu-4080-super']['uuid']=media['config']['media_uuid']
                    p=next(p for p in c['profiles'] if p['tag']=='comfyui')
                    p.update(backend='comfyui',driver='systemd',media_mode='interactive',
                        media_config=str(media['root']/'config.json'),media_config_sha256=native_config_hash(media),endpoint=media['backend'],
                        model='' ,startup_seconds=15)
                    c['sources'].append(dict(source='operator',token_sha256=hashlib.sha256(b'operator').hexdigest(),
                        permitted=['paramount'],default='paramount',maximum='paramount',tags=['comfyui'],qualification=False))
                r=Rack(root/'runtime',configure=configure,environment=media['environment'])
                try:
                    d=r.wait(r.acquire('operator','comfyui','paramount'))
                    original=service(media)
                    fault(media,machine={'new_process':True})
                    restart(media)
                    def completed():
                        current=service(media)
                        assert current['state']!='recovery_required',current
                        return current if current['state']=='ready' and current['generation']['number']==2 else None
                    renewed=wait_for(completed)
                    def updated():
                        current=r.inspect(d)
                        assert current['state']!='recovery_required',current
                        return current if current['state']=='ready' and current['process']['pid']==renewed['generation']['pid'] else None
                    actual=wait_for(updated)
                    self.assertEqual(actual['generation'],d['generation'])
                    self.assertEqual(renewed['lease'],original['lease'])
                    self.assertNotEqual(actual['process']['pid'],d['process']['pid'])
                    r.release(actual); r.wait(actual,'released')
                finally:
                    r.close()

if __name__=='__main__':
    unittest.main(verbosity=2)
