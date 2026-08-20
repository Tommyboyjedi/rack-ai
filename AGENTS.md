# GPU Rack Agent Notes

This repository runs on `gpurack` and uses two local OpenAI-compatible endpoints:
- `local-primary` on the RTX 4060 Ti (`http://127.0.0.1:8017/v1`)
- `local-coder` on the RTX 2060 (`http://127.0.0.1:8018/v1`)

Temporary workaround: do not use JCode swarm to delegate cross-provider coding work.

JCode v0.78.1 is currently unreliable here for swarm delegation because workers can lose swarm state and can also inherit the coordinator endpoint while switching to the `local-coder` model, which routes `local-coder` requests to `8017` and fails.

When the coordinator needs the 2060 coding worker, call `/srv/rack-ai/bin/rack-coder` through `bash` instead. That wrapper starts a fresh JCode run pinned to `local-coder`, which avoids the broken in-process provider handoff.

Treat this as temporary rack glue. Remove it after JCode swarm delegation is trustworthy for cross-provider workers.
