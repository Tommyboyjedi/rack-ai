"""A packaged manifest freezes the complete model directory before any CUDA load."""
import hashlib
from pathlib import Path

def verify(directory):
    from model_manifest import FILES
    root = Path(directory)
    if not root.is_absolute() or not FILES:
        raise ValueError("invalid_model_directory")
    actual = {p.name for p in root.iterdir() if p.is_file()}
    if actual != set(FILES):
        raise ValueError("model_manifest_mismatch")
    for name, expected in FILES.items():
        if Path(name).name != name:
            raise ValueError("invalid_model_manifest")
        with (root / name).open("rb") as stream:
            observed = hashlib.file_digest(stream, "sha256").hexdigest()
        if observed != expected:
            raise ValueError("model_integrity_failure")
