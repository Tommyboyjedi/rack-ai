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

from support import Rack, ROOT, port


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
                args=[
                    "--path", str(model_artifact),
                    "--devices", "0,1",
                    "--max-context", "917504",
                    "--kv-capacity", "917504",
                    "--kv-dtype", "rk8v4",
                    "--gdn-state-fp16",
                    "--rope-yarn",
                    "--spec", "mtp",
                    "--draft-tokens", "3",
                    "--max-concurrency", "1",
                    "--host", "127.0.0.1",
                ],
                device_mib={resource: 16 for resource in RESOURCES},
                host_mib=32,
                cpu_percent=100,
                startup_seconds=3,
                drain_seconds=3,
                stop_seconds=2,
                inference_seconds=5,
            )
            config["profiles"].append(candidate)

        environment = dict(os.environ, RACK_DUAL_HOST_FIXTURE=str(root),
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
        self.assertTrue(profile["enforce_input_limits"])
        self.assertEqual(profile["protocols"], ["chat_completions"])
        self.assertEqual(profile["input_token_counter"], "native_multimodal")
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
            owner = rack.wait(reserve(rack))
            member = owner["services"][TAG]
            self.assertEqual(member["model"], MODEL)
            self.assertEqual(set(rack.claims()), set(RESOURCES))
            started = next(event for event in rack.records()
                if event["kind"] == "start" and event.get("tag") == TAG)
            self.assertEqual(started["visible_devices"],
                ",".join(UUIDS[r] for r in RESOURCES))
            release(rack, owner)
            self.assertEqual(rack.claims(), {})
            self.assertFalse(rack.alive(started["pid"]))

    def test_reject_inference_after_release(self):
        with fixture() as (_, rack):
            from tests.runtime.test_dual_services import payload, request
            owner = rack.wait(reserve(rack))
            member = owner["services"][TAG]
            release(rack, owner)
            rack.wait(owner, "released")
            status, _ = request(rack, (member, "/chat/completions", payload(member)))
            self.assertEqual(status, 409)


class MaxiDockerValidation(unittest.TestCase):
    def test_docker_driver_requires_container_image_and_validates_mounts(self):
        with tempfile.TemporaryDirectory(prefix="rack-maxi-val-") as directory:
            root = Path(directory)
            bin_dir = root / "bin"
            bin_dir.mkdir()
            for name in ("docker", "nvidia-smi"):
                (bin_dir / name).write_text("#!" + sys.executable + "\n" +
                    (ROOT / "tests/runtime/fake_host.py").read_text().split("\n", 1)[1])
                (bin_dir / name).chmod(0o700)

            def make_config(image, mounts=None):
                config = json.loads((ROOT / "config/runtime/config.example.json").read_text())
                profile = copy.deepcopy(json.loads(PROFILE.read_text()))
                profile.update(
                    container_image=image,
                    executable=str(bin_dir / "docker"),
                    executable_sha256=hashlib.sha256((bin_dir / "docker").read_bytes()).hexdigest(),
                )
                if mounts is not None:
                    profile["container_mounts"] = mounts
                config["profiles"].append(profile)
                return config

            env = dict(os.environ, RACK_DUAL_HOST_FIXTURE=str(root),
                        PATH=str(bin_dir) + ":" + os.environ["PATH"])

            # Missing container_image should fail validation
            r = Rack(root / "rack",
                configure=lambda c: c["profiles"].append(make_config(None)),
                environment=env)
            self.assertEqual(r.acquire("athba", TAG, "low")["state"], "unavailable")
            r.close()

            # Relative mount paths should fail validation
            r = Rack(root / "rack",
                configure=lambda c: c["profiles"].append(
                    make_config("sha256:" + "a" * 64,
                        {"relative/path": "/abs/path"})),
                environment=env)
            self.assertEqual(r.acquire("athba", TAG, "low")["state"], "unavailable")
            r.close()

            # Named tag image (not sha256) should be accepted
            r = Rack(root / "rack",
                configure=lambda c: c["profiles"].append(
                    make_config("ghcr.io/iamwavecut/ninfer-all:cuda12.9")),
                environment=env)
            owner = r.wait(reserve(r))
            self.assertEqual(owner["state"], "reserved")
            r.release(owner)
            r.close()


def release(rack, reservation):
    rack.release(reservation)
    rack.wait(reservation, "released")


if __name__ == "__main__":
    unittest.main(verbosity=2)
