"""Typed, conservative configuration for passive rack energy collection."""
from dataclasses import dataclass
from pathlib import Path
import json, math

@dataclass(frozen=True)
class Config:
    state_root: str
    sample_seconds: int = 30
    other_components_watts: float = 50.0
    psu_efficiency: float = 0.85
    expected_gpus: int = 3
    minute_days: int = 30
    hourly_days: int = 400
    daily_days: int = 1825

    @classmethod
    def load(cls, path):
        result = cls(**json.loads(Path(path).read_text()))
        result.validate()
        return result

    def validate(self):
        if any(type(v) is not int for v in [self.sample_seconds, self.expected_gpus, self.minute_days, self.hourly_days, self.daily_days]):
            raise ValueError("collection and retention bounds must be integers")
        if not Path(self.state_root).is_absolute():
            raise ValueError("state root must be absolute")
        if not 5 <= self.sample_seconds <= 30 or not 1 <= self.expected_gpus <= 16:
            raise ValueError("invalid collection bounds")
        if not math.isfinite(self.other_components_watts) or not 0 <= self.other_components_watts <= 1000:
            raise ValueError("invalid additional component allowance")
        if not math.isfinite(self.psu_efficiency) or not 0.5 <= self.psu_efficiency <= 1:
            raise ValueError("invalid assumed PSU efficiency")
        if not 1 <= self.minute_days <= 31 or not 365 <= self.hourly_days <= 730 or not 365 <= self.daily_days <= 3650:
            raise ValueError("invalid history retention")

    def methodology(self):
        return {"id": "component-estimate-v1", "calibrated": False,
                "other_components_watts": self.other_components_watts,
                "psu_efficiency": self.psu_efficiency,
                "formula": "(GPU energy + CPU package energy + assumed other-component energy) / assumed PSU efficiency",
                "total_classification": "estimated", "gpu_classification": "measured_device",
                "note": "Uncalibrated estimate, not a socket measurement. GPU device telemetry excludes PSU losses."}
