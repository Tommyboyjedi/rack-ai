"""Two-card service proofs against private CPU fixtures, never real model evidence."""
from contextlib import contextmanager
import copy
import hashlib
import json
import os
from pathlib import Path
import signal
import sys
import tempfile
import time
import unittest
import urllib.error
import urllib.request

from support import Rack, ROOT, VERSION, port

TAGS = ("dual-brain", "dual-worker")
RESOURCES = ["gpu-4080-super", "gpu-4060ti"]
UUIDS = {resource: "GPU-fixture-" + resource for resource in RESOURCES}
BACKEND = ROOT / "tests/runtime/dual_services_backend.py"


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


@contextmanager
def fixture(changes=None):
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
        affinity = sorted(os.sched_getaffinity(0))[:2]

        def configure(config):
            template = config["profiles"][0]
            for resource, uuid in UUIDS.items():
                config["devices"][resource]["uuid"] = uuid
            for tag in TAGS:
                candidate = copy.deepcopy(json.loads((ROOT / f"config/runtime/profiles/{tag}.json").read_text()))
                listen = port()
                candidate.update(qualified=True, evidence=["synthetic-only"],
                    driver="fixture" if tag == "dual-brain" else "systemd",
                    executable=template["executable"], executable_sha256=template["executable_sha256"],
                    endpoint=f"http://127.0.0.1:{listen}",
                    args=[str(BACKEND), str(listen), candidate["model"], str(root / "rack/events.jsonl"),
                          str(root / f"rack/{tag}.control.json"), tag, "--host", "127.0.0.1"],
                    artifact=str(asset), artifact_sha256=digest(asset),
                    device_mib={resource: 16 for resource in RESOURCES}, host_mib=32, cpu_percent=100,
                    artifact_verify_seconds=3, startup_seconds=3, drain_seconds=3, stop_seconds=2,
                    inference_seconds=5, launch=dict(working_directory=str(root),
                        cpu_affinity=affinity if tag == "dual-worker" else [],
                        environment={"RACK_DUAL_BOUND_VALUE": "bound-at-activation",
                                     "RACK_DUAL_CONFIG_SHA256": digest(pin)},
                        file_pins=[dict(path=str(pin), sha256=digest(pin))]))
                candidate.update((changes or {}).get(tag, {}))
                config["profiles"].append(candidate)

        environment = dict(os.environ, RACK_DUAL_HOST_FIXTURE=str(root),
                           RACK_DUAL_CONFIG_SHA256=digest(pin),
                           PATH=str(binary) + ":" + os.environ["PATH"])
        rack = Rack(root / "rack", configure=configure, environment=environment)
        try:
            yield root, rack
        finally:
            rack.close()
            # Kill only private fixture children after unexpected failed assertions.
            for path in root.glob("*.engine.json"):
                pid = json.loads(path.read_text())["pid"]
                try:
                    owned = Path(f"/proc/{pid}/environ").read_bytes().split(b"\0")
                    if ("RACK_DUAL_HOST_FIXTURE=" + str(root)).encode() in owned:
                        os.kill(pid, signal.SIGKILL)
                except (FileNotFoundError, ProcessLookupError):
                    pass


def reserve(rack, tag):
    return rack.call("cb", "reserve", request=dict(acquisition_id=os.urandom(8).hex(),
        work_id="dual-fixture", services=[tag], priority="medium", ttl_seconds=60))


def wait_reservation(rack, reservation, state="ready"):
    deadline = time.monotonic() + 12
    while time.monotonic() < deadline:
        current = rack.call("cb", "inspect_reservation", reservation_id=reservation["id"])
        if current["state"] == state:
            return current
        if current["state"] == "recovery_required" and state != "recovery_required":
            raise AssertionError(current)
        time.sleep(.03)
    raise AssertionError(current)


def request(rack, call):
    member, suffix, body, *options = call
    owner = options[0] if options else "cb"
    identity = options[1] if len(options) > 1 else os.urandom(8).hex()
    req = urllib.request.Request("http://" + rack.address + member["gateway_path"] + suffix,
        data=json.dumps(body).encode(), headers={"Content-Type": "application/json",
        "Authorization": "Bearer " + owner, "Idempotency-Key": identity})
    try:
        response = urllib.request.urlopen(req, timeout=10)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        content_type = response.headers.get_content_type()
    value = raw.decode() if content_type == "text/event-stream" else json.loads(raw)
    return response.status, value


def payload(member, **changes):
    body = dict(model=member["model"], max_tokens=32, temperature=.7, top_p=.95,
                top_k=40, min_p=0, reasoning_effort="high",
                messages=[dict(role="system", content="Use the requested harmless function."),
                          dict(role="user", content="What is the temperature in Dublin?")])
    body.update(changes)
    return body


def release(rack, reservation):
    rack.call("cb", "release_reservation", reservation_id=reservation["id"])
    return wait_reservation(rack, reservation, "released")


def records(rack):
    return [json.loads(line) for line in rack.events.read_text().splitlines()] if rack.events.exists() else []


def claims(rack):
    return json.loads((rack.root / "authority/managed.json").read_text())["claims"]


def alive(pid):
    try:
        return Path(f"/proc/{pid}/stat").read_text().rsplit(") ", 1)[1].split()[0] != "Z"
    except FileNotFoundError:
        return False


class DualProfileRegistration(unittest.TestCase):
    def test_source_profiles_preserve_ceilings_backend_identity_and_uuid_order(self):
        example = json.loads((ROOT / "config/runtime/config.example.json").read_text())
        expected = {"dual-brain": ("strata", "qwen3.8-flash-next-unsloth-ud-iq4_xs"),
                    "dual-worker": ("llama_cpp", "nex-n2.5-mini")}
        for tag, identity in expected.items():
            with self.subTest(tag=tag):
                profile = json.loads((ROOT / f"config/runtime/profiles/{tag}.json").read_text())
                self.assertEqual(next(p for p in example["profiles"] if p["tag"] == tag), profile)
                self.assertEqual((profile["backend"], profile["model"]), identity)
                self.assertFalse(profile["qualified"])
                self.assertEqual(profile["resources"], RESOURCES)
                self.assertEqual((profile["context_tokens"], profile["max_input_tokens"],
                    profile["max_output_tokens"]), (131072, 65536, 65536))
                self.assertEqual(profile["protocols"], ["chat_completions"])
                self.assertTrue(profile["enforce_input_limits"])
                self.assertNotIn("visual", profile["capabilities"])
                self.assertGreater(profile["inference_seconds"], 120)
                self.assertNotIn("CUDA_VISIBLE_DEVICES", profile["launch"].get("environment", {}))
                self.assertNotIn("taskset", profile["executable"])

    def test_discovery_publishes_true_identity_and_ordinary_owner_can_hold_ready(self):
        with fixture() as (_, rack):
            published = {item["tag"]: item for item in rack.call("cb", "discover")["tags"]}
            for tag in TAGS:
                self.assertEqual((published[tag]["context_tokens"], published[tag]["max_input_tokens"],
                    published[tag]["max_output_tokens"]), (131072, 65536, 65536))
                actual_model = next(profile["model"] for profile in rack.config["profiles"] if profile["tag"] == tag)
                self.assertNotEqual(actual_model, tag)
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                self.assertEqual(member["model"], actual_model)
                self.assertEqual(set(claims(rack)), set(RESOURCES))
                time.sleep(.15)
                held = wait_reservation(rack, owner)
                self.assertEqual(held["services"][tag]["generation"], member["generation"])
                release(rack, owner)
                self.assertEqual(claims(rack), {})


    def test_strata_readiness_rejects_unloaded_wrong_model_and_wrong_context(self):
        with fixture() as (_, rack):
            rack.controls("dual-brain", loaded=False)
            owner = reserve(rack, "dual-brain")
            deadline = time.monotonic() + 2
            while not any(event["kind"] == "start" and event.get("tag") == "dual-brain"
                          for event in records(rack)):
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.01)
            for controls in ({"loaded": False}, {"model": "dual-brain"}, {"context": 4096}):
                rack.controls("dual-brain", **controls)
                time.sleep(.2)
                observed = rack.call("cb", "inspect_reservation", reservation_id=owner["id"])
                self.assertEqual(observed["state"], "preparing")
            rack.controls("dual-brain")
            ready = wait_reservation(rack, owner)
            release(rack, ready)


class DualChatTransport(unittest.TestCase):
    def test_tool_round_trip_preserves_messages_ids_arguments_reasoning_and_usage(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                tools = [dict(type="function", function=dict(name="lookup_temperature",
                    description="Synthetic lookup", parameters=dict(type="object",
                    properties=dict(city=dict(type="string")), required=["city"], additionalProperties=False)))]
                body = payload(member, tools=tools, tool_choice="required", parallel_tool_calls=False)
                status, answer = request(rack, (member, "/chat/completions", body))
                self.assertEqual(status, 200, answer)
                assistant = answer["choices"][0]["message"]
                self.assertEqual(answer["choices"][0]["finish_reason"], "tool_calls")
                self.assertEqual(assistant["reasoning_content"], "Synthetic reason")
                tool = assistant["tool_calls"][0]
                self.assertEqual(json.loads(tool["function"]["arguments"]), {"city": "Dublin"})
                follow = copy.deepcopy(body)
                follow["messages"] += [assistant, dict(role="tool", tool_call_id=tool["id"], content='{"celsius":12}')]
                follow["tool_choice"] = "auto"
                status, final = request(rack, (member, "/chat/completions", follow))
                self.assertEqual(status, 200, final)
                self.assertEqual(final["choices"][0]["finish_reason"], "stop")
                self.assertTrue(final["choices"][0]["message"]["content"])
                self.assertEqual(final["usage"], dict(prompt_tokens=10, completion_tokens=8, total_tokens=18))
                self.assertEqual(final["timings"]["predicted_per_second"], 42.5)
                forwarded = [json.loads(line) for line in Path(str(rack.events) + ".requests").read_text().splitlines()]
                self.assertEqual(forwarded[-1]["body"], follow)
                malformed = copy.deepcopy(follow)
                malformed["messages"][-2]["tool_calls"][0]["function"]["arguments"] = {"city": "Dublin"}
                self.assertEqual(request(rack, (member, "/chat/completions", malformed))[0], 409)
                release(rack, owner)

    def test_buffered_sse_preserves_reasoning_finish_usage_and_done(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                body = payload(member, stream=True, stream_options={"include_usage": True})
                status, raw = request(rack, (member, "/chat/completions", body))
                self.assertEqual(status, 200, raw)
                frames = [json.loads(line[6:]) for line in raw.splitlines()
                          if line.startswith("data: ") and line != "data: [DONE]"]
                self.assertEqual(frames[0]["model"], member["model"])
                self.assertEqual(frames[0]["choices"][0]["delta"]["reasoning_content"], "Synthetic reason")
                self.assertEqual(frames[-1]["choices"][0]["finish_reason"], "stop")
                self.assertEqual(frames[-1]["usage"]["completion_tokens"], 8)
                self.assertTrue(raw.endswith("data: [DONE]\n\n"))
                release(rack, owner)


class DualThinkingControls(unittest.TestCase):
    def test_native_nex_thinking_controls_survive_counting_and_inference_without_top_level_override(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack, "dual-worker"))
            member = owner["services"]["dual-worker"]
            body = payload(member, chat_template_kwargs={"enable_thinking": False, "reasoning_effort": "none"},
                response_format={"type": "json_schema", "json_schema": {"name": "native_no_thinking",
                    "strict": True, "schema": {"type": "object", "properties": {"answer": {"type": "string"}},
                        "required": ["answer"], "additionalProperties": False}}})
            del body["reasoning_effort"]
            original = copy.deepcopy(body)
            status, counted = request(rack, (member, "/chat/completions/preflight", body))
            self.assertEqual(status, 200, counted)
            self.assertTrue(counted["fits"])
            self.assertFalse(any(event["kind"] == "dispatch" and event.get("tag") == "dual-worker"
                                 for event in records(rack)))
            status, answer = request(rack, (member, "/chat/completions", body))
            self.assertEqual(status, 200, answer)
            self.assertEqual(body, original)
            calls = [json.loads(line) for line in Path(str(rack.events) + ".requests").read_text().splitlines()]
            rendered = [call["body"] for call in calls if call["path"] == "/apply-template"]
            generated = [call["body"] for call in calls if call["path"] == "/v1/chat/completions"]
            self.assertEqual(rendered, [original, original])
            self.assertEqual(generated, [original])
            for forwarded in rendered + generated:
                self.assertNotIn("reasoning_effort", forwarded)
                self.assertEqual(forwarded["chat_template_kwargs"],
                                 {"enable_thinking": False, "reasoning_effort": "none"})
            release(rack, owner)


class DualCounterAdmission(unittest.TestCase):
    def test_full_request_is_counted_without_generation_and_native_slack_is_enforced(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                reserve_tokens = 8 if tag == "dual-brain" else 0
                body = payload(member, max_tokens=65536, response_format={"type": "json_schema",
                    "json_schema": {"name": "neutral", "strict": True, "schema": {
                        "type": "object", "properties": {"answer": {"type": "string"}},
                        "required": ["answer"], "additionalProperties": False}}},
                    chat_template_kwargs={"enable_thinking": False})
                for count, fits, reason in ((65536 - reserve_tokens, True, "fits"),
                    (65537 - reserve_tokens, False, "context_limit_exceeded" if reserve_tokens else "input_limit_exceeded")):
                    rack.controls(tag, token_count=count)
                    status, report = request(rack, (member, "/chat/completions/preflight", body))
                    self.assertEqual(status, 200, report)
                    self.assertEqual((report["input_tokens"], report["requested_output_tokens"]), (count, 65536))
                    self.assertEqual(report.get("context_reserve_tokens", 0), reserve_tokens)
                    self.assertEqual(report["total_reserved_tokens"], count + 65536)
                    self.assertEqual((report["fits"], report["reason"]), (fits, reason))
                    for field in ("generation", "profile_hash", "profile_version", "model"):
                        self.assertEqual(report[field], member[field])
                self.assertEqual(sum(event["kind"] == "dispatch" and event["tag"] == tag
                                     for event in records(rack) if "tag" in event), 0)
                if tag == "dual-brain":
                    forwarded = json.loads(Path(str(rack.events) + ".requests").read_text().splitlines()[-1])
                    self.assertEqual(forwarded["path"], "/rackai/input-tokens")
                    self.assertEqual(forwarded["body"], body)
                release(rack, owner)

    def test_tool_history_is_forwarded_to_counter_and_cannot_bypass_output_ceiling(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                messages = [dict(role="user", content="Lookup Dublin"),
                    dict(role="assistant", content=None, tool_calls=[dict(id="call-1", type="function",
                        function=dict(name="lookup_temperature", arguments='{"city":"Dublin"}'))]),
                    dict(role="tool", tool_call_id="call-1", content='{"celsius":12}')]
                body = payload(member, messages=messages, max_tokens=65537)
                status, report = request(rack, (member, "/chat/completions/preflight", body))
                self.assertEqual(status, 200, report)
                self.assertFalse(report["fits"])
                self.assertEqual(report["reason"], "output_limit_exceeded")
                forwarded = [json.loads(line) for line in Path(str(rack.events) + ".requests").read_text().splitlines()]
                expected_path = "/rackai/input-tokens" if tag == "dual-brain" else "/apply-template"
                original = next(value["body"] for value in reversed(forwarded) if value["path"] == expected_path)
                self.assertEqual(original, body)
                self.assertEqual(request(rack, (member, "/chat/completions", body))[0], 409)
                self.assertFalse(any(event["kind"] == "dispatch" and event.get("tag") == tag
                                     for event in records(rack)))
                release(rack, owner)


    def test_explicit_qualification_owner_can_count_unqualified_profile_without_opening_ordinary_admission(self):
        with fixture({"dual-worker": {"qualified": False}}) as (_, rack):
            ordinary = reserve(rack, "dual-worker")
            self.assertEqual(ordinary["state"], "unavailable")
            self.assertEqual(ordinary["services"]["dual-worker"]["reason"], "unqualified_profile")
            profile = next(profile for profile in rack.config["profiles"] if profile["tag"] == "dual-worker")
            request_body = dict(schema=VERSION, source_system="cb", work_id="fixture-qualification",
                acquisition_id="ordinary-cannot-qualify", tag="dual-worker", priority="medium",
                capabilities=profile["capabilities"], context_tokens=131072, ttl_seconds=60, qualification=True)
            status, denied = request(rack, ({"gateway_path": "/runtime/v1"}, "",
                {"operation": "acquire", "request": request_body}))
            self.assertEqual(status, 403, denied)
            self.assertEqual(denied["error"], "qualification_mode_denied")
            qualified = rack.wait(rack.acquire("other", "dual-worker", "medium",
                qualification=True, context_tokens=131072))
            status, report = request(rack, (qualified, "/chat/completions/preflight", payload(qualified), "other"))
            self.assertEqual(status, 200, report)
            self.assertTrue(report["fits"])
            self.assertEqual(report["profile_hash"], qualified["profile_hash"])
            self.assertFalse(any(event["kind"] == "dispatch" and event.get("tag") == "dual-worker"
                                 for event in records(rack)))
            rack.release(qualified)
            rack.wait(qualified, "released")


class DualInferenceAdmission(unittest.TestCase):
    def test_skipping_preflight_cannot_generate_over_limit_and_failed_identity_replays(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                body = payload(member)
                rack.controls(tag, token_count=65537)
                status, rejected = request(rack, (member, "/chat/completions", body, "cb", "over-input-" + tag))
                self.assertEqual(status, 409, rejected)
                self.assertTrue(rejected["error"].startswith("invocation_Failed:"), rejected)
                invocation_id = rejected["error"].split(":", 1)[1]
                receipt = rack.call("cb", "result", invocation_id=invocation_id)
                self.assertEqual(receipt["state"], "failed")
                self.assertEqual(receipt["error"], "input_admission_limits_exceeded")
                self.assertFalse(any(event["kind"] == "dispatch" and event.get("tag") == tag
                                     for event in records(rack)))
                counted = Path(str(rack.events) + ".requests").read_text()
                replay_status, replay = request(rack, (member, "/chat/completions", body, "cb", "over-input-" + tag))
                self.assertEqual((replay_status, replay), (status, rejected))
                self.assertEqual(Path(str(rack.events) + ".requests").read_text(), counted)
                rack.controls(tag, token_count=10)
                status, answer = request(rack, (member, "/chat/completions", body))
                self.assertEqual(status, 200, answer)
                self.assertEqual(sum(event["kind"] == "dispatch" and event.get("tag") == tag
                                     for event in records(rack)), 1)
                release(rack, owner)


    def test_counter_cannot_spend_past_an_explicit_inference_deadline_or_generate(self):
        with fixture() as (_, rack):
            for tag in TAGS:
                owner = wait_reservation(rack, reserve(rack, tag))
                member = owner["services"][tag]
                rack.controls(tag, counter_delay=1.5)
                started = time.monotonic()
                invocation = rack.infer(dict(member, owner="cb"), identity="counter-deadline-" + tag,
                    prompt="", max_tokens=32, timeout_seconds=1,
                    payload=dict(protocol="chat_completions", body=payload(member)))
                result = rack.result(invocation, "failed")
                self.assertLess(time.monotonic() - started, 2.5, result)
                self.assertTrue(result["error"].startswith("input_admission_"), result)
                self.assertFalse(any(event["kind"] == "dispatch" and event.get("tag") == tag
                                     for event in records(rack)))
                release(rack, owner)


class DualResourceOwnership(unittest.TestCase):
    def test_atomic_two_gpu_claims_deny_ties_without_displacing_2060(self):
        with fixture() as (_, rack):
            unrelated = rack.wait(rack.acquire("athba", "local-coder", "medium"))
            for blocker in ("local-primary", "comfyui"):
                held = rack.wait(rack.acquire("athba", blocker, "medium"))
                denied = reserve(rack, "dual-worker")
                self.assertEqual(denied["state"], "unavailable")
                self.assertEqual(sum(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                     for event in records(rack)), 0)
                self.assertNotIn("gpu-4080-super" if blocker == "local-primary" else "gpu-4060ti", claims(rack))
                rack.release(held)
                rack.wait(held, "released")
            brain = wait_reservation(rack, reserve(rack, "dual-brain"))
            brain_member = brain["services"]["dual-brain"]
            denied = reserve(rack, "dual-worker")
            self.assertEqual(denied["state"], "unavailable")
            self.assertEqual(set(claims(rack)), set(RESOURCES + ["gpu-2060"]))
            self.assertEqual(rack.inspect(unrelated)["state"], "ready")
            release(rack, brain)
            self.assertEqual(set(claims(rack)), {"gpu-2060"})
            self.assertEqual(request(rack, (brain_member, "/chat/completions", payload(brain_member)))[0], 409)
            worker = wait_reservation(rack, reserve(rack, "dual-worker"))
            release(rack, worker)
            self.assertEqual(rack.result(rack.infer(unrelated))["state"], "completed")
            self.assertEqual(rack.counts("stop")["local-coder"], 0)
            rack.release(unrelated)
            rack.wait(unrelated, "released")

    def test_higher_priority_drains_running_and_cancels_queued_before_transfer(self):
        with fixture() as (_, rack):
            brain = wait_reservation(rack, reserve(rack, "dual-brain"))
            member = brain["services"]["dual-brain"]
            rack.controls("dual-brain", delay=.7)
            running = rack.infer(dict(member, owner="cb"), identity="running-dual")
            deadline = time.monotonic() + 3
            while not any(event["kind"] == "dispatch" and event.get("tag") == "dual-brain"
                          for event in records(rack)):
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.01)
            queued = rack.infer(dict(member, owner="cb"), identity="queued-dual")
            incoming = rack.call("cb", "reserve", request=dict(acquisition_id="higher-worker",
                work_id="dual-fixture", services=["dual-worker"], priority="high", ttl_seconds=60))
            self.assertEqual(rack.result(queued, "cancelled")["error"], "reservation_superseded_by_higher_priority")
            self.assertEqual(rack.result(running)["state"], "completed")
            incoming = wait_reservation(rack, incoming)
            displaced = rack.call("cb", "inspect_reservation", reservation_id=brain["id"])
            self.assertEqual(displaced["services"]["dual-brain"]["state"], "preempted")
            self.assertEqual(request(rack, (member, "/chat/completions", payload(member)))[0], 409)
            release(rack, incoming)
            self.assertEqual(claims(rack), {})
            self.assertEqual(rack.call("cb", "inspect_reservation", reservation_id=brain["id"])
                             ["services"]["dual-brain"]["state"], "preempted")

    def test_cancelled_running_call_retains_late_result_and_requires_real_release(self):
        with fixture() as (_, rack):
            owner = wait_reservation(rack, reserve(rack, "dual-brain"))
            member = owner["services"]["dual-brain"]
            rack.controls("dual-brain", delay=.6)
            running = rack.infer(dict(member, owner="cb"), identity="cancel-running")
            deadline = time.monotonic() + 3
            while not any(event["kind"] == "dispatch" and event.get("tag") == "dual-brain"
                          for event in records(rack)):
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.01)
            rack.call("cb", "cancel", invocation_id=running["id"])
            self.assertEqual(set(claims(rack)), set(RESOURCES))
            time.sleep(.75)
            receipt = rack.call("cb", "result", invocation_id=running["id"])
            self.assertEqual(receipt["state"], "cancelled")
            self.assertIsNone(receipt["result"])
            self.assertIsNotNone(receipt["late_result"])
            self.assertEqual(sum(event["kind"] == "complete" and event.get("tag") == "dual-brain"
                                 for event in records(rack)), 1)
            release(rack, owner)
            self.assertEqual(claims(rack), {})


class DualLaunchProvenance(unittest.TestCase):
    def test_systemd_launch_propagates_ordered_uuids_working_directory_env_and_affinity(self):
        with fixture() as (root, rack):
            owner = wait_reservation(rack, reserve(rack, "dual-worker"))
            started = next(event for event in records(rack)
                           if event["kind"] == "start" and event.get("tag") == "dual-worker")
            self.assertEqual(started["visible_devices"], ",".join(UUIDS[resource] for resource in RESOURCES))
            self.assertNotIn("2060", started["visible_devices"])
            self.assertEqual(started["cwd"], str(root))
            self.assertEqual(started["custom_environment"], "bound-at-activation")
            self.assertEqual(started["affinity"], sorted(os.sched_getaffinity(0))[:2])
            commands = [json.loads(line) for line in (root / "commands.jsonl").read_text().splitlines()]
            launch = next(command["args"] for command in commands if command["program"] == "systemd-run")
            self.assertIn("--property=KillMode=control-group", launch)
            self.assertIn("--property=Restart=no", launch)
            self.assertIn("--property=WorkingDirectory=" + str(root), launch)
            release(rack, owner)
            self.assertFalse(alive(started["pid"]))

    def test_strata_owned_parent_and_synthetic_engine_are_gone_after_release(self):
        with fixture() as (root, rack):
            owner = wait_reservation(rack, reserve(rack, "dual-brain"))
            started = next(event for event in records(rack)
                           if event["kind"] == "start" and event.get("tag") == "dual-brain")
            engine_path = root / (started["activation"] + ".engine.json")
            engine_pid = json.loads(engine_path.read_text())["pid"]
            self.assertTrue(alive(engine_pid))
            release(rack, owner)
            self.assertFalse(alive(started["pid"]))
            self.assertFalse(alive(engine_pid))
            self.assertEqual(claims(rack), {})

    def test_changed_bound_asset_fails_before_any_service_is_launched(self):
        with fixture() as (root, rack):
            (root / "runtime-config.json").write_text('{"synthetic":"changed"}')
            owner = reserve(rack, "dual-worker")
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                current = rack.call("cb", "inspect_reservation", reservation_id=owner["id"])
                if current["state"] in ("unavailable", "recovery_required"):
                    break
                time.sleep(.03)
            else:
                self.fail(current)
            self.assertIn("hash_mismatch", json.dumps(current))
            self.assertFalse(any(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                 for event in records(rack)))


if __name__ == "__main__":
    unittest.main(verbosity=2)
