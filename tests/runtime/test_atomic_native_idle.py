"""Activity on either side of an atomic native-media/model group protects its peers."""
import hashlib
import json
import sys
import tempfile
import time
import unittest
from pathlib import Path
import requests
from support import Rack, ROOT
sys.path.insert(0,str(ROOT/"tests/media"))
from fixture import receiver, wait_for

class AtomicNativeIdleTests(unittest.TestCase):
    def test_native_activity_retains_whole_atomic_group(self):
        with tempfile.TemporaryDirectory(prefix="rack-idle-shared-") as directory:
            root=Path(directory)
            def media_config(c):
                c["principals"].append(dict(id="cb",token_sha256=hashlib.sha256(b"cb").hexdigest(),
                                            operator=False))
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
                r=Rack(root/"runtime",configure=configure,environment=media["environment"])
                try:
                    group=r.call("cb","reserve",request=dict(acquisition_id="atomic",work_id="atomic",
                        services=["local-image","local-primary"],priority="paramount",ttl_seconds=60))
                    image=r.wait(group["services"]["local-image"],seconds=20)
                    primary=r.wait(group["services"]["local-primary"],seconds=20)
                    requests.post(media["backend"]+"/fixture/control",json={"render_delay":10},timeout=3).raise_for_status()
                    job=json.loads((ROOT/"config/media/fixtures/job-request.json").read_text())
                    job.update(priority="paramount",reservation=dict(id=image["id"],generation=image["generation"]))
                    headers={"Authorization":"Bearer cb"}
                    url=media["api"]+"/api/media/v1/jobs"
                    response=requests.post(url,headers=headers,json=job,timeout=3)
                    self.assertEqual(response.status_code,202,response.text)
                    accepted=response.json()
                    wait_for(lambda:requests.get(url+"/"+accepted["id"],headers=headers,timeout=3).json()["state"]=="running")
                    time.sleep(4)
                    self.assertFalse(r.inspect(primary)["released"], r.inspect(primary))
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
                    r.wait(primary,"expired",seconds=25)
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

    def test_model_activity_retains_native_peer(self):
        with tempfile.TemporaryDirectory(prefix="rack-idle-shared-") as directory:
            root=Path(directory)
            def media_config(c):
                c["principals"].append(dict(id="cb",token_sha256=hashlib.sha256(b"cb").hexdigest(),
                                            operator=False))
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
                r=Rack(root/"runtime",configure=configure,environment=media["environment"])
                try:
                    group=r.call("cb","reserve",request=dict(acquisition_id="atomic",work_id="atomic",
                        services=["local-image","local-primary"],priority="paramount",ttl_seconds=60))
                    image=r.wait(group["services"]["local-image"],seconds=20)
                    primary=r.wait(group["services"]["local-primary"],seconds=20)
                    deadline=time.monotonic()+6
                    sequence=0
                    while time.monotonic()<deadline:
                        sequence+=1
                        r.call("cb","submit_work",request=dict(reservation_id=group["id"],
                            service="local-primary",work_id=f"model-{sequence}",
                            payload=dict(kind="inference",prompt="hello",max_tokens=16,timeout_seconds=5)))
                        self.assertEqual(r.inspect(image)["state"],"ready")
                        time.sleep(.5)
                    expired_image=r.wait(image,"expired",seconds=25)
                    self.assertEqual(expired_image["reason"],"idle_timeout")
                    self.assertTrue(expired_image["released"])
                    r.wait(primary,"expired",seconds=25)
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
