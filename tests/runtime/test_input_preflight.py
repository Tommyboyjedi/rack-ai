"""Exact-preflight contract against synthetic CPU endpoints, never real model calls."""
from contextlib import contextmanager
import copy
import hashlib
import json
import os
from pathlib import Path
import tempfile
import subprocess
import time
import unittest
import urllib.error
import urllib.request

from support import Rack, ROOT, port
from input_preflight_cases import INSTRUCTIONS, scene_cases

TAG = "tiny-uncd"


@contextmanager
def fixture(changes=None):
    with tempfile.TemporaryDirectory(prefix="rack-input-preflight-", dir="/dev/shm") as directory:
        root = Path(directory)
        artifact = root / "synthetic.gguf"
        artifact.write_bytes(b"synthetic preflight artifact, not a model")

        def configure(config):
            candidate = copy.deepcopy(config["profiles"][0])
            listen = port()
            candidate.update(tag=TAG, model=TAG, backend="llama_cpp", input_token_preflight=True,
                context_tokens=4096, max_input_tokens=3072, max_output_tokens=1024,
                resources=["gpu-2060"], device_mib={"gpu-2060": 16},
                endpoint=f"http://127.0.0.1:{listen}", artifact=str(artifact),
                artifact_sha256=hashlib.sha256(artifact.read_bytes()).hexdigest(),
                args=[str(ROOT / "tests/runtime/input_preflight_backend.py"), str(listen), TAG,
                      str(root / "rack/events.jsonl"), str(root / "rack/tiny-uncd.control.json")])
            if changes:
                candidate.update(changes)
            config["profiles"].append(candidate)

        rack = Rack(root / "rack", configure=configure)
        try:
            yield rack
        finally:
            rack.close()


def reserve(rack):
    response = rack.call("cb", "reserve", request=dict(acquisition_id=os.urandom(8).hex(),
        work_id="synthetic-sizing", services=[TAG], priority="low", ttl_seconds=60))
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        response = rack.call("cb", "inspect_reservation", reservation_id=response["id"])
        if response["state"] == "ready":
            return response, response["services"][TAG]
        time.sleep(.03)
    raise AssertionError(response)


def payload(**changes):
    body = dict(model=TAG, messages=[dict(role="user", content="Synthetic location: kitchen.")],
                max_tokens=128, temperature=0, chat_template_kwargs={"enable_thinking": False})
    body.update(changes)
    return body


def preflight(rack, member, body, owner="cb"):
    headers = {"Content-Type": "application/json"}
    if owner is not None:
        headers["Authorization"] = "Bearer " + owner
    request = urllib.request.Request(f"http://{rack.address}" + member["gateway_path"] +
        "/chat/completions/preflight", data=json.dumps(body).encode(), headers=headers)
    try:
        response = urllib.request.urlopen(request, timeout=8)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        try:
            result = json.loads(raw)
        except json.JSONDecodeError:
            if response.status < 400:
                raise
            result = {"error": raw.decode("utf-8", errors="replace")}
        return response.status, result


def backend_calls(rack):
    path = Path(str(rack.events) + ".preflight")
    return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []


def authority_effects(rack):
    document = json.loads((rack.root / "authority/managed.json").read_text())
    demands = {key: {field: value.get(field) for field in
        ("last_activity_at", "deadline", "accepted_calls", "generation", "profile_hash")}
        for key, value in document["data"]["demands"].items()}
    return dict(invocations=document["data"]["invocations"], demands=demands, claims=document["claims"])


class SyntheticSceneCorpus(unittest.TestCase):
    def test_scene_payloads_have_full_instruction_schema_history_and_explicit_provenance(self):
        self.assertGreaterEqual(len(INSTRUCTIONS), 8000)
        self.assertLess(len(INSTRUCTIONS), 9000)
        cases = scene_cases()
        self.assertEqual(len(cases), 3)
        sizes = []
        for case in cases:
            self.assertIn("synthetic", case["provenance"])
            body = case["body"]
            self.assertEqual(body["messages"][0]["content"], INSTRUCTIONS)
            self.assertEqual(body["response_format"]["type"], "json_schema")
            self.assertFalse(body["chat_template_kwargs"]["enable_thinking"])
            history = "\n".join(message["content"] for message in body["messages"][1:])
            self.assertIn("removes her green coat", history)
            self.assertIn("from the hall into the kitchen", history)
            self.assertIn("station tomorrow morning", history)
            sizes.append(len(json.dumps(body)))
        self.assertEqual(sizes, sorted(set(sizes)))


class InputPreflightAccounting(unittest.TestCase):
    def test_full_chat_payload_is_rendered_then_tokenized_without_generation_or_activity(self):
        with fixture() as rack:
            _, member = reserve(rack)
            body = scene_cases()[0]["body"]
            body["tools"] = [dict(type="function", function=dict(name="record_scene", parameters={
                "type": "object", "properties": {"location": {"type": "string"}},
                "required": ["location"], "additionalProperties": False}))]
            body["tool_choice"] = "auto"
            before = authority_effects(rack)
            status, result = preflight(rack, member, body)
            self.assertEqual(status, 200, result)
            self.assertEqual(result["schema"], "rack-ai/input-preflight/v1")
            calls = backend_calls(rack)
            self.assertEqual([call["stage"] for call in calls], ["template", "tokenize"])
            self.assertEqual(calls[0]["body"], body)
            token_request = calls[1]["body"]
            self.assertEqual({key: value for key, value in token_request.items() if key != "content"},
                             dict(add_special=True, parse_special=True, with_pieces=False))
            expected_prompt = "synthetic-template\n" + json.dumps(
                body, ensure_ascii=False, sort_keys=True, separators=(",", ":"))
            self.assertEqual(token_request["content"], expected_prompt)
            self.assertEqual(result["input_tokens"], len(expected_prompt.encode("utf-8")))
            for field in ("model", "generation", "profile_hash", "profile_version"):
                self.assertEqual(result[field], member[field])
            self.assertEqual(result["service"], TAG)
            self.assertEqual(result["reservation_id"], member["id"])
            self.assertEqual((result["max_input_tokens"], result["max_output_tokens"],
                              result["context_tokens"]), (3072, 1024, 4096))
            self.assertEqual(result["requested_output_tokens"], 1024)
            self.assertEqual(result["total_reserved_tokens"], result["input_tokens"] + 1024)
            self.assertFalse(result["fits"])
            self.assertEqual(result["reason"], "input_limit_exceeded")
            self.assertEqual(authority_effects(rack), before)
            self.assertEqual(rack.counts("dispatch")[TAG], 0)

    def test_exact_input_output_and_total_boundaries_and_default_output_budget(self):
        with fixture() as rack:
            _, member = reserve(rack)
            cases = [(3072, 1024, True, "fits"), (3073, 1024, False, "input_limit_exceeded"),
                     (3000, 1025, False, "output_limit_exceeded"), (3000, 1024, True, "fits")]
            for count, output, fits, reason in cases:
                with self.subTest(count=count, output=output):
                    rack.controls(TAG, token_count=count)
                    status, result = preflight(rack, member, payload(max_tokens=output))
                    self.assertEqual(status, 200, result)
                    self.assertEqual(result["input_tokens"], count)
                    self.assertEqual(result["requested_output_tokens"], output)
                    self.assertEqual(result["total_reserved_tokens"], count + output)
                    self.assertEqual((result["fits"], result["reason"]), (fits, reason))
            body = payload()
            del body["max_tokens"]
            status, result = preflight(rack, member, body)
            self.assertEqual(status, 200, result)
            self.assertEqual(result["requested_output_tokens"], 1024)
            self.assertEqual(backend_calls(rack)[-2]["body"]["max_tokens"], 1024)
            self.assertEqual(rack.counts("dispatch")[TAG], 0)


class InputPreflightOwnership(unittest.TestCase):
    def test_requires_owner_bearer_and_current_scoped_capability(self):
        with fixture() as rack:
            _, member = reserve(rack)
            for owner, expected in ((None, 401), ("invalid-token", 401), ("athba", 404)):
                with self.subTest(owner=owner):
                    status, result = preflight(rack, member, payload(), owner=owner)
                    self.assertEqual(status, expected, result)
            changed = dict(member)
            prefix, capability, suffix = member["gateway_path"].rsplit("/", 2)
            changed["gateway_path"] = f"{prefix}/{capability}invalid/{suffix}"
            status, result = preflight(rack, changed, payload())
            self.assertEqual(status, 403, result)
            self.assertEqual(result["error"], "invalid_scoped_capability")
            self.assertEqual(backend_calls(rack), [])
            self.assertEqual(rack.counts("dispatch")[TAG], 0)

    def test_released_member_cannot_be_used_for_preflight(self):
        with fixture() as rack:
            reservation, member = reserve(rack)
            rack.call("cb", "release_reservation", reservation_id=reservation["id"])
            status, result = preflight(rack, member, payload())
            self.assertEqual(status, 409, result)
            self.assertEqual(result["error"], "reservation_not_dispatchable")
            self.assertEqual(backend_calls(rack), [])

    def test_profile_must_opt_in_and_backend_must_be_llama_cpp(self):
        for changes in ({"input_token_preflight": False},
                        {"backend": "vllm", "input_token_preflight": False}):
            with self.subTest(changes=changes), fixture(changes) as rack:
                _, member = reserve(rack)
                status, result = preflight(rack, member, payload())
                self.assertEqual(status, 409, result)
                self.assertEqual(result["error"], "input_preflight_unqualified")
                self.assertEqual(backend_calls(rack), [])


    def test_preempted_member_cannot_reuse_old_capability(self):
        with fixture() as rack:
            _, member = reserve(rack)
            contender = rack.wait(rack.acquire("athba", "local-coder", "high"))
            status, result = preflight(rack, member, payload())
            self.assertEqual(status, 409, result)
            self.assertEqual(result["error"], "reservation_not_dispatchable")
            self.assertEqual(backend_calls(rack), [])
            self.assertEqual(rack.counts("dispatch")[TAG], 0)
            rack.release(contender)

    def test_release_during_tokenization_prevents_a_successful_stale_report(self):
        from concurrent.futures import ThreadPoolExecutor
        with fixture() as rack:
            reservation, member = reserve(rack)
            rack.controls(TAG, tokenize_delay=.5)
            with ThreadPoolExecutor(max_workers=1) as pool:
                pending = pool.submit(preflight, rack, member, payload())
                deadline = time.monotonic() + 3
                while time.monotonic() < deadline:
                    if any(call["stage"] == "tokenize" for call in backend_calls(rack)):
                        break
                    time.sleep(.01)
                else:
                    self.fail("synthetic tokenizer was not reached")
                rack.call("cb", "release_reservation", reservation_id=reservation["id"])
                status, result = pending.result(timeout=5)
            self.assertIn(status, (409, 502), result)
            self.assertIn(result["error"], ("reservation_not_dispatchable",
                "input_preflight_backend_failure", "input_preflight_backend_identity_unproven"))
            self.assertNotIn("input_tokens", result)
            self.assertEqual(rack.counts("dispatch")[TAG], 0)


class InputPreflightFailures(unittest.TestCase):
    def test_http_body_limit_rejects_oversized_input_before_backend_contact(self):
        with fixture() as rack:
            _, member = reserve(rack)
            body = payload(messages=[dict(role="user", content="x" * (1024 * 1024))])
            status, result = preflight(rack, member, body)
            self.assertEqual(status, 413, result)
            self.assertEqual(backend_calls(rack), [])
            self.assertEqual(rack.counts("dispatch")[TAG], 0)

    def test_malformed_or_nontext_requests_fail_before_backend_contact(self):
        with fixture() as rack:
            _, member = reserve(rack)
            bodies = [[], payload(messages=[]), payload(messages="not an array"),
                payload(messages=[dict(role="user", content=[dict(type="text", text="hello")])]),
                payload(messages=[dict(role="user", content=None)]), payload(max_tokens=0),
                payload(max_completion_tokens=16), payload(model="wrong-model"),
                payload(chat_template_kwargs={"enable_thinking": "false"}), payload(store=True)]
            for body in bodies:
                with self.subTest(body=body):
                    status, result = preflight(rack, member, body)
                    self.assertEqual(status, 400, result)
                    self.assertEqual(result["schema"], "rack-ai/input-preflight/v1")
            self.assertEqual(backend_calls(rack), [])
            self.assertEqual(rack.counts("dispatch")[TAG], 0)

    def test_backend_errors_and_malformed_template_or_tokens_fail_closed(self):
        with fixture() as rack:
            _, member = reserve(rack)
            faults = [
                ({"template_status": 503}, "input_preflight_backend_failure"),
                ({"tokenize_status": 503}, "input_preflight_backend_failure"),
                ({"template_response": {}}, "input_preflight_invalid_template"),
                ({"template_response": {"prompt": 5}}, "input_preflight_invalid_template"),
                ({"tokenize_response": {}}, "input_preflight_invalid_tokens"),
                ({"tokenize_response": {"tokens": "not an array"}}, "input_preflight_invalid_tokens"),
                ({"tokenize_response": {"tokens": [1, -1]}}, "input_preflight_invalid_tokens"),
                ({"tokenize_response": {"tokens": [1, True]}}, "input_preflight_invalid_tokens"),
            ]
            before = authority_effects(rack)
            for controls, expected in faults:
                with self.subTest(controls=controls):
                    rack.controls(TAG, **controls)
                    status, result = preflight(rack, member, payload())
                    self.assertEqual(status, 502, result)
                    self.assertEqual(result["error"], expected)
                    self.assertNotIn("input_tokens", result)
                    self.assertNotIn("fits", result)
            self.assertEqual(authority_effects(rack), before)
            self.assertEqual(rack.counts("dispatch")[TAG], 0)

    def test_oversized_backend_result_is_rejected_with_no_approximate_count(self):
        with fixture() as rack:
            _, member = reserve(rack)
            rack.controls(TAG, template_size=4 * 1024 * 1024)
            status, result = preflight(rack, member, payload())
            self.assertEqual(status, 502, result)
            self.assertEqual(result["error"], "input_preflight_response_oversized")
            self.assertNotIn("input_tokens", result)
            self.assertEqual([call["stage"] for call in backend_calls(rack)], ["template"])
            self.assertEqual(rack.counts("dispatch")[TAG], 0)


class InputPreflightDiscovery(unittest.TestCase):
    def test_only_opted_profiles_publish_report_only_interface_and_ready_path(self):
        description = dict(schema="rack-ai/input-preflight/v1", protocol="chat_completions",
            authentication="owner_bearer_and_scoped_capability", request="same_chat_completions_body",
            generates_tokens=False, enforcement="caller_must_check_fits", media="unsupported")
        for enabled in (True, False):
            with self.subTest(enabled=enabled), fixture({"input_token_preflight": enabled}) as rack:
                discovery = rack.call("cb", "discover")
                profile = next(item for item in discovery["tags"] if item["tag"] == TAG)
                ordinary = next(item for item in discovery["tags"] if item["tag"] == "local-primary")
                self.assertNotIn("input_preflight", ordinary)
                _, member = reserve(rack)
                if enabled:
                    self.assertEqual(profile["input_preflight"], description)
                    expected = dict(description, path=member["gateway_path"] + "/chat/completions/preflight")
                    self.assertEqual(member["input_preflight"], expected)
                else:
                    self.assertNotIn("input_preflight", profile)
                    self.assertNotIn("input_preflight", member)


class InputPreflightConfiguration(unittest.TestCase):
    def test_opt_in_rejects_other_backends_and_profiles_without_chat_protocol(self):
        with tempfile.TemporaryDirectory(prefix="rack-preflight-config-", dir="/dev/shm") as directory:
            path = Path(directory) / "config.json"
            for changes in ({"backend": "vllm"}, {"protocols": ["responses"]}):
                with self.subTest(changes=changes):
                    config = json.loads((ROOT / "config/runtime/config.example.json").read_text())
                    profile = next(item for item in config["profiles"] if item["tag"] == TAG)
                    profile.update(changes, input_token_preflight=True)
                    path.write_text(json.dumps(config))
                    path.chmod(0o600)
                    result = subprocess.run([str(ROOT / "target/debug/rack_ai_runtime"), "validate", str(path)],
                                            capture_output=True, text=True, timeout=5)
                    self.assertNotEqual(result.returncode, 0)
                    self.assertIn("input preflight requires qualified text chat backend", result.stderr)
            self.assertEqual(list(Path(directory).iterdir()), [path])


if __name__ == "__main__":
    unittest.main(verbosity=2)
