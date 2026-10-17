#!/usr/bin/env python3
"""Private synthetic systemd/GPU surface; never invokes real host controls."""
import fcntl
import json
import os
from pathlib import Path
import signal
import subprocess
import sys

root = Path(os.environ["RACK_DUAL_HOST_FIXTURE"]).resolve()
assert root.parent == Path("/dev/shm") and root.name.startswith("rack-dual-services-")
assert root.stat().st_uid == os.getuid() and root.stat().st_mode & 0o777 == 0o700
program = Path(sys.argv[0]).name
args = sys.argv[1:]
assert program in {"systemd-run", "systemctl", "nvidia-smi"}
lock = (root / "machine.lock").open("a")
fcntl.flock(lock, fcntl.LOCK_EX)
path = root / "machine.json"
state = json.loads(path.read_text()) if path.exists() else {}


def alive(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


def child_pids(record):
    result = [record["pid"]]
    child = root / (record["activation"] + ".engine.json")
    if child.exists():
        result.append(json.loads(child.read_text())["pid"])
    return result


def launch():
    environment = dict(arg[len("--setenv="):].split("=", 1)
                       for arg in args if arg.startswith("--setenv="))
    activation = environment["RACK_RUNTIME_ACTIVATION"]
    unit = next(arg.split("=", 1)[1] for arg in args if arg.startswith("--unit="))
    assert unit == f"rack-runtime-{activation}.service" and unit not in state
    command = args[args.index("--") + 1:]
    assert Path(command[0]).resolve() == Path(sys.executable).resolve()
    assert Path(command[1]).name == "dual_services_backend.py"
    assert Path(command[4]) == root / "rack/events.jsonl"
    properties = {arg[len("--property="):].split("=", 1)[0]: arg.split("=", 2)[2]
                  for arg in args if arg.startswith("--property=")}
    affinity = properties.get("CPUAffinity")
    preexec = None
    if affinity:
        selected = {int(value) for value in affinity.replace(",", " ").split()}
        preexec = lambda: os.sched_setaffinity(0, selected)
    child = subprocess.Popen(command, env=dict(os.environ, **environment),
        cwd=properties.get("WorkingDirectory"), preexec_fn=preexec,
        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    state[unit] = dict(pid=child.pid, activation=activation, invocation=activation,
        visible_devices=environment["CUDA_VISIBLE_DEVICES"], unit=unit)


def systemctl():
    unit = next((arg for arg in args if arg.startswith("rack-runtime-")), None)
    if "list-units" in args:
        if unit in state and alive(state[unit]["pid"]):
            print(unit + " loaded active running fixture")
    elif "list-unit-files" in args:
        return
    elif "show" in args:
        record = state[unit]
        if "Transient" in args:
            print("Transient=yes\nRestart=no\nEnvironment=RACK_RUNTIME_ACTIVATION=" + record["activation"])
            return
        active = alive(record["pid"])
        cgroup = Path(f'/proc/{record["pid"]}/cgroup').read_text().split("::", 1)[1].strip() if active else ""
        print(f'Id={unit}\nJob=\nInvocationID={record["invocation"] if active else ""}\n'
              f'MainPID={record["pid"] if active else 0}\nControlGroup={cgroup}\n'
              f'ActiveState={"active" if active else "inactive"}')
    elif "stop" in args:
        record = state[unit]
        for pid in reversed(child_pids(record)):
            if alive(pid):
                environment = Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
                assert f'RACK_RUNTIME_ACTIVATION={record["activation"]}'.encode() in environment
                os.kill(pid, signal.SIGTERM)
    else:
        raise AssertionError(args)


def nvidia():
    if any("query-gpu=memory" in arg for arg in args):
        print(16384)
    elif "-x" in args:
        uuid = args[args.index("-i") + 1]
        pids = [pid for record in state.values() if uuid in record["visible_devices"].split(",")
                for pid in child_pids(record) if alive(pid)]
        records = "".join(f"<process_info><pid>{pid}</pid><type>C</type></process_info>" for pid in pids)
        print(f"<nvidia_smi_log><gpu><uuid>{uuid}</uuid><processes>{records}</processes></gpu></nvidia_smi_log>")
    else:
        raise AssertionError(args)


if program == "systemd-run":
    launch()
elif program == "systemctl":
    systemctl()
else:
    nvidia()
if program != "nvidia-smi":
    with (root / "commands.jsonl").open("a") as output:
        output.write(json.dumps(dict(program=program, args=args)) + "\n")
temporary = root / "machine.next"
temporary.write_text(json.dumps(state))
temporary.replace(path)
