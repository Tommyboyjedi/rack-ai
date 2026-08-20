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
