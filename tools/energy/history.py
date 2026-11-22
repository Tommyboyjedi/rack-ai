"""Bounded SQLite rollups, durable checkpoints, and retention independent of leases."""
from dataclasses import asdict
from pathlib import Path
import json, sqlite3

class History:
    def __init__(self, config):
        self.config = config
        root = Path(config.state_root)
        root.mkdir(mode=0o750, parents=True, exist_ok=True)
        self.db = sqlite3.connect(root / "history.sqlite3", timeout=3)
        self.db.execute("PRAGMA journal_mode=WAL")
        self.db.execute("PRAGMA synchronous=FULL")
        self.db.execute("PRAGMA max_page_count=32768")
        self.db.execute("PRAGMA journal_size_limit=4194304")
        self.db.executescript("""
            CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS series(id TEXT PRIMARY KEY,label TEXT NOT NULL,kind TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS buckets(
              size INTEGER NOT NULL,at INTEGER NOT NULL,series TEXT NOT NULL,
              kwh REAL NOT NULL,seconds REAL NOT NULL,PRIMARY KEY(size,at,series));
        """)
        method = json.dumps(config.methodology(), sort_keys=True)
        previous = self.meta("methodology")
        if previous is not None and previous != method:
            raise ValueError("estimate assumptions changed; archive/version the history before changing methodology")
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO meta VALUES('methodology',?)", (method,))

    def meta(self, key):
        row = self.db.execute("SELECT value FROM meta WHERE key=?", (key,)).fetchone()
        return row[0] if row else None

    def record(self, batch):
        reading = batch.reading
        previous_at = float(self.meta("last_at") or 0)
        if reading.at <= previous_at:
            raise ValueError("clock moved backwards; keeping the last durable checkpoint")
        with self.db:
            self.db.execute("INSERT OR IGNORE INTO meta VALUES('started_at',?)", (str(reading.at),))
            metadata = [(g.id, g.name, "measured_device") for g in reading.gpus]
            metadata += [("estimated-total", "Estimated rack total", "estimated"),
                         ("cpu-package", "CPU package", "counter_derived")]
            for item in metadata:
                self.db.execute("INSERT OR IGNORE INTO series VALUES(?,?,?)", item)
            for item in batch.intervals:
                self.db.execute("INSERT INTO series VALUES(?,?,?) ON CONFLICT(id) DO UPDATE SET label=excluded.label",
                                (item.id, item.label, item.kind))
                for size in (60, 3600, 86400):
                    self.add(item, size)
            for key, value in [("checkpoint", json.dumps(asdict(reading))), ("last_at", str(reading.at))]:
                self.db.execute("INSERT OR REPLACE INTO meta VALUES(?,?)", (key, value))
            if not previous_at or int(reading.at) // 3600 != int(previous_at) // 3600:
                for size, days in [(60, self.config.minute_days), (3600, self.config.hourly_days), (86400, self.config.daily_days)]:
                    self.db.execute("DELETE FROM buckets WHERE size=? AND at<?", (size, reading.at - days * 86400))
        self.db.execute("PRAGMA wal_checkpoint(PASSIVE)")

    def add(self, item, size):
        cursor = item.start
        while cursor < item.end:
            at = int(cursor // size) * size
            end = min(item.end, at + size)
            seconds = end - cursor
            kwh = item.kwh * seconds / (item.end - item.start)
            self.db.execute("""INSERT INTO buckets VALUES(?,?,?,?,?)
                ON CONFLICT(size,at,series) DO UPDATE SET
                kwh=buckets.kwh+excluded.kwh,seconds=buckets.seconds+excluded.seconds""",
                (size, at, item.id, kwh, seconds))
            cursor = end
