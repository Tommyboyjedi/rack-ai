"""Loopback worker owned by PR35 systemd hosting, with a fatal generation watchdog."""
from http.server import BaseHTTPRequestHandler
from server import Server
from dataclasses import dataclass
import argparse
import hmac
import json
import os
import threading
from engine import Engine
from voices import Registry

MAX_BODY = 8192
GENERATION_SECONDS = 60

@dataclass(frozen=True)
class Worker:
    engine: Engine
    registry: Registry
    activation: str

class Handler(BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def authorized(self):
        return hmac.compare_digest(self.headers.get("Authorization", ""),
            "Bearer " + self.server.worker.activation)

    def reply(self, response):
        status, kind, data = response
        self.send_response(status)
        self.send_header("Content-Type", kind)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if not self.authorized():
            return self.reply((401, "application/json", b'{"error":"unauthorized"}'))
        if self.path != "/health":
            return self.reply((404, "application/json", b'{}'))
        try:
            worker = self.server.worker
            voices = list(worker.registry.load())
            data = dict(model="ResembleAI/chatterbox-turbo", activation=worker.activation,
                sample_rate=24000, voices=voices, model_loads=1,
                load_seconds=worker.engine.load_seconds)
            self.reply((200, "application/json", json.dumps(data).encode()))
        except (ValueError, OSError):
            self.reply((503, "application/json", b'{"error":"voice_registry_invalid"}'))

    def do_POST(self):
        if not self.authorized():
            return self.reply((401, "application/json", b'{"error":"unauthorized"}'))
        if self.path != "/speech":
            return self.reply((404, "application/json", b'{}'))
        self.connection.settimeout(5)
        try:
            size = int(self.headers.get("Content-Length", "0"))
            if not 0 < size <= MAX_BODY or self.headers.get("Transfer-Encoding"):
                raise ValueError("request_bounds")
            body = json.loads(self.rfile.read(size))
            if set(body) != {"text", "voice"} or not isinstance(body["text"], str):
                raise ValueError("invalid_speech_request")
            text = body["text"]
            if not text.strip() or len(text) > 1000 or len(text.encode()) > 4096 or "\0" in text:
                raise ValueError("speech_text_bounds")
            voices = self.server.worker.registry.load()
            if not isinstance(body["voice"], str) or body["voice"] not in voices:
                raise ValueError("unknown_voice_id")
        except (ValueError, OSError, TypeError):
            return self.reply((422, "application/json", b'{"error":"invalid_speech_request"}'))
        if not self.server.synthesis.acquire(blocking=False):
            return self.reply((429, "application/json", b'{"error":"speech_active"}'))
        # A hung CUDA call cannot outlive this activation. PR35 retains cleanup ownership.
        timer = threading.Timer(GENERATION_SECONDS, lambda: os._exit(124))
        timer.daemon = True
        timer.start()
        try:
            data = self.server.worker.engine.synthesize((text, voices[body["voice"]]))
        except Exception:
            self.reply((500, "application/json", b'{"error":"synthesis_failed"}'))
            return
        finally:
            timer.cancel()
            self.server.synthesis.release()
        self.reply((200, "audio/wav", data))

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--host", choices=["127.0.0.1"], required=True)
    parser.add_argument("--port", type=int, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--voices", required=True)
    args = parser.parse_args()
    activation = os.environ.get("RACK_RUNTIME_ACTIVATION")
    devices = os.environ.get("CUDA_VISIBLE_DEVICES")
    if not activation or devices != "GPU-357ef569-8fac-7c7d-ee1c-51677efb174f":
        raise RuntimeError("managed_exact_2060_activation_required")
    os.environ["HF_HUB_OFFLINE"] = "1"
    os.environ["TRANSFORMERS_OFFLINE"] = "1"
    registry = Registry(args.voices)
    registry.load()
    from integrity import verify
    verify(args.model)
    worker = Worker(Engine(args.model), registry, activation)
    server = Server((args.host, args.port), Handler)
    server.worker = worker
    server.serve_forever()

if __name__ == "__main__":
    main()
