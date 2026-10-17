"""Synthetic tiny-uncd lifecycle proofs; no real GPU, model or live authority."""
from contextlib import contextmanager
import copy
import hashlib
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest
import urllib.request

from support import Rack, ROOT, port

TAG = "tiny-uncd"
PROFILE = ROOT / "config/runtime/profiles/tiny-uncd.json"
FIXTURE_UUID = "GPU-tiny-fixture-2060"


@contextmanager
def fixture(qualified=True):
    # RAM-backed state and shadow host commands cannot touch production authority.
    with tempfile.TemporaryDirectory(prefix="rack-tiny-uncd-", dir="/dev/shm") as directory:
        root = Path(directory)
        binary = root / "bin"
        binary.mkdir()
        for name in ("systemd-run", "systemctl", "nvidia-smi"):
            path = binary / name
            path.write_text("#!" + sys.executable + "\n" +
                            (ROOT / "tests/runtime/tiny_uncd_host.py").read_text().split("\n", 1)[1])
            path.chmod(0o700)
        artifact = root / "synthetic.gguf"
        artifact.write_bytes(b"synthetic-only-not-a-model")

        def configure(config):
            candidate = copy.deepcopy(json.loads(PROFILE.read_text()))
            template = config["profiles"][0]
            listen = port()
            candidate.update(
                qualified=qualified, evidence=["synthetic-process-only"] if qualified else [],
                executable=template["executable"], executable_sha256=template["executable_sha256"],
                args=[str(ROOT / "tests/runtime/backend.py"), str(listen), TAG,
                      str(root / "rack/events.jsonl"), str(root / "rack/tiny-uncd.control.json"),
                      "--host", "127.0.0.1"], endpoint=f"http://127.0.0.1:{listen}",
                artifact=str(artifact), artifact_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest(),
                device_mib={"gpu-2060": 16}, host_mib=32, cpu_percent=100,
                artifact_verify_seconds=3, startup_seconds=3, drain_seconds=3,
                stop_seconds=2, inference_seconds=5)
            config["devices"]["gpu-2060"]["uuid"] = FIXTURE_UUID
            config["profiles"].append(candidate)

        environment = dict(os.environ, RACK_TINY_HOST_FIXTURE=str(root),
                           PATH=str(binary) + ":" + os.environ["PATH"])
        rack = Rack(root / "rack", configure=configure, environment=environment)
        try:
            yield root, rack
        finally:
            rack.close()


def reserve(rack, **changes):
    request = dict(acquisition_id=os.urandom(8).hex(), work_id="synthetic-test",
                   services=[TAG], priority="medium", ttl_seconds=60)
    request.update(changes)
    return rack.call("cb", "reserve", request=request)


def wait_reservation(rack, reservation, state="ready"):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        current = rack.call("cb", "inspect_reservation", reservation_id=reservation["id"])
        if current["state"] == state:
            return current
        if current["state"] == "recovery_required":
            raise AssertionError(current)
        time.sleep(.03)
    raise AssertionError(current)


def chat(rack, member, body):
    request = urllib.request.Request(
        f"http://{rack.address}" + member["gateway_path"] + "/chat/completions",
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(request, timeout=8) as response:
        return json.loads(response.read())


def release(rack, reservation):
    rack.call("cb", "release_reservation", reservation_id=reservation["id"])
    return wait_reservation(rack, reservation, "released")


class TinyProfileRegistration(unittest.TestCase):
    def test_candidate_is_pinned_unqualified_and_uses_managed_2060(self):
        profile = json.loads(PROFILE.read_text())
        example = json.loads((ROOT / "config/runtime/config.example.json").read_text())
        self.assertEqual(next(p for p in example["profiles"] if p["tag"] == TAG), profile)
        self.assertEqual((profile["tag"], profile["model"]), (TAG, TAG))
        self.assertEqual((profile["backend"], profile["driver"]), ("llama_cpp", "systemd"))
        self.assertFalse(profile["qualified"])
        self.assertEqual(profile["evidence"], [])
        self.assertEqual(profile["resources"], ["gpu-2060"])
        self.assertLessEqual(profile["device_mib"]["gpu-2060"], 6144)
        self.assertEqual((profile["context_tokens"], profile["max_input_tokens"],
                          profile["max_output_tokens"]), (4096, 3072, 1024))
        self.assertEqual(profile["protocols"], ["chat_completions"])
        self.assertEqual(profile["artifact"], "/srv/models/qwen3.5-4b-heretic/model-Q6_K.gguf")
        for pin in ("artifact_sha256", "executable_sha256"):
            self.assertRegex(profile[pin], "^[0-9a-f]{64}$")
        args = profile["args"]
        expected = {"--alias": TAG, "--ctx-size": "4096", "--n-gpu-layers": "all",
                    "--parallel": "1", "--temp": "0", "--chat-template-kwargs": '{"enable_thinking":false}'}
        for option, value in expected.items():
            self.assertEqual(args[args.index(option) + 1], value)
        self.assertIn("--jinja", args)
        self.assertFalse(any("CUDA_VISIBLE_DEVICES" in arg for arg in args))

    def test_discovery_exposes_candidate_but_ordinary_reservation_fails_closed(self):
        with fixture(qualified=False) as (_, rack):
            discovered = next(p for p in rack.call("cb", "discover")["tags"] if p["tag"] == TAG)
            self.assertFalse(discovered["qualified"])
            self.assertEqual((discovered["context_tokens"], discovered["max_input_tokens"],
                              discovered["max_output_tokens"]), (4096, 3072, 1024))
            denied = reserve(rack)
            self.assertEqual(denied["state"], "unavailable")
            self.assertEqual(denied["services"][TAG]["reason"], "unqualified_profile")
            self.assertEqual(rack.counts("start")[TAG], 0)


class TinyManagedLifecycle(unittest.TestCase):
    def test_public_reservation_forwards_structured_controls_and_releases_owned_process(self):
        with fixture() as (root, rack):
            reservation = wait_reservation(rack, reserve(rack))
            member = reservation["services"][TAG]
            machine = json.loads((root / "machine.json").read_text())
            self.assertEqual(machine["visible_devices"], FIXTURE_UUID)
            # Fixed fixture text proves transport, not real model JSON generation
            # or JSON Schema enforcement.
            formats = [{"type": "json_object"}, {"type": "json_schema", "json_schema": {
                "name": "metadata", "strict": True, "schema": {"type": "object",
                "properties": {"location": {"type": "string"}}, "required": ["location"],
                "additionalProperties": False}}}]
            for response_format in formats:
                body = dict(model=TAG, messages=[dict(role="user", content="Extract current location.")],
                            max_tokens=16, temperature=0, response_format=response_format,
                            chat_template_kwargs={"enable_thinking": False})
                result = chat(rack, member, body)
                self.assertEqual(result["model"], TAG)
                self.assertEqual(result["choices"][0]["message"]["content"], "fixture response")
                forwarded = json.loads(Path(str(rack.events) + ".requests").read_text().splitlines()[-1])
                self.assertEqual(forwarded, body)
            release(rack, reservation)
            self.assertEqual(rack.counts("start")[TAG], 1)
            self.assertEqual(rack.counts("dispatch")[TAG], 2)
            self.assertEqual(rack.counts("stop")[TAG], 1)
            managed = json.loads((rack.root / "authority/managed.json").read_text())
            self.assertEqual(managed["claims"], {})
            commands = [json.loads(line) for line in (root / "commands.jsonl").read_text().splitlines()]
            launch = next(item["args"] for item in commands if item["program"] == "systemd-run")
            self.assertIn("--setenv=CUDA_VISIBLE_DEVICES=" + FIXTURE_UUID, launch)
            self.assertTrue(any(item["program"] == "systemctl" and "stop" in item["args"] for item in commands))


class TinyResourceContention(unittest.TestCase):
    def test_ties_lose_and_higher_priority_preempts_only_shared_2060(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack, services=["local-primary", TAG]))
            primary = owner["services"]["local-primary"]
            for priority in ("low", "medium"):
                denied = reserve(rack, services=["local-coder"], priority=priority)
                self.assertEqual(denied["state"], "unavailable")
                self.assertTrue(denied["services"]["local-coder"]["reason"].startswith("incumbent_priority:"))
            self.assertEqual(rack.counts("stop")[TAG], 0)
            high = wait_reservation(rack, reserve(rack, services=["local-coder"], priority="high"))
            displaced = wait_reservation(rack, owner, "partial")
            self.assertEqual(displaced["services"][TAG]["state"], "preempted")
            still_primary = displaced["services"]["local-primary"]
            self.assertEqual(still_primary["state"], "ready")
            self.assertEqual(still_primary["generation"], primary["generation"])
            self.assertEqual(still_primary["gateway_path"], primary["gateway_path"])
            result = chat(rack, still_primary, dict(model="local-primary", messages=[
                dict(role="user", content="Unrelated GPU remains available.")], max_tokens=16))
            self.assertEqual(result["model"], "local-primary")
            self.assertEqual(rack.counts("stop")["local-primary"], 0)
            self.assertEqual(rack.counts("start")["local-primary"], 1)
            self.assertEqual(rack.counts("stop")[TAG], 1)
            release(rack, high)
            release(rack, owner)
            managed = json.loads((rack.root / "authority/managed.json").read_text())
            self.assertEqual(managed["claims"], {})


if __name__ == "__main__":
    unittest.main(verbosity=2)
