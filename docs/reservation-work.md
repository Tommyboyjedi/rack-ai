# Reservations and work

RackAI does not assign or cap application priority. Requested priority is caller-owned input on a reservation. Authentication establishes ownership/security, not workload importance or service entitlement. Published qualified services are available uniformly to authenticated ordinary principals.

Reservation and work operations below use the existing authenticated `POST /runtime/v1` endpoint and its existing `{schema, result}` response envelope. No new work-unit version, workspace scheduler, ledger, gateway or native proxy is introduced.

## Read the deployed contract

`GET /runtime/v1/contract` requires the same `Authorization: Bearer <token>`
credentials as `POST /runtime/v1`. It is read-only and returns a JSON object:

- `schema`: `rack-ai/runtime-contract/v1`.
- `contract_version`: `1.4.0`, the published contract revision.
- `documentation`: the complete contents of this document as a string.
- `request_schema`: `config/runtime/request.schema.json` as a JSON object.
- `response_schema`: `config/runtime/response.schema.json` as a JSON object.

All three canonical files are embedded at compile time, so the response describes
that deployed binary, independent of files on the runtime host. Missing or invalid
credentials return the existing HTTP 401 `{schema, error: "unauthorized"}` response.
`discover` remains the dynamic service/capability discovery operation; this endpoint
does not report live service availability or change discovery behavior.

## v2 ownership rules (authoritative)

This section supersedes older compatibility prose below that describes independent
member acquisition, `Held` restoration, partial-ready use, or work waiting for a
future claim. A `reserve` request is an atomic logical-service ownership decision:
it returns `ready` only after every requested member is owned, activated and
ready; if any member is blocked at acquisition, the reservation is
`unavailable`, not partially usable. Services within an existing older
reservation remain independent when only one has physical overlap, so an
unaffected Ready member continues to work while its peer becomes `preempting`
and then terminal `preempted`.

Priority is evaluated only while acquiring a reservation. A Ready member accepts
calls as local `queued` work in FIFO order; dispatch makes no further global
priority decision. A lower/equal incumbent block returns typed `unavailable` and
bounded `retry_after`, and does not create a queued invocation. A higher-priority
acquisition marks the affected owner `preempting`, rejects new calls, cancels all
not-started queued calls with
`reservation_superseded_by_higher_priority`, permits already `running` calls to
finish normally, then transfers ownership and makes the new reservation Ready.
`preempted` is terminal: release merely makes capacity available and RackAI never
reacquires it or replays cancelled work. The client must make a new acquisition.

The durable invocation states are `queued`, `running`, `completed`, `cancelled`,
`failed`, `expired`, and `uncertain`. Terminal response payloads are compacted to
SHA-256 receipts within bounded `terminal_evidence_bytes`; active queued/running
and uncertain calls are never compacted. See
[reservation-ownership-preemption-v2.md](reservation-ownership-preemption-v2.md)
for the state machine, migration and qualification details.

## Reserve services

```json
{
  "operation": "reserve",
  "request": {
    "acquisition_id": "acquisition-123",
    "work_id": "project-456",
    "services": ["local-primary", "local-coder"],
    "priority": "paramount",
    "ttl_seconds": 1800
  }
}
```

The response contains `id`, `state`, `priority` and a `services` object keyed by service name. Each entry describes its existing managed activation/access. The public reservation associates these activations in the same canonical authority document; it does not introduce a second owner of resources.

Admission attempts requested services independently, in request order, at the reservation's single priority. The response always identifies one reservation and lists `requested_services` plus the individual service states. When states differ, the aggregate is `partial`. A failed acquisition is `unavailable`; it does not deny or undo other acquired services. Overlapping exclusive services may be requested, but cannot all be held simultaneously. Higher priority may safely preempt lower priority; ties retain the incumbent. Independent resources coexist.

Use `inspect_reservation`, `refresh_reservation`, `release_reservation` or `cancel_reservation`, each with `reservation_id`. Release/cancel controls all service activations and closes the reservation to refresh. Work is eligible when its selected service is Ready, regardless of other members. Individual service expiry/release does not close unrelated Ready members. Existing generation rotation, qualification, ownership and verified cleanup remain authoritative.

Acquisition replay with the same identity and exact request returns the durable original response, even after subsequent state changes. Changed payload conflicts. Use inspection for current state. A receiver upgraded with an older record lacking an original receipt fails closed on replay instead of inventing one.

`refresh_reservation` attempts only currently unavailable services against current rack state. It retains the reservation ID and priority, does not restart Ready members, and leaves Held members to their existing automatic restoration lifecycle. Missing services are never queued, monitored or assigned a place in line. If nothing becomes available, refresh returns the current state. Refresh does not extend the original reservation TTL and cannot reopen an explicitly released/cancelled reservation.

```json
{"operation":"refresh_reservation","reservation_id":"returned-reservation-id"}
```

An unavailable service rejects new work without creating a pending invocation. A previously acquired Held service retains its identity and pending work; restoration may resume it automatically. The caller decides whether a partial reservation is useful.

## Submit work

```json
{
  "operation": "submit_work",
  "request": {
    "reservation_id": "returned-reservation-id",
    "service": "local-primary",
    "work_id": "work-789",
    "payload": {
      "kind": "inference",
      "prompt": "A bounded request",
      "max_tokens": 128,
      "timeout_seconds": 30
    }
  }
}
```

Work contains no priority and cannot select a service outside its reservation. `work_id` is unique within the authenticated principal. Repeating the exact request reconciles its original invocation; changing the reservation, service or payload conflicts. Neither a disconnected caller nor a retry creates another execution within the retained work record. `inspect_work` and `cancel_work` take `work_id` and enforce ownership.

`inspect_work_execution` takes `work_id` and returns a sanitized, generic execution projection for the authenticated owner. It reports authoritative work identity, current/terminal outcome, known attempt evidence, cleanup/recovery closure, bounded activity/timing/count metadata, and opaque artifact IDs for diagnostic text. It does not expose server filesystem paths, private packet paths, workspace roots, scoped access secrets, or JCode-specific control details. `get_work_artifact` takes one of those opaque artifact IDs and returns a bounded read-only diagnostic payload after the same owner check. Artifacts remain ordinary retained evidence, not authority to execute again after expiry. See [work-execution-public-contract.md](work-execution-public-contract.md) for the evidence inventory and measurement semantics.

The existing invocation record stores accepted/started/completed/cancelled/expired/uncertain state, result, cancellation intent and late evidence. Work inspection also reports waiting/Held from current reservation state. Cancellation before dispatch prevents execution. Cancellation after dispatch records intent and retains late evidence. Receiver restart marks unresolved started work uncertain, never automatically replaying it. Ordinary inference protocol/time limits remain unchanged. Service discovery publishes `context_tokens`, `max_input_tokens` and `max_output_tokens`; public Ready reservation members expose the same profile-frozen limits beside their model and profile version. RackAI enforces explicit output bounds against `max_output_tokens`. Scoped chat/completions and responses calls that omit an output bound receive the reserved profile's `max_output_tokens` before backend dispatch. RackAI does not infer tokenizer-specific prompt length from serialized HTTP bytes; clients and model runtimes remain responsible for true input-token enforcement against the published `max_input_tokens` contract.

## Bounded workspace payload

Use the same work envelope with this payload shape:

```json
{
  "kind": "workspace",
  "workspace": {
    "repository": {"id": "registered-repository", "base_ref": "main", "base_sha": "exact-base-sha"},
    "objective": "Perform this bounded change",
    "allowed_paths": ["src/"],
    "acceptance": {"commands": [["cargo", "test", "--offline"]], "required_artifacts": ["src/lib.rs"]},
    "requirements": {"complexity": "small", "requires_large_context": false},
    "limits": {"max_implementation_attempts": 1, "timeout_seconds": 900, "network": "disabled"}
  }
}
```

Administrator runtime configuration enables this existing execution machinery with `workspace: {registry_root, state_root}`. These are server-owned paths, not client inputs. At least two existing dispatch slots are required so a running workspace cannot occupy the only slot needed by its nested model calls.

RackAI resolves the qualified worker compatible with the reserved model/resources. It injects the reservation's managed endpoint into that execution's private JCode configuration. It does not rewrite the registry or accept caller-supplied model endpoints. `ExecuteWorkspace` receives a typed `WorkspaceRequest` containing the reusable bounded change input directly. It retains repository registration, pinned base, worktree isolation, allowed paths, network controls, acceptance commands, candidate revision and provenance/evidence packets. The old versioned work-unit CLI, request documents, parsers and legacy worker-selection branch have been removed. There is no translation into an older contract.

The trusted runner registers its existing workspace scope with its parent invocation. The original scoped authorization may continue that bounded, already registered execution across generation rotation. It cannot authorize a fresh scope after restoration. Every model dispatch still checks current Ready state and resource ownership. Parent cancellation, expiry or uncertainty closes child dispatch authority. The same registered scope and work identity survive Held/restoration; no replacement workspace is created.

Per-model execution timeout starts at actual model dispatch, so waiting/Held does not consume it. The existing overall workspace wall-clock limit remains a separate finite bound. Uncertain nested execution is retained as uncertainty on the parent work. Acceptance checks the persisted work cancellation fence before accepting a revision.

## Interactive ComfyUI

A Ready interactive `comfyui` service returns `access` with `kind: "native_comfyui"`, `url`, `session_id`, reservation activation identity and generation. Use `url` as the native origin for `/object_info`, prompt submission, history/status and existing WebSocket operations. An LLM `gateway_path` is not returned for ComfyUI.

This is the existing protected media-native HTTP/WebSocket interface. Existing media credentials must identify the same principal as the runtime reservation. Existing session ownership, managed lease checks, backend gate and lifecycle control remain in use. No alternate native transport or security system is added. Access is unavailable while the ComfyUI service is not Ready; a Held or unavailable peer does not block it. The native interface retains its existing current-session semantics; this change does not invent a separate per-request native capability scheme.

The managed `local-image` job API, its existing request format and reservation `{id, generation}` binding remain separate and supported.

## Configuration migration

Remove source `permitted`, `default`, `maximum`, `tags` and `tag_priorities`, workspace `source_admission_policies`, and media principal `ceiling` from ordinary configuration. These application-policy types and fields have been removed. Runtime/media configuration rejects their obsolete fields. Identity/credential fields and existing operational flags remain. No recorded historical request or priority is rewritten.

Legacy single-service `acquire` still works; omitted legacy priority now uses the uniform Low default. Discovery publishes the global `priorities` vocabulary and all service profiles, including qualification status, capabilities, context and input/output token limits. Ordinary unqualified requests still fail qualification independently of application identity. New reserve requests supply priority explicitly.

No deployment or client migration is performed by this change.


## Chatterbox speech and registered voices

Chatterbox Turbo is the live local-tts service with capability audio and protocol
speech. Reserve it through the same reserve operation as other services:

~~~json
{"operation":"reserve","request":{"acquisition_id":"dialogue-123","work_id":"dialogue",
"services":["local-primary","comfyui","local-tts"],"priority":"paramount","ttl_seconds":1800}}
~~~

Each service is independently acquired. Read services.local-tts.state and use its
gateway_path when Ready. An unavailable/Held peer does not block Ready speech;
an unavailable TTS member does not block Ready primary or ComfyUI. Existing Held
members restore through the managed lifecycle; refresh attempts unavailable members.
Priority occurs only on the reservation, never on a speech or work call.

Speech uses the synchronous scoped binary interface rather than the text-inference
submit_work payload. The returned member gateway is bound to the owner and current
activation, with the same canonical reservation authority:

~~~http
POST <services.local-tts.gateway_path>/speech
Authorization: Bearer <principal credential>
Idempotency-Key: <stable utterance identity>
Content-Type: application/json

{"text":"Hello! [chuckle]","voice":"character-jane"}
~~~

Success is bounded audio/wav: PCM16, mono, 24000 Hz, at most 60 seconds /
2,880,044 bytes. Text must be nonblank, at most 1000 characters and 4096 UTF-8 bytes,
without NUL. Exactly one synthesis can be accepted/active/uncertain per TTS member.
The runtime queue wait uses configured max_wait_seconds; generation is bounded to
60 seconds. Same-key/same-payload retries reconcile, never regenerate; expired and
uncertain identities remain terminal/uncertain. A changed payload conflicts. HTTP
429 indicates capacity; scoped validation/state errors return 409 with a JSON error.
Polling, voice registration/listing and lease control do not extend GPU activity.
1800 seconds without real workload activity triggers owned-backend cleanup and release.

Settings remain server-owned: temperature=1.05, top_p=0.95, top_k=1000,
repetition_penalty=1.2; one generation, no rewriting or multiple takes. The model
loads once per activation. Callers cannot supply paths, GPU choices or model settings.

POST <gateway_path>/voices with the same bearer and {} lists
{"voices":["character-jane"]} for a Ready owned TTS member.

POST /runtime/v1/voices/register intentionally requires no authentication.
Send multipart fields voice_id and file (a plain WAV basename):

~~~sh
curl "$RACKAI_BASE_URL/runtime/v1/voices/register" -F 'voice_id=character-jane' -F 'file=@reference.wav;type=audio/wav'
~~~

Success: {"voice_id":"character-jane","registered":true,"sha256":"<sha256>"}.
IDs use 1-128 ASCII letters/digits/underscore/hyphen/period, cannot begin with a
period or contain consecutive periods. WAV references must be PCM16 mono, >5 and
<=30 seconds, at 16000/22050/24000/44100/48000 Hz and <=6 MiB.
Caller paths, traversal, symlinks and invalid formats fail closed. Storage remains
administrator-owned via the Chatterbox profile's existing --voices registry.
Registration/replacement atomically updates that registry and is immediately visible
without reloading the model; no filesystem paths are returned. Upload errors use
422 (invalid), 413 (oversized), 408 (timeout), 429 (capacity), or 503 (storage).
The existing 128-ID / 768-MiB / 1024-retained-upload bounds remain.

The embedded request schema includes speechRequest, voiceListRequest and
voiceRegistrationForm definitions for these separate scoped/multipart routes.
The response schema includes voiceListResponse, voiceRegistrationResponse and
speechResponse definitions; binary WAV is not a runtime JSON work result.


### Media health and recovery

A ComfyUI transport failure closes admission while RackAI verifies the canonical
claim, activation, systemd generation, and GPU/process ownership. A verified owned
backend uses the existing bounded Preparing/startup cleanup lifecycle. A stopped
backend releases its claim only after systemd has no pending job or populated
cgroup, the recorded process generations are gone, and the media GPU is clear.
The supervisor also revisits retained ComfyUI recovery records after receiver
restart. Conflicting or missing ownership evidence retains the claim and
`recovery_required`; reconciliation never replays or deletes invocation history.


## Shared reservation inactivity

Services in one public reservation share the inactivity window (30 minutes by default). Recent accepted/executed model activity or unresolved running work on any member protects the other members from idle retirement. The group becomes idle only after every member has been inactive for the full window and no member has unresolved work. Separate reservations remain independent. Inspection, refresh and renewal do not count as model activity; explicit release/cancel, preemption and backend-failure cleanup retain their existing authority. For public reservations, verified activity on any owned Ready member extends the live members' persisted ownership deadlines to at least one full inactivity window after that activity. Running or uncertain work retains the same protection. The requested TTL bounds initial acquisition and remains effective when there is no workload activity; it is not a hard lifetime cap on an active group. Terminal, preempted, unowned and already elapsed members are never revived. Native media activity is observed through its authenticated queue barrier. A native member is closed only when the group is idle, and its peers wait for that close before idle retirement.

## Opt-in development interaction diagnostics

Public reserve optionally accepts diagnostics: {"retained_model_interactions": true}.
It defaults off and participates in frozen acquisition identity. Existing
inspect_work_execution advertises owner-scoped opaque interaction IDs while the
reservation is live; get_work_artifact retrieves one bounded record using schema
rack-ai/model-interaction/v1. Release/cancel immediately revokes retrieval and
heavyweight documents are deleted, with bounded maintenance retry on I/O failure.
They do not use ordinary 14-day history retention. See
[reservation interaction diagnostics](reservation-interaction-diagnostics.md)
for effective capture, limits, redaction, unavailable metrics and restart.


## Exact input preflight

Qualified text Chat Completions profiles may opt in with the administrator-owned
`input_token_preflight` flag. It defaults false and is omitted from serialization
when false, preserving existing frozen profile hashes. Discovery and reservation
members advertise `input_preflight` only for opted-in profiles, including
`schema: "rack-ai/input-preflight/v1"`, `generates_tokens: false` and
`enforcement: "caller_must_check_fits"`. Ordinary qualification still applies.

Use the advertised member path, or append `/chat/completions/preflight` to its
`gateway_path`, on the same receiver origin:

```http
POST <member.gateway_path>/chat/completions/preflight
Authorization: Bearer <owner credential>
Content-Type: application/json

{"model":"tiny-uncd","messages":[{"role":"user","content":"Extract the current scene."}],"max_tokens":700,"temperature":0,"chat_template_kwargs":{"enable_thinking":false},"response_format":{"type":"json_object"}}
```

The bearer must identify the reservation member's owner. Possession of the
scoped URL alone does not authorize this endpoint. The member must remain Ready,
active and owner of every claimed resource, with the same generation, frozen
profile and backend process. The URL is a secret capability; never log it.


A successful HTTP 200 response is the preflight object directly, without the
ordinary runtime `result` wrapper. Example values below are illustrative:

```json
{
  "schema": "rack-ai/input-preflight/v1",
  "service": "tiny-uncd",
  "model": "tiny-uncd",
  "reservation_id": "member-activation-id",
  "generation": "current-generation",
  "profile_hash": "frozen-profile-sha256",
  "profile_version": "qwen3.5-4b-heretic-q6k-llama-5cb679dce989-8k-preflight-v2",
  "input_tokens": 3712,
  "max_input_tokens": 7168,
  "max_output_tokens": 1024,
  "context_tokens": 8192,
  "requested_output_tokens": 700,
  "total_reserved_tokens": 4412,
  "fits": true,
  "reason": "fits"
}
```

`reservation_id` identifies the selected service member/activation carried by
its scoped path; in a multi-service group it may differ from the public group's
root ID. Compare it with that member's `id`, along with `generation`,
`profile_hash` and `profile_version`. Limits are frozen with that member.

`input_tokens` counts the rendered prompt, not serialized HTTP bytes.
`total_reserved_tokens` is input plus requested output; its name denotes the
fit arithmetic, not a new resource or token reservation. `fits` requires input
at most `max_input_tokens`, output at most `max_output_tokens`, and the combined
count at most `context_tokens`. A lower requested output does not raise the
published input ceiling. Non-fitting HTTP 200 results use `input_limit_exceeded`,
`output_limit_exceeded` or `context_limit_exceeded` in that precedence order.


Send the complete Chat Completions body, including messages, tool definitions,
`response_format`, and admitted `chat_template_kwargs`. If no output bound is
present, preflight inserts the frozen profile's normal output maximum, matching
the scoped inference gateway. Zero, malformed and ambiguous output bounds are
rejected. Supported content is a nonempty messages array with string roles and
string content. Media/content-part arrays, Responses and unsupported template
options fail closed. Explicit schema and thinking controls remain unchanged.

For qualified llama.cpp profiles, the service applies the actual server template
to the complete admitted body, then tokenizes that prompt with both
`add_special=true` and `parse_special=true`. Missing/malformed template or token
results never become a fit result. Required BOS/EOS and applicable role/tool
framing are included. Do not count JSON transport keys or assume the schema's
serialized bytes are prompt tokens. The tested Qwen template adds zero prompt
tokens for its JSON Schema constraint; other models/templates must be qualified
for their actual rendering behavior. Clock/environment-dependent templates are
unsupported unless their rendering/count consistency is separately established.

This capability is opt-in for a frozen qualified profile, not a claim that any
upstream llama.cpp checkout matches every installed binary. Qualification must
compare against actual Chat Completions `usage.prompt_tokens`, including cached
input, and cover supported roles, Unicode, tools, schemas and thinking defaults.
Changes to model, server implementation, template or defaults require fresh
qualification. The boolean flag does not grant callers paths, GPUs or runtimes.


Preflight shares the existing bounded gateway waiter pool. A permit remains held
until the blocking operation finishes, including after a client disconnect. The
two backend reads share a 10-second overall HTTP budget, with a 2-second connect
bound; each response uses the configured response-byte maximum. Incoming HTTP
bodies retain the 1 MiB limit. Redirects and proxies are disabled. The existing
endpoint barrier prevents port reuse during the read, and ownership, process,
generation and profile are verified again before returning. Release, expiry or
preemption can invalidate the operation; failure never returns guessed counts.

Preflight performs no generation, creates no work/invocation ledger record and
does not renew inactivity or ownership. It does not reserve future input space
or extend a reservation. No authority lock is held over backend HTTP. It does
not replace normal inference admission or introduce mandatory input enforcement
into existing inference paths. Callers must reject `fits=false`, preserve the
complete measured payload, check the returned member/profile/generation binding,
and submit only while that same member remains Ready. Recount changed payloads
or replacement activations. A positive response describes the checked instant;
it cannot guarantee later capacity or continued ownership.

The plain `submit_work` inference payload cannot carry `response_format` or
`chat_template_kwargs`. Structured requests use the existing scoped Chat
Completions interface. Do not truncate scene facts, histories or schemas to
satisfy a serialized-byte heuristic, and do not bypass RackAI through a raw
hosting endpoint to obtain counts.


Handler errors use `{"schema":"rack-ai/input-preflight/v1","error":"code"}`:

| HTTP | Codes and meaning |
|---|---|
| 401 | `unauthorized`: missing or invalid bearer |
| 404 | `not_found`: absent member or another owner's member |
| 403 | `invalid_scoped_capability`: bearer owner supplied the wrong capability |
| 409 | `reservation_not_dispatchable`, `stale_generation_or_profile`: ownership or binding changed |
| 409 | `input_preflight_unqualified`: profile/backend/protocol not qualified for counting |
| 409 | `input_preflight_backend_identity_unproven`, `input_preflight_endpoint_busy`: process or dispatch barrier could not be proven |
| 429 | `capacity_gateway_waiters`: shared bounded waiter pool full |
| 400 | Normal Chat validation errors, `inference_limits`, or `input_preflight_unsupported_content` |
| 502 | `input_preflight_backend_failure`, `input_preflight_backend_invalid_json`, `input_preflight_invalid_template`, `input_preflight_invalid_tokens`: backend/count proof unavailable |
| 502 | `input_preflight_response_oversized`, `input_preflight_worker_failure`, `input_preflight_endpoint_unavailable`: bounded read or worker/fence failure |

Malformed HTTP/JSON and oversized requests may use the receiver's normal
extractor errors, including 413. None of these failures proves that the payload
fits; do not replace a missing count with a byte estimate. Existing inference
transport errors and durable reconciliation semantics remain unchanged.
