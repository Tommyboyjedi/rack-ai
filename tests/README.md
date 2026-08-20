# Tests

End-to-end and boundary tests for the rack integration live here.

- `rack_coder_smoke.sh`: direct 2060 worker wrapper test
- `rack_task_smoke.sh`: explicit non-swarm single-worker dispatch test
- `rack_pipeline_smoke.sh`: explicit non-swarm multi-step pipeline test
- `rack_coordinator_smoke.sh`: explicit template coordinator spec generation and execution test
- `rack_coordinator_auto_smoke.sh`: auto-template selection and preview/run test
- `rack_queue_smoke.sh`: durable queue submission and one-shot runner test
- `rack_dag_smoke.sh`: durable DAG submission and multi-invocation runner test
- `rack_resource_admission_smoke.sh`: lease-based resource admission behavior test
- `rack_healthcheck_smoke.sh`: registry-backed endpoint health test
- `rack_change_smoke.sh`: external-repository change prepare/evidence test against a disposable Git fixture
- `rack_change_executor_smoke.sh`: live Podman isolation check; exits 2 when rootless Podman is not installed

Python-only ad hoc harnesses were removed once the Rust-owned live path became the authoritative verification surface.
