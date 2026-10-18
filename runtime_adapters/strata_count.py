"""Count the installed Strata Chat Completions prompt without engine work."""
from dataclasses import dataclass

from strata_images import InputMessages
from strata_vision_count import ImageTokenCounter


@dataclass(frozen=True)
class ChatCounter:
    service: object
    runtime: object

    def count(self, request):
        if not isinstance(request, dict):
            raise ValueError("send a JSON object")
        InputMessages.validate(request)
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
        count = ImageTokenCounter(service, runtime).expanded_count(ids, messages)
        return {"input_tokens": count, "context_reserve_tokens": runtime.CTX_SLACK}
