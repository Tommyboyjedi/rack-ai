import asyncio
import importlib.util
import json
from pathlib import Path
import time
import pytest
from aiohttp import web, ClientSession
from fixture import REPO, port

spec = importlib.util.spec_from_file_location("gate_under_test", REPO / "media/comfy_gate/gate.py")
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)

@pytest.mark.parametrize("value", [{}, {"expires": "bad"}, {"expires": None}, {"expires": False},
    {"expires": 0}, {"expires": float("nan")}, {"expires": 99999999999, "activation": "other"}])
def test_bad_authority_closes(tmp_path, value):
    path = tmp_path / "authority"
    path.write_text(json.dumps({"activation": "owned", "expires": time.time()+30, "mode": "interactive"}))
    authority = gate.Authority({"authority": path, "secret": tmp_path / "secret"})
    path.write_text(json.dumps(value))
    assert authority.status()["mode"] == "closed"

@pytest.mark.parametrize("alias", ["/prompt", "/api/prompt", "/api/api/prompt", "/upload/image"])
def test_gate_aliases_and_enqueue_barrier(tmp_path, alias):
    async def scenario():
        path, secret = tmp_path/"authority", tmp_path/"secret"
        secret.write_text("fixture-key")
        def mode(value):
            path.write_text(json.dumps({"activation": "owned", "mode": value, "expires": time.time()+30}))
        mode("interactive")
        authority = gate.Authority({"authority": path, "secret": secret})
        entry = gate.AdmissionGate(authority)
        entered, finish = asyncio.Event(), asyncio.Event()
        async def enqueue(request):
            entered.set()
            await finish.wait()
            return web.json_response({"accepted": True})
        app = web.Application(middlewares=[entry.middleware])
        app.router.add_post(alias, enqueue)
        app.router.add_post("/rack-gate/barrier", enqueue)
        runner = web.AppRunner(app)
        await runner.setup()
        address = f"http://127.0.0.1:{port()}"
        await web.TCPSite(runner, "127.0.0.1", int(address.rsplit(":",1)[1])).start()
        try:
            async with ClientSession() as client:
                headers = {"X-Rack-Control":"fixture-key","X-Rack-Access":"interactive"}
                assert (await client.post(address+alias)).status == 403
                accepted = asyncio.create_task(client.post(address+alias, headers=headers))
                await asyncio.wait_for(entered.wait(), 2)
                mode("closed")
                barrier = asyncio.create_task(client.post(address+"/rack-gate/barrier", headers=headers))
                await asyncio.sleep(.1)
                assert not barrier.done()
                finish.set()
                assert (await accepted).status == 200
                assert (await (await barrier).json())["mode"] == "closed"
                assert (await client.post(address+alias, headers=headers)).status == 503
                mode("managed")
                assert (await client.post(address+alias, headers=headers)).status == 403
        finally:
            await runner.cleanup()
    asyncio.run(scenario())

def test_pinned_custom_node_entrypoint_registers_before_freeze():
    entrypoint = (REPO / "media/comfy_gate/__init__.py").read_text()
    assert "install(PromptServer.instance.app)" in entrypoint
    assert "middlewares.insert(0, gate.middleware)" in (REPO / "media/comfy_gate/gate.py").read_text()
