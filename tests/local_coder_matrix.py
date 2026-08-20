#!/usr/bin/env python3
"""Broader behavioral checks for the current local-coder worker.

This script exercises multiple bounded tasks against the live local-coder API and
verifies that the model/tool loop completes without getting stuck.
"""

from __future__ import annotations

import json
import subprocess
import sys
import urllib.request
from pathlib import Path

URL = "http://127.0.0.1:8018/v1/chat/completions"
WORKDIR = Path("/tmp/jcode-rack-test")
TOOLS = [
    {
        "type": "function",
        "function": {
            "name": "write",
            "description": "Write a file",
            "parameters": {
                "type": "object",
                "properties": {
                    "file_path": {"type": "string"},
                    "content": {"type": "string"},
                    "intent": {"type": "string"},
                },
                "required": ["file_path", "content"],
            },
        },
    },
    {
        "type": "function",
        "function": {
            "name": "bash",
            "description": "Run a shell command",
            "parameters": {
                "type": "object",
                "properties": {
                    "command": {"type": "string"},
                    "description": {"type": "string"},
                },
                "required": ["command"],
            },
        },
    },
]

CASES = [
    {
        "name": "write_then_stop",
        "user": "Create /tmp/jcode-rack-test/MATRIX_WRITE.txt containing exactly MATRIX_WRITE_OK. After the file is written, reply with exactly COMPLETE and stop.",
        "max_turns": 3,
        "expect_file": WORKDIR / "MATRIX_WRITE.txt",
        "expect_content": "MATRIX_WRITE_OK",
        "expect_final": "COMPLETE",
    },
    {
        "name": "write_then_cat_then_stop",
        "user": "Create /tmp/jcode-rack-test/MATRIX_CAT.txt containing exactly MATRIX_CAT_OK, then use bash exactly once to cat the file. After that reply with exactly COMPLETE and stop.",
        "max_turns": 4,
        "expect_file": WORKDIR / "MATRIX_CAT.txt",
        "expect_content": "MATRIX_CAT_OK",
        "expect_final": "COMPLETE",
    },
    {
        "name": "write_python_test_then_stop",
        "user": "Create /tmp/jcode-rack-test/MATRIX_TEST.py containing Python code that prints exactly MATRIX_TEST_OK, then use bash exactly once to run python3 /tmp/jcode-rack-test/MATRIX_TEST.py. After that reply with exactly COMPLETE and stop.",
        "max_turns": 4,
        "expect_file": WORKDIR / "MATRIX_TEST.py",
        "expect_content_contains": "MATRIX_TEST_OK",
        "expect_final": "COMPLETE",
    },
]


def call(messages: list[dict]) -> dict:
    body = json.dumps(
        {
            "model": "local-coder",
            "messages": messages,
            "tools": TOOLS,
            "tool_choice": "auto",
            "stream": False,
            "temperature": 0,
        }
    ).encode()
    req = urllib.request.Request(URL, data=body, headers={"Content-Type": "application/json"})
    with urllib.request.urlopen(req, timeout=120) as response:
        return json.load(response)


def run_tool(name: str, args: dict) -> str:
    if name == "write":
        path = Path(args["file_path"])
        path.write_text(args["content"], encoding="utf-8")
        return f"WROTE {path}"
    if name == "bash":
        cp = subprocess.run(args["command"], shell=True, capture_output=True, text=True, check=False)
        return (cp.stdout + cp.stderr).strip()
    raise ValueError(f"Unsupported tool {name}")


def run_case(case: dict) -> None:
    path = case["expect_file"]
    if path.exists():
        path.unlink()

    messages = [
        {
            "role": "system",
            "content": "You are a coding assistant. Use tools when needed. When the task is complete, stop calling tools and reply exactly as requested.",
        },
        {"role": "user", "content": case["user"]},
    ]

    final_content = None
    tool_turns = 0
    for _ in range(case["max_turns"]):
        resp = call(messages)
        choice = resp["choices"][0]
        msg = choice["message"]
        messages.append(msg)
        if choice["finish_reason"] == "stop":
            final_content = msg.get("content") or ""
            break
        if choice["finish_reason"] != "tool_calls":
            raise AssertionError(f"{case['name']} unexpected finish_reason {choice['finish_reason']}: {resp}")
        tool_turns += 1
        for tc in msg.get("tool_calls") or []:
            args = json.loads(tc["function"]["arguments"])
            tool_result = run_tool(tc["function"]["name"], args)
            messages.append({"role": "tool", "tool_call_id": tc["id"], "content": tool_result})

    assert final_content == case["expect_final"], f"{case['name']} final={final_content!r}"
    assert path.exists(), f"{case['name']} missing file {path}"
    file_content = path.read_text(encoding='utf-8')
    if "expect_content" in case:
        assert file_content == case["expect_content"], f"{case['name']} content mismatch"
    if "expect_content_contains" in case:
        assert case["expect_content_contains"] in file_content, f"{case['name']} content missing marker"
    assert tool_turns >= 1, f"{case['name']} used no tools"
    print(f"PASS {case['name']} ({tool_turns} tool turns)")


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
