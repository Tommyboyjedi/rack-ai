"""Synthetic credentials only; all password mutation/recovery uses disposable runtimes."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time
import concurrent.futures
import requests
import pytest
from fixture import receiver, REPO, TOKEN, CLIENT, wait_for

FIRST = "a synthetic long passphrase"
SECOND = "a different synthetic passphrase"
AUTH = "state/browser-auth/auth.json"

def sign_in(env, password=TOKEN, session=None):
    session = session or requests.Session()
    response = session.post(env["api"] + "/login", data={"password": password},
        headers={"Origin": env["api"]}, allow_redirects=False, timeout=30)
    return session, response

def change(env, session, current=TOKEN, new=FIRST, **kwargs):
    form = {"current_password": current, "new_password": new, "confirm_password": new}
    form.update(kwargs.pop("form", {}))
    return session.post(env["api"] + "/account", data=form,
        headers=kwargs.pop("headers", {"Origin": env["api"]}), allow_redirects=False, timeout=30, **kwargs)

def status(env, session):
    return session.get(env["api"] + "/api/media/v1/status", timeout=5).status_code

def cooldown():
    time.sleep(1.1)

def assert_api(env):
    for token in [TOKEN, CLIENT]:
        assert requests.get(env["api"]+"/api/media/v1/status",
            headers={"Authorization":"Bearer "+token}, timeout=5).status_code == 200

def test_bootstrap_creation_password_login_and_api_separation(tmp_path):
    with receiver(tmp_path/"machine") as env:
        original = (env["root"]/"config.json").read_bytes()
        assert not (env["root"]/AUTH).exists()
        assert "Use the existing operator credential once" in requests.get(env["api"]+"/login").text
        session, login = sign_in(env)
        assert login.status_code == 303
        assert "Max-Age=2592000" in login.headers["Set-Cookie"]
        browser = json.loads((env["root"]/"state/state.json").read_text())["browsers"][0]
        assert 2591995 <= browser["expires"]-int(time.time()) <= 2592000
        assert "HttpOnly" in login.headers["Set-Cookie"] and "SameSite=Strict" in login.headers["Set-Cookie"]
        assert "Set password" in session.get(env["api"]+"/account").text
        assert_api(env)
        result = change(env, session)
        assert result.status_code == 303, result.text
        path = env["root"]/AUTH
        record = json.loads(path.read_text())
        assert record["password_hash"].startswith("$argon2id$v=19$m=65536,t=3,p=1$")
        assert record["generation"] and record["owner"] == "operator"
        assert FIRST not in path.read_text() and TOKEN not in path.read_text()
        assert path.stat().st_mode & 0o777 == 0o600
        assert path.parent.stat().st_mode & 0o777 == 0o700
        assert "Use the existing operator credential once" not in requests.get(env["api"]+"/login").text
        assert status(env, session) == 200
        assert sign_in(env, FIRST)[1].status_code == 303
        assert sign_in(env, "wrong synthetic password")[1].status_code == 401
        cooldown()
        assert sign_in(env, FIRST)[1].status_code == 303  # clears failure backoff
        assert sign_in(env, TOKEN)[1].status_code == 401
        assert_api(env)
        assert (env["root"]/"config.json").read_bytes() == original
        assert FIRST not in (env["root"]/"receiver.log").read_text()

def test_change_requires_current_rotates_salt_and_revokes_all_other_sessions(tmp_path):
    with receiver(tmp_path/"machine") as env:
        session, _ = sign_in(env)
        assert change(env, session).status_code == 303
        second, _ = sign_in(env, FIRST)
        first_record = json.loads((env["root"]/AUTH).read_text())
        assert change(env, session, current="", new=SECOND).status_code == 401
        cooldown()
        assert change(env, session, current=FIRST, new=SECOND,
            form={"confirm_password": "different"}).status_code == 422
        assert change(env, session, current=FIRST, new="short").status_code == 422
        assert change(env, session, current=FIRST, new=TOKEN).status_code == 422
        assert change(env, session, current=FIRST, new=SECOND).status_code == 303
        next_record = json.loads((env["root"]/AUTH).read_text())
        assert first_record["password_hash"].split("$")[4] != next_record["password_hash"].split("$")[4]
        assert first_record["generation"] != next_record["generation"]
        assert status(env, session) == 200 and status(env, second) == 401
        assert sign_in(env, SECOND)[1].status_code == 303
        assert sign_in(env, FIRST)[1].status_code == 401
        assert_api(env)
        env["restart"]()
        assert status(env, second) == 401 and status(env, session) == 200

def test_account_csrf_logout_and_bearer_cannot_manage_browser_password(tmp_path):
    with receiver(tmp_path/"machine") as env:
        session, _ = sign_in(env)
        for headers in [{}, {"Origin":"https://other.invalid"}]:
            assert change(env, session, headers=headers).status_code == 403
            assert session.post(env["api"]+"/logout", headers=headers, allow_redirects=False).status_code == 403
        assert not (env["root"]/AUTH).exists()
        assert requests.post(env["api"]+"/account", data={"current_password":TOKEN,"new_password":FIRST,"confirm_password":FIRST},
            headers={"Authorization":"Bearer "+TOKEN,"Origin":env["api"]}).status_code == 403
        cookie = session.cookies.get("rack_session")
        response = session.post(env["api"]+"/logout", headers={"Origin":env["api"]}, allow_redirects=False)
        assert response.status_code == 303 and "Max-Age=0" in response.headers["Set-Cookie"]
        assert requests.get(env["api"]+"/api/media/v1/status", cookies={"rack_session":cookie}).status_code == 401
        assert status(env, session) == 401
        assert_api(env)

def test_throttle_is_bounded_shared_and_does_not_block_api_or_recovery(tmp_path):
    with receiver(tmp_path/"machine") as env:
        assert sign_in(env, "wrong")[1].status_code == 401
        throttled = sign_in(env)[1]
        assert throttled.status_code == 429 and 1 <= int(throttled.headers["Retry-After"]) <= 31
        other = requests.post(env["native"]+"/login", data={"password":TOKEN},
            headers={"Origin":env["native"]}, allow_redirects=False, timeout=5)
        assert other.status_code == 429
        assert_api(env)
        cooldown()
        assert sign_in(env, "wrong again")[1].status_code == 401
        delay = sign_in(env)[1]
        assert delay.status_code == 429 and int(delay.headers["Retry-After"]) >= 2
        time.sleep(2.1)
        assert sign_in(env)[1].status_code == 303
        assert sign_in(env)[1].status_code == 303

def test_local_reset_revokes_cookies_and_preserves_api_and_media_state(tmp_path):
    with receiver(tmp_path/"machine") as env:
        session, _ = sign_in(env)
        assert change(env, session).status_code == 303
        original = (env["root"]/"config.json").read_bytes()
        before = json.loads((env["root"]/"state/state.json").read_text())
        binary = Path(os.environ.get("RACK_MEDIA_BINARY", str(REPO/"target/debug/rack_ai_media")))
        result = subprocess.run([str(binary.with_name("rack_ai_media_admin")), str(env["root"]/"config.json"),
            "reset-browser-password"], capture_output=True, text=True, timeout=10, check=True)
        assert TOKEN not in result.stdout and FIRST not in result.stdout
        assert json.loads((env["root"]/AUTH).read_text())["password_hash"] is None
        assert status(env, session) == 401
        assert sign_in(env)[1].status_code == 303
        assert_api(env)
        after = json.loads((env["root"]/"state/state.json").read_text())
        assert before["jobs"] == after["jobs"] and before["sessions"] == after["sessions"]
        assert before["service"]["state"] == after["service"]["state"] == "stopped"
        assert (env["root"]/"config.json").read_bytes() == original
        assert not (env["root"]/"resources/leases/gpu-4080-super.json").exists()

def test_corrupt_record_fails_closed_without_disabling_api(tmp_path):
    with receiver(tmp_path/"machine") as env:
        session, _ = sign_in(env)
        assert change(env, session).status_code == 303
        path = env["root"]/AUTH
        path.write_text('{"password_hash":"broken-private-hash"}')
        assert requests.get(env["api"]+"/login").status_code == 503
        assert sign_in(env)[1].status_code == 503
        assert status(env, session) == 401
        assert_api(env)
        env["restart"]()
        assert_api(env)
        assert sign_in(env)[1].status_code == 503

def test_parallel_changes_cannot_keep_a_revoked_browser_alive(tmp_path):
    with receiver(tmp_path/"machine") as env:
        first, _ = sign_in(env)
        second, _ = sign_in(env)
        with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
            outcomes = list(pool.map(lambda s: change(env, s).status_code, [first,second]))
        assert outcomes.count(303) == 1 and all(code in [303,401,429] for code in outcomes)
        assert sorted([status(env,first), status(env,second)]) == [200,401]
        assert_api(env)

def test_fresh_browser_sets_password_changes_logs_out_and_logs_in(tmp_path):
    from playwright.sync_api import sync_playwright
    with receiver(tmp_path/"machine") as env, sync_playwright() as playwright:
        browser = playwright.chromium.launch(headless=True, chromium_sandbox=True,
            args=["--disable-gpu","--use-angle=swiftshader"], executable_path=os.environ.get("RACK_MEDIA_BROWSER"))
        page=browser.new_page()
        page.goto(env["api"])
        page.get_by_label("Password",exact=True).fill(TOKEN)
        page.get_by_role("button",name="Sign in",exact=True).click()
        page.get_by_role("link",name="Change password",exact=True).click()
        page.get_by_label("Current password",exact=True).fill(TOKEN)
        page.get_by_label("New password",exact=True).fill(FIRST)
        page.get_by_label("Confirm new password",exact=True).fill(FIRST)
        page.get_by_role("button",name="Save password",exact=True).click()
        page.wait_for_url(env["api"]+"/account?saved=1",timeout=30000)
        assert page.get_by_role("status").is_visible()
        page.get_by_role("button",name="Log out",exact=True).click()
        page.wait_for_url(env["api"]+"/login")
        assert "existing operator credential" not in page.content()
        page.get_by_label("Password",exact=True).fill(FIRST)
        page.get_by_role("button",name="Sign in",exact=True).click()
        page.wait_for_url(env["api"]+"/")
        page.get_by_role("link",name="Change password",exact=True).wait_for()
        browser.close()

@pytest.mark.parametrize("action", ["logout","password_change","reset"])
def test_existing_native_browser_websocket_closes_when_session_revoked(tmp_path, action):
    import asyncio
    import websockets
    with receiver(tmp_path/"machine") as env:
        session, _ = sign_in(env)
        other, _ = sign_in(env)
        cookie = other.cookies.get("rack_session")
        started = session.post(env["api"]+"/api/media/v1/sessions",
            json={"schema":"rack-ai/media/v1","idempotency_key":"password-websocket"},
            headers={"Origin":env["api"]}, timeout=5)
        assert started.status_code == 202
        wait_for(lambda: requests.get(env["api"]+"/api/media/v1/status",headers=env["headers"]).json()["state"]=="ready")
        async def proof():
            async with websockets.connect(env["native"].replace("http:","ws:")+"/ws",
                    additional_headers={"Cookie":"rack_session="+cookie},origin=env["native"]) as ws:
                await asyncio.wait_for(ws.recv(),5)
                if action == "logout":
                    assert other.post(env["api"]+"/logout",headers={"Origin":env["api"]},
                        allow_redirects=False).status_code == 303
                elif action == "password_change":
                    assert change(env,session).status_code == 303
                else:
                    binary = Path(os.environ.get("RACK_MEDIA_BINARY",str(REPO/"target/debug/rack_ai_media")))
                    subprocess.run([str(binary.with_name("rack_ai_media_admin")),str(env["root"]/"config.json"),
                        "reset-browser-password"],check=True,capture_output=True,timeout=10)
                await asyncio.wait_for(ws.wait_closed(),8)
                assert ws.close_code is not None
        asyncio.run(proof())
        assert_api(env)
