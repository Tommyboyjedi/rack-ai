"""Restrict the prepared server to the qualified RackAI adapter surface."""
import json
from dataclasses import dataclass
from pathlib import Path

from strata_count import ChatCounter
from strata_guard import ActivationGuard
from strata_images import InputMessages
from strata_status import RuntimeStatus

MAX_BODY_BYTES = 1024 * 1024
GET_PATHS = frozenset(("/health", "/v1/models", "/props", "/slots", "/metrics", "/v1/status"))


@dataclass(frozen=True)
class AdapterSurface:
    runtime: object
    config_path: Path

    def install(self):
        original = self.runtime.make_handler

        def factory(service):
            guard = ActivationGuard(service, self.runtime)
            service.rackai_guard = guard
            service.shared, service.shared_path = {}, None
            handler = original(service)
            original_get, original_post = handler.do_GET, handler.do_POST
            original_body = handler._body
            counter = ChatCounter(service, self.runtime)

            def get(instance):
                path = instance.path.split("?")[0].rstrip("/")
                if path == "/rackai/status":
                    if instance._authorized():
                        try:
                            guard.check()
                            instance._json(200, RuntimeStatus(service, self.config_path).describe())
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
                        guard.check()
                    if path == "/v1/chat/completions":
                        original_post(instance)
                    elif path == "/rackai/input-tokens":
                        self.count(instance, counter)
                    else:
                        instance._json(404, {"error": {"message": "not found"}})
                except self.runtime.EngineDied as error:
                    instance._json(503, {"error": {"message": str(error)}})

            def body(instance):
                value = original_body(instance)
                if instance.path.split("?")[0].rstrip("/") == "/v1/chat/completions":
                    InputMessages.validate(json.loads(value))
                return value

            handler.do_GET, handler.do_POST, handler._body = get, post, body
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
            guard = getattr(counter.service, "rackai_guard", None)
            if guard is not None:
                guard.check()
            instance._json(200, result)
        except self.runtime.BadBody as error:
            instance._json(error.status, {"error": {"message": str(error)}})
        except (ValueError, TypeError) as error:
            instance._json(400, {"error": {"message": str(error)}})
