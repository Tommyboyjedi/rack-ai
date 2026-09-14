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
bounded deadline. Started is persisted before any possible backend call. Receiver death in
that interval is uncertain even if the backend might not have received the request. Unknown
outcomes are never automatically replayed, including workspace tools. Cancellation after
start is a durable request to drain, not a fabricated proof that computation stopped.

## Limits, protocols and errors

Inference binds reservation ID, generation and profile hash. The server checks owner,
capabilities/context, qualification, expiry, output/time limits and one active invocation per
reservation immediately before dispatch. The basic request contains a prompt and explicit
output/time limits. `payload`, when present, carries a qualified Chat Completions or Responses
body without dropping tool definitions, structured-output settings or conversation messages.
Input uses a conservative UTF-8 byte bound against the qualified context envelope; outputs
are explicitly bounded. HTTP request bodies are limited to 1 MiB, backend responses to 4 MiB.
Backend calls have connect and overall deadlines and no automatic retry.

The owner also receives `gateway_path`: append `/chat/completions` or `/responses` to that
path on the receiver origin. It is a secret bearer capability, bound to the authenticated
owner's reservation, current generation and frozen model. Treat it like a password. This
permits RackAI's existing JCode provider configuration to target a scoped endpoint while
keeping JCode inside the existing bounded workspace transaction. The Unix/TCP bridge carries
that path unchanged. Direct review/recovery clients can use the same scoped base URL.
The gateway enforces qualified protocols and output bounds. For existing JCode/review clients that omit an output bound, it supplies the frozen profile maximum before recording and dispatching the complete request; explicit bounds cannot exceed that maximum. An `Idempotency-Key`
identifies a deliberate invocation. Without it, the complete protocol payload digest is the
stable identity: identical payloads replay the same durable result rather than invoke twice.
For a deliberate second identical request, supply a new key.

Qualified SSE is retained and returned as a bounded buffered event stream. Incremental
first-token delivery is not claimed. Unqualified streaming/protocol requests fail closed.
The pure inference surface exposes no workspace tools; repository mutation still requires
the existing workspace executor, path controls, command evidence and semantic review.

HTTP 401 means authentication failure; 403 means source spoofing/policy failure; 404 means
unknown or another owner's record; 409 means identity, generation or reservation-state
conflict. Invalid typed requests use 400/422; oversized bodies use 413. Acquisition priority,
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

`response.schema.json` defines all public success records; `error.schema.json` defines
versioned errors, including typed JSON rejection details. The canonical managed document
is bounded to 32 MiB. Reaching that retained-evidence bound refuses further mutations;
there is no automatic deletion of decisions or uncertain work. Plan an explicit quiescent
archive/migration before sustained production use. Raw backend output remains bounded;
reported token usage exceeding the request limit is uncertain, not successful execution.

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
