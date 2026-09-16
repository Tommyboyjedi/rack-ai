"""Administrator-owned registry; callers never provide paths."""
from dataclasses import dataclass
from pathlib import Path
import hashlib
import io
import json
import re
import wave

MAX_REFERENCE_BYTES = 6 * 1024 * 1024
MAX_REGISTRY_BYTES = 65536
VOICE_ID = re.compile(r"[A-Za-z0-9_-][A-Za-z0-9_.-]{0,127}\Z")

@dataclass(frozen=True)
class Voice:
    identity: str
    digest: str
    data: bytes

class Registry:
    def __init__(self, path):
        self.path = Path(path)
        if not self.path.is_absolute() or self.path.is_symlink():
            raise ValueError("invalid_voice_registry")

    def load(self):
        if self.path.stat().st_mode & 0o077:
            raise ValueError("voice_registry_permissions")
        with self.path.open("rb") as stream:
            raw = stream.read(MAX_REGISTRY_BYTES + 1)
        if len(raw) > MAX_REGISTRY_BYTES:
            raise ValueError("voice_registry_bounds")
        spec = json.loads(raw)
        if set(spec) != {"root", "voices"} or not 1 <= len(spec["voices"]) <= 128:
            raise ValueError("invalid_voice_registry")
        root = Path(spec["root"])
        if not root.is_absolute() or root.resolve() != root:
            raise ValueError("invalid_voice_root")
        result = {}
        for identity, entry in spec["voices"].items():
            if not VOICE_ID.fullmatch(identity) or ".." in identity:
                raise ValueError("invalid_voice_id")
            if set(entry) != {"file", "sha256"}:
                raise ValueError("invalid_voice_entry")
            relative = Path(entry["file"])
            if relative.is_absolute() or ".." in relative.parts or not relative.parts:
                raise ValueError("invalid_voice_path")
            path = root / relative
            if path.resolve() != path or not path.resolve().is_relative_to(root):
                raise ValueError("invalid_voice_path")
            with path.open("rb") as stream:
                data = stream.read(MAX_REFERENCE_BYTES + 1)
            if len(data) > MAX_REFERENCE_BYTES or hashlib.sha256(data).hexdigest() != entry["sha256"]:
                raise ValueError("voice_integrity_failure")
            with wave.open(io.BytesIO(data)) as audio:
                if (audio.getnchannels() != 1 or audio.getsampwidth() != 2
                    or audio.getframerate() not in (16000, 22050, 24000, 44100, 48000)
                    or not 5 < audio.getnframes() / audio.getframerate() <= 30
                    or audio.getcomptype() != "NONE"):
                    raise ValueError("voice_format_bounds")
                if len(audio.readframes(audio.getnframes())) != audio.getnframes() * 2:
                    raise ValueError("voice_truncated")
            result[identity] = Voice(identity, entry["sha256"], data)
        return result
