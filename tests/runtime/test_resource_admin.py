"""Dead-unit recovery through the real local admin helper and disposable machines."""
import hashlib,json,subprocess,sys
from pathlib import Path
import pytest
from support import Rack,ROOT
sys.path.insert(0,str(ROOT/"tests/media"))
from fixture import receiver
def bind_media(c,m):
 c["authority_root"]=m["config"]["resource_root"]
 c["devices"]["gpu-4080-super"]["uuid"]=m["config"]["media_uuid"]
 p=next(p for p in c["profiles"] if p["tag"]=="comfyui")
 p.update(backend="comfyui",driver="systemd",media_mode="managed",
  media_config=str(m["root"]/"config.json"),
  media_config_sha256=hashlib.sha256((m["root"]/"config.json").read_bytes()).hexdigest(),
  endpoint=m["backend"],model=m["config"]["profile"]["checkpoint_sha256"],capabilities=["visual"])
def machine(m,**fields):
 path=m["root"]/"machine.json";s=json.loads(path.read_text());s.update(fields);path.write_text(json.dumps(s))
def admin(r,operation="recover"):
 result=subprocess.run([str(ROOT/"target/debug/rack_ai_resource_admin"),operation,str(r.root/"config.json")],
  env=r.environment,capture_output=True,text=True,timeout=15,check=True)
 return {x["resource"]:x["status"] for x in json.loads(result.stdout)["resources"]}
def test_dead_failed_unit_reset_preserves_healthy_owner(tmp_path):
 with receiver(tmp_path/"media") as m:
  r=Rack(tmp_path/"runtime",configure=lambda c:bind_media(c,m),environment=m["environment"])
  try:
   healthy=r.wait(r.acquire("cb","local-primary","medium"))
   machine(m,unit_failed=True,failed_invocation="old-failed-generation")
   assert admin(r,"inspect")["gpu-4080-super"]=="reset_available"
   report=admin(r);assert report["gpu-4080-super"]=="reset_complete"
   assert report["gpu-4060ti"]=="in_use_preserved"
   assert r.inspect(healthy)["generation"]==healthy["generation"]
   state=json.loads((m["root"]/"machine.json").read_text())
   assert state["mutations"]==[["--user","reset-failed","rack-ai-comfyui-fixture.service"]]
   assert admin(r)["gpu-4080-super"]=="available"
  finally:r.close()
@pytest.mark.parametrize("fields",[{"foreign":True},{"reported_pid":123},{"job":"12 start"},{"gpu_probe_error":True}])
def test_live_or_unproven_effect_is_never_reset(tmp_path,fields):
 with receiver(tmp_path/"media") as m:
  r=Rack(tmp_path/"runtime",configure=lambda c:bind_media(c,m),environment=m["environment"])
  try:
   machine(m,unit_failed=True,failed_invocation="old-failed-generation",**fields)
   assert admin(r)["gpu-4080-super"].startswith("blocked_")
   assert not json.loads((m["root"]/"machine.json").read_text()).get("mutations")
  finally:r.close()
def test_unresolved_media_session_and_changed_config_are_preserved(tmp_path):
 with receiver(tmp_path/"media") as m:
  r=Rack(tmp_path/"runtime",configure=lambda c:bind_media(c,m),environment=m["environment"])
  try:
   machine(m,unit_failed=True,failed_invocation="old-failed-generation")
   path=m["root"]/"state/state.json";state=json.loads(path.read_text())
   state["service"].update(state="recovery_required",mode="interactive");path.write_text(json.dumps(state))
   assert admin(r)["gpu-4080-super"]=="media_session_preserved"
   config=m["root"]/"config.json";data=json.loads(config.read_text());data["min_disk_mb"]+=1;config.write_text(json.dumps(data))
   assert admin(r)["gpu-4080-super"]=="blocked_physical_verification"
   assert not json.loads((m["root"]/"machine.json").read_text()).get("mutations")
  finally:r.close()

def test_identity_free_orphaned_quarantine_is_cleared_after_absence_checks(tmp_path):
 with receiver(tmp_path/"media") as m:
  r=Rack(tmp_path/"runtime",configure=lambda c:bind_media(c,m),environment=m["environment"])
  try:
   path=m["root"]/"state/state.json";state=json.loads(path.read_text())
   state["service"].update(state="recovery_required",mode="closed",error="managed resource no longer owned")
   path.write_text(json.dumps(state))
   assert admin(r,"inspect")["gpu-4080-super"]=="reset_available"
   assert json.loads(path.read_text())["service"]["state"]=="recovery_required"
   assert admin(r)["gpu-4080-super"]=="reset_complete"
   assert json.loads(path.read_text())["service"]["state"]=="stopped"
   assert not json.loads((m["root"]/"machine.json").read_text()).get("mutations")
  finally:r.close()

@pytest.mark.parametrize("blocker",["legacy_lease","released_but_unfinished_session"])
def test_legacy_and_pending_session_fences_survive_reset(tmp_path,blocker):
 with receiver(tmp_path/"media") as m:
  r=Rack(tmp_path/"runtime",configure=lambda c:bind_media(c,m),environment=m["environment"])
  try:
   machine(m,unit_failed=True,failed_invocation="old-failed-generation")
   if blocker=="legacy_lease":
    leases=Path(m["config"]["resource_root"])/"leases";leases.mkdir(exist_ok=True)
    (leases/"gpu-4080-super.json").write_text(json.dumps({"owner":"foreign"}))
    assert admin(r)["gpu-4080-super"]=="blocked_live_or_unproven_process"
   else:
    path=m["root"]/"state/state.json";state=json.loads(path.read_text())
    state["sessions"].append({"id":"pending-session","owner":"operator",
     "request":{"schema":"rack-ai/media/v1","idempotency_key":"pending"},
     "created_at":0,"stopped":False,"release_requested":True,"terminal_reason":None})
    path.write_text(json.dumps(state))
    assert admin(r)["gpu-4080-super"]=="media_session_preserved"
   assert not json.loads((m["root"]/"machine.json").read_text()).get("mutations")
  finally:r.close()
