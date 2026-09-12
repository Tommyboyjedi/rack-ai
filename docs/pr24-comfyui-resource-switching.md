# PR24 — ComfyUI access, remote media API and Music Director integration

Revision: 2026-09-12, **v2 — complete user journey**. Status: implementation contract; code and live qualification are not yet complete.

This revision replaces the CLI-only scope. A thin authenticated HTTP interface, a small browser launcher and the companion Music Director connector are REQUIRED, not deferred. All resource, execution and evidence safeguards from v1 remain required. The source review in `docs/pr24-code-review.md` remains historical evidence; this contract defines the revised deliverable.

## 1. Definition of done from the user's perspective

**Interactive:** Tom opens a stable private launcher bookmark, sees Stopped/Starting/Ready/Busy/Recovery required, clicks Start ComfyUI, and opens the normal ComfyUI interface when ready. Finish session closes admission, drains accepted work and releases the service safely. He does not SSH to start it or write JSON each time. Closing a browser does not cancel a render or falsely release its GPU.

**Application:** In Music Director, Tom selects the Rack AI image backend and a supported image workflow, then clicks Generate. The application shows Waiting for rack / Starting / Generating / Complete or an actionable failure. Rack AI starts ComfyUI automatically if needed; the resulting image is downloaded into the correct project/segment. No separate Start ComfyUI click, copying image files or pasting job IDs is required.

Implement these as ONE coordinated Codex task, with separate repository branches and PRs:

- `Tommyboyjedi/rack-ai`, PR24, `roadmap/pr24-comfyui-resource-switching`: resource correctness, lifecycle, native admission gate, managed image execution, always-available request receiver, launcher, API, artifacts and deployment.
- `Tommyboyjedi/musicvideo-director`, `integration/rack-ai-media`: optional Rack AI image backend, settings, asynchronous job tracking, artifact import and UI tests. Its companion contract is `docs/rack-ai-media-integration.md`.

The user has authorized this specific second-repository integration. Work under each repository's own instructions in separate worktrees; do not put Django/project semantics into Rack AI or edit ATHBA. This task-specific scope does not relax other safety/coding rules. Do not copy private application data/code into the public Rack AI repository; use synthetic contract fixtures.

## 2. Bounded scope and baseline

Reviewed Rack AI main: `e197079c26cd0d0cb0fb2a85ba5a2af605c244b8`. PR32, merged 2026-09-09, already incorporates the PR29–32 stack. Remaining open roadmap PRs did not contain a newer runtime at review. Recheck current refs and local changes; documentation claiming that PR32 remains unmerged is stale.

In clean isolated worktrees incorporate current main into each integration branch with ordinary merges, retaining contract commits. Do not reset/checkout over the live `/srv/rack-ai`, force-push, revive superseded stacks or merge either PR into main. Inspect relevant newer changes rather than repeating the complete previous review. Read each repository's applicable agent/coding rules.

Initial placement: 2060 6 GB remains local-coder; 4060 Ti 16 GB remains local-primary; 4080 Super 16 GB is the dedicated media resource. Existing vLLM and ATHBA configuration/processes remain untouched. A busy or ambiguous media GPU waits or fails closed; no automatic reclamation of development GPUs.

The managed proof supports ONE administrator-approved local image workflow and its bounded typed parameters. Include a real usable example plus a GPU-free fixture. No universal scheduler, dynamic reassignment, heavyweight inference, model catalog, video/audio automation, autonomous music-video orchestration or replacement ComfyUI frontend. Native ComfyUI may run installed compatible workflows; that is not qualification of every video/model combination. Preserve Music Director's existing direct-ComfyUI backend; unsupported Rack AI workflows must be explicit, not silently rerouted.

## 3. Architecture and authority

Keep workspace v1/v2 contracts intact. `visual` metadata does not turn the JCode/Git-worktree executor into a renderer. Add a separate bounded media/service boundary; do not fake a repository, Git revision, JCode worker or workspace transaction for an image.

Reuse generic identity, priority admission, atomic persistence and suitable resource primitives. HTTP, systemd, GPU probes, ComfyUI protocol and files belong in adapters. Use small typed Rust collaborators and narrowly scoped Python admission glue; obey the existing size/parameter/safety rules. Do not grow another monolithic `main.rs` or `campaign_runner.rs` subsystem.

The HTTP adapter, launcher and CLI use the SAME application use cases, state and resource authority. Do not expose arbitrary shell execution or shell out to a CLI whose caller can choose a state root. A small always-on receiver/supervisor remains available while ComfyUI is stopped and consumes no resident image model merely to receive requests. Its startup must reconcile durable state before enabling generation. HTTP request handlers enqueue/control work and return promptly; they do not hold an HTTP request open throughout loading/rendering.

## 4. Actual resource ownership

The direct workspace/JCode route is not the existing GPU-leased queue route, and campaign/repository leases are not GPU leases. This release enforces a dedicated media slot, not fictional global arbitration.

Require a verified administrator mapping to a full physical GPU UUID; never guess GPU index 0. Reject overlap with enabled development worker bindings/protected live inference services, duplicate UUIDs, foreign GPU processes, unknown placement or ambiguous ownership. Retain existing resource IDs/backward-compatible loading; register the real 4080 and retire the planned 3090 placeholder as an active placement option. Host-specific mappings belong in local administrator configuration with checked-in examples. Missing media configuration must not break existing development.

CUDA visibility is placement, not a hostile-code sandbox. Do not promise protection against root/out-of-band administrator reconfiguration or malicious custom nodes. Shared GPUs are separate but CPU/RAM/disk/cooling remain shared: preflight workflow-specific host-memory and disk headroom and use explicit limits.

Harden the common queue/media GPU reservation boundary:

- Bounded serialized acquisition, not `exists()` then write. One canonical administrator-configured machine resource root, independent of job state roots/worktrees; tests inject disposable roots. No implicit live-state migration.
- Owner/generation-bearing handles for acquire/renew/release, not PID/resource-only deletion. Update affected callers; owner-blind release must not delete a media reservation. Legacy/malformed/unknown records block, never grant permission.
- Atomic durable records; roll back partial acquisition on ordinary errors. Crash remnants block until reconciled. A claimed queue item must not be stranded by an acquisition race or preparation/persistence failure.
- Correct affected cleanup/error paths and add bounded queue backoff when all jobs are blocked. Scope refactoring to those responsibilities.

Reservations survive the requesting CLI/browser and remain held while a service/models may be resident, while draining and while cleanup is uncertain. Idle utilization, empty queue, stale heartbeat or `/free` HTTP 200 is not proof of release.

## 5. Owned ComfyUI lifecycle and native admission

Use one isolated, pinned ComfyUI Python environment and an owned user-systemd process tree with verified invocation identity. The Rust receiver/supervisor coordinates it. Do not replace NVIDIA drivers, modify vLLM dependencies or execute whole-stack Compose operations. The tracked Compose model definitions are not proof of the current deployment. No automatic backend restart may bypass reservation/gating.

Persist desired intent before effects. Model `stopped -> reserving -> starting -> ready -> draining -> stopping -> stopped`, plus explicit waiting/failed/recovery_required outcomes. Distinguish desired state from observed facts so late health checks cannot undo release/cancel.

Reserve before start; verify process/UUID/protocol/gate readiness before Ready. On startup failure stop only the owned activation. Release ownership only after its process tree and GPU allocations are confirmed gone; otherwise quarantine the reservation. Close admission and account for in-flight enqueue requests before draining. Drain/stop deadlines produce visible recovery state, not silent interruption of other work. Explicit abort/cancel is separate from graceful Finish session.

Recovery covers receiver/backend restart, host reboot, PID reuse, corrupted state and an unrelated server listening on the port. Never attach by health check alone. All locks, probes, HTTP calls, startup/drain/stop operations are bounded; active supervision retains heartbeats at no intended interval over 30 seconds. Record restoration/recovery evidence. Use configurable idle retention for managed sessions followed by verified stop; do not restart between every queued image. Interactive ownership is explicitly released; document any finite session expiry and drain rather than killing active work.

Native UI needs a REAL admission gate, not just a CLI state flag. Supply tested Rack-AI-owned middleware/extension without upstream fork or frontend modification. Cover `/prompt`, `/api/prompt` and actual pinned routing aliases; serialize gate closure with requests already validating/enqueuing. Start closed; missing/stale/malformed authority closes new admission. Private control credentials never reach browser clients/logs. Check the pinned startup registration path in tests.

Interactive and managed sessions are mutually exclusive initially. In managed mode only authorized supervisor submissions and job controls may mutate the queue; browsers cannot inject/clear jobs. Interactive requests wait behind active managed work, and managed requests wait while an interactive reservation remains open. Status identifies the reason without leaking another principal's data. Neither mode steals a reservation or starts a second conflicting backend.

## 6. Remote API contract — required

Implement and check in a versioned schema/OpenAPI description with shared synthetic request/response fixtures consumed by both repositories. Prefer these routes under `/api/media/v1`; publish exact final forms if an existing framework necessitates a minor change:

| Method and route | Purpose |
| --- | --- |
| GET `/status` | Authorized service state, readiness and generic waiting reason |
| GET `/profiles` | Only available/qualified image operations, versions and permitted parameter bounds |
| POST `/sessions` | Idempotently request an interactive session; return session ID, state and authorized access address when ready |
| GET `/sessions/{id}` | Session state/access details for its owner/operator |
| POST `/sessions/{id}/release` | Durable graceful release intent; return draining/stopped state, not false immediate success |
| POST `/jobs` | Durable managed image admission; implicitly request/start the backend |
| GET `/jobs/{id}` | Durable lifecycle/result/error and artifact manifest |
| POST `/jobs/{id}/cancel` | Durable authorized per-job cancel intent |
| GET `/jobs/{id}/artifacts/{artifact_id}` | Authorized bounded binary download; never an arbitrary filesystem path |

For new accepted work return HTTP 202 with an opaque ID and status location; the request is acknowledged only after it is durably recorded. Identical replay returns that same resource. Changed-payload identity reuse returns 409. Define typed validation/unsupported/unauthorized/unavailable errors, limits and retry semantics. Reads do not start services or render. `Test connection` only tests authentication/version/profile availability.

A job carries versioned opaque work/submission/idempotency identities, an image operation/profile version, permitted prompt/seed/dimension/step parameters, and bounded timeout/priority requirements. Profile identity describes requested functionality, not caller-selected infrastructure. Do not require GPU IDs, shell commands, endpoint locations, repository paths or ComfyUI graph details from Music Director. Freeze resolved template/version/hash and parameters before dispatch.

Bind source identity/ceilings to the authenticated principal server-side. Do not trust a submitted `source_system` to impersonate an operator or escape ATHBA's medium ceiling. Scope idempotency, sessions, jobs and artifacts by principal and validate on every read/write/download. Unknown credentials fail closed. Job/session IDs are not authorization secrets.

Use authenticated private access over the existing permitted network/tunnel; no public forwarding/Funnel or open unauthenticated listener. Raw ComfyUI stays loopback-only. Protect launcher/API AND ComfyUI HTTP/assets/WebSockets through a verified authenticated access path. A working JSON API alone is not native UI access. Prefer serving the native UI at the root of its own protected origin over assuming ComfyUI works under an arbitrary subpath; test actual paths and WebSockets. Do not overwrite existing Tailscale/proxy configuration. When private publication needs unavailable permission, complete the code and report that specific deployment gate.

Use a small existing-framework authentication approach: scoped application credentials kept server-side for Music Director; secure operator browser sessions or validated private identity proxy. Do not put bearer/reservation tokens in query strings, returned URLs, localStorage, HTML, diagnostics, commits or final output. Browser state-changing requests require CSRF/origin controls; GETs are side-effect-free, allowed hosts/origins explicit, no wildcard credentialed CORS. Reject spoofed proxy identity headers. Credentials are provisioned into protected local files/configuration, not printed. No general multi-user identity product is required.

Bound body size, parameter counts, queue depth, concurrent connections, request deadlines and artifact byte counts. Use maintained HTTP/proxy/auth libraries, not handwritten HTTP/crypto. Receiver handlers must not block status/cancel while a render runs.

## 7. Minimal launcher — required

Serve a small plain HTML/JavaScript page with status, Start ComfyUI, Open ComfyUI, Finish session, and clear busy/error/recovery messages. No React build, workflow editor or general rack dashboard.

The launcher stays available while ComfyUI is stopped. Start is an explicit authenticated action, not a side effect of bookmarking/prefetching a GET. Poll durable session state; disable inappropriate controls and make double-click/reload idempotent. Open the normal protected ComfyUI UI in a new tab when ready. Show what is waiting and that Finish lets accepted work drain. Page refresh/browser closure must not lose the session or cancel accepted jobs.

Provide the ACTUAL verified private launcher URL in the final handoff, or explicitly say it is not deployed. An SSH command or invented URL is not the required user experience.

## 8. Managed rendering, replay and outputs

Use a registered API-format ComfyUI template, not editor graph JSON. Permit only typed explicitly mapped inputs; reject arbitrary node replacement, unknown parameters and caller paths. One qualifying local image workflow is sufficient. Do not silently change seed, model or dimensions on failure.

Persist a canonical prompt UUID, exact request and dispatch intent before POST. The reviewed upstream accepts caller UUIDs but DOES NOT deduplicate repeated POSTs. A lost acknowledgement/timeout after possible enqueue is `submission_uncertain`; do not blindly POST again. Reconcile the known ID against the same activation's queue/history with bounded reads. Missing/evicted history or a restarted backend cannot prove success or non-execution. Keep explicit unknown/interrupted outcomes; a new render needs a new caller-authorized submission. Concurrent duplicate requests must not double-render.

Use bounded polling initially; WebSocket progress is optional for managed job correctness. ComfyUI history is in memory/clearable. Rack AI's own persisted job/manifest is authoritative. HTTP 200, a prompt ID or an end event is not success: require valid successful terminal state, required output nodes and validated artifacts. Partial output/node-validation errors fail the expected-output contract.

Persist cancellation before side effects. Late completion cannot erase cancellation. Use verified atomic owned-job cancellation support or safe exclusive-session cleanup, never check-then-global-interrupt on a shared queue. Artifacts associated with a late cancelled job may be retained as evidence but not promoted to successful output.

Use isolated per-job output namespaces; associate files with the exact output nodes/job, copy into a Rack AI-owned artifact root and validate normalized paths/symlink escapes, type/content, expected count and size bounds. Hash and durably manifest files before success; no directory scan or recent-history fallback to unrelated results. Never fabricate a Git result. Retain profile/workflow/runtime/model identity, resource UUID/activation, prompt ID, fixed parameters, timestamps and error/cancel evidence, excluding secrets. Persistence failure is not success and unconfirmed cleanup keeps ownership blocked. Do not promise pixel equality across runtime changes.

## 9. Music Director companion — same task, separate PR

Implement the companion contract in `musicvideo-director` under that repository's rules. Do not leave it as a future task or claim changing its existing Remote ComfyUI URL implements the new API.

Add optional image backend selection, Rack AI URL and server-side credential configuration, read-only Test connection and discovery of supported profiles. Preserve the existing direct/VastAI backend and current projects/workflow designer; avoid unrelated refactoring or live database/media replacement.

Route the real image Generate action through a replaceable gateway. Persist submission identity before networking and a recoverable mapping to the intended project/segment/image slot. Generate sends one `/jobs` request without opening an interactive session. Existing background machinery should poll/import independently of a browser request. Where missing, add the smallest durable worker/reconciliation path and its startup configuration, not a full new queue platform. A browser refresh or process restart must not recreate the render; acknowledge network uncertainty and retry only the identical Rack AI submission. Regenerate deliberately creates a new submission.

Show queued/starting/running/completed/failed/cancelled/uncertain states appropriately. Download only job-owned artifacts through authenticated bounded streaming; validate manifest/hash/content and atomically persist into application media. An import retry fetches the SAME artifact, not a new render. A download error or unmatched output cannot count as a successful project image. Preserve original project provenance and approval flow; no unrelated cached-output fallback on this route.

Unsupported custom workflows/video/lip sync show explicit unsupported status under the Rack AI backend. Never silently switch to a billed provider or reinterpret a custom graph as an approved profile. The existing embedded designer remains usable for the existing direct backend; Rack AI managed generation does not secretly acquire an interactive session.

Additive migrations are tested on disposable databases. The private project's existing secrets/media/database must not be printed, committed, overwritten or migrated in production without a backup and specific authorization. Prepare a disposable test instance when live application changes would be disruptive. Provide exact activation steps for the existing application.

## 10. Deployment and efficient execution

Use the existing authorized `ssh tomp@gpurack` route; rack build/test/service work belongs on the rack, not a replacement NUC runtime. Verify access, repo read/write permissions, dependencies, Git authentication, isolated paths and service-manager availability FIRST and report any approval needed immediately. Never change host-key checks or provision broad privileges to make access work.

Inspect the real deployment read-only. Build in isolated worktrees, use an independently versioned candidate install and start only new Rack AI media/ComfyUI services; don't replace the working development binary/processes. Deploy candidate media API/launcher when existing permissions allow. Use a disposable Music Director instance for tests if the working app is elsewhere or in use. Record exact installed SHAs/config paths and rollback/stop instructions. Do not merge for deployment.

Prepare the isolated ComfyUI environment and required dependencies, reusing available compatible local models. No model collections or paid inference. If no usable image checkpoint is available, report that prerequisite rather than downloading an unbounded collection or accepting new gated license terms. Everything not dependent on that asset must still be implemented/tested. A smoke fixture is not GPU qualification.

Use four internal milestones without four user prompts: ownership/lifecycle; managed image; API/launcher; Music Director/end-to-end. Follow the source map once, implement focused tests, then full applicable suites and one self-review/fix pass. No duplicate whole-repository research or subagent swarm. Persist a compact progress/acceptance checklist, exact commands and evidence paths so resumption uses existing work. Do not declare completion with placeholders/TODOs in required paths.

## 11. Verification and handoff

Automated verification must use isolated job AND canonical resource roots, fake GPU/service probes, controllable real HTTP fixtures, the actual Python gate/middleware, and disposable application databases. Required cases:

- simultaneous acquisition, stale/wrong owner release, partial failure, corrupt/legacy records, independent worktrees sharing resource authority;
- duplicate/missing/wrong UUID, protected resource overlap, foreign processes, zero mutation of existing inference services;
- startup/drain/stop deadlines, in-flight POST drain barrier, browser/CLI exit, supervisor/backend restart, PID reuse, late control races and persistence failure;
- identical/concurrent replay, changed payload, acknowledgement loss, backend/history loss, partial/error output, cancellation versus success;
- unauthorized/spoofed identity, cross-owner reads/cancel/artifacts, CSRF/origin attacks, oversized bodies, path/symlink traversal, invalid/stale/oversized artifacts;
- actual launcher browser controls, readiness/error messages, native UI assets/WebSockets, page reload, stopped-backend startup and receiver responsiveness;
- Music Director settings, real Generate wiring, durable mapping/polling, retry/resume/import, missing credentials, unsupported workflow and legacy backend regression;
- shared schema fixtures and real cross-repository HTTP transport test (fake GPU is permitted here, but an in-process mocked gateway alone is not enough).

The legacy `tests/rack_resource_admission_smoke.sh` deletes/writes a gpu-2060 lease and invokes the coder. NEVER run it unchanged against live state; isolate the fixture/fake worker and audit other scripts first. No test may delete a live lease or unexpectedly invoke an LLM.

Run `cargo fmt --check`, `cargo test --workspace --offline`, focused Python/middleware/API/browser tests, Music Director's applicable tests and `git diff --check` in each repo. Fetch justified dependencies separately when necessary; distinguish missing offline cache from passing tests. Preserve safety tests. Record exact passes, failures, skips and unrelated pre-existing failures honestly.

Live acceptance is a separate gate: protected development identities/placement/health captured before -> launcher loads with backend stopped -> Start/Open native UI -> real local image -> Finish and verified release -> Music Director Generate while ComfyUI is stopped automatically starts it -> correct image appears in the right project -> replay/import retry causes no second render -> managed idle release/repeat activation -> protected development identities remain unchanged/healthy. Use fakes for destructive conflict/crash tests. Do not claim a real browser/device interaction, GPU run or process comparison without evidence.

For browser tests run a real browser against the candidate deployment/fixture where available. A fixture integration pass does not certify Tom's particular remote browser or existing Music Director installation. Report a private-network/actual-client acceptance gate separately when that machine is inaccessible.

Completion report: separate repository SHAs/PR links; CODE_COMPLETE; FIXTURE_E2E_PASSED; RACK_LIVE_QUALIFIED; DIRECTOR_LIVE_QUALIFIED; deployed component SHAs; exact launcher URL (or NOT_DEPLOYED); connection setup without secrets; actual workflow/profile used; how to finish/stop/rollback; explicit remaining blockers. Retain progress if interrupted. Do not claim an unattended duration or guaranteed single-run success. Keep PRs draft until required evidence exists and never merge without explicit instruction.

## Source references

Static code review: `docs/pr24-code-review.md`. ComfyUI protocol research snapshot: `Comfy-Org/ComfyUI@9113c08c2e14f1ca6c0ccab64920777fd01e1bb9`, `server.py`, `execution.py`, `main.py`; verify the actual pinned installed revision. Source snapshot is not deployment proof.

This documentation revision changes no production code, credentials, models or running services.
