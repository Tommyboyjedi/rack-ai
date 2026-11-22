#!/usr/bin/python3
"""Passive energy collector. CPU/GPU counters only; no reservations or inference."""
from pathlib import Path
import fcntl, json, os, socket, sys, time
sys.path.insert(0, str(Path(__file__).resolve().parent))
from config import Config
from model import Reading
from nvml import Nvml
from sensors import Sensors
from intervals import Intervals
from history import History
from report import Report

def notify(message):
    address = os.environ.get("NOTIFY_SOCKET")
    if address:
        if address.startswith("@"):
            address = "\0" + address[1:]
        with socket.socket(socket.AF_UNIX, socket.SOCK_DGRAM) as channel:
            channel.connect(address)
            channel.send(message.encode())

def main():
    if len(sys.argv) != 2:
        raise ValueError("usage: collector.py CONFIG")
    os.umask(0o077)
    config = Config.load(sys.argv[1])
    Path(config.state_root).mkdir(mode=0o750, parents=True, exist_ok=True)
    lock = open(Path(config.state_root) / "collector.lock", "a")
    fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    history = History(config)
    raw = history.meta("checkpoint")
    previous = Reading.decode(json.loads(raw)) if raw else None
    sensors, intervals, report = Sensors(Nvml()), Intervals(config), Report(history)
    notify("READY=1")
    while True:
        started = time.monotonic()
        try:
            current = sensors.read()
            batch = intervals.between((previous, current))
            history.record(batch)
            previous = current
            report.publish(batch)
            notify("WATCHDOG=1")
        except Exception as error:
            print("Energy collection failed: " + type(error).__name__, file=sys.stderr, flush=True)
        time.sleep(max(1, config.sample_seconds - (time.monotonic() - started)))

if __name__ == "__main__":
    main()
