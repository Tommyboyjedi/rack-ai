"""Simulated GPU queue/time through the production native admission barrier."""
import asyncio
import json
import time
import pytest
from aiohttp import web, ClientSession
from fixture import port
from test_gate import gate

def setup(tmp_path):
    authority_path, secret = tmp_path/"authority", tmp_path/"secret"
    secret.write_text("fixture-control")
    authority_path.write_text(json.dumps(dict(activation="owned", mode="interactive", expires=time.time()+120)))
    authority = gate.Authority(dict(authority=authority_path,secret=secret))
    clock, running = [1000], [False]
    work = {"clock":lambda:clock[0],"busy":lambda:running[0]}
    entry = gate.AdmissionGate(authority,work)
    return entry, clock, running, work

def test_simulated_exact_30_minutes_and_control_polling(tmp_path):
    entry, clock, running, work=setup(tmp_path)
    assert entry.activity.observe()["last_activity_at"]==1000
    for at in [1100,1600,2200,2799]:
        clock[0]=at
        assert entry.activity.observe()["last_activity_at"]==1000
        assert not entry.activity.close_if_idle(1800)["idle_closed"]
    clock[0]=2800
    assert entry.activity.close_if_idle(1800)["idle_closed"]
    # Heartbeat, status, and even a backend restart cannot reopen idle admission.
    restarted=gate.AdmissionGate(entry.authority,work)
    assert restarted.status(restarted.activity.observe())["mode"]=="closed"

def test_actual_work_and_completion_refresh_without_polling_refresh(tmp_path):
    entry,clock,running,_=setup(tmp_path)
    entry.activity.observe()
    clock[0]=1600
    assert entry.activity.observe(accepted=True)["last_activity_at"]==1600
    running[0]=True
    clock[0]=7000
    assert not entry.activity.close_if_idle(1800)["idle_closed"]
    assert entry.activity.observe()["last_activity_at"]==7000
    running[0]=False;clock[0]=7100
    assert entry.activity.observe()["last_activity_at"]==7100
    clock[0]=8899
    assert not entry.activity.close_if_idle(1800)["idle_closed"]
    clock[0]=8900
    assert entry.activity.close_if_idle(1800)["idle_closed"]

def test_activity_survives_backend_restart_and_stays_independent(tmp_path):
    entry,clock,running,work=setup(tmp_path)
    entry.activity.observe();clock[0]=1500;entry.activity.observe(accepted=True)
    second_path=tmp_path/"second";second_path.mkdir()
    other,_,_,_=setup(second_path);other.activity.observe()
    restarted=gate.AdmissionGate(entry.authority,work)
    assert restarted.activity.observe()["last_activity_at"]==1500
    assert other.activity.observe()["last_activity_at"]==1000

def test_activity_corruption_and_write_failure_fail_closed(tmp_path,monkeypatch):
    entry,_,_,work=setup(tmp_path)
    entry.activity.path.write_text('{"broken":true}')
    with pytest.raises(ValueError):
        entry.activity.observe()
    entry.activity.path.unlink()
    def failed(*args):raise OSError("fixture disk failure")
    monkeypatch.setattr(gate.os,"replace",failed)
    with pytest.raises(OSError):entry.activity.observe()
    with pytest.raises(web.HTTPServiceUnavailable):entry.activity.observe()

@pytest.mark.parametrize("expiry_first",[False,True])
def test_native_enqueue_idle_race_and_no_status_upload_activity(tmp_path,expiry_first):
    async def scenario():
        entry,clock,running,_=setup(tmp_path)
        entered,finish=asyncio.Event(),asyncio.Event()
        async def prompt(request):
            entered.set()
            await finish.wait()
            running[0]=True
            return web.json_response({"prompt_id":"accepted"})
        async def upload(request):return web.json_response({"name":"uploaded"})
        app=web.Application(middlewares=[entry.middleware])
        app.router.add_post("/api/prompt",prompt)
        app.router.add_post("/upload/image",upload)
        app.router.add_post("/rack-gate/idle",upload)
        app.router.add_get("/rack-gate/status",upload)
        runner=web.AppRunner(app);await runner.setup()
        address=f"http://127.0.0.1:{port()}"
        await web.TCPSite(runner,"127.0.0.1",int(address.rsplit(":",1)[1])).start()
        headers={"X-Rack-Control":"fixture-control","X-Rack-Access":"interactive"}
        body={"activation":"owned","idle_timeout_seconds":1800}
        try:
            async with ClientSession() as client:
                await client.get(address+"/rack-gate/status",headers=headers)
                clock[0]=2000
                assert (await client.post(address+"/upload/image",headers=headers)).status==200
                assert entry.activity.observe()["last_activity_at"]==1000
                clock[0]=2800
                if expiry_first:
                    response=await client.post(address+"/rack-gate/idle",headers=headers,json=body)
                    assert (await response.json())["idle_closed"]
                    assert (await client.post(address+"/api/prompt",headers=headers)).status==503
                    assert not entered.is_set()
                else:
                    accepted=asyncio.create_task(client.post(address+"/api/prompt",headers=headers))
                    await asyncio.wait_for(entered.wait(),2)
                    expiry=asyncio.create_task(client.post(address+"/rack-gate/idle",headers=headers,json=body))
                    await asyncio.sleep(.02)
                    assert not expiry.done()
                    finish.set()
                    assert (await accepted).status==200
                    result=await (await expiry).json()
                    assert result["busy"] and not result["idle_closed"]
                    assert result["last_activity_at"]==2800
        finally:await runner.cleanup()
    asyncio.run(scenario())


def test_activity_metadata_never_masks_authoritative_process_identity(tmp_path):
    entry,_,_,_=setup(tmp_path)
    original=entry.authority.status
    def foreign():
        value=original()
        value["activation"]="foreign-activation"
        value["pid"]=123
        value["invocation"]="foreign-invocation"
        return value
    entry.authority.status=foreign
    status=entry.status(entry.activity.observe())
    assert status["activation"]=="foreign-activation"
    assert status["pid"]==123
    assert status["invocation"]=="foreign-invocation"
