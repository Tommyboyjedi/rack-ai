"""Fence all native children within one RackAI activation."""
from dataclasses import dataclass, field

from strata_engine import EngineGuard


@dataclass
class ActivationGuard:
    service: object
    runtime: object
    engine: EngineGuard = field(init=False)
    vision: object = field(init=False)

    def __post_init__(self):
        self.engine = EngineGuard(self.service.engine, self.runtime)
        vision = getattr(self.service, "vision", None)
        self.vision = EngineGuard(vision, self.runtime) if vision is not None else None
        if vision is not None and not getattr(vision, "rackai_count_protocol", False):
            raise ValueError("the image encoder has not qualified exact counting")

    def check(self):
        self.engine.check(self.service.engine)
        current = getattr(self.service, "vision", None)
        if self.vision is not None:
            self.vision.check(current)
        elif current is not None:
            raise self.runtime.EngineDied("managed vision generation changed")
