"""Build a pinned zipapp including integrity metadata for administrator-supplied weights."""
import argparse
import hashlib
import json
from pathlib import Path
import zipfile

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    files = sorted(p for p in args.model.iterdir() if p.is_file())
    if not files:
        raise ValueError("model_files_missing")
    manifest = {}
    for path in files:
        with path.open("rb") as stream:
            manifest[path.name] = hashlib.file_digest(stream, "sha256").hexdigest()
    with zipfile.ZipFile(args.output, "x", compression=zipfile.ZIP_DEFLATED) as archive:
        for name in ("__main__.py", "engine.py", "voices.py", "integrity.py", "server.py"):
            archive.write(Path(__file__).parent / name, name)
        archive.writestr("model_manifest.py", "FILES = " + repr(manifest) + "\n")
    print(hashlib.sha256(args.output.read_bytes()).hexdigest())

if __name__ == "__main__":
    main()
