"""GPU-free tests for the prepared Strata adapter boundaries."""
import os
from pathlib import Path
import sys
from types import SimpleNamespace

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "runtime_adapters"))
from strata_count import ChatCounter
from strata_http import AdapterSurface
from strata_server import configure
from strata_engine import EngineGuard, ProcessIdentity


def counter_fixture():
    observed = {}

    def encode(messages, tools, kwargs, *, store):
        observed.update(messages=messages, tools=tools, kwargs=kwargs, store=store)
        return [1, 2, 3]

    service = SimpleNamespace(
        with_shared=lambda request, api: {**request, "reasoning_effort": "high"},
        reasoning_budget=lambda request: None,
        encode_prompt=encode, engine=object(),
        tok=SimpleNamespace(tokens=[0] * 10, encode=lambda text, parse_special: [8, 9]),
    )
    runtime = SimpleNamespace(
        openai_to_messages=lambda req: (req["messages"], req.get("tools"), req.get("chat_template_kwargs", {})),
        tool_choice_of=lambda choice: (choice, None),
        forced_call=lambda choice, tools: "prefix" if choice == "required" else None,
        prepare_format=lambda form, messages: ([*messages, {"role": "system", "content": "schema"}], form),
        stop_strings=lambda request: None, check_request_sampling=lambda request: None,
        bias_api=SimpleNamespace(validate_request=lambda *args: None), CTX_SLACK=8,
    )
    return ChatCounter(service, runtime), observed


def test_exact_counter_keeps_schema_and_template_controls_without_cache_storage():
    counter, observed = counter_fixture()
    request = {"messages": [{"role": "user", "content": "neutral"}],
               "response_format": {"type": "json_object"},
               "chat_template_kwargs": {"enable_thinking": False}}
    assert counter.count(request) == {"input_tokens": 3, "context_reserve_tokens": 8}
    assert observed["messages"][-1]["content"] == "schema"
    assert observed["kwargs"] == {"enable_thinking": False}
    assert observed["store"] is False
    assert len(request["messages"]) == 1


def test_forced_tool_prefix_is_counted_when_thinking_is_disabled():
    counter, observed = counter_fixture()
    request = {"messages": [{"role": "user", "content": "neutral"}],
               "tools": [{"name": "probe"}], "tool_choice": "required",
               "chat_template_kwargs": {"enable_thinking": False}}
    assert counter.count(request)["input_tokens"] == 5
    request["chat_template_kwargs"] = {}
    assert counter.count(request)["input_tokens"] == 3
    request["tool_choice"] = "none"
    counter.count(request)
    assert observed["tools"] is None


@pytest.mark.parametrize("addition", [
    {"strata_mcp": True}, {"strata_prefix": {"tokens": 1}},
    {"messages": [{"role": "user", "content": [{"type": "image_url", "image_url": {"url": "x"}}]}]},
])
def test_counter_rejects_unsupported_side_effecting_or_media_content(addition):
    counter, _ = counter_fixture()
    with pytest.raises(ValueError):
        counter.count({"messages": [{"role": "user", "content": "neutral"}], **addition})


def test_schema_with_tools_is_rejected_without_generation():
    counter, _ = counter_fixture()
    with pytest.raises(ValueError, match="with tools"):
        counter.count({"messages": [{"role": "user", "content": "neutral"}],
                       "tools": [{"name": "probe"}], "response_format": {"type": "json_object"}})


def test_adapter_does_not_expose_mutable_upstream_controls(tmp_path):
    class Handler:
        do_GET = lambda self: None
        do_POST = lambda self: None
        _json = lambda self, code, body: setattr(self, "response", (code, body))
    runtime = SimpleNamespace(make_handler=lambda service: Handler, EngineDied=RuntimeError)
    engine = running_engine()
    service = SimpleNamespace(engine=engine, shared={"temperature": 99}, shared_path="mutable")
    AdapterSurface(runtime, tmp_path / "config").install()
    handler_type = runtime.make_handler(service)
    assert service.shared == {} and service.shared_path is None
    handler = handler_type()
    handler.path = "/config"
    handler.do_POST()
    assert handler.response[0] == 404
    handler.path = "/v1/responses"
    handler.do_POST()
    assert handler.response[0] == 404
    engine.proc.poll = lambda: 1
    for route in ("/v1/chat/completions", "/rackai/input-tokens"):
        handler.path = route
        handler.do_POST()
        assert handler.response[0] == 503


def test_startup_requires_managed_activation_before_importing_runtime(tmp_path, monkeypatch):
    config = tmp_path / "config.json"
    config.write_text("{}")
    monkeypatch.delenv("RACK_RUNTIME_ACTIVATION", raising=False)
    with pytest.raises(ValueError, match="activation"):
        configure(["--runtime-root", str(tmp_path), "--config", str(config)])


def running_engine():
    return SimpleNamespace(proc=SimpleNamespace(pid=os.getpid(), poll=lambda: None), alive=lambda: True)


def test_engine_guard_preserves_initial_process_and_denies_native_restart():
    engine = running_engine()
    guard = EngineGuard(engine, SimpleNamespace(EngineDied=RuntimeError))
    guard.check(engine)
    for action in (engine.restart, engine.unload):
        with pytest.raises(RuntimeError, match="new RackAI activation"):
            action()
    replacement = running_engine()
    with pytest.raises(RuntimeError, match="generation changed"):
        guard.check(replacement)


def test_engine_guard_rejects_exit_pid_reuse_and_unready_initial_engine(monkeypatch):
    engine = running_engine()
    guard = EngineGuard(engine, SimpleNamespace(EngineDied=RuntimeError))
    engine.proc.poll = lambda: 1
    with pytest.raises(RuntimeError, match="no longer available"):
        guard.check(engine)
    engine.proc.poll = lambda: None
    changed = ProcessIdentity(engine.proc.pid, guard.identity.start + "-changed")
    monkeypatch.setattr(ProcessIdentity, "read", classmethod(lambda cls, process: changed))
    with pytest.raises(RuntimeError, match="generation changed"):
        guard.check(engine)
    engine.alive = lambda: False
    with pytest.raises(ValueError, match="not become ready"):
        EngineGuard(engine, SimpleNamespace(EngineDied=RuntimeError))
