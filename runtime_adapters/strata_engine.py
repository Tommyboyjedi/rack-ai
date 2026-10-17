"""Fence one engine process for the lifetime of one managed activation."""
from dataclasses import dataclass, field
from pathlib import Path


@dataclass(frozen=True)
class ProcessIdentity:
    pid: int
    start: str

    @classmethod
    def read(cls, process):
        if process is None or process.poll() is not None:
            raise ValueError("the prepared engine is not running")
        fields = Path(f"/proc/{process.pid}/stat").read_text().rsplit(") ", 1)[1].split()
        if fields[0] == "Z":
            raise ValueError("the prepared engine is a zombie")
        return cls(process.pid, fields[19])


@dataclass
class EngineGuard:
    engine: object
    runtime: object
    identity: ProcessIdentity = field(init=False)

    def __post_init__(self):
        self.identity = ProcessIdentity.read(getattr(self.engine, "proc", None))
        if not self.engine.alive():
            raise ValueError("the prepared engine has not become ready")
        self.engine.restart = self.deny_restart
        self.engine.unload = self.deny_restart

    def deny_restart(self, *arguments, **options):
        raise self.runtime.EngineDied("managed engine reload requires a new RackAI activation")

    def check(self, current):
        try:
            identity = ProcessIdentity.read(getattr(current, "proc", None))
        except (OSError, ValueError, IndexError):
            raise self.runtime.EngineDied("managed engine identity is no longer available") from None
        if current is not self.engine or identity != self.identity or not current.alive():
            raise self.runtime.EngineDied("managed engine generation changed")
