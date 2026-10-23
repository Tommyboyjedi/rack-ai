"""Operator-only recovery API and browser control; all helpers are disposable."""
import hashlib,json,os
from pathlib import Path
import pytest,requests
from fixture import receiver,TOKEN
SCHEMA="rack-ai/resource-recovery/v1"
def configure(c):
 root=Path(c["state_root"]);helper=root/"resource-helper"
 helper.write_text("#!/usr/bin/python3\nimport json,sys,pathlib\np=pathlib.Path(sys.argv[2])\nwith p.with_suffix('.calls').open('a') as f:f.write(sys.argv[1]+'\\n')\nprint(p.read_text())\n")
 helper.chmod(0o700);config=root/"resource-report.json"
 config.write_text(json.dumps({"schema":SCHEMA,"resources":[{"resource":"gpu-test","services":["comfyui"],"status":"in_use_preserved"}]}));config.chmod(0o600)
 binding={"executable":str(helper),"sha256":hashlib.sha256(helper.read_bytes()).hexdigest(),"config":str(config)}
 (root/"resource-recovery.json").write_text(json.dumps(binding));(root/"resource-recovery.json").chmod(0o600)
def operator():return {"Authorization":"Bearer "+TOKEN}
def test_operator_only_and_explicit_confirmation(tmp_path):
 with receiver(tmp_path,configure) as r:
  url=r["api"]+"/api/media/v1"
  assert requests.get(url+"/resources",headers=r["headers"],timeout=3).status_code==403
  assert requests.post(url+"/resources/recover",headers=r["headers"],json={"schema":SCHEMA,"confirm":"recover_stuck_resources"},timeout=3).status_code==403
  assert requests.get(url+"/resources",timeout=3).status_code==401
  assert requests.post(url+"/resources/recover",headers=operator(),json={},timeout=3).status_code==422
  assert requests.post(url+"/resources/recover",headers=operator(),json={"schema":SCHEMA,"confirm":"recover_stuck_resources","unit":"arbitrary.service"},timeout=3).status_code==422
  before=json.loads((tmp_path/"state/state.json").read_text())
  result=requests.post(url+"/resources/recover",headers=operator(),json={"schema":SCHEMA,"confirm":"recover_stuck_resources"},timeout=3)
  assert result.status_code==200,result.text
  assert result.json()["resources"][0]["status"]=="in_use_preserved"
  assert json.loads((tmp_path/"state/state.json").read_text())==before
  assert (tmp_path/"state/resource-recovery-last.json").is_file()
def test_helper_hash_mismatch_and_unconfigured_are_fail_closed(tmp_path):
 with receiver(tmp_path/"configured",configure) as r:
  helper=Path(r["config"]["state_root"])/"resource-helper";helper.write_text(helper.read_text()+"# changed\n")
  response=requests.get(r["api"]+"/api/media/v1/resources",headers=operator(),timeout=3)
  assert response.status_code==503
  assert "helper" not in response.text and str(tmp_path) not in response.text
 with receiver(tmp_path/"ordinary") as r:
  response=requests.get(r["api"]+"/api/media/v1/status",headers=operator(),timeout=3)
  assert response.json()["operator"] is True and response.json()["resource_recovery_supported"] is False
  assert requests.get(r["api"]+"/api/media/v1/resources",headers=operator(),timeout=3).status_code==503
def configure_warm(c):
 configure(c)
 p=Path(c["state_root"])/"resource-report.json";report=json.loads(p.read_text())
 report["resources"][0].update(services=["local-coder","local-tts"],status="warm_preserved",preserved_service="local-tts")
 p.write_text(json.dumps(report))
@pytest.mark.parametrize("warm",[False,True])
def test_browser_recovery_confirmation_and_results(tmp_path,warm):
 from playwright.sync_api import sync_playwright
 with receiver(tmp_path,configure_warm if warm else configure) as r, sync_playwright() as p:
  launch={"headless":True,"chromium_sandbox":True,"args":["--disable-gpu","--use-angle=swiftshader"]}
  executable=os.environ.get("RACK_AI_BROWSER_EXECUTABLE")
  if executable:launch["executable_path"]=executable
  browser=p.chromium.launch(**launch)
  try:
   page=browser.new_page(extra_http_headers=operator());page.goto(r["api"])
   page.locator("#resources").wait_for(state="visible")
   expected="Warm model (local-tts) — preserved" if warm else "In use — preserved"
   page.locator("#resource-list").get_by_text(expected,exact=False).wait_for()
   page.once("dialog",lambda d:d.dismiss());page.get_by_role("button",name="Reset stuck resources").click()
   calls=tmp_path/"state/resource-report.calls";assert "recover" not in calls.read_text()
   page.once("dialog",lambda d:d.accept());page.get_by_role("button",name="Reset stuck resources").click()
   page.get_by_text("Recovery checks completed.",exact=False).wait_for()
   assert calls.read_text().splitlines().count("recover")==1
   assert page.locator("#resource-list").inner_text().endswith(expected)
  finally:browser.close()

def test_browser_cookie_origin_is_required_for_recovery(tmp_path):
 from test_passwords import sign_in
 with receiver(tmp_path,configure) as r:
  session,response=sign_in(r)
  assert response.status_code==303
  url=r["api"]+"/api/media/v1/resources/recover"
  payload={"schema":SCHEMA,"confirm":"recover_stuck_resources"}
  for origin in [None,"https://other.invalid"]:
   headers={"Origin":origin} if origin else {}
   assert session.post(url,headers=headers,json=payload,timeout=3).status_code==403
  assert session.post(url,headers={"Origin":r["api"]},json=payload,timeout=3).status_code==200
