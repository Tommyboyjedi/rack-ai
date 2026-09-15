# RackAI runtime contract v1

The service accepts authenticated POST requests at `/runtime/v1`. The schema identifier is
`rack-ai/runtime/v1`. See `config/runtime/request.schema.json`, the synthetic requests in
`config/runtime/fixtures`, and the executable clients in `tests/runtime`. The receiver takes
one administrator-owned, mode-0600 configuration path; callers cannot choose its resource
root, credentials, GPU placement, commands, model artifacts or backend. Configure the same
canonical root for legacy CLI (`RACK_AI_RESOURCE_ROOT`), media and this receiver.

Start an isolated receiver with `target/debug/rack_ai_runtime ADMIN_CONFIG.json`.
The listener must be loopback. Use the existing private ingress boundary for remote access;
never expose raw hosting ports as an alternative route. Possession of a local shell under the
administrator's account is outside the remote-caller authentication boundary.

## Operations

All ordinary requests use `Authorization: Bearer <source credential>`. Token hashes map to
server-owned source policies; changing `source_system` cannot change the authenticated source.
Successful operations return HTTP 200 and `{ "schema": "rack-ai/runtime/v1", "result": ... }`.

| Operation | Fields after `operation` | Result |
|---|---|---|
| `discover` | none | permitted tags, versions, qualification, capabilities, context and priorities; no activation |
| `acquire` | `request` (Acquire schema) | durable reservation decision; inspect until ready |
| `inspect` | `reservation_id` | current reservation and process/generation evidence |
| `control` | `reservation_id`, `request: {generation, action}` | renewal, release or cancellation intent |
| `infer` | `request` (Inference schema) | durable invocation record, initially accepted |
| `result` | `invocation_id` | invocation state, result, actual-start time and activation |
| `reconcile` | `invocation_id` | authoritative durable result/uncertainty; never resubmits |
| `cancel` | `invocation_id` | cancels not-started work; records cancellation intent for bounded in-flight drain |

`action` is `{kind:"renew",ttl_seconds:N}`, `{kind:"release"}` or `{kind:"cancel"}`.
A held reservation must be renewed within its original deadline. No waiting-based promotion
exists. The acquisition identity is scoped to the authenticated principal. Its exact payload
is retained. Replaying a denial returns that same denial even after capacity changes; use a
new acquisition identity for a deliberate later attempt, retaining the logical `work_id`.

Reservation states are `denied`, `preparing`, `ready`, `draining`, `held`, `releasing`,
`released`, `cancelled`, `expired`, and `recovery_required`. A priority denial is definitive
and is not accepted queued work. `reason` retains the conflicting incumbent or safety cause.
The response contains the recorded request/priority, frozen model/profile version/hash,
complete resource IDs, timestamps, generation, victims and process evidence. A renewed or
restored reservation keeps its identity and frozen profile. Restoration rotates generation
and access capability. Read the current record before new dispatch.

Invocation states are `accepted`, `started`, `completed`, `cancelled`, `expired`, and
`uncertain`. Accepted requests may wait during hold or capacity contention, until their
`waiting_deadline`. Optional `wait_seconds` (default: administrator `limits.max_wait_seconds`, at most 86400) bounds admission/capacity/hold waiting independently of `timeout_seconds`. Reservation expiry and cancellation still win. `execution_deadline` is set only when dispatch begins, to `started + timeout_seconds`; readiness probing and hold time do not spend that execution budget. Started is persisted before any possible backend call. Receiver death in
that interval is uncertain even if the backend might not have received the request. Unknown
outcomes are never automatically replayed, including workspace tools. Cancellation after
start persists typed `cancellation: {requested_at}`. Repeated cancellation preserves that intent. A known late successful response becomes `cancelled`, with `result: null` and output in `late_result`. An unknown outcome remains `uncertain` with cancellation intent retained, including after receiver restart. Neither state claims the backend was stopped. A completed response that won the race before cancellation remains completed.

## Limits, protocols and errors

Inference binds reservation ID, generation and profile hash. The server checks owner,
capabilities/context, qualification, expiry, output/time limits and one active invocation per
reservation immediately before dispatch. The basic request contains a prompt and explicit
output/time limits. `payload`, when present, carries a qualified Chat Completions or Responses
body without dropping tool definitions, structured-output settings or conversation messages.
Input uses a conservative UTF-8 byte bound against the qualified context envelope; outputs
are explicitly bounded. HTTP request bodies are limited to 1 MiB. Each accepted invocation freezes its response byte bound (default 256 KiB; administrator maximum 4 MiB).
Backend calls have connect and overall deadlines and no automatic retry.

The owner also receives `gateway_path`: append `/chat/completions` or `/responses` to that
path on the receiver origin. It is a secret bearer capability, bound to the authenticated
owner's reservation, current generation and frozen model. Treat it like a password. This
permits RackAI's existing JCode provider configuration to target a scoped endpoint while
keeping JCode inside the existing bounded workspace transaction. The Unix/TCP bridge carries
that path unchanged. Direct review/recovery clients can use the same scoped base URL.
The gateway enforces qualified protocols and output bounds. For existing JCode/review clients that omit an output bound, it supplies the frozen profile maximum before recording and dispatching the complete request; explicit bounds cannot exceed that maximum. An `Idempotency-Key`
identifies one logical invocation **within an owner and reservation**. Repeating the key and the same request reconciles its durable result; changed payload or bounds conflict. Distinct deliberate identical invocations use distinct explicit keys. The same explicit key on a restored reservation reconciles the original invocation after current-capability authentication; a fresh reservation has a distinct identity scope. Raw infer replay also verifies any supplied replacement generation against the current reservation.

Headerless calls conservatively reconcile equal payloads within one activation. Their fallback identity includes generation; a restored activation cannot collide with a completed unrelated call. Headerless clients cannot distinguish two deliberate identical calls in one activation: they must provide an explicit identity or use the RackAI-owned compatibility scope. Never change an identity just because an HTTP attempt timed out. An uncertain invocation remains uncertain and is never redispatched.

RackAI's JCode harness writes a `/calls/<logical-scope>` suffix into its private scoped provider configuration, derived from the workspace path and task. Within this scope the complete protocol payload identifies a model turn; retries preserve the scope, and distinct workspace transactions have distinct scopes even for identical payloads. Model turns with changed conversation/tool messages have distinct payload identities. Two deliberately identical turns within the same task require explicit distinct keys; they cannot be inferred from transport attempts. The direct reviewer supplies a stable explicit key derived from its full campaign/step/evidence prompt. No companion application changes are required. The sandbox bridge continues to carry the scoped path and raw JCode/review/recovery endpoints remain fenced.

Qualified SSE is retained and returned as a bounded buffered event stream. Incremental
first-token delivery is not claimed. Unqualified streaming/protocol requests fail closed.
The pure inference surface exposes no workspace tools; repository mutation still requires
the existing workspace executor, path controls, command evidence and semantic review.

HTTP 401 means authentication failure; 403 means source spoofing/policy failure; 404 means
unknown or another owner's record; 409 means identity, generation or reservation-state
conflict. HTTP 429 carries precise capacity codes: `capacity_pending_global`, `capacity_pending_reservation`, `capacity_retained_evidence`, `capacity_gateway_waiters`, or `capacity_api_workers`. Invalid typed requests use 400/422; oversized bodies use 413. Acquisition priority,
qualification and resource denials are durable HTTP-200 decisions with `state:"denied"`.
Transport/receiver errors are not evidence of a denial or a completed invocation: reconcile
by replaying the exact acquisition/submission identity. `recovery_required` preserves claims;
no lease is stolen merely because a process is idle, a timeout elapsed or a PID disappeared.

## Managed media

`comfyui` is an interactive profile; `local-image` is a managed profile of the same existing
ComfyUI backend and physical resource. They compete under ordinary priority policy. The
shared adapter reuses media lifecycle, native admission middleware, private access, process
identity, restart logic, artifacts and permanent checkpoint library. Dedicated placement
checks remain on the legacy path; shared activation substitutes a verified complete grant
and actual GPU UUID check. Native GPU work drains through the existing gate barrier.

A managed image job still uses `/api/media/v1/jobs` with its existing image parameters and
source credential. Add `reservation: {id, generation}` from the ready runtime grant. See `config/media/fixtures/job-request-paramount.json`; replace its synthetic zero/one handle with the actual grant. Keep the authenticated principal IDs and credentials aligned between the two RackAI receivers (`operator` and `music-director` in the examples). The
media receiver verifies the owner, unchanged priority, complete grant, model binding and
managed mode. A source cannot borrow an interactive session or another source's Paramount
reservation. Existing recorded requests keep their priorities and payloads. New configured
Music Director image requests use Paramount. No application adapter is included in this PR.

## Later client integration

Provision the source credential and allowed tags/priorities on RackAI first. Discover tags;
acquire independent demands (for example primary and coder); persist the acquisition record;
wait for readiness; persist each submission identity before sending; inspect/reconcile after
transport uncertainty; renew/release explicitly. Denial, held demand and uncertain invocation
are different outcomes. The client continues to own its dependencies, attempts, semantic
meaning and workflow progression. Do not infer completion from HTTP acceptance. Integrate
ATHBA, Music Director and CB only in later separately accepted tasks.

## Configuration validation and retention

Run `target/debug/rack_ai_runtime validate ADMIN_CONFIG.json` before service activation.
This validates policy/profile structure without loading models. Real executable/artifact
hashes, machine memory, process ownership and unit limits are checked again before effects.
A shared ComfyUI unit must have `MemoryMax` no greater than its reserved host budget,
`MemorySwapMax=0`, and a finite `CPUQuota` no greater than the profile limit. The adapter
checks these administrator-owned settings and the frozen `media_config_sha256`/Python executable binding; it does not rewrite the permanent unit.

`response.schema.json` defines all public success records; `error.schema.json` defines versioned errors. Status/result inspection reads an atomic snapshot and never rewrites the authority file. Mutations retain the shared authority lock and compare one encoding against the retained bytes; unchanged mutations skip the durable write. The supervisor first uses a read-only wakeup check, so stable retained history does not take the mutation lock each tick. All resulting ownership and priority decisions are rechecked under that lock.

Validated `limits` bound pending invocations (Accepted plus Started), per-reservation pending work, active dispatch workers, lifecycle workers, gateway waiters, waiting time, response bytes and admission storage. Defaults are published in `config.example.json`. Dispatch workers are considered only for active Ready reservations that own every resource and have no Started/Uncertain invocation; file permits are obtained before spawning. Held work creates no dispatch workers. Admission and control HTTP work have separate bounded slots; gateway waiting is asynchronous.

The canonical document remains bounded to 32 MiB. New acquisition decisions and invocations are refused **before** exhausting that bound: default admission ceiling 30 MiB, including retained bytes plus reserved future output/cleanup capacity. Each pending invocation reserves six times its frozen response bound (worst-case JSON escaping) plus 16 KiB for envelope, cancellation and diagnostics. Each reservation reserves 64 KiB plus profile and victim/claim growth. This intentionally conservative allowance may refuse work well below 30 MiB; terminal records and denied acquisitions consume retained capacity too. Idempotent replay is still available at capacity. Previously accepted completion/cancel/release transitions do not pass through new-work admission and retain their reserved headroom. Nothing is automatically deleted, and ownership checks are unchanged. Legacy on-disk `deadline` fields load as bounded waiting deadlines; legacy response bounds remain 4 MiB rather than silently shrinking accepted output allowances.

Actual I/O failure is separate from a capacity refusal. Failed writes do not commit partial state; a lost completion leaves Started durable, and restart makes it Uncertain. Claims stay fenced until the existing ownership/recovery path proves cleanup. An unavailable disk cannot promise successful terminal persistence merely because logical capacity was reserved.

### Retention operational procedure

1. Monitor retained authority size and `capacity_retained_evidence`; stop new submissions when the ceiling refuses admission. Continue authenticated inspect/reconcile, cancel eligible pending work, and release each owner's reservations. Permit bounded Started work to finish. Do not retry uncertain work under new identities.
2. Verify all accepted work has terminal durable evidence, no unresolved Started/Uncertain operations remain, all affected owned processes have proven cleanup, and the canonical claims map is empty. If any check fails, keep the authority fenced and use its recovery procedure; a timeout is not proof of release.
3. After quiescence, stop this receiver and take a permission-preserving, checksum-verified copy of the entire authority directory, including release receipts and ownership evidence, to operator-controlled immutable storage. Retain the original canonical document and identity history. Do not truncate `managed.json`, discard invocations, clear claims, or start an empty authority behind the same credentials.
4. This PR provides safe admission stopping, not automatic compaction or a new archive lookup protocol. At the ceiling, leave new-work admission stopped until a separately reviewed migration preserves owner-scoped acquisition/invocation reconciliation and proves no unresolved effects. The finite-capacity service can still finish and release already accepted work. A preexisting authority from an older implementation without reserved headroom must be quiesced and capacity-reviewed before upgrade; no production upgrade is performed by this PR.

The near-capacity integration test lowers the same admission threshold to 512 KiB, fills it through real bounded backend results, then proves original held work completes and all claims release after precise refusal. A separate 32 MiB authority test exercises the hard storage boundary without evidence deletion. Filesystem permission failure is injected separately; it is not mislabeled as logical capacity pressure.

Definitive acquisition denial reasons use these stable codes/prefixes:

| Reason | Meaning |
|---|---|
| `incumbent_priority:<id>` | At least one required resource has an equal/higher incumbent |
| `transition_or_recovery_blocked:<id>` | A conflicting activation is not safely preemptible |
| `legacy_ownership_requires_migration:<ids>` | Existing legacy leases require owning-path migration |
| `ownership_uncertain` (optionally followed by diagnostic text) | Canonical ownership cannot be established |
| `insufficient_host_memory` | Complete admitted demands exceed the configured host budget |
| `unqualified_profile` | Normal source attempted an unqualified profile |
| `capability_or_context_unqualified` | Requested capability/context is outside the profile qualification |

Transition `reason` and invocation `error` retain additional machine/protocol diagnostics.
Clients must treat unknown diagnostics as blocked/uncertain and inspect the state;
they must not parse arbitrary OS error prose as permission to retry or steal ownership.

## Bounded workspace call lifetime

The trusted RackAI JCode runner registers its workspace/task call namespace before
starting the harness. `POST <gateway_path>/scopes/<namespace>` accepts typed
`{"operation":"open","deadline_ms":<absolute Unix milliseconds>}` and
`{"operation":"close"}` controls; success is HTTP 204. The deadline comes from the
same workspace timeout budget as the runner's monotonic process deadline, captured
before scope registration or harness setup. It does not replace either the per-call
waiting deadline or execution budget. Its representation matches the existing
workspace `TimeoutSeconds` range; zero/past and out-of-range deadlines are rejected.

A scope is bound to one owner/reservation and retained durably. Repeating the same
registration reconciles its original deadline and never extends or reopens it;
changed registration data conflicts. JCode's private compatibility URL includes
`/calls/<namespace>`. The gateway records the corresponding `workspace_scope` on
inference and rejects missing, closed or expired scopes before admitting new work.
This check and invocation admission share the authority transaction, so delayed
HTTP submission cannot cross a committed closure. Existing invocation identities
still reconcile; changing payload under one explicit identity still conflicts.

On timeout or another runner exit, RackAI closes only that execution scope. Pending
calls become Cancelled with typed intent. Scope expiry is independently enforced by
supervisor reconciliation, dispatch eligibility and the final dispatch transaction.
The Accepted-to-Started transaction is the dispatch boundary: closure winning that
race prevents dispatch; a call already Started retains cancellation/drain/uncertainty
evidence. A valid late backend response is retained as `late_result`, without an
ordinary successful `result`. No model stop is claimed. Completion rechecks the scope
fence. Calls belonging to other scopes and the shared reservation remain intact.

The original capability can close only its registered scope after activation rotates;
it cannot use that exception to register new scopes or invoke a stale model endpoint.
A temporary HTTP disconnection does not close the scope or cancel accepted work.
A valid, still-open workspace can therefore wait through preemption and execute once
on restoration. Companion applications do not need to supply these controls.

Scope controls use the bounded admission/control pools independently of gateway
waiters. Scope registration applies retained-evidence admission and reserves 128
additional bytes per retained scope for closure; no tombstone is deleted. Capacity
refusal remains HTTP 429 `capacity_retained_evidence`. Read-only status/result calls
remain read-only. Follow the existing retention procedure for the complete authority,
including scope records.

Registration must be acknowledged before JCode can start. A failed/unconfirmed close
is an explicit workspace failure retained in the terminal packet. If storage is
unwritable at timeout, the previously committed deadline still forbids late dispatch;
the cancellation record is reconciled when durable storage recovers. Receiver restart
does not reopen scopes or replay uncertain calls. An abruptly lost runner may leave
its scope open until the registered deadline; transport loss itself is not authority
to cancel. This change does not retroactively associate pre-upgrade unscoped calls
with workspace executions; quiesce older runners before any separately authorized
upgrade. No production upgrade or GPU qualification is performed here.

### Bounded retirement after uncertain output

An explicit release/cancel or reservation expiry bounds backend draining. After
that drain deadline, RackAI may stop its verified owned backend, using the same
process, cgroup and GPU cleanup checks. If a live dispatcher still owes a durable
callback, claims remain until it records its late response or uncertain transport
result. An already uncertain invocation stays uncertain, including its original
submission identity and cancellation evidence; shutdown does not fabricate an
answer or reduce its dispatch count. Proven process cleanup permits reservation
release without resolving the unknown answer. Failed ownership/cleanup or storage
proof retains the resource fence. This does not change competing-priority victim
drain policy or permit replay of uncertain work.

## External GPU inactivity policy

The common reservation lifecycle now has a server-configured 1800-second default GPU
inactivity limit, independent of ownership TTL/renewal. See
[activity, idle expiry, media barriers, CB policy and deployment limits](external-reservation-idle.md).
New activity is recorded per reservation as `last_activity_at`; polling and renewal do
not refresh it. Idle expiry retains `reason=idle_timeout`, fences new work, and returns
resources only after the existing verified cleanup. Running/uncertain work remains fenced.
The manual Start/Open/Finish workflow is preserved, with a separate 1800-second default
idle safety backstop. Deploy the native activity gate and both receivers together.
