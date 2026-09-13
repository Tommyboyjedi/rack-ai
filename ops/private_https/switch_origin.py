#!/usr/bin/python3
"""Operator-only migration/rollback. Requires a clean media stop; never edits leases."""
import json
import os
import pwd
from pathlib import Path
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path("/srv/rack-ai-media")
NEW = "https://gpurack.duckdns.org"
OLD = "https://gpurack.tailc214fc.ts.net"

def run(args):
    if args[:2] == ["sudo", "-n"]:
        args = args[2:]
    if args[:2] == ["systemctl", "--user"]:
        user = pwd.getpwnam("tomp")
        args = ["/usr/sbin/runuser", "-u", user.pw_name, "--", "/usr/bin/env",
                "XDG_RUNTIME_DIR=/run/user/" + str(user.pw_uid),
                "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/" + str(user.pw_uid) + "/bus"] + args
    return subprocess.run(args, check=True, text=True, capture_output=True, timeout=45)

def clean(config):
    state = json.loads((ROOT / "state/state.json").read_text())
    if state["service"]["state"] != "stopped":
        raise RuntimeError("Use Rack AI Finish/managed idle release before changing the origin")
    if (Path(config["resource_root"]) / "leases/gpu-4080-super.json").exists():
        raise RuntimeError("Media lease exists; refusing migration")
    unit = run(["systemctl", "--user", "show", config["unit"], "-p", "MainPID", "-p", "ActiveState"]).stdout
    if "MainPID=0\n" not in unit or "ActiveState=inactive\n" not in unit:
        raise RuntimeError("ComfyUI is not cleanly inactive")
    xml = ET.fromstring(run(["nvidia-smi", "-q", "-x"]).stdout)
    gpu = [g for g in xml.findall("gpu") if g.findtext("uuid") == config["media_uuid"]]
    if len(gpu) != 1 or gpu[0].findall("./processes/process_info"):
        raise RuntimeError("Media GPU has processes or unknown identity")

def replace(path, value):
    metadata = path.stat()
    temporary = path.with_suffix(path.suffix + ".new")
    with temporary.open("w") as output:
        os.chmod(temporary, 0o600)
        os.fchown(output.fileno(), metadata.st_uid, metadata.st_gid)
        output.write(value)
        output.flush()
        os.fsync(output.fileno())
    temporary.replace(path)

def serve_launcher_exists():
    state = json.loads(run(["/usr/bin/tailscale", "serve", "status", "--json"]).stdout)
    native = {"Handlers": {"/": {"Proxy": "http://127.0.0.1:8192"}}}
    launcher = {"Handlers": {"/": {"Proxy": "http://127.0.0.1:8191"}}}
    if state.get("TCP", {}).get("8444") != {"HTTPS": True} or state.get("Web", {}).get("gpurack.tailc214fc.ts.net:8444") != native:
        raise RuntimeError("Unexpected native Serve route; refusing migration")
    hosts = [host for host in state.get("Web", {}) if host.endswith(":443")]
    if hosts and hosts != ["gpurack.tailc214fc.ts.net:443"]:
        raise RuntimeError("Unexpected Serve 443 host; refusing migration")
    tcp = state.get("TCP", {}).get("443")
    web = state.get("Web", {}).get("gpurack.tailc214fc.ts.net:443")
    if tcp is None and web is None:
        return False
    if tcp != {"HTTPS": True} or web != launcher:
        raise RuntimeError("Unexpected Serve 443 route; refusing migration")
    return True

def switch(target):
    if os.geteuid() != 0:
        raise RuntimeError("Run this scoped operator switch using sudo")
    if target not in ("duckdns", "tsnet"):
        raise RuntimeError("Expected duckdns or tsnet")
    config = json.loads((ROOT / "config.json").read_text())
    clean(config)
    existing_launcher = serve_launcher_exists()
    if target == "duckdns":
        run(["/usr/bin/setpriv", "--reuid=tomp", "--regid=tomp", "--init-groups",
             "--inh-caps=+net_bind_service", "--ambient-caps=+net_bind_service",
             "/usr/sbin/nginx", "-t", "-p", str(ROOT / "https") + "/",
             "-c", str(ROOT / "https/config/nginx.conf")])
    run(["systemctl", "--user", "stop", "music-director-rack-pr33-web.service",
         "music-director-rack-pr33-worker.service", "rack-ai-media-pr24.service"])
    config["public_origin"] = NEW if target == "duckdns" else OLD
    replace(ROOT / "config.json", json.dumps(config, indent=2) + "\n")
    env = ROOT / "director-test/environment"
    lines = env.read_text().splitlines()
    lines = ["RACK_AI_ALLOWED_ORIGINS=" + config["public_origin"]
             if line.startswith("RACK_AI_ALLOWED_ORIGINS=") else line for line in lines]
    replace(env, "\n".join(lines) + "\n")
    run(["/usr/sbin/runuser", "-u", "tomp", "--",
         "/srv/rack-ai-media/director-test/venv/bin/python",
         "/srv/rack-ai-media/https/ops/director_origin.py"])
    if target == "duckdns":
        if existing_launcher:
            run(["/usr/bin/tailscale", "serve", "--https=443", "off"])
        run(["sudo", "-n", "/usr/bin/systemctl", "enable", "--now", "rack-ai-private-https.service"])
    else:
        run(["sudo", "-n", "/usr/bin/systemctl", "disable", "--now", "rack-ai-private-https.service"])
        run(["sudo", "-n", "/usr/bin/tailscale", "serve", "--bg", "--https=443", "http://127.0.0.1:8191"])
    run(["systemctl", "--user", "start", "rack-ai-media-pr24.service",
         "music-director-rack-pr33-web.service", "music-director-rack-pr33-worker.service"])
    print("Launcher origin switched to " + config["public_origin"], flush=True)

if __name__ == "__main__":
    try:
        switch(sys.argv[1])
    except Exception as error:
        print("Origin switch failed: " + str(error), file=sys.stderr)
        sys.exit(1)
