import concurrent.futures
import hashlib
import json
from pathlib import Path
import uuid
import os
import pytest
import requests
from fixture import receiver, wait_for, REPO, TOKEN


def request():
    value = json.loads((REPO / "config/media/fixtures/job-request.json").read_text())
    value["submission_id"] = value["idempotency_key"] = str(uuid.uuid4())
    return value


def completed(env, identity):
    response = requests.get(env["api"] + "/api/media/v1/jobs/" + identity, headers=env["headers"], timeout=3)
    value = response.json()
    return value if value.get("state") in {"completed", "failed", "interrupted", "cancelled"} else None


def test_real_http_replay_artifacts_isolation_and_release(tmp_path):
    with receiver(tmp_path / "machine") as env:
        api = env["api"] + "/api/media/v1"
        assert requests.get(api + "/status", timeout=2).status_code == 401
        assert requests.get(api + "/status", headers={"X-Rack-Principal": "operator"}, timeout=2).status_code == 401
        assert requests.get(api + "/profiles", headers=env["headers"], timeout=2).status_code == 200
        assert not json.loads((env["root"] / "machine.json").read_text())["active"]
        payload = request()
        with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
            replies = list(pool.map(lambda _: requests.post(api + "/jobs", json=payload, headers=env["headers"], timeout=5), range(8)))
        assert {r.status_code for r in replies} == {202}
        assert len({r.json()["id"] for r in replies}) == 1
        identity = replies[0].json()["id"]
        changed = json.loads(json.dumps(payload))
        changed["parameters"]["seed"] += 1
        assert requests.post(api + "/jobs", json=changed, headers=env["headers"], timeout=3).status_code == 409
        owner = {"Authorization": "Bearer " + TOKEN}
        assert requests.get(api + "/jobs/" + identity, headers=owner, timeout=3).status_code == 404
        assert requests.post(api + "/jobs/" + identity + "/cancel", headers=owner, json={}, timeout=3).status_code == 404
        outcome = wait_for(lambda: completed(env, identity))
        import jsonschema
        jsonschema.validate(outcome, json.loads((REPO / "config/media/job-response.schema.json").read_text()))
        assert outcome["state"] == "completed", outcome
        artifact = outcome["artifacts"][0]
        content = requests.get(api + f"/jobs/{identity}/artifacts/{artifact['id']}", headers=env["headers"], timeout=3)
        assert content.status_code == 200
        assert hashlib.sha256(content.content).hexdigest() == artifact["sha256"]
        assert requests.get(api + f"/jobs/{identity}/artifacts/{artifact['id']}", headers=owner, timeout=3).status_code == 404
        assert requests.post(api + "/jobs", json=payload, headers=env["headers"], timeout=3).json()["id"] == identity
        assert len(requests.get(env["backend"] + "/fixture/control", timeout=3).json()["dispatches"]) == 1
        wait_for(lambda: requests.get(api + "/status", headers=env["headers"], timeout=3).json()["state"] == "stopped")
        assert not list((env["root"] / "resources/leases").glob("*.json"))


@pytest.mark.parametrize("fault,state", [("lost_ack", "completed"), ("lost_history", "interrupted"), ("invalid_image", "failed"), ("wrong_history", "failed"), ("oversized", "failed"),
    ("symlink", "failed"), ("traversal", "failed"), ("partial_output", "failed"), ("wrong_dimensions", "failed")])
def test_dispatch_and_output_failure_modes(tmp_path, fault, state):
    with receiver(tmp_path / "machine") as env:
        requests.post(env["backend"] + "/fixture/control", json={"mode": fault}, timeout=3).raise_for_status()
        value = requests.post(env["api"] + "/api/media/v1/jobs", json=request(), headers=env["headers"], timeout=3).json()
        outcome = wait_for(lambda: completed(env, value["id"]))
        assert outcome["state"] == state, outcome
        assert len(requests.get(env["backend"] + "/fixture/control", timeout=3).json()["dispatches"]) == 1


def test_admission_security_and_cancel_before_dispatch(tmp_path):
    with receiver(tmp_path / "machine") as env:
        api = env["api"] + "/api/media/v1"
        payload = request()
        payload["source_system"] = "operator"
        assert requests.post(api + "/jobs", json=payload, headers=env["headers"], timeout=3).status_code == 422
        payload = request()
        payload["priority"] = "paramount"
        assert requests.post(api + "/jobs", json=payload, headers=env["headers"], timeout=3).status_code == 403
        assert requests.post(api + "/sessions", json={"schema": "rack-ai/media/v1", "idempotency_key": "abc"}, headers=env["headers"], timeout=3).status_code == 403
        job = requests.post(api + "/jobs", json=request(), headers=env["headers"], timeout=3).json()
        requests.post(api + f"/jobs/{job['id']}/cancel", json={}, headers=env["headers"], timeout=3).raise_for_status()
        assert completed(env, job["id"])["state"] == "cancelled"


def test_launcher_native_assets_websocket_and_csrf(tmp_path):
    from playwright.sync_api import sync_playwright
    with receiver(tmp_path / "machine") as env, sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True, chromium_sandbox=True, args=['--disable-gpu', '--use-angle=swiftshader'], executable_path=os.environ.get('RACK_MEDIA_BROWSER'))
        context = browser.new_context()
        page = context.new_page()
        page.goto(env["api"])
        page.get_by_label("Password", exact=True).fill(TOKEN)
        page.get_by_role("button", name="Sign in").click()
        page.wait_for_url(env["api"] + "/")
        page.wait_for_function("document.querySelector('#state').textContent === 'stopped'")
        assert page.locator("#open").is_hidden()
        page.get_by_role("button", name="Start ComfyUI").click()
        page.locator("#open").wait_for(state="visible", timeout=30000)
        assert page.locator("#state").inner_text() == "ready"
        with page.expect_popup() as popup:
            page.get_by_role("link", name="Open ComfyUI").click()
        native = popup.value
        native.wait_for_function("document.body.dataset.ws === 'connected'")
        cookies = {c["name"]: c["value"] for c in context.cookies()}
        assert requests.post(env["api"] + "/api/media/v1/sessions", cookies=cookies,
            json={"schema": "rack-ai/media/v1", "idempotency_key": "csrf"}, timeout=3).status_code == 403
        page.reload()
        page.get_by_role("button", name="Finish session").click()
        page.wait_for_function("document.querySelector('#state').textContent === 'stopped'", timeout=30000)
        assert page.locator("#open").is_hidden()
        assert page.locator("#open").get_attribute("href") is None
        context.close()
        browser.close()


def test_shared_schema_fixtures():
    import jsonschema
    for kind in ["request", "response"]:
        schema = json.loads((REPO / f"config/media/job-{kind}.schema.json").read_text())
        fixture = json.loads((REPO / f"config/media/fixtures/job-{kind}.json").read_text())
        jsonschema.Draft202012Validator(schema).validate(fixture)


def test_oversized_body_and_spoofed_host_are_rejected(tmp_path):
    with receiver(tmp_path / "machine") as env:
        api = env["api"] + "/api/media/v1"
        assert requests.post(api + "/jobs", data=b"x"*20000, headers=env["headers"], timeout=3).status_code == 413
        headers = dict(env["headers"], Host="evil.invalid")
        assert requests.get(api + "/status", headers=headers, timeout=3).status_code == 400
        assert not json.loads((env["root"] / "machine.json").read_text())["active"]


def test_native_module_burst_queues_without_starving_status(tmp_path):
    with receiver(tmp_path / "machine") as env:
        owner = {"Authorization":"Bearer "+TOKEN}
        requests.post(env["api"]+"/api/media/v1/sessions",
            json={"schema":"rack-ai/media/v1","idempotency_key":"module-burst"},headers=owner,timeout=3).raise_for_status()
        wait_for(lambda: requests.get(env["api"]+"/api/media/v1/status",headers=owner,timeout=3).json()["state"] == "ready")
        with concurrent.futures.ThreadPoolExecutor(max_workers=96) as pool:
            futures = [pool.submit(requests.get,env["native"]+f"/assets/module-{i}.js",headers=owner,timeout=8) for i in range(96)]
            assert requests.get(env["api"]+"/api/media/v1/status",headers=owner,timeout=2).status_code == 200
            assert {future.result().status_code for future in futures} == {200}
