#!/usr/bin/env python3
"""Exercise bounded JCode -> local-coder flows repeatedly.

This stays intentionally narrow: single-worker direct runs with strong stop
conditions, so failures are attributable to the local-coder path rather than
swarm orchestration.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

JCODE = "/home/tomp/.local/bin/jcode"
WORKDIR = Path("/tmp/jcode-rack-test")

CASES = [
    {
        "name": "jcode_write_then_stop",
        "target": WORKDIR / "JCODE_MATRIX_WRITE.txt",
        "expected_file": "JCODE_MATRIX_WRITE_OK",
        "prompt": "Using your tools, create /tmp/jcode-rack-test/JCODE_MATRIX_WRITE.txt containing exactly JCODE_MATRIX_WRITE_OK. After the file is written, reply with exactly COMPLETE and stop. Do not call any more tools after that.",
    },
    {
        "name": "jcode_write_cat_then_stop",
        "target": WORKDIR / "JCODE_MATRIX_CAT.txt",
        "expected_file": "JCODE_MATRIX_CAT_OK",
        "prompt": "Using your tools, create /tmp/jcode-rack-test/JCODE_MATRIX_CAT.txt containing exactly JCODE_MATRIX_CAT_OK. Then use bash exactly once to cat the file. After the bash output confirms the file contents, reply with exactly COMPLETE and stop. Do not call any more tools after that.",
    },
    {
        "name": "jcode_python_then_stop",
        "target": WORKDIR / "JCODE_MATRIX_TEST.py",
        "expected_marker": "JCODE_MATRIX_TEST_OK",
        "prompt": "Using your tools, create /tmp/jcode-rack-test/JCODE_MATRIX_TEST.py containing Python code that prints exactly JCODE_MATRIX_TEST_OK. Then use bash exactly once to run python3 /tmp/jcode-rack-test/JCODE_MATRIX_TEST.py. After the bash output confirms the file contents, reply with exactly COMPLETE and stop. Do not call any more tools after that.",
    },
]


def run_case(case: dict) -> None:
    target = case["target"]
    if target.exists():
        target.unlink()

    cp = subprocess.run(
        [JCODE, "--provider-profile", "local-coder", "--model", "local-coder", "-C", str(WORKDIR), "run", case["prompt"]],
        capture_output=True,
        text=True,
        timeout=240,
        check=False,
    )
    output = cp.stdout + cp.stderr
    assert target.exists(), f"{case['name']} missing file\n{output}"
    file_text = target.read_text(encoding="utf-8")
    if "expected_file" in case:
        assert file_text == case["expected_file"], f"{case['name']} file mismatch\n{output}"
    if "expected_marker" in case:
        assert case["expected_marker"] in file_text, f"{case['name']} marker missing\n{output}"
    assert "COMPLETE" in output, f"{case['name']} missing COMPLETE\n{output}"
    print(f"PASS {case['name']}")


def main() -> int:
    try:
        for case in CASES:
            run_case(case)
        return 0
    except Exception as exc:
        print(f"FAIL {exc}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
