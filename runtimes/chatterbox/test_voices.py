import hashlib
import io
import json
import tempfile
import unittest
import wave
from pathlib import Path
from voices import Registry
from engine import Settings

class VoiceTests(unittest.TestCase):
    def test_registry_integrity_bounds_and_hot_reload(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            output = io.BytesIO()
            with wave.open(output, "wb") as wav:
                wav.setnchannels(1); wav.setsampwidth(2); wav.setframerate(24000)
                wav.writeframes(b"\0\0" * (24000 * 6))
            data = output.getvalue()
            (root / "approved.wav").write_bytes(data)
            entry = dict(file="approved.wav", sha256=hashlib.sha256(data).hexdigest())
            spec = dict(root=str(root), voices={"approved": entry})
            path = root / "voices.json"
            path.write_text(json.dumps(spec)); path.chmod(0o600)
            registry = Registry(path)
            self.assertEqual(list(registry.load()), ["approved"])
            spec["voices"]["second"] = entry
            path.write_text(json.dumps(spec))
            self.assertEqual(len(registry.load()), 2)
            for bad in ("../approved.wav", "/etc/passwd"):
                spec["voices"]["approved"] = dict(entry, file=bad)
                path.write_text(json.dumps(spec))
                with self.assertRaises(ValueError): registry.load()
            spec["voices"]["approved"] = dict(entry, sha256="0"*64)
            path.write_text(json.dumps(spec))
            with self.assertRaises(ValueError): registry.load()
            spec["voices"]["approved"] = entry
            path.write_text(json.dumps(spec)); path.chmod(0o644)
            with self.assertRaises(ValueError): registry.load()

    def test_qualified_defaults(self):
        self.assertEqual(Settings(), Settings(1.05, .95, 1000, 1.2, True))

if __name__ == "__main__": unittest.main()
