import hashlib
"""Production HTTP/authority/lifecycle acceptance with disposable hosting processes."""
import json
import sys
from pathlib import Path
from support import Rack
import requests
import os
import time
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/"media"))
from fixture import receiver, TOKEN, wait_for

def native_config_hash(media):
    value=json.loads((media['root']/'config.json').read_text())
    value.pop('profile',None)
    return hashlib.sha256(json.dumps(value,sort_keys=True,separators=(',',':'),ensure_ascii=False).encode()).hexdigest()

def scenario(root, media):
    def configure(config):
        config["authority_root"] = media["config"]["resource_root"]
        config["devices"]["gpu-4080-super"]["uuid"] = media["config"]["media_uuid"]
        p = next(p for p in config["profiles"] if p["tag"]=="comfyui")
        p.update(backend="comfyui",driver="systemd",media_config=str(media["root"]/"config.json"),media_config_sha256=native_config_hash(media),
            model="",endpoint=media["backend"],
            capabilities=["visual"],startup_seconds=10)
    rack = Rack(root,configure=configure,environment=media["environment"])
    try:
        primary = rack.wait(rack.acquire('athba','local-primary','low'))
        coder = rack.wait(rack.acquire('athba','local-coder','low'))
        rack.result(rack.infer(primary)); rack.result(rack.infer(coder))
        comfy = rack.wait(rack.acquire('comfy','comfyui','paramount'))
        rack.result(rack.infer(primary)); rack.result(rack.infer(coder)); use_comfy(media)
        chat = rack.wait(rack.acquire('cb','local-fun-chat','paramount'))
        held = rack.wait(primary,'held')
        pending = rack.infer(held)
        assert rack.inspect(coder)['generation']==coder['generation']
        rack.result(rack.infer(coder)); use_comfy(media); rack.result(rack.infer(chat))
        before = rack.counts('stop')
        denied = rack.acquire('other','big-brain','high')
        assert denied['state']=='denied' and denied['reason'].startswith('incumbent_priority:'), denied
        assert before==rack.counts('stop')
        tie = rack.acquire('other','local-fun-chat','paramount')
        assert tie['state']=='denied'
        assert rack.inspect(coder)['generation']==coder['generation']
        rack.release(chat)
        restored = rack.wait(primary)
        assert restored['generation']!=primary['generation']
        rack.result(pending)
        assert rack.counts('start')['local-primary']==2
        assert rack.counts('stop')['local-coder']==0
        assert not any('stop' in m for m in json.loads((media['root']/'machine.json').read_text()).get('mutations',[]))
        rack.release(comfy); rack.wait(comfy,'released')
        large = rack.wait(rack.acquire('other','big-brain','medium'))
        rack.wait(primary,'held'); rack.wait(coder,'held')
        assert len(large['resources'])==3
        rack.result(rack.infer(large))
        comfy2 = rack.wait(rack.acquire('comfy','comfyui','paramount'))
        rack.wait(large,'held')
        assert rack.counts('stop')['big-brain']==1
        use_comfy(media)
        rack.wait(primary); rack.wait(coder)
        media_dispatches=len(requests.get(media['backend']+'/fixture/control',timeout=3).json()['dispatches'])
        assert media_dispatches==3
        mutations = json.loads((media['root']/'machine.json').read_text())['mutations']
        assert sum('start' in m for m in mutations)==2, mutations
        assert sum('stop' in m for m in mutations)>=1, mutations
        print(json.dumps(dict(PRIORITY_SCENARIO_PASSED=True,media_dispatches=media_dispatches,media_lifecycle_commands=mutations, starts=rack.counts('start'),stops=rack.counts('stop'),dispatches=rack.counts('dispatch'))))
    finally:
        rack.close()

def use_comfy(media):
    identity = os.urandom(8).hex()
    workflow = {"5":{"inputs":{"width":64,"height":64}}, "9":{"inputs":{"filename_prefix":identity}}}
    response = requests.post(media['native']+'/prompt',headers={'Authorization':'Bearer '+TOKEN},
        json={'prompt_id':identity,'prompt':workflow},timeout=3)
    assert response.status_code==200, (response.status_code,response.text)
    def completed():
        response = requests.get(media['native']+'/history/'+identity,headers={'Authorization':'Bearer '+TOKEN},timeout=3)
        return response.status_code==200 and identity in response.json()
    wait_for(completed)

if __name__=='__main__':
    root = Path(sys.argv[1]).resolve()
    def media_config(config):
        config['principals'][0]['id'] = 'comfy'
    with receiver(root/'media',configure=media_config) as media:
        scenario(root/'runtime',media)
