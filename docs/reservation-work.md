# Reservations and work

RackAI does not assign or cap application priority. Requested priority is caller-owned input on a reservation. Authentication establishes ownership/security, not workload importance or service entitlement. Published qualified services are available uniformly to authenticated ordinary principals.

Reservation and work operations below use the existing authenticated `POST /runtime/v1` endpoint and its existing `{schema, result}` response envelope. No new work-unit version, workspace scheduler, ledger, gateway or native proxy is introduced.

## Read the deployed contract

`GET /runtime/v1/contract` requires the same `Authorization: Bearer <token>`
credentials as `POST /runtime/v1`. It is read-only and returns a JSON object:

- `schema`: `rack-ai/runtime-contract/v1`.
- `contract_version`: `1.2.0`, the published contract revision.
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

The existing invocation record stores accepted/started/completed/cancelled/expired/uncertain state, result, cancellation intent and late evidence. Work inspection also reports waiting/Held from current reservation state. Cancellation before dispatch prevents execution. Cancellation after dispatch records intent and retains late evidence. Receiver restart marks unresolved started work uncertain, never automatically replaying it. Ordinary inference token/context/protocol/time limits remain unchanged.

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

Legacy single-service `acquire` still works; omitted legacy priority now uses the uniform Low default. Discovery publishes the global `priorities` vocabulary and all service profiles, including their qualification status. Ordinary unqualified requests still fail qualification independently of application identity. New reserve requests supply priority explicitly.

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
