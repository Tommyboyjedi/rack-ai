#!/usr/bin/env python3
"""Disposable CPU server: transport/lifecycle proofs, never model-quality evidence."""
import http.server
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

listen, model, events, controls, tag = sys.argv[1:6]
assert sys.argv[6:] == ["--host", "127.0.0.1"]
activation = os.environ["RACK_RUNTIME_ACTIVATION"]
root = Path(os.environ["RACK_DUAL_HOST_FIXTURE"])
engine = None


def control():
    path = Path(controls)
    return json.loads(path.read_text()) if path.exists() else {}


def event(kind, extra=None):
    record = dict(kind=kind, model=model, tag=tag, pid=os.getpid(), activation=activation)
    record.update(extra or {})
    with open(events, "a") as output:
        output.write(json.dumps(record) + "\n")


def stop(*_):
    if engine is not None and engine.poll() is None:
        engine.terminate()
        engine.wait(timeout=2)
    event("stop")
    raise SystemExit(0)


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, value):
        data = json.dumps(value).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        settings = control()
        event("probe", dict(path=self.path))
        observed_model = settings.get("model", model)
        context = settings.get("context", 131072)
        if self.path == "/rackai/status":
            self.reply(dict(schema="rack-ai/strata-runtime/v1", service="strata",
                model=observed_model, max_context=context,
                structured_output=dict(method="prompt_and_validate", constrained_decoding=False),
                loaded=settings.get("loaded", True), engine_pid=engine.pid,
                config_sha256=os.environ.get("RACK_DUAL_CONFIG_SHA256", "")))
        elif self.path == "/health":
            self.reply(dict(status="ok", service="strata", model=observed_model, max_context=context,
                            loaded=settings.get("loaded", True)))
        elif self.path == "/props":
            self.reply(dict(default_generation_settings=dict(n_ctx=context),
                            total_slots=1, chat_template="synthetic-template"))
        else:
            self.reply(dict(data=[dict(id=observed_model)]))

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with open(events + ".requests", "a") as output:
            output.write(json.dumps(dict(path=self.path, body=body, tag=tag)) + "\n")
        if self.path == "/rackai/input-tokens":
            time.sleep(control().get("counter_delay", 0))
            self.reply(dict(input_tokens=control().get("token_count", 10), context_reserve_tokens=8))
            return
        if self.path == "/apply-template":
            self.reply(dict(prompt="synthetic:" + json.dumps(body, sort_keys=True)))
            return
        if self.path == "/tokenize":
            time.sleep(control().get("counter_delay", 0))
            self.reply(dict(tokens=[0] * control().get("token_count", 10)))
            return
        self.complete(body)

    def complete(self, body):
        assert body["model"] == model
        event("dispatch")
        time.sleep(control().get("delay", 0))
        tool_messages = any(message["role"] == "tool" for message in body["messages"])
        message = dict(role="assistant", content="Synthetic final answer", reasoning_content="Synthetic reason")
        finish = "stop"
        if body.get("tools") and not tool_messages:
            message.update(content=None, tool_calls=[dict(id="call-fixture-1", type="function",
                function=dict(name="lookup_temperature", arguments='{"city":"Dublin"}'))])
            finish = "tool_calls"
        result = dict(id=activation, model=model,
            choices=[dict(index=0, message=message, finish_reason=finish)],
            usage=dict(prompt_tokens=10, completion_tokens=8, total_tokens=18),
            timings=dict(predicted_per_second=42.5))
        if body.get("stream"):
            frames = [dict(model=model, choices=[dict(index=0, delta=message, finish_reason=None)]),
                      dict(model=model, choices=[dict(index=0, delta={}, finish_reason=finish)],
                           usage=result["usage"])]
            data = ("".join("data: " + json.dumps(frame) + "\n\n" for frame in frames)
                    + "data: [DONE]\n\n").encode()
            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream")
            self.send_header("Content-Length", str(len(data)))
            self.end_headers()
            self.wfile.write(data)
        else:
            self.reply(result)
        event("complete")


signal.signal(signal.SIGTERM, stop)
if tag == "dual-brain":
    engine = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(120)"],
                              stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                              stderr=subprocess.DEVNULL)
    (root / (activation + ".engine.json")).write_text(json.dumps(dict(pid=engine.pid)))
server = http.server.ThreadingHTTPServer(("127.0.0.1", int(listen)), Handler)
event("start", dict(cwd=os.getcwd(), visible_devices=os.environ.get("CUDA_VISIBLE_DEVICES"),
                    custom_environment=os.environ.get("RACK_DUAL_BOUND_VALUE"),
                    affinity=sorted(os.sched_getaffinity(0))))
server.serve_forever()
