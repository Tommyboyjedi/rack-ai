"""Configured pin verification uses private CPU fixtures, never live services."""
from contextlib import contextmanager
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest

from support import ROOT
from test_dual_services import (alive, claims, digest, fixture, records, release,
                                reserve, wait_reservation)


@contextmanager
def verifier_fixture(mode="valid"):
    with tempfile.TemporaryDirectory(prefix="rack-pin-verifier-", dir="/dev/shm") as directory:
        root = Path(directory)
        pins = [root / "first pin.txt", root / "second pin.txt"]
        for index, path in enumerate(pins):
            path.write_text("independent synthetic content " + str(index))
        verifier = root / "configured verifier"
        calls = root / "verifier-argv.jsonl"
        program = (
            "import json, os, subprocess, sys, time\n"
            f"with open({str(calls)!r}, 'a') as out:\n"
            "    out.write(json.dumps(sys.argv[1:]) + '\\n')\n"
        )
        if mode == "blocked":
            program += f"while not os.path.exists({str(root / 'continue')!r}): time.sleep(.01)\n"
        if mode in ("valid", "nonexecutable", "missing", "blocked"):
            program += "os.execv('/usr/bin/sha256sum', ['sha256sum'] + sys.argv[1:])\n"
        elif mode == "nonzero":
            program += "raise SystemExit(7)\n"
        elif mode == "timeout":
            program += "time.sleep(10)\n"
        else:
            program += (
                "lines = subprocess.check_output(['/usr/bin/sha256sum'] + sys.argv[1:], text=True).splitlines()\n"
                + {"partial": "lines = lines[:1]\n",
                   "extra": "lines += lines[:1]\n",
                   "reordered": "lines.reverse()\n",
                   "mismatch": "lines[0] = '0' * 64 + lines[0][64:]\n"}[mode]
                + "print('\\n'.join(lines))\n"
            )
        if mode != "missing":
            verifier.write_text("#!" + sys.executable + "\n" + program)
            verifier.chmod(0o600 if mode == "nonexecutable" else 0o700)
        launch = dict(file_pin_verifier=str(verifier),
                      file_pins=[dict(path=str(path), sha256=digest(path)) for path in pins])
        with fixture({"dual-worker": dict(launch=launch, artifact_verify_seconds=5 if mode == "blocked" else 1)}) as (_, rack):
            yield root, rack


def failed_reservation(rack, reservation):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        current = rack.call("cb", "inspect_reservation", reservation_id=reservation["id"])
        if current["state"] in ("unavailable", "recovery_required"):
            return current
        time.sleep(.03)
    raise AssertionError(current)


class ConfiguredFilePinVerifier(unittest.TestCase):
    def test_explicit_verifier_receives_every_pin_in_one_ordered_argv_and_releases(self):
        with verifier_fixture() as (root, rack):
            owner = wait_reservation(rack, reserve(rack, "dual-worker"))
            argv = [json.loads(line) for line in (root / "verifier-argv.jsonl").read_text().splitlines()]
            expected = ["--", str(root / "first pin.txt"), str(root / "second pin.txt")]
            self.assertTrue(argv)
            self.assertTrue(all(call == expected for call in argv), argv)
            self.assertEqual(sum(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                 for event in records(rack)), 1)
            member = owner["services"]["dual-worker"]
            self.assertEqual(rack.result(rack.infer(dict(member, owner="cb")))["state"], "completed")
            release(rack, owner)
            self.assertEqual(claims(rack), {})

    def test_omitted_verifier_preserves_path_lookup(self):
        with fixture() as (root, rack):
            calls = root / "path-verifier-argv.jsonl"
            wrapper = root / "bin/sha256sum"
            wrapper.write_text("#!" + sys.executable + "\nimport json, os, sys\n"
                + f"with open({str(calls)!r}, 'a') as out: out.write(json.dumps(sys.argv[1:]) + '\\n')\n"
                + "os.execv('/usr/bin/sha256sum', ['sha256sum'] + sys.argv[1:])\n")
            wrapper.chmod(0o700)
            owner = wait_reservation(rack, reserve(rack, "dual-worker"))
            argv = [json.loads(line) for line in calls.read_text().splitlines()]
            self.assertIn(["--", str(root / "runtime-config.json")], argv)
            # Artifact hashing still uses its established PATH command.
            self.assertTrue(any(str(root / "synthetic.gguf") in call for call in argv), argv)
            release(rack, owner)

    def test_artifact_integrity_still_blocks_a_valid_configured_pin_verifier(self):
        with verifier_fixture() as (_, rack):
            artifact = next(p["artifact"] for p in rack.config["profiles"] if p["tag"] == "dual-worker")
            Path(artifact).write_text("changed synthetic model artifact")
            current = failed_reservation(rack, reserve(rack, "dual-worker"))
            self.assertIn("hash_mismatch", json.dumps(current))
            self.assertFalse(any(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                 for event in records(rack)))


class FilePinVerifierFailures(unittest.TestCase):
    def test_bad_verifiers_never_launch_or_transfer_claims_and_preserve_unrelated_service(self):
        for mode in ("missing", "nonexecutable", "nonzero", "timeout", "partial", "extra", "reordered", "mismatch"):
            with self.subTest(mode=mode), verifier_fixture(mode) as (root, rack):
                incumbent = rack.wait(rack.acquire("athba", "local-primary", "low"))
                unrelated = rack.wait(rack.acquire("athba", "local-coder", "medium"))
                incumbent_claims = claims(rack)
                began = time.monotonic()
                incoming = rack.call("cb", "reserve", request=dict(acquisition_id="bad-verifier-" + mode,
                    work_id="pin-verifier-fixture", services=["dual-worker"], priority="high", ttl_seconds=60))
                failed = failed_reservation(rack, incoming)
                self.assertLess(time.monotonic() - began, 6, failed)
                expected = "launch_integrity_unavailable" if mode in (
                    "missing", "nonexecutable", "nonzero", "timeout") else "launch_file_hash_mismatch"
                self.assertIn(expected, json.dumps(failed))
                self.assertFalse(any(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                     for event in records(rack)))
                member = failed["services"]["dual-worker"]
                self.assertIsNone(member["process"])
                self.assertFalse(member["effect_started"])
                self.assertTrue(all(incumbent_claims.get(resource) == owner
                                    for resource, owner in claims(rack).items()))
                # Existing priority policy retires a preempted incumbent even
                # when the incoming candidate fails; it never restores that lease.
                rack.wait(incumbent, "preempted")
                self.assertFalse(alive(incumbent["process"]["pid"]))
                self.assertEqual(set(claims(rack)), {"gpu-2060"})
                self.assertEqual(rack.inspect(unrelated)["state"], "ready")
                self.assertEqual(rack.result(rack.infer(unrelated))["state"], "completed")
                self.assertEqual(rack.counts("stop")["local-coder"], 0)
                rack.release(unrelated)
                rack.wait(unrelated, "released")

    def test_invalid_configured_verifier_paths_are_rejected_without_effects(self):
        with fixture() as (root, rack):
            profile = next(p for p in rack.config["profiles"] if p["tag"] == "dual-worker")
            for path in ("sha256sum", "", "/usr/bin/../bin/sha256sum", "/usr/bin/sha\n256sum",
                         "/usr/bin/sha\r256sum", "/usr/bin/sha\0sum", "/usr/bin/sha\udcffsum"):
                with self.subTest(path=repr(path)):
                    profile["launch"]["file_pin_verifier"] = path
                    config = root / "invalid-config.json"
                    config.write_text(json.dumps(rack.config))
                    config.chmod(0o600)
                    result = subprocess.run([str(ROOT / "target/debug/rack_ai_runtime"), "validate", str(config)],
                                            capture_output=True, text=True, timeout=5)
                    self.assertNotEqual(result.returncode, 0, result.stdout)
                    if "\udcff" not in path:
                        self.assertIn("invalid launch controls", result.stderr)
            self.assertEqual(records(rack), [])
            self.assertEqual(claims(rack), {})


class FilePinVerifierPreflightGate(unittest.TestCase):
    def test_configured_verifier_finishes_before_incumbent_shutdown_or_claim_transfer(self):
        with verifier_fixture("blocked") as (root, rack):
            incumbent = rack.wait(rack.acquire("athba", "local-primary", "low"))
            original_claims = claims(rack)
            incoming = rack.call("cb", "reserve", request=dict(acquisition_id="blocking-verifier",
                work_id="preflight-gate", services=["dual-worker"], priority="high", ttl_seconds=60))
            deadline = time.monotonic() + 3
            while not (root / "verifier-argv.jsonl").exists():
                self.assertLess(time.monotonic(), deadline)
                time.sleep(.01)
            try:
                self.assertTrue(alive(incumbent["process"]["pid"]))
                self.assertEqual(rack.counts("stop")["local-primary"], 0)
                self.assertEqual(claims(rack), original_claims)
                self.assertFalse(any(event["kind"] == "start" and event.get("tag") == "dual-worker"
                                     for event in records(rack)))
            finally:
                (root / "continue").touch()
            ready = wait_reservation(rack, incoming)
            self.assertEqual(rack.inspect(incumbent)["state"], "preempted")
            self.assertFalse(alive(incumbent["process"]["pid"]))
            release(rack, ready)
            self.assertEqual(claims(rack), {})


if __name__ == "__main__":
    unittest.main(verbosity=2)
