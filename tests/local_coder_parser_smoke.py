#!/usr/bin/env python3
"""Smoke tests for the temporary local-coder parser plugin.

Runs inside the vLLM image so it can import the plugin against the same parser
base classes used by the live coder service.
"""

from __future__ import annotations

import subprocess
import textwrap

IMAGE = "0a51ea5b4ae2"
PLUGIN_MOUNT = "/srv/rack-ai/plugins:/plugins:ro"

SCRIPT = r'''
from vllm.tool_parsers.abstract_tool_parser import ToolParserManager
ToolParserManager.import_tool_parser('/plugins/vllm_qwen25_coder_temp_parser.py')
Parser = ToolParserManager.get_tool_parser('qwen25_coder_temp')

cases = {
    'fenced_json': (
        '```json\n{"name":"write","arguments":{"file_path":"/tmp/x","content":"OK"}}\n```',
        '<tool_call>{"name": "write", "arguments": {"file_path": "/tmp/x", "content": "OK"}}</tool_call>',
    ),
    'function_block': (
        '<tool_call>\n<function=bash>\n<parameter=command>\ncat /tmp/x\n</parameter>\n</function>\n</tool_call>',
        '<tool_call>{"name": "bash", "arguments": {"command": "cat /tmp/x"}}</tool_call>',
    ),
    'batch_payload': (
        '{"name":"batch","arguments":{"tool_calls":[{"tool":"bash","command":"cat /tmp/x","intent":"show"},{"tool":"write","file_path":"/tmp/y","content":"Y"}]}}',
        '<tool_call>{"name": "bash", "arguments": {"command": "cat /tmp/x", "intent": "show"}}</tool_call>\n<tool_call>{"name": "write", "arguments": {"file_path": "/tmp/y", "content": "Y"}}</tool_call>',
    ),
}

for name, (source, expected) in cases.items():
    got = Parser._normalize_model_output(source)
    assert got == expected, f'{name} mismatch\nGOT: {got!r}\nEXP: {expected!r}'
    print('PASS', name)
'''


def main() -> int:
    cp = subprocess.run(
        [
            'docker', 'run', '--rm', '-i', '--entrypoint', 'python3',
            '-v', PLUGIN_MOUNT,
            IMAGE, '-'
        ],
        input=textwrap.dedent(SCRIPT),
        text=True,
        capture_output=True,
        check=False,
    )
    print(cp.stdout, end='')
    if cp.returncode != 0:
        print(cp.stderr, end='')
    return cp.returncode


if __name__ == '__main__':
    raise SystemExit(main())
