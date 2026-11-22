"""Energy API ownership, bounded snapshot reading and actual labelled browser rendering."""
import json,time,os
from pathlib import Path
import requests
from fixture import receiver,TOKEN

def headers():return {"Authorization":"Bearer "+TOKEN}
def configure(c):
    root=Path(c["state_root"]);now=time.time()
    series=[{"id":"estimated-total","label":"Estimated rack total","classification":"estimated","kwh":.03,"coverage_seconds":60,"points":[{"at":now-60,"kwh":.03,"seconds":60}]},
            {"id":"GPU-fixture","label":"Fixture GPU <b>literal</b>","classification":"measured_device","kwh":.01,"coverage_seconds":60,"points":[{"at":now-60,"kwh":.01,"seconds":60}]}]
    snapshot={"schema":"rack-ai/energy/v1","collected_at":now,"started_at":now-60,"sample_seconds":30,"warnings":[],
              "methodology":{"other_components_watts":50,"psu_efficiency":.85,"calibrated":False},
              "latest":{"estimated_total_watts":180,"cpu_package_watts":35,"gpus":[{"id":"GPU-fixture","name":"Fixture GPU","watts":50}]},
              "history":{key:{"start":now-86400,"end":now,"bucket_seconds":300,"series":series} for key in ["day","week","month","year"]}}
    path=root/"energy-snapshot.json";path.write_text(json.dumps(snapshot))
    binding=root/"energy.json";binding.write_text(json.dumps({"snapshot":str(path)}));binding.chmod(0o600)
def test_energy_operator_only_and_optional(tmp_path):
    with receiver(tmp_path/"enabled",configure) as r:
        url=r["api"]+"/api/media/v1"
        assert requests.get(url+"/energy",headers=headers(),timeout=3).status_code==200
        assert requests.get(url+"/energy",headers=r["headers"],timeout=3).status_code==403
        assert requests.get(url+"/energy",timeout=3).status_code==401
        value=requests.get(url+"/status",headers=headers(),timeout=3).json()
        assert value["energy_supported"] is True
    with receiver(tmp_path/"disabled") as r:
        value=requests.get(r["api"]+"/api/media/v1/status",headers=headers(),timeout=3).json()
        assert value["energy_supported"] is False
def test_energy_stale_corrupt_missing_oversize_fail_safely(tmp_path):
    with receiver(tmp_path,configure) as r:
        path=tmp_path/"state/energy-snapshot.json";saved=json.loads(path.read_text())
        saved["collected_at"]=time.time()-180;path.write_text(json.dumps(saved))
        response=requests.get(r["api"]+"/api/media/v1/energy",headers=headers(),timeout=3)
        assert response.status_code==200 and response.json()["stale"] is True
        for content in ["not json","x"*(1024*1024+1)]:
            path.write_text(content)
            response=requests.get(r["api"]+"/api/media/v1/energy",headers=headers(),timeout=3)
            assert response.status_code==503 and str(tmp_path) not in response.text
        path.unlink()
        assert requests.get(r["api"]+"/api/media/v1/energy",headers=headers(),timeout=3).status_code==503
def test_energy_browser_measured_estimated_and_history(tmp_path):
    from playwright.sync_api import sync_playwright
    with receiver(tmp_path,configure) as r,sync_playwright() as p:
        browser=p.chromium.launch(executable_path=os.environ.get("RACK_AI_BROWSER_EXECUTABLE"),headless=True,chromium_sandbox=True,args=["--disable-gpu","--use-angle=swiftshader"])
        try:
            page=browser.new_page(extra_http_headers=headers());errors=[];posts=[]
            page.on("pageerror",lambda e:errors.append(str(e)))
            page.on("request",lambda r:posts.append(r.url) if r.method=="POST" else None)
            page.goto(r["api"])
            page.locator("#energy").wait_for(state="visible")
            page.locator("#energy-rows").get_by_text("Estimated — uncalibrated",exact=True).wait_for()
            page.locator("#energy-rows").get_by_text("Measured GPU telemetry",exact=True).wait_for()
            assert "50 W" in page.locator("#energy-assumptions").inner_text()
            assert "85%" in page.locator("#energy-assumptions").inner_text()
            assert page.locator("#energy-rows b").count()==0
            page.locator("#energy-period").select_option("year")
            assert page.locator("#energy-chart polyline").count()==2
            page.get_by_role("button",name="Refresh energy",exact=True).click()
            page.reload();page.locator("#energy-rows").get_by_text("0.0300 kWh",exact=True).wait_for()
            assert not errors and not posts
        finally:browser.close()
