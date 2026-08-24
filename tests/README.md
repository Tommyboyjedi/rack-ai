# Tests

End-to-end and boundary tests for the rack integration live here.

- `rack_coder_smoke.sh`: direct 2060 worker wrapper test through the qualified JCode local-coder path
- `rack_task_smoke.sh`: explicit non-swarm single-worker dispatch test
- `rack_pipeline_smoke.sh`: explicit non-swarm multi-step pipeline test
- `rack_coordinator_smoke.sh`: explicit template coordinator spec generation and execution test
- `rack_coordinator_auto_smoke.sh`: auto-template selection and preview/run test
- `rack_queue_smoke.sh`: durable queue submission and one-shot runner test
- `rack_dag_smoke.sh`: durable DAG submission and multi-invocation runner test
- `rack_resource_admission_smoke.sh`: lease-based resource admission behavior test
- `rack_healthcheck_smoke.sh`: registry-backed endpoint health test
- `rack_change_smoke.sh`: external-repository change prepare/evidence test against a disposable Git fixture (`--prepare-only`)
- `rack_change_executor_smoke.sh`: live rootless Podman acceptance-check test; exits 2 when rootless Podman or the executor image is missing
- `rack_change_implement_smoke.sh`: live JCode-through-worktree change with Podman acceptance; requires rootless Podman, the executor image, and local-coder on :8018
- `rack_change_path_policy_smoke.sh`: live Podman bash write outside `allowed_paths`, then the Git/path gate must reject; exits 2 when rootless Podman or the executor image is missing
- `rack_campaign_smoke.sh`: disposable Git fixture campaign with two local commits, no-op/path-policy rejection, JSON status/events, and restart-without-duplicate-commit; uses `cargo run -p rack_ai_cli --features campaign-test-seams` so fixture implementer and live-health bypass stay out of the operator `bin/rack-campaign` path; exits 2 when rootless Podman or the executor image is missing
- `rack_campaign_supervision_smoke.sh`: fixture-backed `campaign supervise` recovery/retention smoke using the versioned operations config and test seams; proves interrupted `running` work is resumed through the supervisor path, stale recorded campaign containers are cleared first, and auxiliary retention pruning is enforced
- `rack_campaign_live_model_smoke.sh`: opt-in (`RACK_AI_LIVE_SMOKE=1`) real two-step campaign proving `local-coder -> 8018/local-coder/minimal`, `local-primary -> 8017/local-primary`, JCode direct worker execution, and independent Rack AI Git/acceptance inspection. It retains its fixture, attempts, review packets, and transcripts and exits 2 when either local endpoint is unavailable.

Python-only ad hoc harnesses were removed once the Rust-owned live path became the authoritative verification surface.
