#!/usr/bin/env python3
import json
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
CONFIG_DIR = REPO_ROOT / "config"
STATE_DIR = REPO_ROOT / "state"
LEASES_DIR = STATE_DIR / "resources" / "leases"


class RegistryError(RuntimeError):
    pass


def load_json(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        data = json.load(handle)
    if not isinstance(data, dict):
        raise RegistryError(f"expected JSON object in {path}")
    return data


def write_json(path: Path, payload: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def _index_registry(filename: str, key: str) -> dict[str, dict]:
    payload = load_json(CONFIG_DIR / filename)
    entries = payload.get(key, [])
    if not isinstance(entries, list):
        raise RegistryError(f"{filename} field {key!r} must be a list")
    index = {}
    for entry in entries:
        if not isinstance(entry, dict):
            raise RegistryError(f"invalid entry in {filename}")
        entry_id = entry.get("id")
        if not entry_id:
            raise RegistryError(f"entry in {filename} is missing id")
        index[entry_id] = entry
    return index


def load_workers_index() -> dict[str, dict]:
    return _index_registry("workers.json", "workers")


def load_resources_index() -> dict[str, dict]:
    return _index_registry("resources.json", "resources")


def load_models_index() -> dict[str, dict]:
    return _index_registry("models.json", "models")


def unique_ordered(values: list[str]) -> list[str]:
    seen = set()
    ordered = []
    for value in values:
        if value in seen:
            continue
        seen.add(value)
        ordered.append(value)
    return ordered


def extract_step_workers(spec: dict) -> list[str]:
    workers = []
    if "steps" in spec:
        steps = spec.get("steps", [])
        if not isinstance(steps, list):
            raise RegistryError("steps must be a list")
        for step in steps:
            if not isinstance(step, dict):
                raise RegistryError("step entries must be objects")
            worker = step.get("worker")
            if worker:
                workers.append(worker)
    elif spec.get("worker"):
        workers.append(spec["worker"])

    if not workers:
        raise RegistryError("task spec does not declare any workers")
    return unique_ordered(workers)


def derive_task_placement(spec: dict) -> dict:
    workers_index = load_workers_index()
    worker_ids = extract_step_workers(spec)
    resource_ids = []
    model_ids = []
    backends = []
    for worker_id in worker_ids:
        worker = workers_index.get(worker_id)
        if worker is None:
            raise RegistryError(f"unknown worker: {worker_id}")
        resource_id = worker.get("resource_id")
        model_id = worker.get("model_id")
        backend = worker.get("backend")
        if resource_id:
            resource_ids.append(resource_id)
        if model_id:
            model_ids.append(model_id)
        if backend:
            backends.append(backend)
    return {
        "worker_ids": worker_ids,
        "resource_ids": unique_ordered(resource_ids),
        "model_ids": unique_ordered(model_ids),
        "backends": unique_ordered(backends),
    }


def ensure_task_placement(spec: dict) -> dict:
    placement = spec.get("placement")
    if isinstance(placement, dict):
        return placement
    placement = derive_task_placement(spec)
    spec["placement"] = placement
    return placement


def lease_path(resource_id: str) -> Path:
    return LEASES_DIR / f"{resource_id}.json"


def read_lease(resource_id: str) -> dict | None:
    path = lease_path(resource_id)
    if not path.exists():
        return None
    payload = load_json(path)
    payload.setdefault("resource_id", resource_id)
    payload.setdefault("lease_path", str(path))
    return payload


def busy_resources(resource_ids: list[str]) -> list[dict]:
    busy = []
    for resource_id in resource_ids:
        payload = read_lease(resource_id)
        if payload is not None:
            busy.append(payload)
    return busy


def acquire_leases(task_id: str, placement: dict, acquired_at: str) -> dict[str, str]:
    lease_paths = {}
    LEASES_DIR.mkdir(parents=True, exist_ok=True)
    for resource_id in placement.get("resource_ids", []):
        path = lease_path(resource_id)
        if path.exists():
            raise RegistryError(f"resource busy: {resource_id}")
        payload = {
            "task_id": task_id,
            "resource_id": resource_id,
            "worker_ids": placement.get("worker_ids", []),
            "model_ids": placement.get("model_ids", []),
            "acquired_at": acquired_at,
        }
        path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
        lease_paths[resource_id] = str(path)
    return lease_paths


def release_leases(resource_ids: list[str]) -> None:
    for resource_id in resource_ids:
        path = lease_path(resource_id)
        if path.exists():
            path.unlink()


def list_leases() -> list[dict]:
    if not LEASES_DIR.exists():
        return []
    leases = []
    for path in sorted(LEASES_DIR.glob("*.json")):
        payload = load_json(path)
        payload.setdefault("resource_id", path.stem)
        payload.setdefault("lease_path", str(path))
        leases.append(payload)
    return leases
