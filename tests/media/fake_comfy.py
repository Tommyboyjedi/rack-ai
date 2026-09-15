"""Real HTTP fixture using the production admission middleware, with synthetic PNGs."""
import asyncio
import importlib.util
import json
import os
from pathlib import Path
import sys
from aiohttp import web
if len(sys.argv) > 1 and sys.argv[1] == "--process-generation-fixture":
    import time
    time.sleep(180)
    sys.exit(0)
from PIL import Image

root = Path(sys.argv[1])
gate_path = Path(__file__).resolve().parents[2] / "media/comfy_gate/gate.py"
spec = importlib.util.spec_from_file_location("rack_gate", gate_path)
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
authority = module.Authority({"authority": root / "authority.json", "secret": root / "control.secret"})
gate = module.AdmissionGate(authority, {"clock": __import__("time").time, "busy": lambda: bool(pending)})
history, pending = {}, {}
dispatches = []
invocation = ""
fault = {"mode": "normal", "delay": 0.1}
original_gate_status = authority.status
def fixture_gate_status():
    value = original_gate_status()
    value["pid"] = json.loads((root / "machine.json").read_text())["pid"]
    value.update(fault.get("gate_override", {}))
    return value
authority.status = fixture_gate_status


@web.middleware
async def startup(request, handler):
    global invocation
    if request.path.startswith("/fixture/"):
        return await handler(request)
    machine = json.loads((root / "machine.json").read_text())
    if not machine["active"]:
        raise web.HTTPServiceUnavailable()
    if request.path == "/rack-gate/status" and fault.get("gate_unavailable"):
        raise web.HTTPServiceUnavailable()
    if invocation != machine["invocation"]:
        invocation = machine["invocation"]
        authority.activation = json.loads((root / "authority.json").read_text())["activation"]
        authority.invocation = invocation
        history.clear()
        pending.clear()
    return await gate.middleware(request, handler)


async def fixture(request):
    if request.method == "POST":
        updates = await request.json()
        machine = updates.pop("machine", None)
        if machine:
            value = json.loads((root / "machine.json").read_text())
            value.update(machine)
            temporary = root / "fixture-machine.next"
            temporary.write_text(json.dumps(value))
            temporary.replace(root / "machine.json")
        fault.update(updates)
    return web.json_response({"dispatches": dispatches, "fault": fault})


async def prompt(request):
    value = await request.json()
    identity = value["prompt_id"]
    await asyncio.sleep(fault["delay"])
    if identity in pending:
        raise web.HTTPBadRequest()
    dispatches.append(identity)
    pending[identity] = value["prompt"]
    asyncio.create_task(render(identity, value["prompt"]))
    if fault["mode"] == "lost_ack":
        request.transport.close()
    return web.json_response({"prompt_id": identity, "node_errors": {}})


async def render(identity, workflow):
    await asyncio.sleep(fault.get("render_delay", 0.15))
    if identity not in pending:
        return
    output = root / "output" / (workflow["9"]["inputs"]["filename_prefix"] + "_00001_.png")
    output.parent.mkdir(parents=True, exist_ok=True)
    params = workflow["5"]["inputs"]
    Image.new("RGB", (params["width"], params["height"]), (24, 80, 130)).save(output)
    if fault["mode"] == "invalid_image":
        output.write_bytes(b"invalid png")
    if fault["mode"] == "oversized":
        with output.open("wb") as file:
            file.truncate(17 * 1024 * 1024)
    if fault["mode"] == "symlink":
        outside = root / "outside.png"
        outside.write_bytes(output.read_bytes())
        output.unlink()
        output.symlink_to(outside)
    if fault["mode"] == "wrong_dimensions":
        Image.new("RGB", (params["width"]+64,params["height"])).save(output)
    if fault["mode"] != "lost_history":
        history[identity] = {"status": {"status_str": "success", "completed": True},
            "prompt": [0, identity, workflow],
            "outputs": {"9": {"images": [{"filename": output.name,
                "subfolder": str(output.parent.relative_to(root / "output")), "type": "output"}]}}}
    if fault["mode"] == "wrong_history":
        history[identity]["prompt"][1] = "unrelated"
    if fault["mode"] == "traversal":
        history[identity]["outputs"]["9"]["images"][0]["filename"] = "../outside.png"
    if fault["mode"] == "partial_output":
        history[identity]["outputs"]["9"]["images"] = []
    pending.pop(identity, None)


async def queue(request):
    return web.json_response({"queue_running": [[0, identity] for identity in pending], "queue_pending": []})


async def get_history(request):
    identity = request.match_info["identity"]
    return web.json_response({identity: history[identity]} if identity in history else {})


async def cancel(request):
    pending.pop(request.match_info["identity"], None)
    return web.json_response({"status": "cancelled"})


async def native(request):
    return web.Response(content_type="text/html", text="<html><title>ComfyUI fixture</title><h1>ComfyUI fixture</h1><script>const s=new WebSocket('ws://'+location.host+'/ws');s.onmessage=()=>document.body.dataset.ws='connected';</script></html>")


async def asset(request):
    await asyncio.sleep(0.3)
    return web.Response(text="export const ready=true;",content_type="text/javascript")


async def websocket(request):
    ws = web.WebSocketResponse()
    await ws.prepare(request)
    await ws.send_json({"type": "status"})
    async for message in ws:
        if message.type == web.WSMsgType.TEXT:
            await ws.send_str(message.data)
    return ws


app = web.Application(middlewares=[startup])
app.router.add_route("*", "/fixture/control", fixture)
for prefix in ["", "/api"]:
    app.router.add_post(prefix + "/prompt", prompt)
app.router.add_get("/queue", queue)
app.router.add_get("/history/{identity}", get_history)
app.router.add_post("/api/jobs/{identity}/cancel", cancel)
app.router.add_get("/rack-gate/status", queue)
app.router.add_post("/rack-gate/barrier", queue)
app.router.add_post("/rack-gate/idle", queue)
app.router.add_post("/rack-gate/admit-job", queue)
app.router.add_get("/", native)
app.router.add_get("/ws", websocket)
app.router.add_get("/assets/{name}", asset)
(root / "machine.json").write_text(json.dumps({"active": False, "pid": os.getpid(), "controller_pid": os.getpid(), "invocation": ""}))
web.run_app(app, host="127.0.0.1", port=int(sys.argv[2]), print=None)
