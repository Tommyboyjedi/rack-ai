#!/usr/bin/python3
"""Generate an explicit private listener from the installed Tailscale CLI."""
import os
import sys
from settings import Settings, tailnet_ipv4

def write_binding(settings, content):
    target = settings.root / "config/listen.conf"
    temporary = target.with_suffix(".new")
    with temporary.open("w") as output:
        os.chmod(temporary, 0o600)
        output.write(content)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(target)

def prepare(settings):
    address = tailnet_ipv4()
    write_binding(settings, "listen " + address + ":443 ssl;\n")
    print("HTTPS binding prepared for Tailscale IPv4 " + address, flush=True)
    return address

if __name__ == "__main__":
    try:
        prepare(Settings.load())
    except Exception as error:
        print("Private HTTPS preparation failed: " + str(error), file=sys.stderr)
        sys.exit(1)
