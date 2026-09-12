import json
import time
import uuid
import requests
import pytest
from fixture import receiver, wait_for, TOKEN
from test_http import request, completed

def status(env):
    return requests.get(env["api"]+"/api/media/v1/status", headers=env["headers"], timeout=3).json()

def fault(env, **values):
    requests.post(env["backend"]+"/fixture/control", json=values, timeout=3).raise_for_status()

def submit(env):
    return requests.post(env["api"]+"/api/media/v1/jobs", json=request(), headers=env["headers"], timeout=3).json()

@pytest.mark.parametrize("change", ["wrong_uuid", "duplicate_protected", "overlap", "missing_protected"])
def test_invalid_placement_never_starts(tmp_path, change):
    def configure(c):
        if change == "wrong_uuid": c["media_uuid"] = "GPU-" + "0"*36
        elif change == "duplicate_protected": c["protected"][1]["uuid"] = c["protected"][0]["uuid"]
        elif change == "overlap": c["protected"][0]["uuid"] = c["media_uuid"]
        else: c["protected"] = c["protected"][:1]
    with receiver(tmp_path/"machine", configure) as env:
        submit(env)
        wait_for(lambda: status(env)["state"] == "recovery_required")
        machine = json.loads((env["root"]/"machine.json").read_text())
        assert not machine["active"] and not machine.get("mutations")
        assert not list((env["root"]/"resources/leases").glob("*.json"))

def test_foreign_gpu_process_waits_without_effects(tmp_path):
    with receiver(tmp_path/"machine") as env:
        fault(env, machine={"foreign": True})
        submit(env)
        wait_for(lambda: status(env)["state"] == "waiting")
        assert not json.loads((env["root"]/"machine.json").read_text()).get("mutations")

@pytest.mark.parametrize("effect", ["start_ignored", "stop_ignored"])
def test_lifecycle_deadline_retains_reservation(tmp_path, effect):
    with receiver(tmp_path/"machine", lambda c: c.update(start_timeout=3, stop_timeout=3)) as env:
        fault(env, machine={effect: True})
        submit(env)
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert list((env["root"]/"resources/leases").glob("*.json"))

def test_receiver_restart_reconciles_known_prompt_without_redispatch(tmp_path):
    with receiver(tmp_path/"machine") as env:
        fault(env, render_delay=8)
        job = submit(env)
        wait_for(lambda: requests.get(env["backend"]+"/fixture/control", timeout=3).json()["dispatches"])
        env["restart"]()
        outcome = wait_for(lambda: completed(env, job["id"]))
        assert outcome["state"] == "completed", outcome
        assert len(requests.get(env["backend"]+"/fixture/control", timeout=3).json()["dispatches"]) == 1
        wait_for(lambda: status(env)["state"] == "stopped")

def test_changed_invocation_quarantines_without_stopping_foreign_service(tmp_path):
    with receiver(tmp_path/"machine") as env:
        fault(env, render_delay=20)
        submit(env)
        wait_for(lambda: status(env)["state"] == "ready")
        fault(env, machine={"invocation":uuid.uuid4().hex})
        wait_for(lambda: status(env)["state"] == "recovery_required")
        machine = json.loads((env["root"]/"machine.json").read_text())
        assert machine["active"]
        assert all("stop" not in args for args in machine["mutations"])
        assert list((env["root"]/"resources/leases").glob("*.json"))

def test_cancel_during_render_never_publishes_late_image(tmp_path):
    with receiver(tmp_path/"machine") as env:
        fault(env, render_delay=8)
        job = submit(env)
        wait_for(lambda: requests.get(env["backend"]+"/fixture/control", timeout=3).json()["dispatches"])
        response = requests.post(env["api"]+job["location"]+"/cancel", json={}, headers=env["headers"], timeout=3)
        assert response.status_code == 202
        wait_for(lambda: status(env)["state"] == "stopped")
        outcome = completed(env, job["id"])
        assert outcome["state"] == "cancelled" and not outcome["artifacts"]

def test_drain_deadline_keeps_accepted_native_work_owned(tmp_path):
    with receiver(tmp_path/"machine", lambda c: c.update(drain_timeout=3)) as env:
        headers = {"Authorization":"Bearer "+TOKEN}
        session = requests.post(env["api"]+"/api/media/v1/sessions",
            json={"schema":"rack-ai/media/v1","idempotency_key":str(uuid.uuid4())}, headers=headers, timeout=3).json()
        wait_for(lambda: status(env)["state"] == "ready")
        fault(env, render_delay=30)
        # The native HTTP proxy injects the private gate credential, like its browser path.
        managed = request()
        workflow = {"5":{"inputs":{"width":64,"height":64}},"9":{"inputs":{"filename_prefix":"native-proof/image"}}}
        requests.post(env["native"]+"/prompt", json={"prompt_id":str(uuid.uuid4()),"prompt":workflow},
                      headers=headers, timeout=3).raise_for_status()
        requests.post(env["api"]+session["location"]+"/release", json={}, headers=headers, timeout=3).raise_for_status()
        wait_for(lambda: status(env)["state"] == "recovery_required")
        machine = json.loads((env["root"]/"machine.json").read_text())
        assert machine["active"]
        assert all("stop" not in args for args in machine["mutations"])


def test_persistence_failure_never_acknowledges_job(tmp_path):
    with receiver(tmp_path/"machine") as env:
        directory = env["root"]/"state"
        directory.chmod(0o500)
        try:
            response = requests.post(env["api"]+"/api/media/v1/jobs", json=request(), headers=env["headers"], timeout=3)
            assert response.status_code != 202
            assert not json.loads((directory/"state.json").read_text())["jobs"]
            assert not json.loads((env["root"]/"machine.json").read_text())["active"]
        finally:
            directory.chmod(0o700)


def test_corrupt_state_is_preserved_and_startup_fails_closed(tmp_path):
    import subprocess
    from fixture import REPO
    with receiver(tmp_path/"machine") as env:
        env["process"].kill()
        env["process"].wait(timeout=5)
        path = env["root"]/"state/state.json"
        path.write_text("{broken")
        result = subprocess.run([str(REPO/"target/debug/rack_ai_media"),str(env["root"]/"config.json")],
            env=env["environment"],capture_output=True,timeout=5)
        assert result.returncode != 0 and path.read_text() == "{broken"
        assert not json.loads((env["root"]/"machine.json").read_text())["active"]
