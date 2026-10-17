"""Disposable CPU backend for preflight transport tests; tokens are synthetic bytes."""
import http.server
import json
import os
from pathlib import Path
import signal
import sys
import time

port, model, events, controls = sys.argv[1:5]
activation = os.environ["RACK_RUNTIME_ACTIVATION"]


def control():
    path = Path(controls)
    return json.loads(path.read_text()) if path.exists() else {}


def event(kind):
    with open(events, "a") as output:
        output.write(json.dumps(dict(kind=kind, model=model, pid=os.getpid(), activation=activation)) + "\n")


def rendered(body):
    # Stable, inspectable fixture template, not a real model's Jinja template.
    return "synthetic-template\n" + json.dumps(body, ensure_ascii=False, sort_keys=True, separators=(",", ":"))


def synthetic_tokens(text):
    return list(text.encode("utf-8"))


def stop(*_):
    event("stop")
    sys.exit(0)


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def reply(self, value, status=200):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        event("probe")
        time.sleep(control().get("probe_delay", 0))
        self.reply({"data": [{"id": control().get("model", model)}]})

    def do_POST(self):
        request = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        stage = {"/apply-template": "template", "/tokenize": "tokenize"}.get(self.path)
        if stage is not None:
            with open(events + ".preflight", "a") as output:
                output.write(json.dumps(dict(stage=stage, body=request)) + "\n")
            options = control()
            time.sleep(options.get(stage + "_delay", 0))
            if options.get(stage + "_status", 200) != 200:
                self.reply({"error": "synthetic backend failure"}, options[stage + "_status"])
                return
            if stage + "_response" in options:
                self.reply(options[stage + "_response"])
                return
            if stage == "template":
                prompt = rendered(request)
                if "template_size" in options:
                    prompt = "x" * options["template_size"]
                self.reply({"prompt": prompt})
            else:
                tokens = synthetic_tokens(request["content"])
                if "token_count" in options:
                    tokens = list(range(options["token_count"]))
                self.reply({"tokens": tokens})
            return
        if self.path != "/v1/chat/completions":
            self.reply({"error": "unsupported synthetic route"}, 404)
            return
        assert request["model"] == model
        event("dispatch")
        with open(events + ".requests", "a") as output:
            output.write(json.dumps(request) + "\n")
        self.reply({"id": activation, "model": model, "choices": [
            {"message": {"role": "assistant", "content": "synthetic generation"}}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2}})
        event("complete")


signal.signal(signal.SIGTERM, stop)
server = http.server.ThreadingHTTPServer(("127.0.0.1", int(port)), Handler)
event("start")
server.serve_forever()
