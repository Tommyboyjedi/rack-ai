from contextlib import contextmanager
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import time
import requests

REPO = Path(__file__).resolve().parents[2]
TOKEN = "synthetic-operator-credential-not-a-live-secret"
CLIENT = "synthetic-director-credential-not-a-live-secret"


def port():
    with socket.socket() as server:
        server.bind(("127.0.0.1", 0))
        return server.getsockname()[1]


def wait_for(action, timeout=35):
    deadline = time.monotonic() + timeout
    last = None
    while time.monotonic() < deadline:
        try:
            value = action()
            if value:
                return value
        except (requests.RequestException, ValueError, KeyError) as error:
            last = error
        time.sleep(0.15)
    raise AssertionError(f"Fixture deadline: {last}")


@contextmanager
def receiver(root, configure=None):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    # Hold all selections together so the kernel cannot return one port twice.
    selected = [socket.socket() for _ in range(3)]
    try:
        for listener in selected:
            listener.bind(("127.0.0.1", 0))
        ports = [listener.getsockname()[1] for listener in selected]
    finally:
        for listener in selected:
            listener.close()
    backend, api, native = [f"http://127.0.0.1:{p}" for p in ports]
    for name in ["output", "state", "resources", "bin"]:
        (root / name).mkdir()
    (root / "control.secret").write_text("synthetic-private-control-key-not-shared-with-browser")
    (root / "authority.json").write_text("{}")
    checkpoint = root / "fixture.safetensors"
    checkpoint.write_bytes(b"synthetic checkpoint, no GPU")
    config = {"listen": f"127.0.0.1:{ports[1]}", "native_listen": f"127.0.0.1:{ports[2]}",
        "public_origin": api, "native_origin": native, "backend": backend,
        "state_root": str(root / "state"), "resource_root": str(root / "resources"), "output_root": str(root / "output"),
        "authority_file": str(root / "authority.json"), "control_secret_file": str(root / "control.secret"),
        "unit": "rack-ai-comfyui-fixture.service", "media_uuid": "GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b",
        "protected": [{"container": "coder", "uuid": "GPU-357ef569-8fac-7c7d-ee1c-51677efb174f"},
                      {"container": "primary", "uuid": "GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c"}],
        "profile": {"id": "local-image", "version": 1, "checkpoint": str(checkpoint),
            "checkpoint_sha256": hashlib.sha256(checkpoint.read_bytes()).hexdigest(),
            "runtime_revision": "d43a5fa20c8547ff42d13232f589a06536c42b97", "available": True},
        "start_timeout": 20, "drain_timeout": 10, "stop_timeout": 10, "idle_seconds": 3, "session_seconds": 3600,
        "min_memory_mb": 1, "min_disk_mb": 1,
        "principals": [{"id": "operator", "token_sha256": hashlib.sha256(TOKEN.encode()).hexdigest(), "operator": True},
                       {"id": "director", "token_sha256": hashlib.sha256(CLIENT.encode()).hexdigest(), "operator": False}]}
    config["runtime"] = {"python": sys.executable, "script": str(REPO / "tests/media/fake_comfy.py"),
                         "directory": str(Path.cwd())}
    if configure:
        configure(config)
    (root / "config.json").write_text(json.dumps(config))
    os.chmod(root / "config.json", 0o600)
    for program in ["systemctl", "nvidia-smi", "docker"]:
        path = root / "bin" / program
        path.write_text(f"#!{sys.executable}\n" + (REPO / "tests/media/fake_machine.py").read_text().split("\n", 1)[1])
        os.chmod(path, 0o700)
    environment = dict(os.environ, RACK_MEDIA_FIXTURE=str(root), PATH=str(root / "bin") + ":" + os.environ["PATH"])
    with (root / "comfy.log").open("w") as comfy_log, (root / "receiver.log").open("w") as receiver_log:
        comfy_environment = dict(environment, CUDA_VISIBLE_DEVICES=config["media_uuid"],
            RACK_MEDIA_AUTHORITY_FILE=config["authority_file"], RACK_MEDIA_CONTROL_SECRET_FILE=config["control_secret_file"])
        comfy = subprocess.Popen([sys.executable, str(REPO / "tests/media/fake_comfy.py"), str(root), str(ports[0])],
            stdout=comfy_log, stderr=subprocess.STDOUT, env=comfy_environment)
        process = None
        children = [comfy]
        try:
            wait_for(lambda: requests.get(backend + "/fixture/control", timeout=1).status_code == 200)
            binary = os.environ.get("RACK_MEDIA_BINARY", str(REPO / "target/debug/rack_ai_media"))
            process = subprocess.Popen([binary, str(root / "config.json")], env=environment, stdout=receiver_log, stderr=subprocess.STDOUT)
            children.append(process)
            def restart():
                previous = children[-1]
                previous.kill()
                previous.wait(timeout=5)
                replacement = subprocess.Popen([binary, str(root / "config.json")], env=environment, stdout=receiver_log, stderr=subprocess.STDOUT)
                children.append(replacement)
                wait_for(lambda: requests.get(api + "/api/media/v1/status", headers={"Authorization": "Bearer " + CLIENT}, timeout=2).status_code == 200)
                return replacement
            headers = {"Authorization": "Bearer " + CLIENT}
            def startup_ready():
                if process.poll() is not None:
                    raise AssertionError("Media receiver exited during fixture startup: " + (root / "receiver.log").read_text())
                reply = requests.get(api + "/api/media/v1/status", headers=headers, timeout=2)
                return reply.status_code == 200
            try:
                wait_for(startup_ready)
            except Exception as error:
                raise AssertionError(f"Media fixture startup failed: ports={ports}; receiver={(root / 'receiver.log').read_text()}; backend={(root / 'comfy.log').read_text()}") from error
            yield {"root": root, "api": api, "native": native, "backend": backend, "process": process,
                   "config": config, "headers": headers, "environment": environment, "restart": restart}
        finally:
            machine = json.loads((root / "machine.json").read_text())
            pid = machine.get("pid")
            if pid in machine.get("spawned_pids", []):
                try:
                    binding = ("RACK_MEDIA_FIXTURE=" + str(root)).encode()
                    if binding in Path(f"/proc/{pid}/environ").read_bytes().split(bytes([0])):
                        import signal
                        os.kill(pid, signal.SIGTERM)
                except (ProcessLookupError, FileNotFoundError):
                    pass
            for child in reversed(children):
                if child and child.poll() is None:
                    child.terminate()
                    try:
                        child.wait(timeout=5)
                    except subprocess.TimeoutExpired:
                        child.kill()
                        child.wait(timeout=5)
