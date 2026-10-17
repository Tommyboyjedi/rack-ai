"""Restrict the prepared server to the qualified RackAI adapter surface."""
import hashlib
import json
from dataclasses import dataclass
from pathlib import Path

from strata_count import ChatCounter
from strata_engine import EngineGuard

MAX_BODY_BYTES = 1024 * 1024
GET_PATHS = frozenset(("/health", "/v1/models", "/props", "/slots", "/metrics", "/v1/status"))


@dataclass(frozen=True)
class AdapterSurface:
    runtime: object
    config_path: Path

    def install(self):
        original = self.runtime.make_handler

        def factory(service):
            guard = EngineGuard(service.engine, self.runtime)
            service.shared, service.shared_path = {}, None
            handler = original(service)
            original_get, original_post = handler.do_GET, handler.do_POST
            counter = ChatCounter(service, self.runtime)

            def get(instance):
                path = instance.path.split("?")[0].rstrip("/")
                if path == "/rackai/status":
                    if instance._authorized():
                        try:
                            guard.check(service.engine)
                            instance._json(200, self.status(service))
                        except self.runtime.EngineDied as error:
                            instance._json(503, {"error": {"message": str(error)}})
                elif path in GET_PATHS:
                    original_get(instance)
                else:
                    instance._json(404, {"error": {"message": "not found"}})

            def post(instance):
                path = instance.path.split("?")[0].rstrip("/")
                try:
                    if path in ("/v1/chat/completions", "/rackai/input-tokens"):
                        guard.check(service.engine)
                    if path == "/v1/chat/completions":
                        original_post(instance)
                    elif path == "/rackai/input-tokens":
                        self.count(instance, counter)
                    else:
                        instance._json(404, {"error": {"message": "not found"}})
                except self.runtime.EngineDied as error:
                    instance._json(503, {"error": {"message": str(error)}})

            handler.do_GET, handler.do_POST = get, post
            return handler

        self.runtime.make_handler = factory

    def count(self, instance, counter):
        if not instance._authorized() or instance._foreign_page():
            return
        try:
            body = instance._body()
            if len(body) > MAX_BODY_BYTES:
                instance._json(413, {"error": {"message": "request body too large"}})
                return
            result = counter.count(json.loads(body))
            instance._json(200, result)
        except self.runtime.BadBody as error:
            instance._json(error.status, {"error": {"message": str(error)}})
        except (ValueError, TypeError) as error:
            instance._json(400, {"error": {"message": str(error)}})

    def status(self, service):
        engine = service.engine
        process = getattr(engine, "proc", None)
        return {
            "schema": "rack-ai/strata-runtime/v1", "service": "strata",
            "model": service.model, "max_context": engine.max_context,
            "loaded": service.loaded(), "engine_pid": process.pid if process else None,
            "config_sha256": hashlib.sha256(self.config_path.read_bytes()).hexdigest(),
            "structured_output": {"method": "prompt_and_validate", "constrained_decoding": False},
        }
