"""Temporary vLLM tool parser for Qwen2.5-Coder on the rack.

This exists only to normalize the model's current fenced-JSON tool output into
Hermes-style <tool_call>...</tool_call> regions so vLLM can emit structured
OpenAI tool_calls. Remove this once the coder model is replaced with one that
natively interoperates with vLLM tool calling on the rack.
"""

from __future__ import annotations

import json
import re
from collections.abc import Sequence

from vllm.entrypoints.openai.chat_completion.protocol import ChatCompletionRequest
from vllm.entrypoints.openai.engine.protocol import DeltaMessage
from vllm.tool_parsers import ToolParserManager
from vllm.tool_parsers.hermes_tool_parser import Hermes2ProToolParser


_FENCED_JSON_RE = re.compile(
    r"```(?:json)?\s*(\{.*?\})\s*```", re.DOTALL | re.IGNORECASE
)


@ToolParserManager.register_module(["qwen25_coder_temp", "qwen25_coder_json_temp"])
class Qwen25CoderTempToolParser(Hermes2ProToolParser):
    """Temporary fallback parser for fenced JSON tool calls."""

    @staticmethod
    def _is_tool_call_payload(payload: object) -> bool:
        return (
            isinstance(payload, dict)
            and isinstance(payload.get("name"), str)
            and isinstance(payload.get("arguments"), dict)
        )

    @classmethod
    def _normalize_fenced_json_blocks(cls, text: str) -> str | None:
        parts: list[str] = []
        found = False
        last_end = 0

        for match in _FENCED_JSON_RE.finditer(text):
            prefix = text[last_end : match.start()]
            if prefix.strip():
                parts.append(prefix)

            try:
                payload = json.loads(match.group(1))
            except json.JSONDecodeError:
                parts.append(match.group(0))
                last_end = match.end()
                continue

            if cls._is_tool_call_payload(payload):
                parts.append(f"<tool_call>{json.dumps(payload, ensure_ascii=False)}</tool_call>")
                found = True
            else:
                parts.append(match.group(0))

            last_end = match.end()

        suffix = text[last_end:]
        if suffix.strip():
            parts.append(suffix)

        return "\n".join(parts) if found else None

    @classmethod
    def _normalize_bare_json_sequence(cls, text: str) -> str | None:
        stripped = text.strip()
        if not stripped.startswith("{"):
            return None

        decoder = json.JSONDecoder()
        index = 0
        payloads: list[dict] = []

        while index < len(stripped):
            try:
                payload, next_index = decoder.raw_decode(stripped, index)
            except json.JSONDecodeError:
                return None

            if not cls._is_tool_call_payload(payload):
                return None

            payloads.append(payload)
            index = next_index
            while index < len(stripped) and stripped[index].isspace():
                index += 1

        if not payloads:
            return None

        return "\n".join(
            f"<tool_call>{json.dumps(payload, ensure_ascii=False)}</tool_call>"
            for payload in payloads
        )

    @classmethod
    def _normalize_model_output(cls, text: str) -> str:
        if "<tool_call>" in text:
            return text

        normalized = cls._normalize_fenced_json_blocks(text)
        if normalized is not None:
            return normalized

        normalized = cls._normalize_bare_json_sequence(text)
        if normalized is not None:
            return normalized

        return text

    def extract_tool_calls(
        self,
        model_output: str,
        request: ChatCompletionRequest,
    ):
        return super().extract_tool_calls(
            self._normalize_model_output(model_output),
            request,
        )

    def extract_tool_calls_streaming(
        self,
        previous_text: str,
        current_text: str,
        delta_text: str,
        previous_token_ids: Sequence[int],
        current_token_ids: Sequence[int],
        delta_token_ids: Sequence[int],
        request: ChatCompletionRequest,
    ) -> DeltaMessage | None:
        return super().extract_tool_calls_streaming(
            self._normalize_model_output(previous_text),
            self._normalize_model_output(current_text),
            delta_text,
            previous_token_ids,
            current_token_ids,
            delta_token_ids,
            request,
        )
