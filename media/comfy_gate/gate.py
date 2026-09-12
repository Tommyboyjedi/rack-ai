"""Admission barrier shared by all pinned ComfyUI mutation routes and aliases."""
import asyncio
import hmac
import json
import math
import os
import time
from pathlib import Path
from aiohttp import web

MAX_AUTHORITY_BYTES = 4096
SAFE_METHODS = frozenset({"GET", "HEAD", "OPTIONS"})


class Authority:
    def __init__(self, paths):
        self.paths = paths
        self.activation = self.read().get("activation", "")
        self.invocation = os.environ.get("INVOCATION_ID", "")

    def read(self):
        try:
            raw = self.paths["authority"].read_bytes()
            if len(raw) > MAX_AUTHORITY_BYTES:
                return {}
            value = json.loads(raw)
            if not isinstance(value, dict):
                return {}
            return value
        except (OSError, ValueError, TypeError):
            return {}

    def authentic(self, request):
        try:
            secret = self.paths["secret"].read_text().strip()
            supplied = request.headers.get("X-Rack-Control", "")
            return bool(secret) and hmac.compare_digest(secret, supplied)
        except OSError:
            return False

    def status(self):
        value = self.read()
        mode = value.get("mode", "closed")
        expires = value.get("expires", 0)
        if (not isinstance(expires, (int, float)) or isinstance(expires, bool)
                or not math.isfinite(expires)
                or not self.activation or value.get("activation") != self.activation
                or expires < time.time()):
            mode = "closed"
        if mode not in {"closed", "interactive", "managed"}:
            mode = "closed"
        return {"activation": self.activation, "invocation": self.invocation,
                "pid": os.getpid(), "mode": mode, "protocol": "rack-gate/v1"}


class AdmissionGate:
    def __init__(self, authority):
        self.authority = authority
        self.barrier = asyncio.Lock()

    @web.middleware
    async def middleware(self, request, handler):
        path = request.path.removeprefix("/api")
        if path in {"/rack-gate/status", "/rack-gate/barrier"}:
            if not self.authority.authentic(request):
                raise web.HTTPForbidden()
            async with self.barrier:
                return web.json_response(self.authority.status())
        if request.method in SAFE_METHODS:
            return await handler(request)
        # Hold the barrier through validation AND enqueue, not just the state read.
        async with self.barrier:
            status = self.authority.status()
            if not self.authority.authentic(request):
                raise web.HTTPForbidden()
            if status["mode"] == "closed":
                raise web.HTTPServiceUnavailable(text="Rack AI admission is closed")
            if request.headers.get("X-Rack-Access") != status["mode"]:
                raise web.HTTPForbidden()
            return await handler(request)


def install(app):
    paths = {"authority": Path(os.environ["RACK_MEDIA_AUTHORITY_FILE"]),
             "secret": Path(os.environ["RACK_MEDIA_CONTROL_SECRET_FILE"])}
    gate = AdmissionGate(Authority(paths))
    app.middlewares.insert(0, gate.middleware)
    # Routes exist before app freeze; middleware provides the authenticated response.
    async def control(request):
        raise web.HTTPForbidden()
    app.router.add_get("/rack-gate/status", control)
    app.router.add_post("/rack-gate/barrier", control)
    return gate
