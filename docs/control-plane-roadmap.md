# Rack AI Control Plane Roadmap

Date: 2026-08-20
Status: Active working direction

## Current understanding

`/srv/rack-ai` should be formalized as the stable rack control plane, not as another coding agent.

That means this repository is the layer above execution engines and inference engines. It should understand:
- what hardware exists in the rack
- what model or service roles are assigned to that hardware
- which execution backends are available
- which workloads should run where
- which resources are currently busy or free
- how work should be queued, retried, timed out, and observed

Under this model:
- JCode is one execution backend
- vLLM is one inference backend
- future ComfyUI, vision, speech, audio, or other services are additional backends
- Rack AI is the system of record for rack capability and execution policy

This is not a replacement for JCode.
This is the control plane that can use JCode where it is useful, bypass it where it is weak, and continue working as the rack gains new GPUs and services.

## Why this direction is correct

The rack is heterogeneous and will remain heterogeneous.

The control plane must know things that a generic coding agent should not need to know, for example:
- GPU VRAM sizes and upgrade history
- which model is the best planner, coder, verifier, or test worker
- which workloads fit on which GPUs
- when a request should wait because a GPU or model is already in use
- how to prefer one backend over another for a given task type

This lets the rack gain capability by registration and policy, not by rewriting orchestration every time hardware changes.

## Present state

The repository already contains the first working slices of this architecture:
- explicit worker entrypoints
- a direct local-coder worker path that does not rely on JCode swarm
- a coordinator path for local-primary
- explicit task specs
- pipeline execution for planner -> implementer -> verifier flows
- task templates
- coordinator-driven spec generation
- structured run manifests

This is already more than a workaround. It is the beginning of a real control plane.

## Engineering stance

For now, we should resist adding more agent intelligence.

The next milestone is execution infrastructure, not smarter prompting.

The control plane needs durable machinery before it needs more cleverness.

## Next milestone

The next concrete engineering slice is:
- task DAG
- persistent queue
- worker registry
- resource/model registry
- retries/timeouts
- run state

This is the backbone required for a durable local AI appliance made out of heterogeneous GPUs and models.

## Proposed implementation plan

### Phase 1: durable execution core

Goal: make Rack AI a reliable job engine.

Deliverables:
- define a stable task and run schema
- define a DAG schema with explicit dependencies
- add a persistent queue store on disk
- add run state transitions such as `queued`, `running`, `succeeded`, `failed`, `timed_out`, and `blocked`
- add retry policy and timeout policy per task type
- add a queue runner that can resume after process restart
- add run and task ids that remain stable across retries

Suggested repository additions:
- `config/schemas/` for task, run, and DAG schema documents
- `state/queue/` for queued and active jobs
- `state/runs/` for authoritative run state
- `bin/rack-runner` for the durable queue executor

### Phase 2: worker and resource registry

Goal: teach Rack AI what the rack contains.

Deliverables:
- a worker registry describing every execution backend
- a model and service registry describing planner/coder/vision/speech roles
- a resource registry describing GPU ownership, VRAM, concurrency limits, and backend affinity
- backend health checks and liveness probes
- a scheduling policy that selects workers based on task type and availability

Suggested repository additions:
- `config/workers.json`
- `config/resources.json`
- `config/models.json`
- `bin/rack-healthcheck`

### Phase 3: scheduling and policy

Goal: make execution placement deterministic and extensible.

Deliverables:
- scheduling policy for task type -> backend -> model -> resource binding
- queue admission rules when resources are busy
- per-backend cooldowns or exclusivity rules
- reservation or lease model for GPU ownership during runs
- structured failure reasons that distinguish policy, backend, and model errors

### Phase 4: operator interface

Goal: make the control plane observable and usable.

Deliverables:
- status command for queue, workers, resources, and runs
- run history and run summary output
- pending/failed job inspection
- manual retry, cancel, and requeue commands
- eventual local UI or API if useful later

Suggested repository additions:
- `bin/rack-status`
- `bin/rack-retry`
- `bin/rack-cancel`
- `bin/rack-queue`

## JCode position

We should continue tracking JCode upstream instead of fork-diverging unless there is no reasonable alternative.

Current position:
- JCode remains useful as a direct execution backend
- native swarm is not currently reliable enough for this rack configuration
- upstream is moving toward a DAG-first architecture, which aligns with our direction
- if upstream provider/model routing becomes reliable later, Rack AI should be able to delete workaround code and delegate more execution back into JCode

That is the desired outcome: delete local workaround code when upstream becomes good enough.

## Immediate checkpoint

The next checkpoint should be:

Make `/srv/rack-ai` a durable job execution engine.

Specifically, the next implementation target is:
1. persistent task/run state on disk
2. DAG-aware queue submission
3. worker registry and resource registry
4. queue runner with retries/timeouts
5. status and inspection commands

Once that works, the rack has the backbone needed for the larger goal:

One intelligent local appliance composed of heterogeneous GPUs, models, and services.


## Progress log

### 2026-08-20: durable execution core slice 1

Completed in this slice:
- added schema documents for task spec, run state, and task DAG under `config/schemas/`
- added durable state directories under `state/queue/` and `state/runs/`
- added `bin/rack-submit` for persistent queue submission
- added `bin/rack-runner` for one-shot durable queue execution with retries/timeouts state handling
- added `bin/rack-status` for queue and run inspection
- added `tests/rack_queue_smoke.sh` to prove submit -> run -> succeed -> inspect flow

What is now true:
- tasks can be submitted into an on-disk queue
- authoritative run state is written to disk independently of conversational context
- the runner can transition tasks through `queued`, `running`, `succeeded`, `failed`, and `timed_out`
- queue inspection is available through a stable command surface
- `rack-task` remains the execution unit below the queue layer

What remains for the next slice of Phase 1:
- extend the queue model from simple serialized jobs to explicit DAG-aware dependency scheduling
- add stronger retry policy controls and blocked-state handling
- improve run history summarization and operator inspection
- connect worker and resource registry data into scheduling decisions

### 2026-08-20: worker, resource, and model registry slice

Completed in this slice:
- added `config/workers.json` as the authoritative worker registry for planner, coder, and fallback backends
- added `config/resources.json` to describe current and planned rack GPU capacity and concurrency limits
- added `config/models.json` to describe endpoint bindings and model roles independently of execution wrappers
- added `bin/rack-healthcheck` to validate active model endpoints against the declared registry
- added `tests/rack_healthcheck_smoke.sh` to prove registry-backed endpoint checks pass on the live rack

What is now true:
- rack capability is now described in versioned configuration rather than only in wrapper scripts
- worker identity, model identity, and hardware resource identity are separated cleanly
- active inference endpoints can be checked through a stable control-plane command
- future hardware such as a 3090 can be represented in registry state before it is physically installed
- later scheduling logic can bind work through registry data instead of hard-coded assumptions

What remains for the next slice of Phase 1:
- use the worker and resource registry for actual queue placement and admission control
- add resource occupancy tracking during runs rather than assuming single-process exclusivity
- add retry policy, timeout policy, and blocked-state controls at the queue layer
- extend durable queue execution from linear jobs to dependency-aware DAG scheduling

### 2026-08-20: registry-backed placement and admission slice

Completed in this slice:
- added `bin/racklib.py` as a shared control-plane registry helper for workers, resources, models, and leases
- updated `bin/rack-task` so worker resolution comes from the worker registry instead of a hard-coded map
- updated `bin/rack-submit` to persist derived placement metadata with each queued task
- updated `bin/rack-runner` to defer jobs when required resources are already leased and to acquire/release resource leases around execution
- updated `bin/rack-status` to expose leases, placement, and admission state
- added `state/resources/leases/` as durable occupancy state and `tests/rack_resource_admission_smoke.sh` to prove busy-resource deferral works

What is now true:
- the queue layer now knows which workers, models, backends, and GPU resources a task depends on
- execution admission is no longer implicit in wrapper scripts alone
- resource occupancy is represented on disk and can block incompatible work without losing queue state
- worker routing inside `rack-task` now follows versioned registry data rather than a duplicated code map
- the control plane can safely represent planned concurrency rules before full DAG scheduling exists

Verification completed in this slice:
- `./tests/rack_queue_smoke.sh`
- `./tests/rack_resource_admission_smoke.sh`
- `./tests/rack_task_smoke.sh`
- `./tests/rack_coordinator_smoke.sh`
- `./tests/rack_coordinator_auto_smoke.sh`
- `./tests/rack_healthcheck_smoke.sh`

What remains for the next slice of Phase 1:
- extend queue submission and execution from linear jobs to explicit dependency-aware DAG runs
- add richer retry policy and blocked-state handling beyond simple requeue behavior
- add stronger operator commands for retry, cancel, and requeue
- evolve lease handling from simple file presence to deliberate scheduling policy and admission ordering

### 2026-08-20: dependency-aware DAG execution slice

Completed in this slice:
- extended the task schema to support explicit DAG nodes with worker, cwd, prompt, dependencies, and artifact checks
- extended run state to persist `dag_state` and `active_node_id`
- updated `bin/rack-submit` to initialize durable node state for DAG tasks
- updated `bin/rack-runner` to advance one ready DAG node per invocation while preserving queue state between nodes
- updated `bin/rack-status` to surface DAG progress in addition to queue, lease, and placement state
- added `tests/rack_dag_smoke.sh` to prove a planner -> coder -> verifier DAG executes durably across multiple runner invocations

What is now true:
- the control plane can represent work as dependencies instead of only linear step lists
- DAG progress is written to disk node by node rather than existing only in conversational context
- a partially completed task can be resumed by the durable runner without losing completed node history
- placement and admission are now applied at the runnable node level rather than only at the whole-task level
- this is enough to support a first real execution graph above JCode and vLLM without introducing premature scheduling complexity

Verification completed in this slice:
- `./tests/rack_dag_smoke.sh`
- `./tests/rack_queue_smoke.sh`
- `./tests/rack_resource_admission_smoke.sh`
- `./tests/rack_task_smoke.sh`
- `./tests/rack_coordinator_smoke.sh`
- `./tests/rack_coordinator_auto_smoke.sh`
- `./tests/rack_healthcheck_smoke.sh`

What remains for the next slice of Phase 1:
- add richer retry semantics at the node level instead of task-level attempt counting only
- add operator commands for retry, cancel, and requeue against durable run state
- add explicit blocked-state handling for dependency deadlocks or exhausted prerequisites
- improve scheduler policy so admission ordering is intentional rather than simple queue scan order
