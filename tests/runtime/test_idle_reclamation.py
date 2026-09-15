"""Idle reclamation uses real fixture processes and delayed GPU-release observations."""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
from support import Rack, ROOT

def expire_clock(r, demand):
    root = Path(r.config["authority_root"])
    with (root/"authority.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        path = root/"managed.json"
        state = json.loads(path.read_text())
        state["data"]["demands"][demand["id"]]["last_activity_at"] = int(time.time()) - 1800
        temporary = root/"idle-test.next"
        temporary.write_text(json.dumps(state))
        temporary.chmod(0o600)
        temporary.replace(path)

class IdleReclamationTests(unittest.TestCase):
    def exercise(self, options):
        driver, fault = options
        with tempfile.TemporaryDirectory(prefix="rack-idle-reclaim-") as directory:
            root = Path(directory)
            commands = root/"bin"; commands.mkdir()
            for name in ("systemd-run", "systemctl", "docker", "nvidia-smi"):
                path = commands/name
                path.write_text("#!"+sys.executable+"\n"+
                    (ROOT/"tests/runtime/fake_host.py").read_text().split("\n", 1)[1])
                path.chmod(0o700)
            def configure(c):
                self.assertEqual(c.get("idle_timeout_seconds", 1800), 1800)
                p = c["profiles"][2]
                p.update(driver=driver, resources=["gpu-2060"], device_mib={"gpu-2060":16},
                    stop_seconds=1)
                p["args"] += ["--host", "127.0.0.1"]
                if driver == "docker":
                    p.update(container_image="sha256:"+"a"*64, executable=str(commands/"docker"),
                        executable_sha256=hashlib.sha256((commands/"docker").read_bytes()).hexdigest())
            environment = dict(os.environ, RACK_HOST_FIXTURE=str(root),
                PATH=str(commands)+":"+os.environ["PATH"])
            r = Rack(root/"rack", configure, environment)
            try:
                coder = r.wait(r.acquire("athba", "local-coder", "low"))
                resident = r.wait(r.acquire("cb", "local-fun-chat", "paramount"))
                r.wait(coder, "held")
                if fault == "running":
                    r.controls("local-fun-chat", delay=3)
                    invocation = r.infer(resident)
                    deadline = time.monotonic()+3
                    while time.monotonic()<deadline:
                        current = r.call("cb", "result", invocation_id=invocation["id"])
                        if current["state"] == "started": break
                        time.sleep(.02)
                    self.assertEqual(current["state"], "started")
                    expire_clock(r, resident)
                    time.sleep(.5)
                    self.assertEqual(r.inspect(resident)["state"], "ready")
                    self.assertEqual(r.counts("stop")["local-fun-chat"], 0)
                    self.assertEqual(r.result(invocation)["state"], "completed")
                    self.assertGreaterEqual(r.inspect(resident)["last_activity_at"], current["started"])
                # GPU residency after exit is transient, and free-memory capacity is not cleanup.
                faults = {"gpu_cleanup_seconds":10 if fault=="cleanup" else .6, "memory_mib":1}
                (root/"faults.json").write_text(json.dumps(faults))
                expire_clock(r, resident)
                expected = "recovery_required" if fault=="cleanup" else "expired"
                terminal = r.wait(resident, expected)
                self.assertEqual(r.counts("stop")["local-fun-chat"], 1)
                if fault == "cleanup":
                    self.assertEqual(terminal["reason"], "gpu_cleanup_deadline")
                    state = json.loads((Path(r.config["authority_root"])/"managed.json").read_text())
                    self.assertEqual(state["claims"]["gpu-2060"], resident["id"])
                    self.assertEqual(r.inspect(coder)["state"], "held")
                    return
                self.assertEqual(terminal["reason"], "idle_timeout")
                self.assertIsNone(terminal["process"])
                restored = r.wait(coder)
                self.assertNotEqual(restored["generation"], coder["generation"])
                self.assertEqual(r.result(r.infer(restored))["state"], "completed")
                r.release(restored); r.wait(restored, "released")
            finally: r.close()

    def test_loaded_backend_and_delayed_vram_reclaim_then_restore(self):
        for driver in ("systemd", "docker"):
            with self.subTest(driver=driver): self.exercise((driver, None))

    def test_inflight_blocks_idle_until_completion(self):
        self.exercise(("systemd", "running"))

    def test_failed_gpu_cleanup_keeps_recovery_claim(self):
        self.exercise(("systemd", "cleanup"))
