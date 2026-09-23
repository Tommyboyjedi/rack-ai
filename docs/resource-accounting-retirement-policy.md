# Resource accounting: approved history retirement and active-call follow-up

## Decision status and scope

- Decision date: **2026-09-23**.
- **Historical accounting resolution: APPROVED by the operator. Implement this first.**
- **Active-call accounting: separate required follow-up. Its replacement design is not yet approved; do not implement it in the historical-retirement change.**
- This document records a required change, not a claim that it has been implemented, tested, merged, or deployed.

The approved historical lifecycle is:

```text
work finishes and reservation cleanup is verified
  -> retire working history from active accounting immediately
  -> retain an owner-scoped, separately stored archive for 14 days
  -> expire unpinned operational history automatically
```

Fourteen days is the troubleshooting/reconciliation retention window. It is **not** a delay before execution capacity is returned.

For this subject, this decision supersedes older prose requiring all historical invocation data to remain indefinitely in the live authority document. It does not supersede authentication, execution fencing, path safety, truthful outcomes, or unresolved-recovery evidence requirements. Operational API changes must be documented and qualified with the implementation; this documentation PR alone changes no deployed contract.

## 1. Problem and evidence baseline

The inspected repository baseline is `78705c09a30cecf2b413a20d34f0d353dfc86a50`.

At that baseline, [capacity::retention](../crates/rack_ai_runtime/src/capacity.rs) serializes the whole authority document and adds future output allowances for queued/running invocations:

```text
serialized authority bytes
+ SUM(response_bytes * 6 + 16384 for each queued/running invocation)
<= retention_admission_bytes
```

The approved response allowance is 3 MiB (`3145728` bytes), while the admission ceiling is 30 MiB (`31457280` bytes). A model invocation reserves 18 MiB plus 16 KiB before retained state is counted. Two such invocation allowances already exceed 30 MiB without any history. These are accounting reservations, not measured occupied GPU memory.

Terminal request compaction reduced payload size, but retained invocation/reservation metadata still contributes to the live document. Requests and results of historically `uncertain` invocations are excluded from the terminal compaction passes. Safe physical recovery does not by itself remove that historical data.

There are therefore two separate defects/gaps to address:

1. **Historical accumulation:** completed, unrelated work continues consuming active admission headroom.
2. **Active-call accounting:** even an empty history does not make the existing future-output reservation arithmetic suitable for all intended execution concurrency.

The first is resolved by the approved design below. The second remains the next task; clearing history must not be presented as fixing it.

Retained execution evidence is not automatically appended to another application's model prompt. This decision addresses shared storage/admission interference, not model conversation context.

## 2. Required separation

| Storage/accounting area | Purpose | Lifetime |
| --- | --- | --- |
| Active authority | Current ownership, live execution, dispatch fences, pending recovery and necessary bounded coordination | Only while operationally needed |
| Historical archive and its lookup index | Retired identities, outcomes, result/evidence references, replay/reconciliation and diagnostics | Through reservation closure plus 14 days, subject to the explicit protections below |
| Application assets | Accepted code, repositories, models, client-owned state and durable product artifacts | Not governed by this diagnostic-history policy |

Historical payloads, closure receipts, tombstones and archive indexes must not accumulate in the document whose serialized size controls active admission. Moving only response bodies while retaining an unbounded list of small historical entries there is not an acceptable implementation.

Archive access remains owner-scoped. Use bounded, targeted lookup: do not load all archives into memory or scan/re-serialize all history for each submission. Separate files or an indexed repository are implementation choices; do not introduce another scheduler or execution authority.

## 3. Approved historical lifecycle

### 3.1 Verified closure, not release acknowledgement

Retirement eligibility follows verified reservation closure and reconciliation, not an HTTP 200 release acknowledgement or a caller-side timeout.

Use the existing ownership and recovery proofs to establish that no queued/running work, executable workspace scope, outstanding mutating callback, or unresolved physical effect remains attributable to the retiring reservation. An atomic group cannot be fully retired while a member still requires live coordination. Preserve the necessary bounded links where another active record depends on them.

Normal verified release may leave a legitimately transferred warm backend resident. Archival must not force warm-cache eviction or stop another reservation's backend. For uncertain executions, the existing `current_effect=proven_absent` recovery evidence is the relevant proof; do not invent cleanup or manually clear claims.

At eligibility, schedule retirement immediately through the normal bounded runtime lifecycle. Do not wait 14 days, wait for capacity refusal, or require an operator reset. A temporary archival I/O failure must be visible and retryable; it must not be hidden as successful retirement.

### 3.2 What retires

Archive the reservation's retired invocation records and associated request/response payloads, closure/recovery receipts, scopes, submission identities and packet references that are no longer required in active authority. Remove those historical entries from active accounting after the archive is durable and discoverable.

Keep enough archived information to:

- authenticate the owner and identify the original reservation/generation/profile;
- compare original request/work identity, including durable digests where payloads were already compacted;
- return the original retained outcome/result, or accurately report that older payload content was already unavailable;
- explain cancellation and proven cleanup without rewriting historical uncertainty;
- locate retained workspace packets and provenance needed for client reconciliation.

A digest proves identity; it cannot reconstruct a discarded result. Do not fabricate missing historical results during migration.

### 3.3 Long-running reservations

Safely completed calls must retire during long-running reservations too. A client must not need to release/reacquire merely to drain historical payloads or invocation records out of active accounting.

Preserve any information still needed by an active parent workspace, callback fence or recovery operation before retiring a child record. Public result and same-identity replay lookups must resolve the archived call while its reservation remains open.

For calls archived before reservation closure, retain their reconciliation history through the reservation lifetime and then for 14 days after verified closure. This prevents an open reservation from losing its retry history. Such archives remain outside active admission; a long-lived reservation does not imply unlimited disk capacity.

### 3.4 Fixed 14-day expiration

Persist a trustworthy `closed_at` for the verified closure and derive expiration as `closed_at + 14 days` (14 * 24 * 60 * 60 seconds). Reads, inspection, retries, restarts and archive relocation must not refresh it.

Delete expired, unpinned operational archives and their historical indexes through an automatic, bounded sweep. Record sweep health, pending retirement, archive bytes and deletion failures using the existing operational reporting surface. Fourteen days bounds age after closure, not maximum storage consumption; monitor actual free disk and archive growth without silently introducing a new per-client quota or reducing approved retention.

Explicit operator preservation is an exception with a recorded reason and owner. Preserved evidence stays outside active accounting. Do not pin every failed run automatically. Expiry must not delete data still referenced by live execution or necessary recovery; expose those dependencies rather than silently retaining everything indefinitely.

Historical `uncertain` is not a perpetual retention exemption once physical cleanup and callback fencing are proven. Archive and expire it with its original outcome intact.

## 4. Replay, lookup and crash safety

### 4.1 Stable owner-scoped lookup

Submission replay, work/invocation inspection, result retrieval and reservation inspection must locate either the active record or its retained archive. Identical requests reconcile without another dispatch; changed same-identity requests still conflict during the supported retention window. Cross-owner requests remain denied.

Retiring completed calls must not make `find()` miss a prior work ID and dispatch it again. In particular, audit [work submission/inspection](../crates/rack_ai_runtime/src/work.rs), [inference replay](../crates/rack_ai_runtime/src/inference.rs), scoped gateway replay and reservation acquisition replay rather than only changing the retention loop.

Existing published packet references must remain usable for the promised retention window. Do not break client readers by moving or deleting their packet files without compatible resolution. Data still required by an application needs explicit preservation/export; do not silently reclassify application assets as expiring diagnostics.

### 4.2 Expired history is never execution authority

An expired or unknown server-issued reservation ID or scoped capability must not recreate a reservation, reopen a scope, dispatch work or turn an archive lookup miss into success. Return a documented non-executing unavailable/expired result; do not claim a past outcome that has been deleted.

The 14-day policy is a finite reconciliation guarantee, not a promise to recognize arbitrary client-supplied idempotency keys forever. Specify and test the expiry/namespace rules for both acquisition and work replay. A deliberately new request is not a delayed replay. Do not quietly widen existing identity scopes or solve this by keeping an ever-growing in-memory tombstone set. Any necessary public identity/expiry contract change must be explicit and separately reviewed before deployment.

### 4.3 Durable handoff

Persist and verify the archive and lookup mapping before removing the live record. The live transition and archive publication must be recoverable across interruption using the repository's established durable-write mechanisms.

At every restart boundary, lookup must recover the original identity/outcome or fail closed without dispatch. Duplicate physical copies during a recoverable handoff are acceptable; conflicting execution authority or losing the only receipt is not. Expiry must respect concurrent readers and active references. Never return an HTTP success for an archive write/deletion that did not durably complete.

Do not remove active safety reservations, suppress I/O errors, purge `managed.json`, or rewrite uncertain work as completed in order to demonstrate space reclamation.

## 5. Existing-state migration

This applies to **existing historical data**, not only calls completed after deployment.

Provide a restart-safe migration/retirement path over the current authority state with a checksum-verified backup and before/after counts and byte accounting. Eligible existing closed records should retire without manual history edits or a fresh empty authority.

Use evidenced historical closure times when available. For legacy records without a trustworthy closure time, record the first newly verified closure during migration, explicitly identifying that timestamp source; do not invent an old date or expire evidence prematurely. Never reset a previously established expiry on later migration passes.

Preserve existing compacted digests, ownership, outcome, packet and recovery evidence. Data required by still-active records is not eligible until those dependencies have been safely retained or resolved. Backups and operator-preserved evidence remain outside active admission and have explicit operational ownership; do not create a hidden unbounded second archive.

## 6. Historical implementation acceptance

Use deterministic fixtures, realistic old-format history, an injected clock and existing public runtime operations. A single clean-state smoke is insufficient.

Required evidence:

1. Repeated sequential reservations for different owners and projects finish and retire; active-state bytes and headroom return to a stable bounded baseline instead of growing with lifetime call count.
2. A long-lived reservation archives completed calls without losing results or replay identity and without retaining a lifetime list of invocations in active accounting.
3. A retired request replays once without redispatch; changed payloads conflict; another owner cannot read it. Exercise work, raw inference, scoped gateway and reservation replay paths.
4. Interrupt archive publication, lookup handoff and active removal; restart and prove no receipt loss, duplicate execution or resurrection.
5. Just before and after 14 days, clock-driven tests prove the promised lookup/expiry behavior. Reads and restarts do not extend expiry. Expired references cannot execute again.
6. Unresolved physical work remains protected. A historically uncertain but proven-absent operation retires and expires without altering its outcome. Warm residency and unrelated active work remain intact.
7. Protected application assets and explicitly preserved/actively referenced evidence survive expiry. Expired ordinary indexes and payloads are actually removed.
8. Existing production-format state migrates without purge; missing historical payloads are reported honestly; archival I/O failure and full-disk behavior remain safe and observable.

Run the focused tests, applicable runtime/contract tests, `cargo test --workspace --offline`, formatting/checks and `git diff --check`. Qualify migration first on a copy of real state. Report deployed behavior only if separately authorized deployment and live verification actually occurred.

The success claim is narrowly: **history-driven active-state growth is eliminated**. Remaining 429s caused solely by simultaneously active reservations must be reported as the next workstream, not hidden by reducing capabilities.

## 7. Active-call accounting: required next change, design deferred

The operator explicitly deferred this work until after historical accounting. Record the problem now; keep the existing active-call arithmetic and approved numerical limits unchanged in the historical implementation.

The next design must establish:

- distinct, explainable budgets for actual execution resources, queued request storage, response working/storage capacity and historical artifacts;
- a qualified workspace parent/child path that cannot reserve away the resources its own child needs;
- documented service concurrency supported by the combined budgets, including independently placed services;
- safe headroom for completion, cancellation and recovery, not just admission;
- capacity errors that identify the actual exhausted resource rather than disguising historical storage pressure as compute contention;
- restart/replay correctness and tests of intended concurrency with historical state already retired.

No new replacement formula, byte ceiling, per-turn token allowance, queue quota, storage engine or streaming representation is approved here. In particular, do not "fix" the active 18-MiB-per-call reservation by changing the agreed 3 MiB response allowance or by silently dropping the expansion reservation. Design and qualify the replacement separately.

Historical retirement does not claim to fix oversized backend wire responses, excessive model generation, tool compatibility or cancellation latency. Those failures must retain their true classifications.

## 8. Change control and implementation sequence

1. Keep this PR documentation-only and open for review; it records the operator-approved historical policy.
2. Implement historical retirement, archived lookup, expiry and old-state migration in a separate focused implementation PR referencing this decision.
3. Demonstrate the historical acceptance criteria without changing model settings, tool profiles, token limits, workspace timeouts, active-call reservation arithmetic or companion applications.
4. Address active-call accounting in the following design/implementation task. Do not rerun a broad application campaign merely to rediscover the already documented active-call constraint.

Changing token/response/time limits is not a substitute for diagnosing execution or accounting defects. Changing operator-approved limits requires explicit operator approval.

Related contracts: [reservations and work](reservation-work.md), [ownership and preemption](reservation-ownership-preemption-v2.md), [runtime public contract](runtime-public-contract.md), and [generic workspace execution](generic-bounded-workspace-execution.md).
