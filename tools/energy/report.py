"""Public, bounded history snapshot; never includes raw counters, paths or boot IDs."""
from pathlib import Path
import json, os, tempfile

MAX_SNAPSHOT_BYTES = 1024 * 1024
WINDOWS = {"day": (86400, 60, 300), "week": (7 * 86400, 3600, 3600),
           "month": (30 * 86400, 3600, 21600), "year": (365 * 86400, 86400, 86400)}

class Report:
    def __init__(self, history):
        self.history = history

    def create(self, batch):
        reading = batch.reading
        latest = {item.id: item.kwh * 3_600_000 / (item.end - item.start) for item in batch.intervals}
        return {"schema": "rack-ai/energy/v1", "collected_at": reading.at,
                "started_at": float(self.history.meta("started_at")), "sample_seconds": self.history.config.sample_seconds,
                "methodology": self.history.config.methodology(), "warnings": sorted(set(batch.issues)),
                "latest": {"estimated_total_watts": latest.get("estimated-total"), "cpu_package_watts": latest.get("cpu-package"),
                           "gpus": [{"id": g.id, "name": g.name, "watts": latest.get(g.id, g.watts),
                                     "classification": "measured_device"} for g in reading.gpus]},
                "retention": {"minute_days": self.history.config.minute_days, "hourly_days": self.history.config.hourly_days,
                              "daily_days": self.history.config.daily_days},
                "history": {key: self.window((reading.at, spec)) for key, spec in WINDOWS.items()}}

    def window(self, query):
        now, (span, size, stride) = query
        start = int((now - span) // stride) * stride
        metadata = self.history.db.execute("SELECT id,label,kind FROM series ORDER BY id").fetchall()
        rows = self.history.db.execute("""SELECT series,(at/?)*? AS bucket,SUM(kwh),SUM(seconds)
            FROM buckets WHERE size=? AND at>=? AND at<? GROUP BY series,bucket ORDER BY bucket""",
            (stride, stride, size, start, now)).fetchall()
        result = {ident: {"id": ident, "label": label, "classification": kind, "kwh": 0.0, "coverage_seconds": 0.0, "points": []}
                  for ident, label, kind in metadata}
        for ident, at, kwh, seconds in rows:
            item = result[ident]
            item["kwh"] += kwh
            item["coverage_seconds"] += seconds
            item["points"].append({"at": at, "kwh": kwh, "seconds": seconds})
        return {"start": start, "end": now, "bucket_seconds": stride, "series": list(result.values())}

    def publish(self, batch):
        raw = json.dumps(self.create(batch), separators=(",", ":"), allow_nan=False).encode()
        if len(raw) > MAX_SNAPSHOT_BYTES:
            raise ValueError("public energy snapshot exceeds bound; durable history retained")
        root = Path(self.history.config.state_root)
        fd, name = tempfile.mkstemp(prefix=".energy-", dir=root)
        try:
            with os.fdopen(fd, "wb") as f:
                f.write(raw); f.flush(); os.fsync(f.fileno()); os.fchmod(f.fileno(), 0o640)
            os.replace(name, root / "public.json")
            parent = os.open(root, os.O_DIRECTORY)
            try: os.fsync(parent)
            finally: os.close(parent)
        finally:
            if os.path.exists(name): os.unlink(name)
