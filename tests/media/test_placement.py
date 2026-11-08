"""Physical GPU protection supports host-managed and legacy Docker workers."""
import json

import pytest
import requests

from fixture import receiver, wait_for, TOKEN


def host_workers(config):
    for worker in config["protected"]:
        worker["placement"] = "host"


@pytest.mark.parametrize("host_managed", [False, True])
def test_native_placement_preserves_worker_isolation(tmp_path, host_managed):
    with receiver(tmp_path / "media", configure=host_workers if host_managed else None) as env:
        docker = env["root"] / "bin/docker"
        docker.write_text("#!/bin/sh\nexit 1\n")
        docker.chmod(0o700)
        headers = {"Authorization": "Bearer " + TOKEN}
        api = env["api"] + "/api/media/v1"
        reply = requests.post(api + "/sessions", headers=headers,
                              json={"schema": "rack-ai/media/v1", "idempotency_key": "placement"},
                              timeout=3)
        reply.raise_for_status()
        session = api + "/sessions/" + reply.json()["id"]

        def settled():
            status = requests.get(api + "/status", headers=headers, timeout=3).json()
            return status if status["state"] in {"ready", "recovery_required"} else None

        status = wait_for(settled)
        if host_managed:
            assert status["state"] == "ready", status
            requests.post(session + "/release", headers=headers, json={}, timeout=3).raise_for_status()
            wait_for(lambda: requests.get(session, headers=headers, timeout=3).json()["state"] == "stopped")
            assert not list((env["root"] / "resources/leases").glob("*.json"))
        else:
            assert status["state"] == "recovery_required", status
            service = json.loads((env["root"] / "state/state.json").read_text())["service"]
            assert "docker" in service["error"]
            assert not json.loads((env["root"] / "machine.json").read_text())["active"]


@pytest.mark.parametrize("fault", ["overlap", "missing", "duplicate"])
def test_host_worker_gpu_mapping_fails_closed(tmp_path, fault):
    def configure(config):
        host_workers(config)
        if fault == "overlap":
            config["protected"][0]["uuid"] = config["media_uuid"]
        elif fault == "missing":
            config["protected"][0]["uuid"] = "GPU-00000000-0000-0000-0000-000000000000"
        else:
            config["protected"][1]["uuid"] = config["protected"][0]["uuid"]

    with receiver(tmp_path / "media", configure=configure) as env:
        headers = {"Authorization": "Bearer " + TOKEN}
        api = env["api"] + "/api/media/v1"
        reply = requests.post(api + "/sessions", headers=headers,
                              json={"schema": "rack-ai/media/v1", "idempotency_key": "invalid-placement"},
                              timeout=3)
        reply.raise_for_status()
        session = api + "/sessions/" + reply.json()["id"]
        wait_for(lambda: requests.get(api + "/status", headers=headers, timeout=3).json()["state"] == "recovery_required")
        assert not json.loads((env["root"] / "machine.json").read_text())["active"]
        assert not list((env["root"] / "resources/leases").glob("*.json"))
