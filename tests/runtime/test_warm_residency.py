import hashlib
import json
import os
import sys
import tempfile
import time
import unittest
from pathlib import Path

from support import Rack, ROOT, VERSION


def docker_rack(root):
    bin_dir = root / "bin"
    bin_dir.mkdir()
    for name in ["docker", "nvidia-smi"]:
        path = bin_dir / name
        path.write_text("#!" + sys.executable + "\n" + (ROOT / "tests/runtime/fake_host.py").read_text().split("\n", 1)[1])
        path.chmod(0o700)

    def configure(config):
        for profile in config["profiles"]:
            if profile["backend"] != "vllm":
                continue
            profile["driver"] = "docker"
            profile["container_image"] = "sha256:" + "a" * 64
            profile["executable"] = str(bin_dir / "docker")
            profile["executable_sha256"] = hashlib.sha256((bin_dir / "docker").read_bytes()).hexdigest()
            profile["args"] += ["--host", "127.0.0.1"]

    environment = dict(os.environ, RACK_HOST_FIXTURE=str(root), PATH=str(bin_dir) + ":" + os.environ["PATH"])
    return Rack(root / "rack", configure=configure, environment=environment)


def managed(rack):
    return json.loads((rack.root / "authority" / "managed.json").read_text())


def wait_warm(rack, count=1):
    deadline = time.monotonic() + 5
    doc = managed(rack)
    while time.monotonic() < deadline:
        doc = managed(rack)
        if len(doc["data"].get("warm_residencies", {})) == count:
            return doc
        time.sleep(0.03)
    raise AssertionError(doc)


class WarmResidencyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix="rack-pr40-warm-")
        self.rack = docker_rack(Path(self.directory.name))

    def tearDown(self):
        self.rack.close()
        self.directory.cleanup()

    def stale_inference(self, reservation):
        request = dict(
            schema=VERSION,
            submission_id="stale-after-release",
            reservation_id=reservation["id"],
            generation=reservation["generation"],
            profile_hash=reservation["profile_hash"],
            prompt="hello",
            max_tokens=16,
            timeout_seconds=5,
        )
        return self.rack.call(reservation["owner"], "infer", status=409, request=request)

    def test_release_caches_backend_and_same_profile_adopts_without_second_start(self):
        r = self.rack
        first = r.wait(r.acquire("athba", "local-primary", "low", identity="first"))
        first_process = first["process"]
        self.assertNotEqual(first["generation"], first_process["activation"])
        r.result(r.infer(first))

        r.release(first)
        released = r.wait(first, "released")
        self.assertIsNone(released["process"])
        stale = self.stale_inference(first)
        self.assertEqual(stale["error"], "reservation_not_dispatchable")
        doc = wait_warm(r)
        self.assertEqual(doc["claims"], {})
        self.assertEqual(r.counts("stop")["local-primary"], 0)
        warm = next(iter(doc["data"]["warm_residencies"].values()))
        self.assertEqual(warm["process"], first_process)

        second = r.wait(r.acquire("cb", "local-primary", "low", identity="second"))
        self.assertNotEqual(second["id"], first["id"])
        self.assertNotEqual(second["generation"], first["generation"])
        self.assertEqual(second["process"], first_process)
        self.assertEqual(r.counts("start")["local-primary"], 1)
        self.assertEqual(managed(r)["data"].get("warm_residencies", {}), {})
        r.result(r.infer(second))
        r.release(second)
        r.wait(second, "released")
        wait_warm(r)

    def test_receiver_restart_revalidates_warm_residency(self):
        r = self.rack
        first = r.wait(r.acquire("athba", "local-primary", "low", identity="restart-first"))
        first_process = first["process"]
        r.release(first)
        r.wait(first, "released")
        wait_warm(r)

        r.process.kill()
        r.process.wait(timeout=5)
        r.log.close()
        r.start()
        wait_warm(r)

        second = r.wait(r.acquire("cb", "local-primary", "low", identity="restart-second"))
        self.assertEqual(second["process"], first_process)
        self.assertEqual(r.counts("start")["local-primary"], 1)

    def test_incompatible_profile_pressure_evicts_warm_cache(self):
        r = self.rack
        primary = r.wait(r.acquire("athba", "local-primary", "low", identity="evict-primary"))
        r.release(primary)
        r.wait(primary, "released")
        wait_warm(r)

        chat = r.wait(r.acquire("cb", "local-fun-chat", "low", identity="evict-chat"))
        self.assertEqual(r.counts("stop")["local-primary"], 1)
        self.assertEqual(r.counts("start")["local-fun-chat"], 1)
        self.assertEqual(managed(r)["data"].get("warm_residencies", {}), {})
        r.result(r.infer(chat))


if __name__ == "__main__":
    unittest.main(verbosity=2)
