"""CB uses two independent logical grants; native admitted work drains before idle cleanup."""
import hashlib
import json
import sys
import tempfile
import unittest
from pathlib import Path
import requests
from support import Rack, ROOT
sys.path.insert(0,str(ROOT/"tests/media"))
from fixture import receiver, wait_for

class SharedIdleTests(unittest.TestCase):
    def test_cb_image_work_keeps_only_its_resource_active(self):
        with tempfile.TemporaryDirectory(prefix="rack-idle-shared-") as directory:
            root=Path(directory)
            def media_config(c):
                c["principals"].append(dict(id="cb",token_sha256=hashlib.sha256(b"cb").hexdigest(),
                                            ceiling="paramount",operator=False))
            with receiver(root/"media",configure=media_config) as media:
                def configure(c):
                    c["idle_timeout_seconds"]=3
                    c["authority_root"]=media["config"]["resource_root"]
                    c["devices"]["gpu-4080-super"]["uuid"]=media["config"]["media_uuid"]
                    p=next(p for p in c["profiles"] if p["tag"]=="comfyui")
                    p.update(tag="local-image",backend="comfyui",driver="systemd",media_mode="managed",
                        media_config=str(media["root"]/"config.json"),
                        media_config_sha256=hashlib.sha256((media["root"]/"config.json").read_bytes()).hexdigest(),
                        endpoint=media["backend"],model=media["config"]["profile"]["checkpoint_sha256"],
                        capabilities=["visual"],startup_seconds=15)
                    for source in c["sources"]:
                        source["tags"]=["local-image" if t=="comfyui" else t for t in source["tags"]]
                    next(s for s in c["sources"] if s["source"]=="cb")["tags"]=["local-primary","local-image"]
                r=Rack(root/"runtime",configure=configure,environment=media["environment"])
                try:
                    image=r.wait(r.acquire("cb","local-image","paramount"),seconds=20)
                    primary=r.wait(r.acquire("cb","local-primary","paramount"))
                    requests.post(media["backend"]+"/fixture/control",json={"render_delay":10},timeout=3).raise_for_status()
                    job=json.loads((ROOT/"config/media/fixtures/job-request.json").read_text())
                    job.update(priority="paramount",reservation=dict(id=image["id"],generation=image["generation"]))
                    headers={"Authorization":"Bearer cb"}
                    url=media["api"]+"/api/media/v1/jobs"
                    response=requests.post(url,headers=headers,json=job,timeout=3)
                    self.assertEqual(response.status_code,202,response.text)
                    accepted=response.json()
                    wait_for(lambda:requests.get(url+"/"+accepted["id"],headers=headers,timeout=3).json()["state"]=="running")
                    expired=r.wait(primary,"expired")
                    self.assertEqual(expired["reason"],"idle_timeout")
                    self.assertFalse(r.inspect(image)["released"], r.inspect(image))
                    def result():
                        value=requests.get(url+"/"+accepted["id"],headers=headers,timeout=3).json()
                        return value if value["state"] in ["completed","failed","interrupted"] else None
                    completed=wait_for(result)
                    self.assertEqual(completed["state"],"completed",completed)
                    current=r.inspect(image)
                    self.assertIsNotNone(current["last_activity_at"])
                    expired_image=r.wait(image,"expired",seconds=25)
                    self.assertEqual(expired_image["reason"],"idle_timeout")
                    self.assertTrue(expired_image["released"])
                    state=json.loads((Path(r.config["authority_root"])/"managed.json").read_text())
                    self.assertEqual(state["claims"],{})
                    other=r.acquire("athba","local-primary","medium")
                    self.assertEqual(other["state"],"preparing")
                    r.release(other)
                except BaseException:
                    import shutil
                    shutil.copytree(root,ROOT/"evidence/idle-reservations-20260915"/root.name)
                    raise
                finally:r.close()
