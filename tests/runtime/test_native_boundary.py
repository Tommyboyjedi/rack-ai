import copy
import hashlib
import json
import sys
import time
from pathlib import Path
import requests
from support import Rack, ROOT
sys.path.insert(0,str(ROOT/"tests/media"))
from fixture import receiver, wait_for
from test_shared_media import native_config_hash

def test_cb_native_owns_gpu_without_image_identity(tmp_path):
    def media_config(c):
        Path(c["profile"]["checkpoint"]).unlink()
        c["principals"].append(dict(id="cb",token_sha256=hashlib.sha256(b"cb").hexdigest(),operator=False))
    with receiver(tmp_path/"media",configure=media_config) as media:
        def configure(c):
            c["authority_root"]=media["config"]["resource_root"]
            c["devices"]["gpu-4080-super"]["uuid"]=media["config"]["media_uuid"]
            p=next(p for p in c["profiles"] if p["tag"]=="comfyui")
            p.update(backend="comfyui",driver="systemd",media_mode="interactive",model="",media_config=str(media["root"]/"config.json"),media_config_sha256=native_config_hash(media),endpoint=media["backend"],startup_seconds=15)
            image=copy.deepcopy(p);image.update(tag="local-image",media_mode="managed",model=media["config"]["profile"]["checkpoint_sha256"],media_config_sha256=hashlib.sha256((media["root"]/"config.json").read_bytes()).hexdigest());c["profiles"].append(image)
        r=Rack(tmp_path/"runtime",configure=configure,environment=media["environment"])
        try:
            native=r.wait(r.acquire("cb","comfyui","paramount"),seconds=20)
            assert "gateway_path" not in native
            assert native["access"]["kind"]=="native_comfyui"
            assert native["access"]["url"]==media["native"]
            info=requests.get(native["access"]["url"]+"/object_info",headers={"Authorization":"Bearer cb"},timeout=3)
            assert info.status_code==200,info.text
            assert "model" not in native
            saved=json.loads((Path(r.config["authority_root"])/"managed.json").read_text())["data"]["demands"][native["id"]]
            assert "model" not in saved["profile"]
            assert r.acquire("cb","local-image","paramount")["state"]=="denied"
            # Changes to only the optional recipe cannot invalidate an owned native session.
            f=media["root"]/"config.json";c=json.loads(f.read_text());c["profile"]["checkpoint_sha256"]="f"*64;f.write_text(json.dumps(c))
            reply=requests.get(media["native"]+"/queue",headers={"Authorization":"Bearer cb"},timeout=3)
            assert reply.status_code==200,reply.text
            r.release(native);r.wait(native,"released")
            assert not json.loads((Path(r.config["authority_root"])/"managed.json").read_text())["claims"]
            group=r.call('cb','reserve',request=dict(acquisition_id='native-group',work_id='combined',
                services=['local-primary','comfyui'],priority='medium',ttl_seconds=60))
            def group_wait(expected):
                end=time.monotonic()+15
                while time.monotonic()<end:
                    current=r.call('cb','inspect_reservation',reservation_id=group['id'])
                    if current['state']==expected:return current
                    time.sleep(.05)
                raise AssertionError(current)
            combined=group_wait('ready')
            url=combined['services']['comfyui']['access']['url']
            assert url==media['native']
            assert requests.get(url+'/object_info',headers={'Authorization':'Bearer cb'},timeout=3).status_code==200
            contender=r.wait(r.acquire('other','local-fun-chat','paramount'))
            partial=group_wait('partial')
            assert partial['services']['comfyui']['state']=='ready'
            assert requests.get(url+'/object_info',headers={'Authorization':'Bearer cb'},timeout=3).status_code==200
            r.release(contender);group_wait('ready')
            assert requests.get(url+'/object_info',headers={'Authorization':'Bearer cb'},timeout=3).status_code==200
            r.call('cb','release_reservation',reservation_id=group['id']);group_wait('released')

        finally:r.close()
