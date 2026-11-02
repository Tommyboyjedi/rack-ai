"""maxi-brain contract proofs using private Docker fixtures, never live model evidence."""
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
import subprocess

from support import Rack, ROOT, port
from test_dual_services import wait_reservation, records, claims, alive, payload, request


TAG = "maxi-brain"
MODEL = "qwen3.6-35b-a3b"
RESOURCES = ["gpu-4080-super", "gpu-4060ti"]
UUIDS = {resource: "GPU-fixture-" + resource for resource in RESOURCES}
PROFILE = ROOT / "config/runtime/profiles/maxi-brain.json"


@contextmanager
def fixture():
    with tempfile.TemporaryDirectory(prefix="rack-maxi-brain-", dir="/dev/shm") as directory:
        root = Path(directory)
        binary = root / "bin"
        binary.mkdir()
        for name in ("docker", "nvidia-smi"):
            command = binary / name
            command.write_text("#!" + sys.executable + "\n" +
                (ROOT / "tests/runtime/fake_host.py").read_text().split("\n", 1)[1])
            command.chmod(0o700)
        model_artifact = root / "qwen3_6_35b_a3b.ninfer"
        model_artifact.write_bytes(b"not-a-ninfer-model")
        mmproj = root / "mmproj-BF16.gguf"
        mmproj.write_bytes(b"not-a-mmproj")

        def configure(config):
            listen = port()
            candidate = copy.deepcopy(json.loads(PROFILE.read_text()))
            for resource, uuid in UUIDS.items():
                config["devices"][resource]["uuid"] = uuid
            candidate.update(
                qualified=True,
                evidence=["synthetic-only"],
                executable=str(binary / "docker"),
                executable_sha256=hashlib.sha256((binary / "docker").read_bytes()).hexdigest(),
                container_image="sha256:" + "a" * 64,
                endpoint=f"http://127.0.0.1:{listen}",
                args=[str(ROOT / "tests/runtime/backend.py"), str(listen),
                      candidate["model"], str(root / "rack/events.jsonl"),
                      str(root / "rack" / (TAG + ".control.json")), "--host", "127.0.0.1"],
                device_mib={resource: 16 for resource in RESOURCES},
                host_mib=32,
                cpu_percent=100,
                startup_seconds=3,
                drain_seconds=3,
                stop_seconds=2,
                inference_seconds=5,
            )
            config["profiles"].append(candidate)

        environment = dict(os.environ, RACK_HOST_FIXTURE=str(root),
                           PATH=str(binary) + ":" + os.environ["PATH"])
        rack = Rack(root / "rack", configure=configure, environment=environment)
        rack.controls(TAG, context=131072)
        try:
            yield root, rack
        finally:
            rack.close()


def reserve(rack, priority="medium"):
    return rack.call("cb", "reserve", request=dict(acquisition_id=os.urandom(8).hex(),
        work_id="maxi-fixture", services=[TAG], priority=priority, ttl_seconds=60))


class MaxiProfileContract(unittest.TestCase):
    def test_profile_preserves_ninfer_backend_docker_driver_and_resources(self):
        profile = json.loads(PROFILE.read_text())
        example = json.loads((ROOT / "config/runtime/config.example.json").read_text())
        self.assertEqual(next(p for p in example["profiles"] if p["tag"] == TAG), profile)
        self.assertEqual((profile["backend"], profile["driver"], profile["model"]),
                          ("ninfer", "docker", MODEL))
        self.assertFalse(profile["qualified"])
        self.assertEqual(profile["resources"], RESOURCES)
        self.assertEqual(set(profile["device_mib"]), set(RESOURCES))
        self.assertEqual((profile["context_tokens"], profile["max_input_tokens"],
                           profile["max_output_tokens"]), (131072, 122880, 8192))
        self.assertIn("--path", profile["args"])
        self.assertIn("--devices", profile["args"])
        self.assertIn("--max-context", profile["args"])
        self.assertIn("--kv-capacity", profile["args"])
        self.assertIn("--kv-dtype", profile["args"])
        self.assertIn("--gdn-state-fp16", profile["args"])
        self.assertIn("--rope-yarn", profile["args"])
        self.assertIn("--spec", profile["args"])
        self.assertIn("--draft-tokens", profile["args"])
        self.assertIn("--max-concurrency", profile["args"])
        self.assertNotIn("--tensor-parallel", profile["args"])
        self.assertFalse(profile.get("enforce_input_limits", False))
        self.assertEqual(profile["protocols"], ["chat_completions"])
        self.assertNotIn("input_token_counter", profile)
        self.assertIn("visual", profile["capabilities"])
        self.assertEqual((profile["max_images_per_request"], profile["max_image_bytes"],
            profile["max_image_pixels"]), (2, 262144, 4194304))
        self.assertEqual(profile["image_mime_types"],
            ["image/png", "image/jpeg", "image/gif", "image/bmp"])
        self.assertIn("container_image", profile)
        self.assertEqual(profile["container_image"], "ghcr.io/iamwavecut/ninfer-all:cuda12.9")
        self.assertIn("container_mounts", profile)
        self.assertIn("/srv/models", profile["container_mounts"])

    def test_profile_in_config_example_matches_standalone_file(self):
        example = json.loads((ROOT / "config/runtime/config.example.json").read_text())
        standalone = json.loads(PROFILE.read_text())
        self.assertEqual(standalone,
            next(p for p in example["profiles"] if p["tag"] == TAG))


class MaxiManagedLifecycle(unittest.TestCase):
    def test_discovery_reserve_start_and_release_docker_container(self):
        with fixture() as (root, rack):
            published = next(p for p in rack.call("cb", "discover")["tags"] if p["tag"] == TAG)
            self.assertEqual((published["context_tokens"], published["max_input_tokens"],
                published["max_output_tokens"]), (131072, 122880, 8192))
            self.assertEqual(published["image_input"]["max_images_per_request"], 2)
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            self.assertEqual(member["model"], MODEL)
            self.assertEqual(set(claims(rack)), set(RESOURCES))
            started = next(event for event in records(rack)
                if event["kind"] == "start" and event.get("model") == MODEL)
            commands = [json.loads(line) for line in (root / "commands.jsonl").read_text().splitlines()]
            argv = next(c["args"] for c in commands if c["args"][0] == "run")
            self.assertEqual([argv[i + 1] for i, item in enumerate(argv) if item == "--gpus"],
                             ["device=" + UUIDS[r] for r in RESOURCES])
            release(rack, owner)
            self.assertEqual(claims(rack), {})
            self.assertFalse(alive(started["pid"]))

    def test_reject_inference_after_release(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            release(rack, owner)
            wait_reservation(rack, owner, "released")
            status, _ = request(rack, (member, "/chat/completions", payload(member)))
            self.assertEqual(status, 409)


class MaxiDockerValidation(unittest.TestCase):
    def test_docker_driver_requires_container_image_and_validates_mounts(self):
        with fixture() as (root, rack):
            for changes in ({"container_image": None}, {"container_mounts": {"relative": "/abs"}}):
                config = copy.deepcopy(rack.config)
                next(p for p in config["profiles"] if p["tag"] == TAG).update(changes)
                path = root / "invalid-config.json"
                path.write_text(json.dumps(config))
                path.chmod(0o600)
                checked = subprocess.run([str(ROOT / "target/debug/rack_ai_runtime"), "validate", str(path)],
                                         capture_output=True, text=True, timeout=5)
                self.assertNotEqual(checked.returncode, 0)
                self.assertIn("invalid pinned container profile", checked.stderr)

    def test_docker_argv_has_separate_gpus_flags_per_device(self):
        """Multi-GPU Docker profiles must emit --gpus device=<UUID> per device,
        never a single --gpus device=UUID1,UUID2 which Docker rejects (exit 125)."""
        with fixture() as (root, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            # Trigger a container start to populate commands.jsonl
            self.assertIsNotNone(member)
            release(rack, owner)
            # Read the captured docker run argv
            commands = [json.loads(l) for l in
                        (root / 'commands.jsonl').read_text().splitlines()]
            run_argv = next((c['args'] for c in commands if c['args'][0] == 'run'), None)
            self.assertIsNotNone(run_argv, "no docker run call recorded")
            # Collect all --gpus values
            gpus_indices = [i for i, a in enumerate(run_argv) if a == '--gpus']
            self.assertGreater(len(gpus_indices), 0,
                "docker run must include --gpus flags")
            # Each --gpus must be followed by a single device=UUID value
            gpus_values = [run_argv[i + 1] for i in gpus_indices]
            self.assertEqual(len(gpus_values), len(gpus_indices),
                "each --gpus must have a following device value")
            for val in gpus_values:
                self.assertNotIn(',', val,
                    f"--gpus value must not contain commas: {val}")
                self.assertTrue(val.startswith('device='),
                    f"--gpus value must start with device=: {val}")
            # Verify all two resource UUIDs appear
            uuids_in_argv = [v[len('device='):] for v in gpus_values]
            for resource in RESOURCES:
                self.assertIn(UUIDS[resource], uuids_in_argv,
                    f"UUID for {resource} not found in --gpus flags")


def release(rack, reservation):
    rack.call("cb", "release_reservation", reservation_id=reservation["id"])
    wait_reservation(rack, reservation, "released")


if __name__ == "__main__":
    unittest.main(verbosity=2)
