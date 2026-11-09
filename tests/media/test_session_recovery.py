"""Operator recovery never adopts or kills a live or differently owned backend."""
import json
import os
import uuid

import pytest
import requests

from fixture import receiver, wait_for, TOKEN
from test_recovery import fault, status
from test_restart import start, service, machine

OWNER = {"Authorization": "Bearer " + TOKEN}
BODY = {"schema": "rack-ai/media/v1", "confirm": "recover_comfyui"}


def recover(env, headers=OWNER, body=None):
    return requests.post(env["api"] + "/api/media/v1/recover", headers=headers,
                         json=BODY if body is None else body, timeout=15)


def fail_backend(env, **changes):
    original = service(env)
    updates = {"active": False, "unit_failed": True,
               "failed_invocation": original["invocation"]}
    updates.update(changes)
    fault(env, machine=updates)
    wait_for(lambda: status(env)["state"] == "recovery_required")
    return original


def test_recovery_reopens_native_session_with_fresh_owned_lease(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, original = start(env)
        (env["root"] / "output/interrupted.txt").write_text("failed session output")
        fail_backend(env)
        reply = recover(env)
        assert reply.status_code == 202, reply.text
        wait_for(lambda: status(env)["state"] == "ready")
        current = service(env)
        assert current["session"] == session["id"]
        assert current["activation"] != original["activation"]
        assert current["lease"]["generation"] != original["lease"]["generation"]
        assert current["error"] is None
        assert not (env["root"] / "output/interrupted.txt").exists()
        assert requests.get(env["native"], headers=OWNER, timeout=5).status_code == 200
        audit = json.loads((env["root"] / "state/session-recovery-last.json").read_text())
        assert audit["operator"] == "operator"
        assert audit["service"]["activation"] == original["activation"]
        assert any("reset-failed" in mutation for mutation in machine(env)["mutations"])


def test_recovery_preserves_pending_finish(tmp_path):
    with receiver(tmp_path / "machine") as env:
        session, _ = start(env)
        fail_backend(env)
        requests.post(env["api"] + session["location"] + "/release",
                      headers=OWNER, json={}, timeout=5).raise_for_status()
        reply = recover(env)
        assert reply.status_code == 202, reply.text
        wait_for(lambda: status(env)["state"] == "stopped")
        detail = requests.get(env["api"] + session["location"], headers=OWNER, timeout=5).json()
        assert detail["state"] == "stopped"
        assert not list((env["root"] / "resources/leases").glob("*.json"))
        assert sum("start" in command for command in machine(env)["mutations"]) == 1


@pytest.mark.parametrize("obstacle", ["non_operator", "other_owner", "live_backend",
                                    "gpu_process", "changed_invocation", "changed_unit",
                                    "managed_claim", "managed_mode", "changed_lease"])
def test_recovery_refuses_unproven_or_foreign_ownership(tmp_path, obstacle):
    def configure(config):
        if obstacle == "other_owner":
            config["principals"][1]["operator"] = True

    with receiver(tmp_path / "machine", configure=configure) as env:
        start(env)
        changes = {}
        if obstacle == "live_backend":
            changes.update(active=True, invocation=uuid.uuid4().hex, unit_failed=False)
        if obstacle == "gpu_process":
            changes["foreign"] = True
        if obstacle == "changed_invocation":
            changes["failed_invocation"] = uuid.uuid4().hex
        if obstacle == "changed_unit":
            changes["definition_runtime"] = True
        fail_backend(env, **changes)
        if obstacle == "managed_claim":
            (env["root"] / "resources/managed.json").write_text(
                json.dumps({"claims": {"gpu-4080-super": "foreign"}, "data": {}}))
        if obstacle == "managed_mode":
            path = env["root"] / "state/state.json"
            saved = json.loads(path.read_text())
            saved["service"]["mode"] = "managed"
            path.write_text(json.dumps(saved))
        if obstacle == "changed_lease":
            path = env["root"] / "resources/leases/gpu-4080-super.json"
            saved = json.loads(path.read_text())
            saved["owner"] = "another-owner"
            path.write_text(json.dumps(saved))
        lease_path = env["root"] / "resources/leases/gpu-4080-super.json"
        before = lease_path.read_bytes()
        mutations = machine(env)["mutations"][:]
        reply = recover(env, headers=env["headers"] if obstacle in {"non_operator", "other_owner"} else OWNER)
        assert reply.status_code in {403, 409, 503}, reply.text
        assert lease_path.read_bytes() == before
        assert machine(env)["mutations"] == mutations


def test_recovery_requires_failure_and_explicit_confirmation(tmp_path):
    with receiver(tmp_path / "machine") as env:
        start(env)
        assert recover(env).status_code == 409
        fail_backend(env)
        assert recover(env, body={}).status_code == 422
        assert recover(env, body={"schema": "rack-ai/media/v1", "confirm": "wrong"}).status_code == 422
        assert not any("reset-failed" in command for command in machine(env)["mutations"])


def test_recovery_button_tracks_failure_and_reopens_session(tmp_path):
    from playwright.sync_api import sync_playwright

    with receiver(tmp_path / "machine") as env, sync_playwright() as playwright:
        browser = playwright.chromium.launch(
            executable_path=os.environ["RACK_MEDIA_BROWSER"], headless=True,
            chromium_sandbox=True, args=["--disable-gpu", "--use-angle=swiftshader"])
        page = browser.new_page()
        page.goto(env["api"])
        page.get_by_label("Password", exact=True).fill(TOKEN)
        page.get_by_role("button", name="Sign in").click()
        page.wait_for_url(env["api"] + "/")
        button = page.get_by_role("button", name="Recover session", exact=True)
        assert button.is_hidden()
        page.get_by_role("button", name="Start ComfyUI", exact=True).click()
        page.locator("#open").wait_for(state="visible", timeout=30000)
        assert button.is_hidden()
        fail_backend(env)
        button.wait_for(state="visible", timeout=10000)
        assert page.locator("#finish + #session-recover").count() == 1
        page.once("dialog", lambda dialog: dialog.accept())
        button.click()
        page.locator("#open").wait_for(state="visible", timeout=30000)
        assert button.is_hidden()
        assert page.locator("#state").inner_text() == "ready"
        browser.close()


def test_identity_free_startup_failure_recovers_pending_session(tmp_path):
    with receiver(tmp_path / "machine") as env:
        docker = env["root"] / "bin/docker"
        original = docker.read_text()
        docker.write_text("#!/bin/sh\nexit 1\n")
        reply = requests.post(env["api"] + "/api/media/v1/sessions", headers=OWNER,
                              json={"schema": "rack-ai/media/v1", "idempotency_key": "startup-failure"},
                              timeout=5)
        reply.raise_for_status()
        wait_for(lambda: status(env)["state"] == "recovery_required")
        assert service(env)["lease"] is None
        docker.write_text(original)
        accepted = recover(env)
        assert accepted.status_code == 202, accepted.text
        wait_for(lambda: status(env)["state"] == "ready")
        assert service(env)["session"] == reply.json()["id"]


def test_failed_reset_keeps_lease_and_allows_explicit_retry(tmp_path):
    with receiver(tmp_path / "machine") as env:
        start(env)
        fail_backend(env)
        systemctl = env["root"] / "bin/systemctl"
        original = systemctl.read_text()
        marker = 'if program == "systemctl":'
        assert marker in original
        systemctl.write_text(original.replace(
            marker, 'if program == "systemctl" and "reset-failed" in args: sys.exit(5)\n' + marker, 1))
        lease = env["root"] / "resources/leases/gpu-4080-super.json"
        before = lease.read_bytes()
        reply = recover(env)
        assert reply.status_code == 500, reply.text
        assert lease.read_bytes() == before
        assert status(env)["state"] == "recovery_required"
        systemctl.write_text(original)
        reply = recover(env)
        assert reply.status_code == 202, reply.text
        wait_for(lambda: status(env)["state"] == "ready")
