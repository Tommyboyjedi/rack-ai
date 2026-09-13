# PR24 source review — 2026-09-12

Current permanent deployment and ComfyUI revision are recorded in [PR24 qualification](pr24-qualification.md). Historical source snapshot references below are review provenance, not installation instructions.

## Scope and evidence limits

This is a static integration review of Rack AI's current execution, routing, GPU resource, state, CLI and deployment surfaces, with relevant test inspection and an upstream ComfyUI protocol review. It is not a blanket security certification of every legacy campaign/planning module.

Repository content and PR/branch metadata were read through GitHub. No local gpurack checkout, unpushed changes, running processes, installed ComfyUI environment or physical GPU mappings were accessible to this review. No Rust/Python tests or live GPU jobs were executed here. Test source and historical qualification reports are not fresh passing results.

The implementation contract is `docs/pr24-comfyui-resource-switching.md`. The handoff is `docs/pr24-codex-prompt.md`.

## Authoritative baseline

- Reviewed main: `e197079c26cd0d0cb0fb2a85ba5a2af605c244b8`.
- PR32 head: `469dc13c4d669266de21c629cc449f889364b7e2`; merged 2026-09-09T20:58:55Z.
- GitHub compare `main...pr32-generic-capability-routing` returned `behind`, zero ahead and four behind. That execution stack is already included in main.
- PR29/30/31 are incorporated historical records, not an instruction to rebuild the old stack.
- Open PR18 changes only three roadmap/evaluation documents; PR24 changes its design document; PR25 changes its roadmap; PR33 changes documentation for heavyweight reasoning. None supplies a newer ComfyUI runtime.
- Older README and PR32 document text still describing an open/unmerged stack is stale. Prefer Git ancestry and source to that prose.

Implementation should incorporate current main into the existing PR24 branch without rewriting published history. Do not merge older experimental branches just because they remain on GitHub.

## Findings that determine this PR

### 1. Visual metadata is not a media executor

`crates/rack_ai_application/src/generic_routing.rs` defines reasoning/coding/visual/audio and source-priority/selection evidence. However, `crates/rack_ai_infrastructure/src/registry_work_unit_worker_selector.rs::worker_profile` explicitly rejects non-JCode worker kinds. Selection resolves a `JCodeWorkerConfigResolver` runtime.

`ExecuteWorkUnit` calls `ExecuteChange` in `ImplementAndVerify` mode and returns worktree, packet and revision evidence. `work_unit_command.rs` constructs that same synchronous path. Adding a ComfyUI entry to `workers.json` will not turn this path into a renderer.

**Decision:** a separate bounded media request/execution boundary; reuse generic policy concepts, not Git-change mechanics.

### 2. The direct workspace path is not the leased queue path

`run_next_task.rs` uses `LeaseRepository`. In contrast, `execute_work_unit.rs`, `execute_change.rs`, `work_unit_command.rs` and the inspected `jcode_change_implementer.rs` path have no GPU lease acquisition. The generic selector's availability test reads configured model/resource status, not live lease state or GPU residency.

`campaign_lease.rs` coordinates campaigns and repositories under `state/campaigns`; it is not a physical-GPU ownership service.

**Decision:** PR24 uses an enforced dedicated media resource and leaves development placement unchanged. Shared queue/media reservations must be correct, but dynamic development/media reassignment requires later admission work across every relevant execution path. Do not claim this PR implements a universal resource scheduler.

### 3. Existing GPU lease acquisition and release are unsafe for concurrent services

`file_system_lease_repository.rs` checks existence and subsequently writes the lease. Acquisition is not an atomic compare-and-create transaction. Release removes files by placement without verifying the releasing owner. Failure partway through a resource set does not roll back preceding acquisitions. The writer does not use the existing atomic durable-write helper.

Its tests exercise sequential acquire/block/release and an already-busy file, not simultaneous contenders or stale-owner release.

**Decision:** bounded locking, owner/generation handles, guarded release, partial-failure handling, durable records and concurrency tests are prerequisite work, not optional scheduler sophistication.

### 4. Queue error paths and blocked-work spinning become material

`RunNextTask::select_task` claims the queue entry before `execute` acquires leases. Later preparation/state-write errors can return before ordinary release. In CLI `main.rs`, `run_runner` loops again on `NoAdmissibleTasks` without a delay.

A ComfyUI session can hold a resource for much longer than an ordinary queue task, making these paths operationally significant.

**Decision:** preserve claimed work on acquisition failure, explicitly handle cleanup uncertainty, and add bounded contention backoff. Limit refactoring to affected responsibilities.

### 5. Per-job roots currently separate physical lease state

`RepositoryPaths::leases_dir` is derived from the instance root. CLI initialization derives this from `--state-root`. Two independent state roots can consequently hold independent files bearing the same resource ID.

**Decision:** media activation and cooperating queue users need one administrator-owned machine resource-state authority, separate from job evidence roots. Tests must explicitly inject disposable roots; do not migrate or delete live records implicitly.

### 6. Deployment configuration is not current hardware truth

`config/resources.json` has a 2060, 4060 Ti and planned 3090 slot, but no installed 4080. `ResourceRecord` has a descriptive `device_hint`, not a GPU UUID binding. `HealthcheckService` probes model endpoints and configured status; it does not establish ComfyUI process ownership or physical GPU identity.

The tracked `compose.yaml` already UUID-pins the two older GPUs. However, its coder command names Qwen2.5-Coder-3B while the newer model registry names eqaq-v2. This discrepancy is evidence to inspect live deployment, not permission to redeploy whichever file looks convenient.

**Decision:** verified host-local mappings, a separate isolated ComfyUI service, and no whole-stack Compose operation or existing model-service changes.

### 7. Existing subprocess helpers are not sufficient proof of service reclamation

`HostWorkspaceExecutor` is a bounded command adapter, not a long-lived service lifecycle manager. `WallClockWait` times out by killing the immediate PID and briefly waiting; that alone does not prove a persistent service's full descendant tree and GPU allocations are gone.

`durable_file.rs` supplies useful atomic write/fsync/rename behavior and a file-lock wrapper. Its existing blocking lock call is not itself a bounded acquisition policy.

**Decision:** use an owned service process tree with verified activation identity and bounded control. Reuse suitable persistence primitives, not unsupported lifecycle guarantees. Broader host-executor hardening is not silently folded into this PR.

### 8. An existing resource smoke test is destructive against live state

`tests/rack_resource_admission_smoke.sh` deletes and replaces `state/resources/leases/gpu-2060.json` in its repository root, then invokes the coder. It is not a safe GPU-free contention test for the running rack.

**Decision:** isolate the affected fixture, fake the worker, and test the production CLI against temporary state. Audit older smoke scripts before running them. A green test must not be obtained by deleting a real reservation.

## Upstream ComfyUI findings

Reviewed source snapshot: `Comfy-Org/ComfyUI@9113c08c2e14f1ca6c0ccab64920777fd01e1bb9`.

- `execution.py::PromptQueue` stores its queue, running set and history in process memory. History is also bounded/clearable. It is useful reconciliation evidence, not durable Rack AI state.
- `server.py` accepts a canonical caller-supplied prompt UUID but inserts every valid POST into the queue. Reposting an ID does not provide exactly-once execution.
- A successful `/prompt` response may include node errors for invalid output branches. `main.py` also emits an end-of-execution notification after an error. Neither acknowledgement nor an end event alone proves success.
- `/free` sets worker flags; the HTTP response is not a physical memory-release barrier.
- Native `/prompt` is also reachable through `/api/prompt`. Safe draining must fence both and account for validation/enqueue requests already in flight.
- The reviewed newer `/api/jobs/{id}/cancel` uses an atomic ID-specific queue operation. The older `/interrupt` implementation performs a separate check followed by an interrupt; do not infer equivalent race safety from its optional ID argument.
- `main.py` constructs the server before loading custom nodes. A small owned admission middleware is a plausible extension point, but its actual startup/gating behavior must be tested with the pinned deployment.

These findings require durable submission intent, no blind POST retry, explicit unknown outcomes, session ownership, a real admission barrier and verified artifacts. The installed runtime remains unverified.

## Source map for implementation

All Rack AI paths below are relative to the reviewed main commit:

| Responsibility | Starting files |
| --- | --- |
| Rules/boundary | `AGENTS.md`, `coding_principles.MD`, `agent.MD`, `docs/engineering-contract.md`, `docs/generic-bounded-workspace-execution.md` |
| Generic routing | `crates/rack_ai_application/src/generic_routing.rs`, `crates/rack_ai_infrastructure/src/registry_work_unit_worker_selector.rs` |
| Workspace execution | `crates/rack_ai_application/src/execute_work_unit.rs`, `execute_change.rs`; `crates/rack_ai_cli/src/work_unit_command.rs` |
| GPU leases/queue | `crates/rack_ai_infrastructure/src/file_system_lease_repository.rs`, `repository_paths.rs`; `crates/rack_ai_application/src/run_next_task.rs` |
| Persistence, distinct campaign leases | `crates/rack_ai_application/src/durable_file.rs`, `campaign_lease.rs` |
| Runtime adapters | `crates/rack_ai_infrastructure/src/jcode_change_implementer.rs`, `host_workspace_executor.rs`, `wall_clock_wait.rs` |
| Inventory/health/configuration | `crates/rack_ai_infrastructure/src/resource_record.rs`, `file_system_registry_repository.rs`, `healthcheck_service.rs`; `config/{workers,models,resources}.json`, `compose.yaml` |
| CLI/tests | `crates/rack_ai_cli/src/main.rs`, `tests/rack_resource_admission_smoke.sh`, adjacent inline Rust tests |

Primary source entry points:

- https://github.com/Tommyboyjedi/rack-ai/tree/e197079c26cd0d0cb0fb2a85ba5a2af605c244b8
- https://github.com/Tommyboyjedi/rack-ai/pull/32
- https://github.com/Comfy-Org/ComfyUI/tree/9113c08c2e14f1ca6c0ccab64920777fd01e1bb9
- https://docs.comfy.org/development/comfyui-server/comms_routes

## Acceptance interpretation

The shortest responsible implementation is dedicated ComfyUI ownership plus one bounded image workflow, not a fake integration demonstrated only by submitting HTTP. Compilation, unit/fixture tests, actual GPU rendering and live non-interference are separate evidence categories. The implementation report must distinguish them.
