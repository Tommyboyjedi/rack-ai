# PR24 — ComfyUI service ownership and bounded image execution

Revision: 2026-09-12. Status: implementation contract; runtime implementation and live qualification are not yet complete.

## 1. Outcome and scope

Deliver two milestones in the same implementation task:

**A. Interactive ComfyUI:** Rack AI safely reserves the RTX 4080 Super, starts an isolated ComfyUI service, lets the operator use the normal ComfyUI browser interface, then drains and releases it without disturbing the existing development services.

**B. Managed image execution:** Rack AI accepts one bounded, typed image-generation request, executes an administrator-approved ComfyUI API workflow, and retains a durable result and verified image artifacts. Supply one real local image workflow profile, not a model/profile collection. The managed path must work without browser automation.

The initial deployment is deliberately partitioned:

| Resource | Initial assignment |
| --- | --- |
| RTX 2060 6 GB | Existing local-coder service; unchanged |
| RTX 4060 Ti 16 GB | Existing local-primary service; unchanged |
| RTX 4080 Super 16 GB | Dedicated ComfyUI/media service managed by this PR |

This replaces the old assumption that every ComfyUI session requires development to stop. A conflicting reservation produces a bounded wait/deferred result, not forced reclamation. Sharing a host still shares RAM, CPU, storage and cooling; separate GPUs do not imply zero performance interference.

**Not included:** automatic vLLM drain/restart, dynamic GPU reassignment, heavyweight multi-GPU inference, general preemption/fairness, a universal scheduler, model discovery/download catalogs, a new graphical interface, media application semantics, automated video/audio workflow support, or a ComfyUI fork. Native interactive workflows remain ComfyUI's responsibility, subject to the installed service's resource envelope. Those broader features remain PR25/PR33 work.

## 2. Implementation baseline

Reviewed Rack AI baseline: `main` at `e197079c26cd0d0cb0fb2a85ba5a2af605c244b8`.

PR32 was merged on 2026-09-09 and incorporates the PR29–32 execution stack. Its head `469dc13c4d669266de21c629cc449f889364b7e2` is an ancestor of the reviewed main. The remaining open PR18/24/25/33 changes are documentation, not a newer runtime. Older README/PR32-document statements that this stack is unmerged are stale.

Implement on this PR's branch, `roadmap/pr24-comfyui-resource-switching`, after incorporating current `origin/main` with an ordinary merge in a clean, isolated worktree. Preserve these documentation commits. Do not force-push, reset another worktree, revive superseded branches, or merge this PR into main. Recheck ancestry and unpushed local changes before editing; an unexamined live checkout is not assumed identical to GitHub.

Read the repository's mandatory agent/coding rules and `docs/pr24-code-review.md`. The review is a source map, not a requirement to repeat a repository-wide investigation.

## 3. Architectural boundaries

Keep the existing workspace transaction and its v1/v2 wire contracts intact. `visual` is currently a generic routing label; the implementation selector still requires JCode and `ExecuteWorkUnit` still executes a Git worktree change. Do not force a ComfyUI render through `ChangeImplementer`, `WorkspaceExecutor`, JCode, a fake repository or a fake Git revision.

Add a separate media/service application boundary. Reuse generic identity, source-priority policy, typed errors, atomic persistence and resource primitives where they genuinely fit. Keep ComfyUI HTTP, systemd, GPU probes and filesystem handling in infrastructure adapters. The Rust domain/application layers must not import ComfyUI or contain Python workflow-engine logic.

Use small typed collaborators for configuration, reservation, lifecycle transitions, service control, ComfyUI protocol, job persistence and artifact validation. Respect `coding_principles.MD`; do not append another large subsystem to `main.rs` or `campaign_runner.rs`. Thin CLI dispatch is sufficient. No new Rack AI HTTP server or dashboard is required.

## 4. Dedicated-resource admission, not a fictional global scheduler

The existing `run-next` queue uses GPU lease files, while the direct workspace/JCode paths do not share that admission boundary. Campaign/repository leases are not GPU leases. This PR must not claim to fix all rack-wide scheduling merely by adding a service record.

For this bounded release, **enforce the dedicated-slot restriction**:

- The configured media resource and physical GPU UUID must not overlap any enabled development worker's binding or any protected live inference service.
- Verify the administrator's deployment mapping and actual processes before activation. An unknown/conflicting binding, foreign GPU process, duplicate physical UUID or ambiguous ownership fails closed.
- Never repurpose the 2060 or 4060 Ti, and never change their running model configuration. Reassignment while a media reservation exists is unsupported and must be rejected by the new configuration/activation path.
- Managed activation requires a qualified dedicated deployment. Arbitrary out-of-band administrator changes and hostile custom nodes are not a security boundary this PR can enforce. State that limitation rather than promising protection against root or manually launched foreign software.

Represent the real 4080 resource without inventing a UUID. Retain existing resource IDs and backward-compatible loading; retire the planned 3090 placeholder as an active placement option. Put host-specific verified bindings in administrator-owned configuration, with a checked-in example. Missing media configuration leaves existing development use working but media unavailable.

Use a full GPU UUID for the ComfyUI launch binding. CUDA device numbering inside that process may be remapped; do not assume the physical 4080 is always GPU index 0. `CUDA_VISIBLE_DEVICES` is placement configuration, not a hostile-code sandbox.

## 5. Resource reservation correctness

Harden the GPU reservation primitive used by cooperating queue/media callers rather than introducing a second competing lock directory:

1. Serialize acquisition with bounded locking; the existing `exists()` followed by `fs::write()` is insufficient.
2. Return an owner/generation-bearing reservation handle. Release and renewal must verify that handle, not just a resource name or PID.
3. Roll back partial acquisition on ordinary errors. If a process dies mid-transaction, remaining records must block admission until reconciled; never leave an apparently free resource whose service may still run.
4. Use atomic durable writes. Malformed, unknown or legacy lease records block new ownership; do not silently delete them or treat missing fields as permission.
5. Use one administrator-configured, canonical machine resource-state location for media and the cooperating queue path. Per-job `--state-root` and another Git worktree must not create an independent ownership universe for the same real GPU. Tests explicitly use temporary resource roots; no automatic live-state migration.
6. Preserve old serialized fields/read compatibility where possible. An ownership-aware internal lease API change is allowed, with all affected callers/tests updated. Owner-blind release must not remain a route capable of deleting a media reservation.
7. Correct directly affected queue error paths: acquisition races must not strand a claimed task; persistence/execution failures must not silently leak or incorrectly release ownership. A runner repeatedly finding only blocked work needs bounded backoff, not a busy loop.

A service reservation outlives the requesting CLI process and browser connection. It remains held while models are resident, while draining, and whenever cleanup is uncertain. Neither an empty queue, idle GPU utilization, `/free` returning HTTP 200, nor a stale heartbeat is proof that the GPU is released.

## 6. Service lifecycle and deployment

Provide a single-host, administrator-configured ComfyUI service adapter. Prefer a dedicated user-systemd unit for the ComfyUI process tree and a small Rust media supervisor; do not build multiple deployment backends in this PR. Give commands and configuration explicit deadlines and bounds. Use service-manager process-tree ownership and invocation identity, not PID-only killing.

The service definition must pin an independently installed ComfyUI environment and required dependencies/custom nodes. Do not install into a vLLM environment, replace NVIDIA drivers, or run whole-stack `compose down/up`. The tracked Compose file is not proof of current live model configuration. No worker auto-restart may bypass reservation/admission; a restarted supervisor must reconcile before authorizing work.

Required state transitions, with durable intent before side effects:

`stopped -> reserving -> starting -> ready -> draining -> stopping -> stopped`

Include explicit `waiting`, `failed` and `recovery_required` outcomes. Keep desired operator state separate from observed process/backend state so a late health check cannot undo release or cancellation.

Startup reserves the GPU before starting the process, verifies its identity and physical placement, checks ComfyUI protocol readiness and the admission gate, and only then exposes a ready session. Failed startup stops only the process tree owned by that activation. Release the reservation only after confirming cleanup; otherwise retain it with recovery evidence.

Release closes admission, accounts for in-flight submissions, drains accepted work within a configured deadline, stops the owned service, and verifies that its process tree and GPU allocations are gone before releasing ownership. A drain timeout leaves the session closed to new work and still reserved; cancellation/abort is a separate explicit operator action. Never kill unrelated processes, reset a GPU or clear another user's jobs to make a test pass.

Recovery distinguishes supervisor restart, backend restart, complete host reboot, PID reuse, stale state and an unknown service already listening on the port. Do not attach to an unrelated ComfyUI instance because its HTTP health check happens to pass. Persist ownership/activation identity and reconcile actual service/process/GPU facts. Every individual probe, lock acquisition, startup, drain and stop attempt is bounded; an intentionally long-lived supervisor is not an excuse for unbounded operations. Active supervision must retain liveness evidence with no intended heartbeat gap over 30 seconds.

Default to loopback access and document browser access through an existing authorized tunnel. Do not expose an unauthenticated public listener. Provide resource limits appropriate to the qualified workflow, including host-memory and disk-headroom checks; do not promise that arbitrary workflows fit in 16 GB.

## 7. Real submission fencing for the native UI

A CLI flag alone cannot drain a server whose native browser can still POST work. Implement a small Rack-AI-owned ComfyUI admission extension/middleware, without modifying the frontend or forking upstream. Its mechanism must be tested against the pinned ComfyUI startup/routing behavior.

The gate starts closed and is authorized only for the current activation/reservation generation. Missing, stale or malformed authorization closes admission. Cover both native `/prompt` and the `/api/prompt` alias. Closing the gate must serialize with submissions already validating/enqueuing; a queue snapshot taken before those submissions finish is not a drain barrier.

Use a protected local control channel. Do not expose service-management authority or reservation credentials to ordinary browser clients or log secrets. When the supervisor loses authority, new submissions stop; already accepted work may finish, with the GPU still reserved until reconciliation.

Interactive sessions and managed sessions are mutually exclusive in this MVP. Interactive mode preserves ordinary ComfyUI behavior while admitted. Managed mode allows only the supervisor's submissions and authorized per-job control, preventing the browser from injecting or clearing unrelated work during the managed execution. Do not implement simultaneous mixed ownership of one queue in this PR.

## 8. Bounded managed image contract

Add a separate versioned media request/result schema and CLI path. The caller supplies opaque work/submission/idempotency identities, a registered image operation/profile, allowed parameters and bounded execution requirements. A workflow/profile identifier describes the requested operation, not a caller-selected GPU, endpoint, executable or arbitrary filesystem path.

Validate and record source-priority admission using the existing policy semantics; do not raise ATHBA's ceiling. This release does not promise global priority scheduling or preemption. Managed work waiting for an interactive session to end stays durably queued with a clear reason, without occupying the 2060/4060 Ti.

Use an administrator-approved **API-format** workflow template, not the UI editor graph JSON. Supply one real local image-generation example and a separate GPU-free test fixture. Reuse an available model where possible. Permit only explicitly mapped typed parameters, such as prompt, seed and bounded dimensions/steps; reject unknown parameters, arbitrary node replacement and caller-provided paths. Freeze the resolved workflow and profile/version/hash before dispatch. Do not quietly change the model, seed or resolution after failure.

Persist a unique canonical ComfyUI prompt UUID and submission intent before POST. Record the acknowledgement and backend activation identity durably. The reviewed upstream accepts caller-supplied UUIDs but **does not deduplicate repeated POSTs with that UUID**. Therefore:

- Identical idempotent replay returns the existing Rack AI job/result without another render. Reusing an identity for a changed payload is a conflict, including concurrent submissions.
- A timeout/lost response after a possible enqueue becomes `submission_uncertain`, not an automatic retry.
- Reconcile that known ID against the same backend's queue/history with bounded reads. If the outcome cannot be established, retain an explicit unknown/interrupted outcome and require a new caller-authorized submission for a new render.
- A backend restart or missing history cannot be converted into success or proof that the POST never executed.

Use bounded HTTP polling of queue/history for the initial implementation. WebSocket previews/progress are optional and must not be required for correctness. ComfyUI history is in memory and may be cleared/evicted; Rack AI's own persisted job and terminal manifest are authoritative across restarts.

HTTP 200, a returned prompt ID, or a WebSocket end event is not rendering success. Validate terminal status, required output-node success and all expected artifacts. A partial-output validation response or error history must not be reported as a complete job.

Persist operator cancellation before effects; late completion cannot erase it. Cancel only the owned job using a verified, atomic per-job API where available, or a safe exclusive-session cleanup path. Never use a check-then-global-interrupt sequence against a shared queue. Pin and verify API support rather than guessing the installed version has it.

## 9. Artifacts and evidence

Write managed outputs into an isolated per-job output namespace. Copy/retain expected files in a Rack AI-owned job-artifact root before declaring success. Validate normalized paths, allowed output roots, symlink/traversal escapes, file type/content, count and byte limits. Do not scan a directory and assume any existing image belongs to the current job. Retain hashes and exact output associations.

Record: Rack AI job identity, effective request/profile, resolved workflow hash, fixed seed/parameters, ComfyUI revision and dependency/model identity available to the profile, selected resource/physical UUID, backend activation identity, prompt ID, timestamps, status/error/cancellation evidence, and artifact manifest. No credentials, arbitrary environment dump or fabricated Git result.

Persist terminal evidence before cleanup removes the only recoverable result. A persistence or artifact failure is not success. An unconfirmed stop may coexist with a terminal job failure but must keep the resource quarantined/reserved. Do not promise pixel-identical reproduction across runtime/hardware changes.

## 10. Operator surface and bounded implementation order

Provide documented equivalents of these new CLI operations, with machine-readable output and meaningful exit codes:

- `media preflight`, `media open`, `media status`, `media release`;
- `media submit <request>`, `media inspect <id>`, `media cancel <id>`;
- `media supervise`, including a bounded `--once` reconciliation for tests/operations.

Preflight is read-only and distinguishes code readiness, configuration readiness, backend readiness and workflow qualification. Opening/submitting must not pretend that queued/waiting work is already usable/complete. Include install/start/access/release/recovery instructions and an opt-in qualification command.

Implement A, test it, then B, test it, in one task. Do not stop after planning, after adding types/configuration, or after a fake-only proof. Do not start a separate scheduler/refactoring project. Additional abstractions, dependencies and files must earn their place through these concrete requirements.

## 11. Required verification

Run new tests through public application/CLI boundaries with fake GPU/process probes, a controllable HTTP backend and isolated resource/job roots. Test the actual Python admission middleware with a lightweight HTTP application as well as Rust-side adapters; a mocked ComfyUI client alone does not prove the drain race is closed.

Required regression groups:

| Group | Required cases |
| --- | --- |
| Ownership | Concurrent acquisition; wrong/stale owner release; partial acquisition; malformed/legacy records; independent job roots sharing one physical-resource authority |
| Placement | Missing/duplicate/wrong UUID; development-resource overlap; foreign process; no commands affecting protected services |
| Lifecycle | Startup failure; CLI/browser exit; drain race with in-flight POST; drain/stop timeout; supervisor/backend restart; PID reuse; persistence failure; verified release and repeat activation |
| Managed jobs | Validation failure; idempotent/concurrent replay; changed-payload conflict; lost POST acknowledgement; absent/evicted history; backend restart; terminal error/partial output; cancellation versus late success |
| Artifacts | Missing/invalid image; traversal/symlink escape; stale file; size/count limits; persistence failure before terminalization |
| Compatibility | Existing development configuration without media; v1/v2 workspace behavior; updated queue ownership/cleanup/backoff; existing status/lease readers |

The current `tests/rack_resource_admission_smoke.sh` directly deletes/writes the repository's `gpu-2060` lease and invokes the coder. **Do not run it unchanged against live state.** Refactor the affected smoke fixture to temporary roots/fake workers or use an isolated equivalent that tests the real production CLI. Audit applicable older smoke scripts before execution. No automated test may delete a live lease or unintentionally invoke a real model.

Required baseline: `cargo fmt --check`, `cargo test --workspace --offline`, targeted Rust/Python/CLI tests, and `git diff --check`. If a justified new dependency must first be fetched, report that separately; do not disguise a missing offline cache as a passing offline run. Preserve safety tests and review the final diff once the implementation is complete.

### Live acceptance, separate from fixture success

When authorized access and prerequisites are available, retain evidence for:

1. Existing 2060/4060 Ti service identity, GPU placement and health before activation.
2. Correctly reserved 4080; real ComfyUI UI reachable through the documented access path.
3. A real local GPU image workflow executed successfully; image verified.
4. A managed request producing a verified artifact/manifest; identical replay causes no second render.
5. Release/drain/stop and confirmed GPU reclamation; second activation works.
6. Existing development service identities/configuration remain unchanged and healthy afterward.

Use fakes for dangerous conflict/crash cases that would otherwise interrupt development. Do not claim an actual browser interaction, live model run, recovery test or unchanged process identity without its evidence.

If rack access, permissions, a model or a usable environment is missing, complete code, fixtures and runbook, and report the exact remaining live gate with commands. Do not fabricate qualification, silently broaden privileges or stop all implementation work at the first environment blocker. Code completion and live qualification are separate report fields.

## 12. Upstream reference snapshot

Protocol reviewed against Comfy-Org/ComfyUI `9113c08c2e14f1ca6c0ccab64920777fd01e1bb9` (`server.py`, `execution.py`, `main.py`). This is a research snapshot, not a claim that it is installed or qualified on gpurack. Pin the actual tested runtime and verify any supported alternative explicitly.

Primary references:

- https://docs.comfy.org/development/comfyui-server/comms_routes
- https://github.com/Comfy-Org/ComfyUI/blob/9113c08c2e14f1ca6c0ccab64920777fd01e1bb9/server.py
- https://github.com/Comfy-Org/ComfyUI/blob/9113c08c2e14f1ca6c0ccab64920777fd01e1bb9/execution.py
- https://github.com/Comfy-Org/ComfyUI/blob/9113c08c2e14f1ca6c0ccab64920777fd01e1bb9/main.py
- https://docs.nvidia.com/cuda/cuda-programming-guide/05-appendices/environment-variables.html

No production code, live service, GPU assignment or model installation is changed by this documentation revision. Do not merge without explicit operator instruction.
