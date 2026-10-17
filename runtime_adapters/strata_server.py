"""RackAI entry point composing the prepared, pinned Strata installation."""
import argparse
import importlib
import json
import os
from pathlib import Path
import sys

from strata_http import AdapterSurface


def configure(arguments):
    parser = argparse.ArgumentParser(add_help=False)
    parser.add_argument("--runtime-root", required=True, type=Path)
    parser.add_argument("--config", required=True, type=Path)
    parsed, remaining = parser.parse_known_args(arguments)
    root = parsed.runtime_root.resolve(strict=True)
    config_path = parsed.config.resolve(strict=True)
    config = json.loads(config_path.read_text())
    if not os.environ.get("RACK_RUNTIME_ACTIVATION"):
        raise ValueError("managed activation identity is required")
    visible = os.environ.get("CUDA_VISIBLE_DEVICES", "").split(",")
    if len(visible) != 2 or len(set(visible)) != 2 or any(not x.startswith("GPU-") for x in visible):
        raise ValueError("exactly two granted physical GPU UUIDs are required")
    if config.get("gpu") != [] or config.get("env", {}).get("CUDA_VISIBLE_DEVICES") is not None:
        raise ValueError("Strata must inherit the granted GPU UUID ordering")
    if config.get("parallel") != 1 or config.get("fit_max_tokens", False):
        raise ValueError("one parallel slot and rejecting context overflow are required")
    prohibited = ("mcp_servers", "vision", "images", "before_load", "lazy_load", "idle_unload_s")
    if any(config.get(key) for key in prohibited):
        raise ValueError("independent workers and lifecycle controls are not supported")
    if "--layer-split" not in config.get("args", []):
        raise ValueError("the prepared two-device layer split is required")
    sys.path[:0] = [str(root), str(root / "tools")]
    runtime = importlib.import_module("serve.server")
    structured = importlib.import_module("serve.structured")
    if runtime.CTX_SLACK != 8 or structured.jsonschema_modules() is None:
        raise ValueError("qualified context accounting and JSON Schema validation are required")
    runtime.body_limit = lambda: 1024 * 1024
    AdapterSurface(runtime, config_path).install()
    sys.argv = [sys.argv[0], "--config", str(config_path), *remaining]
    return runtime


def main():
    runtime = configure(sys.argv[1:])
    return runtime.main()


if __name__ == "__main__":
    raise SystemExit(main())
