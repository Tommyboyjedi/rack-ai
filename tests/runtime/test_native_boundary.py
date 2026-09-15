import copy
import hashlib
import json
import sys
from pathlib import Path
import requests
from support import Rack, ROOT
sys.path.insert(0,str(ROOT/"tests/media"))
from fixture import receiver, wait_for
from test_shared_media import native_config_hash

def test_cb_native_owns_gpu_without_image_identity(tmp_path):
    def media_config(c):
        Path(c["profile"]["checkpoint"]).unlink()
        c["principals"].append(dict(id="cb",token_sha256=hashlib.sha256(b"cb").hexdigest(),ceiling="paramount",operator=False))
    with receiver(tmp_path/"media",configure=media_config) as media:
        def configure(c):
            c["authority_root"]=media["config"]["resource_root"]
            c["devices"]["gpu-4080-super"]["uuid"]=media["config"]["media_uuid"]
            p=next(p for p in c["profiles"] if p["tag"]=="comfyui")
            p.update(backend="comfyui",driver="systemd",media_mode="interactive",model="",media_config=str(media["root"]/"config.json"),media_config_sha256=native_config_hash(media),endpoint=media["backend"],startup_seconds=15)
            image=copy.deepcopy(p);image.update(tag="local-image",media_mode="managed",model=media["config"]["profile"]["checkpoint_sha256"],media_config_sha256=hashlib.sha256((media["root"]/"config.json").read_bytes()).hexdigest());c["profiles"].append(image)
            next(s for s in c["sources"] if s["source"]=="cb")["tags"]=["comfyui","local-image"]
        r=Rack(tmp_path/"runtime",configure=configure,environment=media["environment"])
        try:
            native=r.wait(r.acquire("cb","comfyui","paramount"),seconds=20)
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
        finally:r.close()
