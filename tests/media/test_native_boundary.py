"""Optional image recipes cannot gate native ComfyUI ownership."""
import json
from pathlib import Path
import pytest
import requests
from fixture import receiver, wait_for, TOKEN, REPO

@pytest.mark.parametrize("damage", ["missing", "changed"])
def test_native_starts_with_invalid_optional_checkpoint(tmp_path, damage):
    def configure(c):
        p=Path(c["profile"]["checkpoint"])
        if damage=="missing":p.unlink()
        else:p.write_bytes(b"changed checkpoint")
    with receiver(tmp_path/"media",configure=configure) as env:
        api=env["api"]+"/api/media/v1";h={"Authorization":"Bearer "+TOKEN}
        s=requests.post(api+"/sessions",headers=h,json={"schema":"rack-ai/media/v1","idempotency_key":"native"},timeout=3).json()
        wait_for(lambda:requests.get(api+"/sessions/"+s["id"],headers=h,timeout=3).json()["state"]=="ready")
        leases=list((env["root"]/"resources/leases").glob("*.json"))
        assert leases
        for p in leases:assert json.loads(p.read_text())["model_ids"]==[]
        workflow={"5":{"inputs":{"width":64,"height":64}},"4":{"class_type":"CheckpointLoaderSimple","inputs":{"ckpt_name":"other.safetensors"}},"9":{"inputs":{"filename_prefix":"native-other"}}}
        reply=requests.post(env["native"]+"/prompt",headers=h,json={"prompt_id":"native-other","prompt":workflow},timeout=3)
        assert reply.status_code==200,reply.text
        wait_for(lambda:"native-other" in requests.get(env["native"]+"/history/native-other",headers=h,timeout=3).json())
        requests.post(api+"/sessions/"+s["id"]+"/release",headers=h,json={},timeout=3).raise_for_status()
        wait_for(lambda:requests.get(api+"/sessions/"+s["id"],headers=h,timeout=3).json()["state"]=="stopped")
        job=json.loads((REPO/"config/media/fixtures/job-request.json").read_text())
        accepted=requests.post(api+"/jobs",headers=env["headers"],json=job,timeout=3)
        assert accepted.status_code==202,accepted.text
        j=wait_for(lambda:(v if (v:=requests.get(api+"/jobs/"+accepted.json()["id"],headers=env["headers"],timeout=3).json())["state"]=="failed" else None))
        assert "checkpoint" in j["error"],j
        assert len(requests.get(env["backend"]+"/fixture/control",timeout=3).json()["dispatches"])==1
