#!/usr/bin/python3
"""Idempotent A record maintenance, with explicit removal of unwanted AAAA."""
import fcntl
import sys
import time
from binding import synchronize
from duckdns import DuckDns
from settings import Settings, RESOLVERS, resolve

def update(settings):
    address = synchronize(settings)
    api = DuckDns(settings)
    answers = [resolve((settings.hostname, "AAAA"), r) for r in RESOLVERS]
    if any(answers):
        api.clear_addresses(address)
        print("Removed prior A/AAAA records; restoring Tailscale A explicitly", flush=True)
    api.set_ipv4(address)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        valid = all(resolve((settings.hostname, "A"), r) == (address,)
                    and not resolve((settings.hostname, "AAAA"), r) for r in RESOLVERS)
        if valid:
            print("DuckDNS A verified for Tailscale IPv4 " + address + "; no AAAA", flush=True)
            return address
        print("Waiting for private DNS propagation", flush=True)
        time.sleep(10)
    raise RuntimeError("DuckDNS A/AAAA verification deadline exceeded")

def main():
    settings = Settings.load()
    with (settings.root / "run/dns-update.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
        update(settings)

if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print("DuckDNS updater failed: " + str(error), file=sys.stderr)
        sys.exit(1)
