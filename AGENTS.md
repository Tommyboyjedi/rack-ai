# GPU Rack Agent Notes

This repository runs on `gpurack` and uses two local OpenAI-compatible endpoints:
- `local-primary` on the RTX 4060 Ti (`http://127.0.0.1:8017/v1`)
- `local-coder` on the RTX 2060 (`http://127.0.0.1:8018/v1`)

Temporary workaround: do not use JCode swarm to delegate cross-provider coding work.

JCode v0.78.1 is currently unreliable here for swarm delegation because workers can lose swarm state and can also inherit the coordinator endpoint while switching to the `local-coder` model, which routes `local-coder` requests to `8017` and fails.

Current working rack pattern:
- `bin/rack-primary` uses fresh direct JCode runs for the 4060 Ti coordinator path.
- `bin/rack-coder` uses a repo-local direct OpenAI-compatible tool loop against the 2060 worker endpoint.
- `bin/rack-coder-jcode` preserves the old direct JCode coder wrapper as a fallback/debug path.
- `bin/rack-coordinator` can either use an explicit template or auto-select one from the request.
- `bin/rack-task` runs those specs and writes structured manifests under `logs/runs/`.

Treat this as temporary rack glue. Remove it after JCode swarm delegation is trustworthy for cross-provider workers.
