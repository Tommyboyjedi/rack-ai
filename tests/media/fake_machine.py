#!/usr/bin/env python3
"""Disposable machine-command fixture. Never installed into a service PATH."""
import json
import os
from pathlib import Path
import sys
import uuid
root = Path(os.environ["RACK_MEDIA_FIXTURE"])
path = root / "machine.json"
state = json.loads(path.read_text())
program = Path(sys.argv[0]).name
args = sys.argv[1:]
media = "GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b"
protected = ["GPU-357ef569-8fac-7c7d-ee1c-51677efb174f", "GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c"]
if program == "systemctl":
    if "show" in args:
        active = state["active"]
        pid = state["pid"] if active else 0
        cgroup = Path(f"/proc/{state['pid']}/cgroup").read_text().split("::", 1)[1].strip() if active else ""
        print(f"InvocationID={state['invocation'] if active else ''}\nMainPID={pid}\nControlGroup={cgroup}\nActiveState={'active' if active else 'inactive'}")
    elif "start" in args:
        if not state.get("start_ignored"):
            state.update(active=True, invocation=uuid.uuid4().hex)
    elif "stop" in args:
        if not state.get("stop_ignored"):
            state.update(active=False)
    else:
        sys.exit(2)
elif program == "nvidia-smi":
    if any("query-gpu" in a for a in args):
        print(f"{media}, NVIDIA GeForce RTX 4080 SUPER\n{protected[0]}, NVIDIA GeForce RTX 2060\n{protected[1]}, NVIDIA GeForce RTX 4060 Ti")
    elif state.get("foreign"):
        print(f"{media}, 1")
    elif state["active"]:
        print(f"{media}, {state['pid']}")
elif program == "docker":
    print(json.dumps([{"DeviceIDs": [protected[0] if "coder" in args else protected[1]]}]))
else:
    sys.exit(3)
if program == "systemctl" and any(a in args for a in ("start", "stop")):
    state.setdefault("mutations", []).append(args)
    temporary = root / "machine.next"
    temporary.write_text(json.dumps(state))
    temporary.replace(path)
