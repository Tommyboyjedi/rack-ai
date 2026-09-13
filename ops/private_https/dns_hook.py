#!/usr/bin/python3
"""Certbot's automatic DNS-01 auth/cleanup hooks; bounded propagation checks."""
import os
import sys
import time
from duckdns import DuckDns
from settings import Settings, RESOLVERS, resolve

PROPAGATION_SECONDS = 240
POLL_SECONDS = 10

def challenge(settings, action):
    if os.environ.get("CERTBOT_DOMAIN") != settings.hostname:
        raise RuntimeError("Unapproved ACME challenge domain")
    api = DuckDns(settings)
    if action == "cleanup":
        api.clear_txt()
        print("DuckDNS challenge cleanup accepted", flush=True)
        return
    value = os.environ.get("CERTBOT_VALIDATION", "")
    if action != "auth" or not value or not all(c.isalnum() or c in "-_" for c in value):
        raise RuntimeError("Invalid ACME challenge")
    api.set_txt(value)
    deadline = time.monotonic() + PROPAGATION_SECONDS
    while time.monotonic() < deadline:
        if all(value in resolve(("_acme-challenge."+settings.hostname, "TXT"), r) for r in RESOLVERS):
            print("DuckDNS challenge visible through both independent resolvers", flush=True)
            return
        print("Waiting for DNS challenge propagation", flush=True)
        time.sleep(POLL_SECONDS)
    raise RuntimeError("DNS challenge propagation deadline exceeded")

if __name__ == "__main__":
    try:
        challenge(Settings.load(), sys.argv[1])
    except Exception as error:
        print("DNS-01 hook failed: " + str(error), file=sys.stderr)
        sys.exit(1)
