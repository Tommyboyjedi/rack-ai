"""Pinned operational paths; no credentials are configuration values."""
from dataclasses import dataclass
from pathlib import Path
import ipaddress
import json
import subprocess

TIMEOUT = 15
TAILNET = ipaddress.IPv4Network("100.64.0.0/10")
RESOLVERS = ("1.1.1.1", "8.8.8.8")

@dataclass(frozen=True)
class Settings:
    hostname: str
    token_file: Path
    root: Path

    @classmethod
    def load(cls):
        source = Path(__file__).resolve().parent.parent / "config/operations.json"
        data = json.loads(source.read_text())
        host = data["hostname"]
        if host != "gpurack.duckdns.org":
            raise RuntimeError("Unapproved DuckDNS hostname")
        return cls(host, Path(data["token_file"]), source.parent.parent)

def tailnet_ipv4():
    result = subprocess.run(["tailscale", "ip", "-4"], check=True, capture_output=True,
                            text=True, timeout=TIMEOUT)
    address = result.stdout.strip()
    validate_tailnet_ip(address)
    return address

def validate_tailnet_ip(address):
    try:
        parsed = ipaddress.IPv4Address(address)
    except ipaddress.AddressValueError:
        raise RuntimeError("Expected one Tailscale IPv4 address") from None
    if parsed not in TAILNET:
        raise RuntimeError("Refusing a non-Tailscale IPv4 address")
    return parsed

def resolve(query, resolver):
    host, kind = query
    result = subprocess.run(["dig", "+time=4", "+tries=1", "+noall", "+answer", "+comments", "@"+resolver, host, kind],
                            check=True, capture_output=True, text=True, timeout=TIMEOUT)
    if "status: NOERROR" not in result.stdout and "status: NXDOMAIN" not in result.stdout:
        raise RuntimeError("Independent DNS resolver failed")
    answers = []
    for line in result.stdout.splitlines():
        if line and not line.startswith(";"):
            fields = line.split(None, 4)
            if len(fields) == 5 and fields[3] == kind:
                answers.append(fields[4].strip().strip('"'))
    return tuple(answers)
