# Tests

End-to-end and boundary tests for the rack integration live here.

- `rack_coder_smoke.sh`: direct 2060 worker wrapper test
- `rack_task_smoke.sh`: explicit non-swarm single-worker dispatch test
- `rack_pipeline_smoke.sh`: explicit non-swarm multi-step pipeline test
- `rack_coordinator_smoke.sh`: template-driven coordinator spec generation and execution test
- `jcode_local_coder_matrix.py`: retained to measure direct JCode coder behavior separately from the working direct worker path
