"""mini-brain contract proofs using private CPU fixtures, never live model evidence."""
from contextlib import contextmanager
import copy
import json
import os
from pathlib import Path
import sys
import tempfile
import time
import unittest

from support import Rack, ROOT, port
from test_dual_services import (BACKEND, alive, claims, counted_requests, digest,
    image_body, payload, records, release, request, wait_reservation)

TAG = "mini-brain"
MODEL = "qwen3.6-35b-a3b"
RESOURCES = ["gpu-4060ti", "gpu-2060"]
UUIDS = {resource: "GPU-fixture-" + resource for resource in RESOURCES}
PROFILE = ROOT / "config/runtime/profiles/mini-brain.json"


@contextmanager
def fixture():
    # The established host fixture enforces this private directory/command boundary.
    with tempfile.TemporaryDirectory(prefix="rack-dual-services-", dir="/dev/shm") as directory:
        root = Path(directory)
        binary = root / "bin"
        binary.mkdir()
        for name in ("systemd-run", "systemctl", "nvidia-smi"):
            command = binary / name
            command.write_text("#!" + sys.executable + "\n" +
                (ROOT / "tests/runtime/dual_services_host.py").read_text().split("\n", 1)[1])
            command.chmod(0o700)
        asset = root / "synthetic.gguf"
        asset.write_bytes(b"not-a-model")
        pin = root / "runtime-config.json"
        pin.write_text('{"synthetic":true}')

        def configure(config):
            template = config["profiles"][0]
            for resource, uuid in UUIDS.items():
                config["devices"][resource]["uuid"] = uuid
            candidate = copy.deepcopy(json.loads(PROFILE.read_text()))
            listen = port()
            candidate.update(qualified=True, evidence=["synthetic-only"],
                executable=template["executable"], executable_sha256=template["executable_sha256"],
                endpoint=f"http://127.0.0.1:{listen}",
                args=[str(BACKEND), str(listen), candidate["model"], str(root / "rack/events.jsonl"),
                      str(root / f"rack/{TAG}.control.json"), TAG, "--host", "127.0.0.1"],
                artifact=str(asset), artifact_sha256=digest(asset),
                device_mib={resource: 16 for resource in RESOURCES}, host_mib=32, cpu_percent=100,
                artifact_verify_seconds=3, startup_seconds=3, drain_seconds=3, stop_seconds=2,
                inference_seconds=5, launch=dict(working_directory=str(root),
                    cpu_affinity=sorted(os.sched_getaffinity(0))[:2],
                    environment={"RACK_DUAL_BOUND_VALUE": "bound-at-activation"},
                    file_pins=[dict(path=str(pin), sha256=digest(pin))]))
            config["profiles"].append(candidate)

        environment = dict(os.environ, RACK_DUAL_HOST_FIXTURE=str(root),
                           PATH=str(binary) + ":" + os.environ["PATH"])
        rack = Rack(root / "rack", configure=configure, environment=environment)
        rack.controls(TAG, context=32768)
        try:
            yield root, rack
        finally:
            rack.close()


def reserve(rack, priority="medium"):
    return rack.call("cb", "reserve", request=dict(acquisition_id=os.urandom(8).hex(),
        work_id="mini-fixture", services=[TAG], priority=priority, ttl_seconds=60))


def configure_count(rack, **changes):
    rack.controls(TAG, context=32768, **changes)


class MiniProfileContract(unittest.TestCase):
    def test_profile_preserves_proven_settings_uuid_order_and_isolated_runtime(self):
        profile = json.loads(PROFILE.read_text())
        example = json.loads((ROOT / "config/runtime/config.example.json").read_text())
        self.assertEqual(next(p for p in example["profiles"] if p["tag"] == TAG), profile)
        self.assertEqual((profile["backend"], profile["driver"], profile["model"]),
                         ("llama_cpp", "systemd", MODEL))
        self.assertFalse(profile["qualified"])
        self.assertEqual(profile["resources"], RESOURCES)
        self.assertEqual(set(profile["device_mib"]), set(RESOURCES))
        self.assertLessEqual(profile["device_mib"]["gpu-2060"], 6144)
        self.assertLessEqual(profile["device_mib"]["gpu-4060ti"], 16384)
        self.assertEqual((profile["context_tokens"], profile["max_input_tokens"],
                          profile["max_output_tokens"]), (32768, 24576, 8192))
        self.assertEqual(profile["launch"]["cpu_affinity"], list(range(16)))
        self.assertEqual(profile["cpu_percent"], 1600)
        self.assertNotIn("CUDA_VISIBLE_DEVICES", profile["launch"].get("environment", {}))
        self.assertNotEqual(profile["executable"], "/srv/rack-ai/runtimes/llama-cpp/llama-server")
        args = profile["args"]
        expected = {"--ctx-size": "32768", "--parallel": "1", "--device": "CUDA0,CUDA1",
            "--split-mode": "layer", "--tensor-split": "3,1", "--gpu-layers": "all",
            "--fit": "off", "--flash-attn": "on", "--cache-type-k": "q8_0",
            "--cache-type-v": "q8_0", "--threads": "16", "--threads-batch": "16",
            "--reasoning": "on", "--spec-type": "draft-mtp", "--spec-draft-n-max": "2",
            "--mmproj": "/srv/models/qwen3.6-35b-a3b/mmproj-BF16.gguf"}
        for flag, value in expected.items():
            self.assertEqual(args[args.index(flag) + 1], value, flag)
        self.assertIn("--jinja", args)
        self.assertIn("--metrics", args)
        self.assertEqual(profile["artifact"],
            "/srv/models/qwen3.6-35b-a3b/Qwen3.6-35B-A3B-UD-IQ4_XS.gguf")
        pins = {pin["path"] for pin in profile["launch"]["file_pins"]}
        self.assertIn(expected["--mmproj"], pins)
        self.assertTrue(profile["enforce_input_limits"])
        self.assertEqual(profile["protocols"], ["chat_completions"])
        self.assertEqual(profile["input_token_counter"], "native_multimodal")
        self.assertIn("visual", profile["capabilities"])
        self.assertEqual((profile["max_images_per_request"], profile["max_image_bytes"],
            profile["max_image_pixels"]), (2, 262144, 4194304))
        self.assertEqual(profile["image_mime_types"],
            ["image/png", "image/jpeg", "image/gif", "image/bmp"])


class MiniManagedLifecycle(unittest.TestCase):
    def test_discovery_reserve_hold_uuid_launch_and_release_cleanup(self):
        with fixture() as (root, rack):
            published = next(p for p in rack.call("cb", "discover")["tags"] if p["tag"] == TAG)
            self.assertEqual((published["context_tokens"], published["max_input_tokens"],
                published["max_output_tokens"]), (32768, 24576, 8192))
            self.assertEqual(published["image_input"]["max_images_per_request"], 2)
            self.assertEqual(published["input_preflight"]["media"], "images")
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            self.assertEqual(member["model"], MODEL)
            self.assertEqual(set(claims(rack)), set(RESOURCES))
            started = next(event for event in records(rack)
                if event["kind"] == "start" and event.get("tag") == TAG)
            self.assertEqual(started["visible_devices"], ",".join(UUIDS[r] for r in RESOURCES))
            self.assertNotIn("4080", started["visible_devices"])
            self.assertEqual(started["affinity"], sorted(os.sched_getaffinity(0))[:2])
            self.assertEqual(started["cwd"], str(root))
            time.sleep(.15)
            held = wait_reservation(rack, owner)
            self.assertEqual(held["services"][TAG]["generation"], member["generation"])
            commands = [json.loads(line) for line in (root / "commands.jsonl").read_text().splitlines()]
            launch = next(c["args"] for c in commands if c["program"] == "systemd-run")
            self.assertIn("--property=KillMode=control-group", launch)
            self.assertIn("--property=Restart=no", launch)
            release(rack, owner)
            self.assertEqual(claims(rack), {})
            self.assertFalse(alive(started["pid"]))
            self.assertEqual(request(rack, (member, "/chat/completions", payload(member)))[0], 409)
            self.assertEqual(request(rack, (member, "/chat/completions/preflight", payload(member)))[0], 409)

    def test_two_gpu_conflicts_and_priority_transfer_preserve_unrelated_4080(self):
        with fixture() as (_, rack):
            unrelated = rack.wait(rack.acquire("athba", "comfyui", "medium"))
            for tag in ("local-primary", "local-coder"):
                blocker = rack.wait(rack.acquire("athba", tag, "medium"))
                denied = reserve(rack)
                self.assertEqual(denied["state"], "unavailable")
                self.assertEqual(rack.inspect(blocker)["state"], "ready")
                self.assertEqual(rack.inspect(unrelated)["state"], "ready")
                rack.release(blocker)
                rack.wait(blocker, "released")
            self.assertFalse(any(e["kind"] == "start" and e.get("tag") == TAG for e in records(rack)))
            owner = wait_reservation(rack, reserve(rack, "low"))
            member = owner["services"][TAG]
            high = rack.wait(rack.acquire("athba", "local-primary", "high"))
            wait_reservation(rack, owner, "preempted")
            self.assertEqual(rack.inspect(unrelated)["state"], "ready")
            self.assertNotIn("gpu-2060", claims(rack))
            self.assertEqual(request(rack, (member, "/chat/completions", payload(member)))[0], 409)
            rack.release(high)
            rack.wait(high, "released")
            self.assertEqual(wait_reservation(rack, owner, "preempted")["state"], "preempted")
            release(rack, owner)
            fresh = wait_reservation(rack, reserve(rack))
            self.assertNotEqual(fresh["services"][TAG]["generation"], member["generation"])
            release(rack, fresh)
            self.assertEqual(set(claims(rack)), {"gpu-4080-super"})
            self.assertEqual(rack.inspect(unrelated)["state"], "ready")
            self.assertEqual(rack.counts("stop")["comfyui"], 0)
            rack.release(unrelated)
            rack.wait(unrelated, "released")
            self.assertEqual(claims(rack), {})


class MiniChatContract(unittest.TestCase):
    def test_reasoning_completion_preserves_parameters_content_and_usage(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            body = payload(member, chat_template_kwargs={"enable_thinking": True})
            status, answer = request(rack, (member, "/chat/completions", body))
            self.assertEqual(status, 200, answer)
            self.assertEqual(answer["model"], MODEL)
            self.assertEqual(answer["choices"][0]["message"]["reasoning_content"], "Synthetic reason")
            self.assertTrue(answer["choices"][0]["message"]["content"])
            self.assertEqual(answer["usage"], dict(prompt_tokens=10, completion_tokens=8, total_tokens=18))
            self.assertTrue(all(call["body"] == body for call in counted_requests(rack)))
            release(rack, owner)

    def test_tool_call_round_trip_preserves_ids_arguments_and_history(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            tools = [dict(type="function", function=dict(name="lookup_temperature",
                description="Synthetic lookup", parameters=dict(type="object",
                properties=dict(city=dict(type="string")), required=["city"], additionalProperties=False)))]
            body = payload(member, tools=tools, tool_choice="required", parallel_tool_calls=False)
            status, answer = request(rack, (member, "/chat/completions", body))
            self.assertEqual(status, 200, answer)
            self.assertEqual(answer["choices"][0]["finish_reason"], "tool_calls")
            assistant = answer["choices"][0]["message"]
            tool = assistant["tool_calls"][0]
            self.assertEqual(json.loads(tool["function"]["arguments"]), {"city": "Dublin"})
            follow = copy.deepcopy(body)
            follow["messages"] += [assistant, dict(role="tool", tool_call_id=tool["id"], content='{"celsius":12}')]
            follow["tool_choice"] = "auto"
            status, answer = request(rack, (member, "/chat/completions", follow))
            self.assertEqual(status, 200, answer)
            self.assertEqual(answer["choices"][0]["finish_reason"], "stop")
            self.assertTrue(answer["choices"][0]["message"]["content"])
            self.assertEqual(counted_requests(rack)[-1]["body"], follow)
            release(rack, owner)


class MiniNativeImageContract(unittest.TestCase):
    def test_image_preflight_and_generation_forward_identical_complete_requests(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            body = image_body(member)
            body["chat_template_kwargs"] = {"enable_thinking": True}
            original = copy.deepcopy(body)
            # Match the fixture's actual generated prompt usage; this is transport proof.
            configure_count(rack, token_count=10)
            status, counted = request(rack, (member, "/chat/completions/preflight", body))
            self.assertEqual(status, 200, counted)
            self.assertTrue(counted["fits"])
            self.assertEqual(counted["input_tokens"], 10)
            self.assertEqual(counted["profile_hash"], member["profile_hash"])
            self.assertFalse(any(e["kind"] == "dispatch" for e in records(rack)))
            status, answer = request(rack, (member, "/chat/completions", body))
            self.assertEqual(status, 200, answer)
            self.assertEqual(answer["usage"]["prompt_tokens"], counted["input_tokens"])
            calls = counted_requests(rack)
            self.assertEqual([call["path"] for call in calls],
                ["/v1/chat/completions/input_tokens", "/v1/chat/completions/input_tokens", "/v1/chat/completions"])
            self.assertTrue(all(call["body"] == original for call in calls))
            self.assertEqual(body, original)
            release(rack, owner)

    def test_native_input_output_bounds_and_invalid_counter_fail_before_generation(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack))
            member = owner["services"][TAG]
            body = image_body(member)
            body["max_tokens"] = 8192
            configure_count(rack, token_count=24576)
            status, counted = request(rack, (member, "/chat/completions/preflight", body))
            self.assertEqual(status, 200, counted)
            self.assertTrue(counted["fits"])
            self.assertEqual(counted["total_reserved_tokens"], 32768)
            for count, output, reason in ((24577, 8192, "input_limit_exceeded"),
                                         (10, 8193, "output_limit_exceeded")):
                with self.subTest(reason=reason):
                    configure_count(rack, token_count=count)
                    body["max_tokens"] = output
                    status, counted = request(rack, (member, "/chat/completions/preflight", body))
                    self.assertEqual(status, 200, counted)
                    self.assertEqual((counted["fits"], counted["reason"]), (False, reason))
                    self.assertEqual(request(rack, (member, "/chat/completions", body))[0], 409)
            body["max_tokens"] = 32
            for count_reply in ({"object": "wrong", "input_tokens": 10},
                                {"object": "response.input_tokens", "input_tokens": True}):
                with self.subTest(count_reply=count_reply):
                    configure_count(rack, count_reply=count_reply)
                    status, rejected = request(rack, (member, "/chat/completions/preflight", body))
                    self.assertEqual(status, 502, rejected)
                    self.assertEqual(request(rack, (member, "/chat/completions", body))[0], 409)
            self.assertFalse(any(e["kind"] == "dispatch" for e in records(rack)))
            release(rack, owner)


if __name__ == "__main__":
    unittest.main(verbosity=2)
