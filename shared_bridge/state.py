"""Durable bridge state. Only the selected service and reservation identity persist."""
from dataclasses import asdict, dataclass
import json
import os
from pathlib import Path
import threading
import time
import uuid

STATE_VERSION = 1


@dataclass(frozen=True)
class Reservation:
    id: str
    acquisition_id: str
    service: str
    created_at: int

    @classmethod
    def from_dict(cls, raw):
        return cls(
            id=str(raw["id"]),
            acquisition_id=str(raw["acquisition_id"]),
            service=str(raw["service"]),
            created_at=int(raw.get("created_at", 0)),
        )


@dataclass(frozen=True)
class Outcome:
    """A bounded, RackAI-derived result kept to avoid a busy retry loop."""

    state: str
    reason: str | None
    retry_after: int
    recorded_at: int

    @classmethod
    def from_dict(cls, raw):
        return cls(
            state=str(raw["state"]),
            reason=None if raw.get("reason") is None else str(raw["reason"]),
            retry_after=int(raw.get("retry_after", 2)),
            recorded_at=int(raw.get("recorded_at", 0)),
        )


@dataclass(frozen=True)
class Switch:
    to_service: str
    phase: str
    started_at: int

    @classmethod
    def from_dict(cls, raw):
        return cls(
            to_service=str(raw["to_service"]),
            phase=str(raw["phase"]),
            started_at=int(raw.get("started_at", 0)),
        )


@dataclass(frozen=True)
class State:
    selected_service: str
    reservation: Reservation | None = None
    outcome: Outcome | None = None
    switch: Switch | None = None
    enabled: bool = True

    def as_dict(self):
        return {
            "version": STATE_VERSION,
            "selected_service": self.selected_service,
            "enabled": self.enabled,
            "reservation": None if self.reservation is None else asdict(self.reservation),
            "outcome": None if self.outcome is None else asdict(self.outcome),
            "switch": None if self.switch is None else asdict(self.switch),
        }

    @classmethod
    def from_dict(cls, raw, default_service):
        return cls(
            selected_service=str(raw.get("selected_service") or default_service),
            enabled=raw.get("enabled", True) is True,
            reservation=_optional(Reservation.from_dict, raw.get("reservation")),
            outcome=_optional(Outcome.from_dict, raw.get("outcome")),
            switch=_optional(Switch.from_dict, raw.get("switch")),
        )


def _optional(parse, raw):
    return None if raw is None else parse(raw)


class StateStore:
    """Atomic, mode-0600 JSON state. A corrupt file is quarantined, never guessed."""

    def __init__(self, path):
        self._path = Path(path)
        self._lock = threading.Lock()

    def load(self, default_service):
        if not self._path.exists():
            return State(default_service)
        try:
            raw = json.loads(self._path.read_text(encoding="utf-8"))
        except (ValueError, OSError):
            self._quarantine()
            return State(default_service)
        return State.from_dict(raw, default_service)

    def save(self, state):
        with self._lock:
            self._path.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            temporary = self._path.with_name(
                f"{self._path.name}.tmp-{os.getpid()}-{threading.get_ident()}-{uuid.uuid4().hex[:8]}"
            )
            descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_TRUNC, 0o600)
            with os.fdopen(descriptor, "w", encoding="utf-8") as handle:
                json.dump(state.as_dict(), handle, sort_keys=True)
                handle.flush()
                os.fsync(handle.fileno())
            os.replace(temporary, self._path)
            os.chmod(self._path, 0o600)

    def _quarantine(self):
        suffix = f".corrupt-{int(time.time())}"
        try:
            os.replace(self._path, self._path.with_name(self._path.name + suffix))
        except OSError:
            pass
