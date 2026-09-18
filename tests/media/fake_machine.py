#!/usr/bin/env python3
"""Disposable machine-command fixture. Never installed into a service PATH."""
import json
import os
from pathlib import Path
import sys
import uuid
import subprocess
import signal
root = Path(os.environ["RACK_MEDIA_FIXTURE"])
path = root / "machine.json"
state = json.loads(path.read_text())
program = Path(sys.argv[0]).name
args = sys.argv[1:]
media = "GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b"
protected = ["GPU-357ef569-8fac-7c7d-ee1c-51677efb174f", "GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c"]
if program == "systemctl":
    if "show" in args:
        config = json.loads((root / "config.json").read_text())
        runtime = config["runtime"]
        script = "unexpected.py" if state.get("definition_runtime") else runtime["script"]
        print(f'ExecStart={{ path={runtime["python"]} ; argv[]={runtime["python"]} {script} fixture ; }}')
        print(f'WorkingDirectory={runtime["directory"]}')
        print(f'Environment=CUDA_VISIBLE_DEVICES={config["media_uuid"]} RACK_MEDIA_AUTHORITY_FILE={config["authority_file"]} RACK_MEDIA_CONTROL_SECRET_FILE={config["control_secret_file"]}')
        print("MemoryMax=" + str(state.get("memory_limit", 32 * 1024 * 1024)))
        print("MemorySwapMax=0")
        print("CPUQuotaPerSecUSec=" + state.get("cpu_quota", "1s"))
        print("Type=simple")
        print("Restart=no")
        print("KillMode=control-group")
        active = state["active"]
        pid = state["pid"] if active else 0
        cgroup = Path(f"/proc/{state['pid']}/cgroup").read_text().split("::", 1)[1].strip() if active else ""
        print(f"Id={state.get('unit', 'rack-ai-comfyui-fixture.service')}\nJob={state.get('job', '')}\nInvocationID={state['invocation'] if active else ''}\nMainPID={pid}\nControlGroup={cgroup}\nActiveState={'active' if active else 'inactive'}")
    elif "start" in args:
        if not state.get("start_ignored"):
            state.update(active=True, invocation=uuid.uuid4().hex)
            if state.get("new_process"):
                config = json.loads((root / "config.json").read_text())
                runtime = config["runtime"]
                environment = dict(os.environ, CUDA_VISIBLE_DEVICES=config["media_uuid"],
                    RACK_MEDIA_AUTHORITY_FILE=config["authority_file"],
                    RACK_MEDIA_CONTROL_SECRET_FILE=config["control_secret_file"])
                if state.get("wrong_gpu"):
                    environment["CUDA_VISIBLE_DEVICES"] = protected[0]
                command = [runtime["python"], runtime["script"], "--process-generation-fixture"]
                if state.get("wrong_runtime"):
                    command = [runtime["python"], "-c", "import time; time.sleep(180)"]
                child = subprocess.Popen(command, cwd=runtime["directory"], env=environment,
                    stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
                state["pid"] = child.pid
                state.setdefault("spawned_pids", []).append(child.pid)
    elif "stop" in args:
        if not state.get("stop_ignored"):
            if state["pid"] in state.get("spawned_pids", []):
                try:
                    os.kill(state["pid"], signal.SIGTERM)
                except ProcessLookupError:
                    pass
            state.update(active=False, job="")
    else:
        sys.exit(2)
elif program == "nvidia-smi":
    if any("query-gpu=memory." in a for a in args):
        print(state.get("memory_mib",16384))
    elif any("query-gpu" in a for a in args):
        print(f"{media}, NVIDIA GeForce RTX 4080 SUPER\n{protected[0]}, NVIDIA GeForce RTX 2060\n{protected[1]}, NVIDIA GeForce RTX 4060 Ti")
    elif state.get("gpu_probe_error"):
        sys.exit(5)
    elif "-x" in args:
        pid = 1 if state.get("foreign") else state["pid"] if state["active"] else None
        process = f"<process_info><pid>{pid}</pid><type>{state.get('process_type','C')}</type></process_info>" if pid else ""
        print(f"<nvidia_smi_log><gpu><uuid>{media}</uuid><processes>{process}</processes></gpu></nvidia_smi_log>")
    else:
        sys.exit(4)
elif program == "docker":
    print(json.dumps([{"DeviceIDs": [protected[0] if "coder" in args else protected[1]]}]))
else:
    sys.exit(3)
if program == "systemctl" and any(a in args for a in ("start", "stop")):
    state.setdefault("mutations", []).append(args)
    temporary = root / "machine.next"
    temporary.write_text(json.dumps(state))
    temporary.replace(path)
