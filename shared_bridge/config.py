"""Typed, validated Shared-Bridge configuration."""
from dataclasses import dataclass
import json
from pathlib import Path
from urllib.parse import urlparse

PRIORITIES = ("low", "medium", "high", "paramount")
MAX_TTL_SECONDS = 86400


@dataclass(frozen=True)
class Config:
    """One deployment of the bridge. Values come from an operator-owned file."""

    runtime_url: str
    token_path: Path
    state_path: Path
    public_base_url: str
    listen_host: str
    listen_port: int
    priority: str = "medium"
    ttl_seconds: int = 1800
    default_service: str = "mini-brain"
    request_timeout: float = 20.0
    switch_timeout_seconds: int = 3600

    @classmethod
    def load(cls, path):
        raw = json.loads(Path(path).read_text(encoding="utf-8"))
        config = cls(
            runtime_url=raw["runtime_url"],
            token_path=Path(raw["token_path"]),
            state_path=Path(raw["state_path"]),
            public_base_url=raw["public_base_url"],
            listen_host=raw.get("listen_host", "127.0.0.1"),
            listen_port=int(raw["listen_port"]),
            priority=raw.get("priority", "medium"),
            ttl_seconds=int(raw.get("ttl_seconds", 1800)),
            default_service=raw.get("default_service", "mini-brain"),
            request_timeout=float(raw.get("request_timeout", 20.0)),
            switch_timeout_seconds=int(raw.get("switch_timeout_seconds", 3600)),
        )
        config.validate()
        return config

    def validate(self):
        base = urlparse(self.public_base_url)
        if base.scheme != "https" or not base.netloc or base.path not in ("", "/"):
            raise ValueError("public_base_url must be an https origin without a path")
        if not self.runtime_url.startswith(("http://127.0.0.1:", "http://[::1]:")):
            raise ValueError("runtime_url must be a loopback RackAI endpoint")
        if self.priority not in PRIORITIES:
            raise ValueError("priority must be one of " + ", ".join(PRIORITIES))
        if not 0 < self.listen_port < 65536:
            raise ValueError("listen_port out of range")
        if not 0 < self.ttl_seconds <= MAX_TTL_SECONDS:
            raise ValueError("ttl_seconds out of range")
        if self.switch_timeout_seconds <= 0:
            raise ValueError("switch_timeout_seconds must be positive")


def public_url(config, path):
    """Join a RackAI scoped path onto the externally reachable origin."""
    return config.public_base_url.rstrip("/") + path
