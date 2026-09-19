# PR40 — Warm backend residency cache

## Status

Focused design and implementation contract for RackAI. This PR begins as documentation-only so the ownership boundary is explicit before code is changed.

Base: `main` at `03ea2f7db5f2e1b009da00eae23b15be0b1b0e0d`.

## Trigger

A live ATHBA PR31 qualification on 2026-09-19 proved that a cold `local-primary` activation can take several minutes. ATHBA released its reservation before readiness, and RackAI correctly stopped the owned Docker/vLLM backends. The next reservation would therefore pay the entire cold-start cost again.

The rack's current applications commonly request the same logical services repeatedly. In particular, `local-primary` on the RTX 4060 Ti and interactive ComfyUI on the RTX 4080 Super are expected to be reused by multiple applications. Tying backend lifetime exactly to client reservation lifetime creates unnecessary unload/reload churn.

## Decision

Separate **client ownership** from **backend residency**.

A reservation still grants exclusive client ownership and authenticated access exactly as today. Releasing, expiring or otherwise ending the reservation must close that access immediately and terminalize the reservation.

After a safe normal retirement, RackAI may keep the verified backend process resident as an **unowned warm cache entry**. The cached backend is an internal optimisation only.

A warm backend:

- is not a reservation;
- has no client owner;
- has no priority;
- exposes no scoped gateway/session capability;
- must never accept client work without a fresh Ready reservation;
- must never block a real reservation;
- may be reused only by an exactly compatible logical profile after fresh verification;
- must be evicted automatically when an incompatible reservation needs its resources;
- must be stopped instead of cached whenever outcome/ownership/health is uncertain.

Clients must not know whether a reservation was satisfied by a cold start or warm adoption.

## Core invariants

### 1. Reservation termination remains authoritative

Explicit release/cancel, idle expiry, TTL expiry and preemption continue to close admission under the existing reservation state machine.

No cached backend can keep an old reservation Ready or resurrect it.

Every new client use requires a new acquisition decision, reservation generation and access capability.

### 2. Cached residency is not ownership

Current resource claims represent active reservation authority. Warm residency must not masquerade as an incumbent reservation or participate in client priority comparison.

A cached backend has no scheduling priority. Any valid incoming reservation may evict it when the incoming profile is incompatible.

### 3. Same-profile acquisition should adopt rather than restart

If a new acquisition requests a profile whose exact verified backend is already resident on the exact required resources, RackAI should:

1. make the ordinary acquisition/priority decision;
2. claim the required resources for the new reservation;
3. verify the cached process/container/systemd identity, executable/image/profile binding, endpoint ownership, model identity and GPU placement;
4. bind the resident backend to the new reservation's internal activation relationship without exposing the old client's identity;
5. perform readiness verification;
6. publish the new reservation Ready under a fresh reservation generation/access key.

The backend process may remain the same. The client reservation identity must be new.

### 4. Backend identity must be separate from reservation identity

Today hosted process/container identity is tied to `Demand.generation` through `RACK_RUNTIME_ACTIVATION`, Docker labels and systemd unit names.

Do not weaken those checks or simply hand an old reservation process to a new owner.

Introduce the smallest explicit backend/residency identity needed to distinguish:

- client reservation generation/access identity; from
- physical backend process/container residency identity.

Process verification must remain exact.

### 5. Conflicting acquisition evicts cache

If a reservation needs resources occupied by an incompatible warm backend:

- the cache is not an incumbent and cannot deny the acquisition on priority grounds;
- RackAI fences the resources against concurrent adoption/start;
- stops only the verified cached backend;
- proves process/container/systemd disappearance and GPU reclamation;
- then performs the ordinary activation for the incoming reservation.

Failure to prove eviction must fail closed through existing recovery semantics.

### 6. Unsafe outcomes are never cached

Do not retain a backend warm after:

- `recovery_required`;
- unknown start outcome;
- uncertain ownership/process/container identity;
- uncertain running invocation;
- failed health/readiness verification;
- contradictory GPU/process evidence;
- preemption cleanup where safe transfer requires stopping the old backend;
- administrator/runtime shutdown when continued residency cannot be proven.

Normal explicit release and ordinary idle retirement are the primary cache candidates.

Cancellation may cache only if implementation can prove no uncertain work/effect remains; otherwise stop conservatively.

### 7. Warm cache cannot receive work directly

All inference, workspace, speech and native-media admission must still verify a current Ready reservation and current client capability.

Raw backend ports remain private.

There is no public "use cached model" API.

### 8. Atomic reservations remain atomic

For multi-service reservations, warm adoption is just another internal preparation path.

The group becomes Ready only when every requested member has successfully reached the existing atomic Ready publication barrier.

A warm local-primary must not make half of a new atomic group externally usable while another member cold-starts.

## Residency record

Implement the smallest durable internal representation that safely records warm backend state.

The record should be sufficient to prove at least:

- unique residency/backend identity;
- exact profile tag/version/hash;
- exact physical resources;
- process evidence;
- container/systemd identity where applicable;
- endpoint;
- time cached;
- latest successful readiness/health observation;
- lifecycle state such as resident/evicting/recovery-required.

Do not copy client access keys or credentials into residency state.

Do not store a fake client owner.

Persist residency under the canonical RackAI authority or another equally serialized RackAI-owned authority only if atomic resource fencing remains provable. Do not create a second competing ownership system.

## Resource accounting

The scheduler must distinguish:

- actively claimed resources;
- physically occupied warm-cache resources;
- actually free resources.

Warm residency physically consumes VRAM/host memory even though it owns no client reservation.

Host/GPU capacity checks must account for that occupancy.

At the same time, warm residency is always lower precedence than a real compatible or conflicting acquisition.

Concurrent acquisition/adoption/eviction must serialize so two clients cannot both adopt or replace the same resident backend.

## Cache lifetime

Initial implementation should be **pressure-evicted**, not client-TTL-driven.

A healthy cached backend may remain resident until one of these occurs:

- a conflicting real reservation needs its resources;
- backend health fails;
- profile/configuration changes make it incompatible;
- RackAI startup/recovery cannot re-establish trusted identity;
- an explicit operator/cache-maintenance action evicts it;
- optional future cache-age policy is introduced.

Do not reuse the existing reservation idle timeout as an automatic warm-cache destruction timer in the first implementation. The purpose of this feature is specifically to avoid immediate unload/reload churn after reservation inactivity or release.

A future bounded cache-age/power policy may be added separately.

## Profile compatibility

Warm adoption requires exact compatibility, not merely the same broad capability.

At minimum compare the frozen profile properties that determine runtime identity and correctness, including:

- profile tag/version/hash;
- backend/driver;
- model identity;
- executable/hash;
- container image and mounts;
- launch arguments;
- endpoint;
- GPU/resource mapping;
- relevant artifact/config hashes;
- protocol/qualification binding.

If compatibility cannot be proven, evict and cold-start.

## ComfyUI

The same architectural optimisation should be usable for the qualified interactive ComfyUI service on the RTX 4080 Super.

However, cached backend residency must remain separate from client native-session ownership.

Ending a reservation/session must revoke the old client's native admission immediately. A later reservation may reuse a still-running verified ComfyUI backend only after establishing a new authenticated reservation/session boundary.

Do not weaken the existing native queue/admission barrier.

If the current media architecture cannot safely separate backend process residency from session ownership in this PR, implement the generic residency layer for managed model backends first and document the precise remaining ComfyUI integration step. Do not fake support.

## Expected immediate benefit

For repeated requests to `local-primary`:

First request:

`cold start -> Ready -> work -> release -> cached resident`

Next compatible request:

`acquire -> verify/adopt resident backend -> Ready -> work`

rather than:

`acquire -> reload model -> compile/warm up -> Ready`

The same principle should ultimately apply to interactive ComfyUI where its native ownership boundary permits it safely.

## Public contract

No client API change is required.

Existing operations and states remain authoritative:

- reserve/acquire;
- inspect;
- submit work/infer/native access;
- release/cancel;
- preemption;
- recovery.

Clients neither request nor observe residency policy as a scheduling primitive.

Optional inspection may expose non-secret diagnostic evidence that a reservation used a warm backend, but this is not required for client correctness.

## Implementation areas to inspect

Likely relevant modules include:

- `retirement.rs`;
- `hosting.rs`;
- `container.rs`;
- `process.rs`;
- `preflight.rs`;
- `planner.rs`;
- `transition.rs`;
- `backend.rs`;
- `reservation.rs`;
- `supervisor.rs`;
- `recovery.rs`;
- media/native ComfyUI lifecycle modules;
- canonical authority types/schema/tests.

Trace actual dependencies before editing.

## Required proof

Use fixture/disposable backends first.

At minimum prove:

1. normal release closes the old reservation immediately while retaining an eligible backend resident;
2. old scoped access cannot dispatch after release even though the backend remains alive;
3. a new same-profile reservation adopts the resident backend without starting a second backend;
4. the new reservation has a fresh reservation generation/access capability;
5. exact process/container identity and model readiness are reverified before Ready;
6. incompatible profile acquisition evicts the cache, proves GPU cleanup, then starts the new backend;
7. cache eviction has no priority and cannot deny a valid reservation;
8. concurrent acquisitions cannot double-adopt/double-start;
9. uncertain/recovery-required outcomes are never cached;
10. receiver restart either safely reconciles the resident backend or fails closed/evicts it;
11. atomic multi-service Ready publication remains atomic with a mix of warm and cold members;
12. release/adopt cycles preserve existing durable invocation evidence and never replay work.

Then perform a bounded live qualification on gpurack for `local-primary`:

- cold acquire;
- prove Ready;
- release;
- prove the backend/container remains resident but old reservation access is closed;
- acquire `local-primary` again under a fresh reservation;
- prove it reaches Ready through warm adoption without recreating/reloading the container;
- perform one harmless inference;
- release cleanly.

If safe ComfyUI support is implemented in this PR, separately prove the equivalent native-session boundary without weakening its admission gate.

## Non-goals

- no client application changes;
- no ATHBA/CB/Music Video Director semantics in RackAI;
- no raw endpoint exposure;
- no model change;
- no GPU purchase/configuration change;
- no weakening of preemption or recovery;
- no automatic replay;
- no persistent client reservation merely to keep a model warm;
- no broad scheduler rewrite;
- no speculative model-placement optimisation unrelated to residency.

## Deployment rule

Do not deploy this feature merely because unit tests pass.

Implementation must first preserve all existing reservation/preemption/recovery tests and complete the bounded live adoption proof.

Production cutover remains an explicit operator step after review.
