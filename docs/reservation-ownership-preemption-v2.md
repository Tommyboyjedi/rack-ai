# RackAI reservation ownership and preemption v2

## Purpose

Define the RackAI boundary for multi-application resource ownership, preemption and queued work. Applications decide what logical services they need and when. RackAI owns physical resource arbitration, service activation and execution while a reservation owns the service.

## Core rules

1. Priority applies to reservation acquisition only. There is no per-call priority.
2. A Ready reservation gives exclusive ownership of its logical services until release, expiry or higher-priority preemption.
3. Calls submitted inside a Ready reservation may be queued, running, completed, cancelled or failed. Queued is not the same as unavailable/preempted.
4. If a higher-priority reservation needs a service already owned by a lower-priority reservation, the lower reservation enters PREEMPTING. New calls are rejected, all not-started queued calls are cancelled with a typed `reservation_superseded_by_higher_priority` result, currently running calls are allowed to finish and return normally, then the service transfers to the higher-priority reservation.
5. The displaced reservation becomes PREEMPTED/terminal for that ownership claim. RackAI does not automatically restore or reacquire it later. The application decides whether it still needs the service and explicitly requests it again.
6. A new acquisition blocked by an equal/higher-priority incumbent is UNAVAILABLE/denied-now, not queued work. Return an explicit reason and bounded `retry_after` guidance.
7. Identical failed acquisition attempts may be served from a short-lived decision cache so applications can poll/retry without creating excessive RackAI retained evidence or placement work. A materially changed request is a new decision.
8. Applications request logical services, never GPUs/models/placement. A logical service may map to one GPU or multiple GPUs and may change implementation without client changes.
9. Applications need not reserve every possible future service at startup. They may explicitly acquire and release/change service sets during their lifetime.
10. Some service sets are atomic. Example: a Paramount CB request for `{comfyui, local-primary, local-tts}` must not become Ready until the entire requested set is owned and ready.
11. Once a reservation is Ready, ordinary calls against it must not participate in global priority arbitration again. They execute only within that owner's reservation.
12. RackAI internal capacity/evidence accounting must not leak as routine application scheduling. A Ready reservation should be able to accept calls up to documented per-reservation queue/bounds. Capacity exhaustion of RackAI's own retained authority is an operational/control-plane limit, not a substitute for reservation arbitration.

## Required state distinctions

### Acquisition / ownership

- `preparing`
- `ready`
- `unavailable` / denied-now because of incumbent priority or safety
- `preempting`
- `preempted`
- `releasing`
- `released`
- `expired`
- `recovery_required`

### Calls within a Ready reservation

- `queued`
- `running`
- `completed`
- `cancelled`
- `failed`
- `uncertain`

`queued` means accepted work waiting behind other calls for the same current owner. It must never mean that the application does not own the service.

## Historical uncertainty and current ownership

A historical uncertainty is durable evidence, not a perpetual assertion that a GPU
is physically occupied. RackAI records historical start and invocation outcomes
separately from its conclusion about the effect that exists now.

RecoveryRequired is an automatically supervised, stop-only cleanup state for every
managed backend. It is not a permanent physical ownership state. RackAI closes
admission, cancels queued calls with `reservation_recovery_cleanup`, and waits
for known running calls to finish. Uncertain invocation outcomes remain durable.
Unknown workspace executor effects remain fenced until their absence is proven.

RackAI resolves effects using the exact reservation and generation: process boot
and start identity, transient unit and invocation identity, container ID, pinned
image, activation label and environment, or the bound media lease and backend
generation. It stops only attributed effects, closes native admission, cancels
owned recreation intents, and cleans owned sessions and jobs without replay.
It verifies process/container/unit disappearance and an empty configured GPU
allocation before atomically recording `current_effect=proven_absent` and
releasing the claim. Historical reasons, invocation identities and uncertain
results are preserved; the verified cleanup process is retained as evidence.

Missing, unreadable, foreign or contradictory ownership evidence retains the
claim and exposes `recovery_error`. Cleanup attempts are bounded and retried at
a two-second minimum interval so a temporarily unavailable probe or delayed
cleanup can recover. RackAI never recreates the old reservation or replays old
work. Capacity becomes available to a new explicit acquisition. A client renew
or release cannot bypass the recovery fence or replace the original failure.

A stopped receiver with stale recovery metadata can normalize itself only after
all unit, process, GPU and ownership checks pass and no foreign session or job
can recreate the effect. No offline authority migration or manual record deletion
is part of this recovery path.

## Preemption example

ATHBA Low owns `local-primary` and has one running call plus nine queued calls. CB Paramount requests an atomic set including `local-primary`.

RackAI must:

1. mark ATHBA ownership `preempting`;
2. reject new ATHBA calls for that service;
3. cancel the nine queued/not-started ATHBA calls with typed reason `reservation_superseded_by_higher_priority`;
4. allow the one running ATHBA call to finish and return its normal result;
5. transfer the service only after the running call drains safely;
6. make CB Ready only when its entire atomic requested set is ready;
7. leave ATHBA's displaced ownership terminal/preempted;
8. not silently reacquire or restart ATHBA work when CB later releases the service.

ATHBA owns the decision to resubmit cancelled semantic work or request the service again.

## Boundary examples

- ATHBA Low may own `local-primary` + `local-coder` while MusicVideoDirector owns `comfyui`.
- CB Paramount requesting `comfyui` + `local-primary` + `local-tts` supersedes the conflicting lower-priority reservations.
- `local-coder` may remain unaffected if CB does not require its resources.
- Future ATHBA `big-brain` is just another logical service from the client perspective even if RackAI implements it with two or three GPUs.

## Non-goals

- no per-call priority;
- no automatic restoration of preempted reservations;
- no application-visible GPU/model placement;
- no hidden replay of cancelled queued calls;
- no treating blocked acquisition as queued inference work;
- no companion-app changes in this PR until the RackAI contract and state machines are proven.

## Implementation target

Refactor the current RackAI runtime/reservation/invocation logic to preserve existing authentication, provenance, durable evidence and safe uncertainty handling while implementing the ownership rules above. Existing accepted/running work must remain fail-safe. Migration/compatibility behavior must be explicit and tested before production cutover.


## Implemented state machine

`ready` is the only state that can accept a new invocation. Submission assigns a
monotonic local queue position and a dispatch worker starts only the oldest queued
call for that logical service. Dispatch does not read or compare priority: the
reservation acquisition was the sole priority decision.

A new lower/equal acquisition returns `unavailable` with an `incumbent_priority`
or safety reason and a bounded `retry_after` hint. It creates neither an
invocation nor a retained demand. A short-lived cache is keyed by authenticated
caller plus the material logical-service request (not the caller's rotating
acquisition id) and the current relevant claim environment. The cache is bounded
to 128 entries, expires after two seconds, and is bypassed automatically when a
claim, incumbent generation, state, priority, or requested physical mapping
changes.

Higher priority acquisition first records the incumbent as `preempting`, cancels
only its `queued` invocations with the exact terminal error
`reservation_superseded_by_higher_priority`, and retains the incumbent's physical
claim while `running` work drains. Once all running work has a known terminal
outcome and the owned backend is stopped, its claim is removed, the old logical
ownership becomes terminal `preempted`, and the incoming claim transfers. An
`uncertain` invocation never proves a drain and therefore blocks transfer
fail-closed. The supervisor contains no reconsideration path for `preempted`:
release makes capacity available; only a client-created, new acquisition may use
it.

For a multi-service `reserve`, RackAI validates every logical service, its
physical overlap, and every incumbent before changing any claim. A refusal makes
the whole reservation `unavailable`. For a preemptible set, each member can stage
its own safe stop/start work, but a member is not published `ready` until every
member has a transferred claim, a started process, and an independent readiness
probe. This prevents partial-ready gateway access. Conversely, preempting one
member of an older multi-service reservation leaves its non-conflicting members
ready and usable.

## Capacity and evidence model

There are three intentional, independently bounded capacities:

1. `max_pending` and `max_pending_per_reservation` limit live queued/running
   calls. These are the queue contract for a Ready reservation.
2. `max_calls_per_reservation` limits the number of durable idempotency receipts
   admitted over one reservation lifetime. It is explicit client-visible call
   capacity, not a global scheduling or evidence-storage fallback.
3. `terminal_evidence_bytes` bounds full terminal result payloads. Older
   terminal payload bodies are compacted to durable SHA-256 receipt fields while
   retaining the invocation identity, request, terminal state, error, and digest
   for reconciliation. Queued, running, and uncertain records are never
   compacted. `retention_admission_bytes` then reserves only active response
   headroom and compact control data.

This prevents completed historical output from causing
`capacity_retained_evidence` for ordinary calls under an already Ready
reservation. A compacted completed result deliberately reports no original body;
the digest proves which retained body was compacted and prevents RackAI from
pretending it can reproduce an output it no longer retains. The authority's
existing 32 MiB hard write bound remains the final fail-safe corruption/storage
fence.

## Compatibility and cutover migration

No historical authority is deleted or recreated. Deserialization translates only
legacy spelling while preserving durable identities and evidence:

| Legacy persisted value | v2 interpretation |
| --- | --- |
| `denied` | `unavailable` |
| `draining` | `preempting` |
| `held` | terminal `preempted`; never a restoration candidate |
| invocation `accepted` | `queued` |
| invocation `started` | `running` |

On receiver recovery, any persisted running invocation is already changed to
`uncertain` by the existing restart fence, so a legacy or v2 preempting transfer
cannot occur until an operator-supported reconciliation establishes safety. A
legacy `held` record is intentionally not reacquired. Existing claim maps,
request identities, invocation evidence, workspace scopes, and unknown work stay
in the authority; no migration clears them. Before a production cutover, inspect
those records, reconcile unresolved Started/Uncertain work, and require an
operator-approved deployment window. This PR does not deploy the receiver.

## Qualification scope

`tests/runtime/test_reservation_ownership_v2.py` uses only RackAI's disposable
fixture receiver. It proves one running plus nine queued calls, typed queued
cancellation, safe drain before Ready, no automatic restoration, explicit
reacquisition, atomic all-or-unavailable membership, unaffected logical service
ownership, decision-cache invalidation, and terminal evidence compaction. It
does not call ATHBA, CB, or a live RackAI workload.
