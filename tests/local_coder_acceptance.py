#!/usr/bin/env python3
"""Acceptance checks for the temporary local-coder setup on gpurack.

Checks:
1. Raw OpenAI-compatible tool loop completes as write -> bash -> final answer.
2. JCode direct coder run can create a file and stop with a constrained prompt.
"""

from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

URL = "http://127.0.0.1:8018/v1/chat/completions"
JCODE = "/home/tomp/.local/bin/jcode"
WORKDIR = "/tmp/jcode-rack-test"
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


def run_raw_loop_check() -> None:
    path = Path(WORKDIR) / "ACCEPTANCE_RAW_LOOP.txt"
    if path.exists():
        path.unlink()

    messages = [
        {
            "role": "system",
            "content": "You are a coding assistant. Use tools when appropriate. After tool results are provided and the task is complete, stop calling tools and answer briefly.",
        },
        {
            "role": "user",
            "content": f"Create {path} containing exactly RAW_ACCEPT_OK, then use bash to cat the file.",
        },
    ]

    resp1 = call(messages)
    msg1 = resp1["choices"][0]["message"]
    assert resp1["choices"][0]["finish_reason"] == "tool_calls", resp1
    assert msg1["tool_calls"][0]["function"]["name"] == "write", msg1
    messages.append(msg1)

    for tc in msg1["tool_calls"]:
        args = json.loads(tc["function"]["arguments"])
        Path(args["file_path"]).write_text(args["content"], encoding="utf-8")
        messages.append({"role": "tool", "tool_call_id": tc["id"], "content": f"WROTE {args['file_path']}"})

    resp2 = call(messages)
    msg2 = resp2["choices"][0]["message"]
    assert resp2["choices"][0]["finish_reason"] == "tool_calls", resp2
    assert msg2["tool_calls"][0]["function"]["name"] == "bash", msg2
    messages.append(msg2)

    for tc in msg2["tool_calls"]:
        args = json.loads(tc["function"]["arguments"])
        cp = subprocess.run(args["command"], shell=True, capture_output=True, text=True, check=False)
        messages.append({"role": "tool", "tool_call_id": tc["id"], "content": (cp.stdout + cp.stderr).strip()})

    resp3 = call(messages)
    msg3 = resp3["choices"][0]["message"]
    assert resp3["choices"][0]["finish_reason"] == "stop", resp3
    assert "RAW_ACCEPT_OK" in (msg3.get("content") or ""), msg3
    assert path.read_text(encoding="utf-8") == "RAW_ACCEPT_OK"


def run_jcode_check() -> None:
    path = Path(WORKDIR) / "ACCEPTANCE_JCODE.txt"
    if path.exists():
        path.unlink()

    prompt = (
        f"Using your tools, create {path} containing exactly JCODE_ACCEPT_OK. "
        "Then use bash exactly once to cat the file. "
        "After the bash output confirms the file contents, reply with exactly COMPLETE and stop. "
        "Do not call any more tools after that."
    )

    cp = subprocess.run(
        [JCODE, "--provider-profile", "local-coder", "--model", "local-coder", "-C", WORKDIR, "run", prompt],
        capture_output=True,
        text=True,
        timeout=180,
        check=False,
    )
    output = cp.stdout + cp.stderr
    assert path.exists(), output
    assert path.read_text(encoding="utf-8") == "JCODE_ACCEPT_OK", output
    assert "COMPLETE" in output, output


def main() -> int:
    try:
        run_raw_loop_check()
        print("PASS raw_loop")
        run_jcode_check()
        print("PASS jcode_stop")
        return 0
    except Exception as exc:
        print(f"FAIL {exc}")
        return 1


if __name__ == "__main__":
    sys.exit(main())
