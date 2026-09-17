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
