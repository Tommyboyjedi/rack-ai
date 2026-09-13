#!/usr/bin/python3
"""Validate the renewed certificate/config and reload only the owned nginx."""
from pathlib import Path
import subprocess
import sys
import socket
import ssl
import time
from settings import Settings, tailnet_ipv4

UNIT = "rack-ai-private-https.service"

def reload_proxy(settings):
    prefix = str(settings.root) + "/"
    config = str(settings.root / "config/nginx.conf")
    lineage = settings.root / "acme/certbot/live" / settings.hostname
    subprocess.run(["openssl", "verify", "-verify_hostname", settings.hostname, "-purpose", "sslserver", "-CAfile", "/etc/ssl/certs/ca-certificates.crt",
                    "-untrusted", str(lineage / "chain.pem"), str(lineage / "cert.pem")],
                   check=True, timeout=20)
    subprocess.run(["openssl", "x509", "-in", str(lineage / "cert.pem"), "-noout",
                    "-checkhost", settings.hostname, "-checkend", "86400"], check=True, timeout=20)
    state = subprocess.check_output(["systemctl", "show", UNIT, "-p", "ActiveState",
                                    "-p", "MainPID", "-p", "ControlGroup", "-p", "ExecStart"], text=True, timeout=10)
    values = dict(line.split("=", 1) for line in state.splitlines())
    if values["ActiveState"] != "active":
        print("Certificate is ready; private HTTPS proxy is not active", flush=True)
        return
    pid = int(values["MainPID"])
    expected_command = "argv[]=/usr/sbin/nginx -p " + prefix + " -c " + config + " -g daemon off;"
    if pid < 2 or "path=/usr/sbin/nginx ;" not in values["ExecStart"] or expected_command not in values["ExecStart"]:
        raise RuntimeError("Unexpected systemd HTTPS proxy command")
    if (settings.root / "run/nginx.pid").read_text().strip() != str(pid):
        raise RuntimeError("HTTPS proxy PID file differs from systemd")
    if values["ControlGroup"] != "/system.slice/" + UNIT:
        raise RuntimeError("Unexpected HTTPS proxy cgroup")
    subprocess.run(["nginx", "-s", "reload", "-p", prefix, "-c", config], check=True, timeout=20)
    expected = ssl.PEM_cert_to_DER_cert((lineage / "cert.pem").read_text())
    context = ssl.create_default_context()
    deadline = time.monotonic() + 30
    address = tailnet_ipv4()
    while time.monotonic() < deadline:
        try:
            with socket.create_connection((address, 443), timeout=3) as raw:
                with context.wrap_socket(raw, server_hostname=settings.hostname) as tls:
                    if tls.getpeercert(binary_form=True) == expected:
                        print("Renewed certificate served; reloaded only " + UNIT, flush=True)
                        return
        except OSError:
            pass
        time.sleep(1)
    raise RuntimeError("HTTPS proxy did not serve the renewed certificate before deadline")

if __name__ == "__main__":
    try:
        reload_proxy(Settings.load())
    except Exception as error:
        print("Private HTTPS reload failed: " + str(error), file=sys.stderr)
        sys.exit(1)
