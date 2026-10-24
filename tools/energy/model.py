"""Observation and interval values; raw checkpoints never reach the public report."""
from dataclasses import dataclass, field

@dataclass(frozen=True)
class Counter:
    joules: float
    modulus: float | None = None

@dataclass(frozen=True)
class Gpu:
    id: str
    name: str
    watts: float | None
    joules: float | None

@dataclass(frozen=True)
class Reading:
    at: float
    monotonic: float
    boot: str
    gpus: list[Gpu]
    packages: dict[str, Counter]
    issues: list[str] = field(default_factory=list)

    @classmethod
    def decode(cls, raw):
        return cls(raw["at"], raw["monotonic"], raw["boot"],
                   [Gpu(**g) for g in raw["gpus"]],
                   {key: Counter(**value) for key, value in raw["packages"].items()},
                   raw.get("issues", []))

@dataclass(frozen=True)
class Interval:
    id: str
    label: str
    kind: str
    start: float
    end: float
    kwh: float

@dataclass
class Batch:
    reading: Reading
    intervals: list[Interval] = field(default_factory=list)
    issues: list[str] = field(default_factory=list)
