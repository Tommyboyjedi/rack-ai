#!/usr/bin/env python3
"""Synthetic systemd/NVIDIA commands for the isolated tiny-uncd CPU fixture."""
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

root = Path(os.environ["RACK_TINY_HOST_FIXTURE"]).resolve()
assert root.parent == Path("/dev/shm") and root.name.startswith("rack-tiny-uncd-")
assert root.stat().st_uid == os.getuid() and root.stat().st_mode & 0o777 == 0o700
name = Path(sys.argv[0]).name
args = sys.argv[1:]
assert name in {"systemd-run", "systemctl", "nvidia-smi"}
lock = (root / "machine.lock").open("a")
fcntl.flock(lock, fcntl.LOCK_EX)
path = root / "machine.json"
state = json.loads(path.read_text()) if path.exists() else {}


def alive():
    try:
        return Path(f'/proc/{state["pid"]}/stat').read_text().rsplit(") ", 1)[1].split()[0] != "Z"
    except (KeyError, FileNotFoundError):
        return False


def launch():
    assert not alive()
    environment = dict(arg[len("--setenv="):].split("=", 1)
                       for arg in args if arg.startswith("--setenv="))
    activation = environment["RACK_RUNTIME_ACTIVATION"]
    unit = next(arg.split("=", 1)[1] for arg in args if arg.startswith("--unit="))
    assert unit == f"rack-runtime-{activation}.service"
    command = args[args.index("--") + 1:]
    assert Path(command[0]).resolve() == Path(sys.executable).resolve()
    assert Path(command[1]).name == "backend.py"
    assert Path(command[4]) == root / "rack/events.jsonl"
    child = subprocess.Popen(command, env=dict(os.environ, **environment),
                             stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                             stderr=subprocess.DEVNULL)
    state.update(pid=child.pid, activation=activation, invocation=activation,
                 visible_devices=environment["CUDA_VISIBLE_DEVICES"], unit=unit)


def systemctl():
    if "list-units" in args:
        if state.get("unit") == args[-1]:
            status = "active running" if alive() else "inactive dead"
            print(state["unit"] + " loaded " + status + " fixture")
    elif "list-unit-files" in args:
        pass
    elif "show" in args:
        assert state["unit"] in args
        if "Transient" in args:
            print("Transient=yes\nRestart=no\nEnvironment=RACK_RUNTIME_ACTIVATION=" + state["activation"])
            return
        active = alive()
        cgroup = Path(f'/proc/{state["pid"]}/cgroup').read_text().split("::", 1)[1].strip() if active else ""
        invocation = state["invocation"] if active else ""
        print(f'Id={state["unit"]}\nJob=\nInvocationID={invocation}\n'
              f'MainPID={state["pid"] if active else 0}\nControlGroup={cgroup}\n'
              f'ActiveState={"active" if active else "inactive"}')
    elif "stop" in args:
        assert state["unit"] in args
        if alive():
            environment = Path(f'/proc/{state["pid"]}/environ').read_bytes().split(b"\0")
            assert f'RACK_RUNTIME_ACTIVATION={state["activation"]}'.encode() in environment
            os.kill(state["pid"], signal.SIGTERM)
    else:
        raise AssertionError(args)


def nvidia():
    if any("query-gpu=memory" in arg for arg in args):
        print(6144)  # Synthetic free/total capacity; no physical GPU is queried.
    elif "-x" in args:
        uuid = args[args.index("-i") + 1]
        allocated = alive() and uuid in state["visible_devices"].split(",")
        record = f'<process_info><pid>{state["pid"]}</pid><type>C</type></process_info>' if allocated else ""
        print(f'<nvidia_smi_log><gpu><uuid>{uuid}</uuid><processes>{record}</processes></gpu></nvidia_smi_log>')
    else:
        raise AssertionError(args)


if name == "systemd-run":
    launch()
elif name == "systemctl":
    systemctl()
else:
    nvidia()
if name != "nvidia-smi":
    with (root / "commands.jsonl").open("a") as output:
        output.write(json.dumps(dict(program=name, args=args)) + "\n")
temporary = root / "machine.next"
temporary.write_text(json.dumps(state))
temporary.replace(path)
