"""Count the installed Strata Chat Completions prompt without engine work."""
from dataclasses import dataclass


@dataclass(frozen=True)
class ChatCounter:
    service: object
    runtime: object

    def count(self, request):
        if not isinstance(request, dict):
            raise ValueError("send a JSON object")
        self._text_only(request)
        runtime, service = self.runtime, self.service
        request = service.with_shared(request, "openai")
        messages, tools, kwargs = runtime.openai_to_messages(request)
        if runtime.tool_choice_of(request.get("tool_choice"))[0] == "none":
            tools = None
        force = runtime.forced_call(request.get("tool_choice"), tools)
        messages, validator = runtime.prepare_format(request.get("response_format"), messages)
        if validator is not None and tools:
            raise ValueError("structured response_format with tools is not supported")
        service.reasoning_budget(request)
        runtime.stop_strings(request)
        runtime.check_request_sampling(request)
        runtime.bias_api.validate_request(request, service.engine, len(service.tok.tokens))
        ids = service.encode_prompt(messages, tools, kwargs, store=False)
        if force and kwargs.get("enable_thinking", True) is False:
            ids = ids + service.tok.encode(force, parse_special=True)
        return {"input_tokens": len(ids), "context_reserve_tokens": runtime.CTX_SLACK}

    def _text_only(self, request):
        if request.get("strata_mcp") or request.get("strata_prefix") is not None:
            raise ValueError("only stateless text Chat Completions are supported")
        messages = request.get("messages")
        if not isinstance(messages, list) or not messages:
            raise ValueError("messages must be a nonempty array")
        for message in messages:
            if not isinstance(message, dict):
                raise ValueError("messages must contain objects")
            content = message.get("content")
            if content is None or isinstance(content, str):
                continue
            if not isinstance(content, list) or any(
                not isinstance(part, dict) or part.get("type") != "text"
                or not isinstance(part.get("text"), str) for part in content
            ):
                raise ValueError("only text message content is supported")
