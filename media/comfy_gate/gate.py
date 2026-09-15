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


class Activity:
    """Durable activity evidence, never an independent resource ownership grant."""
    def __init__(self, authority, work):
        self.authority, self.work = authority, work
        self.path = authority.paths["authority"].with_suffix(".activity.json")
        self.record = None
        self.failed = False

    def observe(self, accepted=False):
        if self.failed:
            raise web.HTTPServiceUnavailable(text="activity persistence failed")
        activation = self.authority.activation
        if not activation:
            raise web.HTTPServiceUnavailable(text="missing activity activation")
        if self.record is None or self.record["activation"] != activation:
            try:
                raw = self.path.read_bytes()
                if len(raw) > MAX_AUTHORITY_BYTES:
                    raise ValueError("oversized activity evidence")
                value = json.loads(raw)
                if (not isinstance(value, dict) or
                        not isinstance(value.get("last_activity_at"), int) or
                        isinstance(value["last_activity_at"], bool) or
                        value["last_activity_at"] < 0 or
                        not isinstance(value.get("busy"), bool) or
                        not isinstance(value.get("idle_closed"), bool)):
                    raise ValueError("invalid activity evidence")
                self.record = value if value.get("activation") == activation else None
            except FileNotFoundError:
                self.record = None
            if self.record is None:
                self.record = dict(activation=activation, last_activity_at=int(self.work["clock"]()),
                                   busy=False, idle_closed=False)
                self.save()
        busy = bool(self.work["busy"]())
        before = dict(self.record)
        # A previously observed busy queue becoming empty is work completion.
        if accepted or busy or self.record["busy"]:
            self.record["last_activity_at"] = max(self.record["last_activity_at"],
                                                   int(self.work["clock"]()))
        self.record["busy"] = busy
        if before != self.record:
            self.save()
        return dict(self.record)

    def close_if_idle(self, seconds):
        value = self.observe()
        if not value["busy"] and self.work["clock"]() - value["last_activity_at"] >= seconds:
            self.record["idle_closed"] = True
            self.save()
        return dict(self.record)

    def save(self):
        import tempfile
        temporary = None
        try:
            fd, temporary = tempfile.mkstemp(prefix=".rack-activity-", dir=self.path.parent)
            with os.fdopen(fd, "w") as output:
                json.dump(self.record, output)
                output.flush()
                os.fsync(output.fileno())
            os.replace(temporary, self.path)
            directory = os.open(self.path.parent, os.O_RDONLY | os.O_DIRECTORY)
            try:
                os.fsync(directory)
            finally:
                os.close(directory)
        except BaseException:
            self.failed = True
            raise
        finally:
            if temporary is not None and os.path.exists(temporary):
                os.unlink(temporary)


class AdmissionGate:
    def __init__(self, authority, work=None):
        self.authority = authority
        self.barrier = asyncio.Lock()
        self.activity = Activity(authority, work or {"clock": time.time, "busy": lambda: False})

    def status(self, activity):
        value = self.authority.status()
        value.update({key: activity[key] for key in ("last_activity_at", "busy", "idle_closed")})
        if activity["idle_closed"]:
            value["mode"] = "closed"
        return value

    @web.middleware
    async def middleware(self, request, handler):
        path = request.path.removeprefix("/api")
        if path in {"/rack-gate/status", "/rack-gate/barrier", "/rack-gate/idle", "/rack-gate/admit-job"}:
            if not self.authority.authentic(request):
                raise web.HTTPForbidden()
            async with self.barrier:
                if path == "/rack-gate/admit-job":
                    body = await request.json()
                    status = self.status(self.activity.observe())
                    if status["mode"] != "managed" or body.get("activation") != self.authority.activation:
                        raise web.HTTPConflict(text="managed GPU admission is closed")
                    activity = self.activity.observe(accepted=True)
                elif path == "/rack-gate/idle":
                    body = await request.json()
                    seconds = body.get("idle_timeout_seconds")
                    if (not isinstance(seconds, int) or isinstance(seconds, bool)
                            or not 1 <= seconds <= 86400
                            or body.get("activation") != self.authority.activation):
                        raise web.HTTPConflict(text="invalid idle policy or activation")
                    activity = self.activity.close_if_idle(seconds)
                else:
                    activity = self.activity.observe()
                return web.json_response(self.status(activity))
        if request.method in SAFE_METHODS:
            return await handler(request)
        # Hold the barrier through validation AND enqueue, not just the state read.
        async with self.barrier:
            status = self.status(self.activity.observe())
            if not self.authority.authentic(request):
                raise web.HTTPForbidden()
            if status["mode"] == "closed":
                raise web.HTTPServiceUnavailable(text="Rack AI admission is closed")
            if request.headers.get("X-Rack-Access") != status["mode"]:
                raise web.HTTPForbidden()
            response = await handler(request)
            if path == "/prompt" and 200 <= response.status < 300:
                self.activity.observe(accepted=True)
            return response


def install(app):
    paths = {"authority": Path(os.environ["RACK_MEDIA_AUTHORITY_FILE"]),
             "secret": Path(os.environ["RACK_MEDIA_CONTROL_SECRET_FILE"])}
    from server import PromptServer
    def busy():
        running, pending = PromptServer.instance.prompt_queue.get_current_queue()
        return bool(running or pending)
    gate = AdmissionGate(Authority(paths), {"clock": time.time, "busy": busy})
    app.middlewares.insert(0, gate.middleware)
    # Routes exist before app freeze; middleware provides the authenticated response.
    async def control(request):
        raise web.HTTPForbidden()
    app.router.add_get("/rack-gate/status", control)
    app.router.add_post("/rack-gate/barrier", control)
    app.router.add_post("/rack-gate/idle", control)
    app.router.add_post("/rack-gate/admit-job", control)
    return gate
