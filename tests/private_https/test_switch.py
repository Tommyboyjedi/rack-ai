import importlib
import json
import subprocess
import sys
from pathlib import Path
from unittest.mock import Mock

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "ops/private_https"))
switch = importlib.import_module("switch_origin")

def serve(native=True, launcher=False):
    state = {"TCP": {}, "Web": {}}
    for enabled, port, backend in ((native, "8444", "8192"), (launcher, "443", "8191")):
        if enabled:
            state["TCP"][port] = {"HTTPS": True}
            state["Web"]["gpurack.tailc214fc.ts.net:" + port] = {
                "Handlers": {"/": {"Proxy": "http://127.0.0.1:" + backend}}}
    return state

@pytest.mark.parametrize("launcher", [True, False])
def test_resume_accepts_owned_or_already_disabled_launcher(monkeypatch, launcher):
    result = subprocess.CompletedProcess([], 0, json.dumps(serve(launcher=launcher)))
    monkeypatch.setattr(switch, "run", lambda args: result)
    assert switch.serve_launcher_exists() == launcher

@pytest.mark.parametrize("mutation", ["native_missing", "foreign_launcher", "partial_launcher"])
def test_foreign_or_missing_routes_fail_before_mutation(monkeypatch, mutation):
    state = serve(launcher=True)
    if mutation == "native_missing":
        state = serve(native=False, launcher=True)
    elif mutation == "foreign_launcher":
        state["Web"]["gpurack.tailc214fc.ts.net:443"]["Handlers"]["/"]["Proxy"] = "http://127.0.0.1:9999"
    else:
        del state["Web"]["gpurack.tailc214fc.ts.net:443"]
    run = Mock(return_value=subprocess.CompletedProcess([], 0, json.dumps(state)))
    monkeypatch.setattr(switch, "run", run)
    with pytest.raises(RuntimeError, match="Unexpected"):
        switch.serve_launcher_exists()
    assert run.call_args.args[0] == ["/usr/bin/tailscale", "serve", "status", "--json"]
    assert run.call_count == 1

def test_nginx_preflight_runs_as_service_user_before_stopping_anything(tmp_path, monkeypatch):
    (tmp_path / "config.json").write_text("{}")
    monkeypatch.setattr(switch, "ROOT", tmp_path)
    monkeypatch.setattr(switch.os, "geteuid", lambda: 0)
    monkeypatch.setattr(switch, "clean", lambda config: None)
    monkeypatch.setattr(switch, "serve_launcher_exists", lambda: True)
    commands = []
    def fail_preflight(args):
        commands.append(args)
        raise RuntimeError("preflight refused")
    monkeypatch.setattr(switch, "run", fail_preflight)
    with pytest.raises(RuntimeError, match="preflight refused"):
        switch.switch("duckdns")
    assert len(commands) == 1
    assert commands[0][:6] == ["/usr/bin/setpriv", "--reuid=tomp", "--regid=tomp",
        "--init-groups", "--inh-caps=+net_bind_service", "--ambient-caps=+net_bind_service"]
    assert commands[0][6:8] == ["/usr/sbin/nginx", "-t"]
    assert (tmp_path / "config.json").read_text() == "{}"

def test_other_host_sharing_443_is_not_overwritten(monkeypatch):
    state = serve(launcher=True)
    state["Web"]["other.example:443"] = {"Handlers": {"/": {"Proxy": "http://127.0.0.1:9999"}}}
    monkeypatch.setattr(switch, "run",
        lambda args: subprocess.CompletedProcess([], 0, json.dumps(state)))
    with pytest.raises(RuntimeError, match="Unexpected Serve 443 host"):
        switch.serve_launcher_exists()
