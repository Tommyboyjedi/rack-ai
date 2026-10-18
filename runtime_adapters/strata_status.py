"""Truthful native process and loaded-model status for managed readiness."""
import hashlib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class RuntimeStatus:
    service: object
    config_path: Path

    def describe(self):
        engine = self.service.engine
        process = getattr(engine, "proc", None)
        vision = getattr(self.service, "vision", None)
        encoder = getattr(vision, "proc", None)
        return {
            "schema": "rack-ai/strata-runtime/v1", "service": "strata",
            "model": self.service.model, "max_context": engine.max_context,
            "loaded": self.service.loaded(), "engine_pid": process.pid if process else None,
            "config_sha256": hashlib.sha256(self.config_path.read_bytes()).hexdigest(),
            "structured_output": {"method": "prompt_and_validate", "constrained_decoding": False},
            "vision": {
                "enabled": vision is not None,
                "loaded": vision.alive() if vision is not None else False,
                "pid": encoder.pid if encoder is not None else None,
                "directory": str(vision.dir) if vision is not None else None,
                "protocol": "rackai-count-v1" if getattr(vision, "rackai_count_protocol", False) else None,
            },
        }
