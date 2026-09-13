"""Normal native Restart is a durable, leased, Rack-supervised generation change."""
import json
import time
import uuid
import requests
import pytest
from fixture import receiver, wait_for, TOKEN
from test_recovery import fault, status, submit

OWNER = {"Authorization": "Bearer " + TOKEN}

def service(env):
    return json.loads((env["root"] / "state/state.json").read_text())["service"]

def start(env):
    reply = requests.post(env["api"] + "/api/media/v1/sessions",
        headers=OWNER, json={"schema": "rack-ai/media/v1", "idempotency_key": str(uuid.uuid4())}, timeout=5)
    assert reply.status_code == 202, reply.text
    wait_for(lambda: status(env)["state"] == "ready")
    return reply.json(), service(env)

def restart(env):
    response = requests.post(env["native"] + "/v2/manager/reboot", headers=OWNER, timeout=12)
    assert response.status_code == 202, response.text
    return response.json()

def finish(env, session):
    response = requests.post(env["api"] + session["location"] + "/release", json={}, headers=OWNER, timeout=12)
    assert response.status_code == 202, response.text
    wait_for(lambda: status(env)["state"] == "stopped")
    assert not list((env["root"] / "resources/leases").glob("*.json"))

def machine(env):
    return json.loads((env["root"] / "machine.json").read_text())

def phase(env, expected):
    current = service(env)
    return current if current.get("restart", {}).get("phase") == expected else None

def test_normal_and_repeated_interactive_restart_preserve_session_and_lease(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, original = start(env)
        lease_path = env["root"] / "resources/leases/gpu-4080-super.json"
        lease_bytes = lease_path.read_bytes()
        fault(env, machine={"new_process": True})
        previous = original
        for number in [2, 3]:
            accepted = restart(env)
            repeated = restart(env)
            assert repeated == accepted
            waiting = requests.get(env["native"] + "/", headers=OWNER, timeout=3)
            assert waiting.status_code == 503 and "Restarting" in waiting.text
            seen = []
            def ready():
                value = service(env); seen.append(value["state"])
                assert value["activation"] == original["activation"]
                assert value["session"] == original["session"]
                assert value["lease"] == original["lease"]
                assert lease_path.read_bytes() == lease_bytes
                return value if value["state"] == "ready" else None
            current = wait_for(ready)
            assert "recovery_required" not in seen
            assert current["generation"]["number"] == number
            assert current["generation"]["pid"] != previous["generation"]["pid"]
            assert current["invocation"] != previous["invocation"]
            assert requests.get(env["native"] + "/", headers=OWNER, timeout=3).status_code == 200
            previous = current
        assert sum("start" in command for command in machine(env)["mutations"]) == 3
        finish(env, session)

@pytest.mark.parametrize("replacement", ["foreign", "wrong_runtime", "wrong_gpu", "wrong_unit", "invalid_gate", "definition_runtime"])
def test_foreign_replacement_rejected_without_stopping_it(tmp_path, replacement):
    with receiver(tmp_path / "machine") as env:
        _, original = start(env)
        fault(env, machine={"new_process": True})
        restart(env)
        wait_for(lambda: phase(env, "start_pending"))
        if replacement == "wrong_unit":
            fault(env, machine={"unit": "rack-ai-comfyui-unrelated.service"})
        elif replacement == "invalid_gate":
            fault(env, gate_override={"activation": "foreign-activation"})
        else:
            fault(env, machine={replacement: True})
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert service(env)["lease"] == original["lease"]
        assert sum("stop" in command for command in machine(env)["mutations"]) == 1

def test_restart_timeout_fails_closed_without_restart_loop(tmp_path):
    with receiver(tmp_path / "machine", lambda c: c.update(start_timeout=3)) as env:
        _, original = start(env)
        fault(env, machine={"start_ignored": True})
        restart(env)
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert "restart startup deadline" in service(env)["error"]
        assert service(env)["lease"] == original["lease"]
        time.sleep(3)
        assert sum("start" in command for command in machine(env)["mutations"]) == 2

@pytest.mark.parametrize("when", ["draining", "stopping", "start_pending", "starting"])
def test_finish_during_restart_does_not_resurrect_backend(tmp_path, when):
    with receiver(tmp_path / "machine") as env:
        session, _ = start(env)
        if when == "starting":
            fault(env, machine={"new_process": True}, gate_unavailable=True)
            # Gate must still be available when admitting and draining the restart.
            fault(env, gate_unavailable=False)
        restart(env)
        wait_for(lambda: phase(env, when))
        if when == "starting":
            wait_for(lambda: sum("start" in command for command in machine(env)["mutations"]) == 2)
        starts = sum("start" in command for command in machine(env)["mutations"])
        if when == "starting":
            fault(env, gate_unavailable=True)
        finish(env, session)
        time.sleep(2)
        assert status(env)["state"] == "stopped"
        assert sum("start" in command for command in machine(env)["mutations"]) == starts
        assert not machine(env)["active"]

def test_receiver_restart_retains_durable_restart_and_never_releases_early(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, original = start(env)
        fault(env, machine={"new_process": True})
        restart(env)
        wait_for(lambda: phase(env, "start_pending"))
        env["restart"]()
        wait_for(lambda: status(env)["state"] == "ready")
        assert service(env)["lease"] == original["lease"]
        assert service(env)["session"] == original["session"]
        assert sum("start" in command for command in machine(env)["mutations"]) == 2
        finish(env, session)

def test_managed_mode_cannot_request_or_adopt_restart(tmp_path):
    with receiver(tmp_path / "machine") as env:
        fault(env, render_delay=20)
        submit(env)
        wait_for(lambda: status(env)["state"] == "ready")
        response = requests.post(env["native"] + "/v2/manager/reboot", headers=OWNER, timeout=5)
        assert response.status_code == 409
        assert service(env)["restart"] is None
        fault(env, machine={"invocation": uuid.uuid4().hex})
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert sum("start" in command for command in machine(env)["mutations"]) == 1

def test_unannounced_interactive_invocation_replacement_still_quarantines(tmp_path):
    with receiver(tmp_path / "machine") as env:
        start(env)
        fault(env, machine={"invocation": uuid.uuid4().hex})
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert all("stop" not in command for command in machine(env)["mutations"])

def test_finish_cancels_pending_systemd_start_without_late_resurrection(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, _ = start(env)
        fault(env, machine={"start_ignored": True})
        restart(env)
        wait_for(lambda: phase(env, "starting"))
        wait_for(lambda: sum("start" in command for command in machine(env)["mutations"]) == 2)
        fault(env, machine={"job": "123"})
        finish(env, session)
        assert machine(env)["job"] == ""
        assert sum("start" in command for command in machine(env)["mutations"]) == 2

def test_restart_is_not_a_get_and_cannot_be_cross_principal_or_csrf(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, original = start(env)
        assert requests.get(env["native"] + "/v2/manager/reboot", headers=OWNER, timeout=3).status_code == 405
        assert requests.post(env["native"] + "/v2/manager/reboot", headers=env["headers"], timeout=3).status_code == 409
        login = requests.Session()
        assert login.post(env["native"] + "/login", data={"credential": TOKEN}, headers={"Origin": env["native"]}, timeout=3).status_code == 200
        assert login.post(env["native"] + "/v2/manager/reboot", timeout=3).status_code == 403
        assert service(env)["generation"] == original["generation"]
        finish(env, session)
