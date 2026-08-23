# PR12 — Adaptive Multi-Worker Scheduling and Controlled Parallelism

## Status

This is a post-PR11 implementation contract. It revives the useful design intent from superseded PR5, but implementation must be based on the proven PR7–PR11 interfaces and evidence model rather than copied mechanically from PR5.

Do not implement before PR9 has passed and the recovery/escalation/research foundations are stable.

## Goal

Use the rack's available local model roles more intelligently by assigning work according to capability, dependency and current execution state, while preserving deterministic authority, review independence and controlled integration.

The objective is throughput and robustness after sequential autonomy is already proven. Parallelism must not be used to hide weak planning/recovery semantics.

## Required architectural principles

1. Rack AI remains the sole orchestration authority.
2. Every worker operates under explicit bounded task authority.
3. Parallel work must use isolated worktrees/branches or equivalent controlled isolation.
4. Deterministic acceptance and independent semantic review remain mandatory.
5. PR7 recovery, PR10 escalation and PR11 research are reused; do not invent parallel variants.
6. Integration is a first-class controlled phase, not an uncontrolled merge of worker outputs.

## Capability-aware worker model

Represent registered workers/capabilities explicitly enough to reason about:

- worker/model identity;
- endpoint/runtime role;
- implementation vs reasoning/review suitability;
- supported tool capabilities;
- context/resource limits;
- current availability/busy state;
- optional cost/latency preference for local resources;
- health/degraded state.

Initial known roles include `local-coder` and `local-primary`, but the design must not hard-code exactly two workers.

## Scheduling inputs

Scheduling decisions should consider at least:

- task capability requirements;
- dependency graph/readiness;
- write-path overlap/conflict risk;
- previous failure/recovery classifications;
- whether stronger local reasoning has been requested;
- worker health/availability;
- configured concurrency/resource limits;
- review independence constraints.

Do not schedule work merely because a GPU is idle if dependency or conflict evidence says the work is not safely parallelisable.

## Work decomposition relationship

PR12 does not require arbitrary objective-to-campaign planning. It may schedule a predeclared/dependency-aware campaign or a validated plan produced by a later planner.

The scheduler must operate on explicit bounded work units with declared authority and dependencies.

## Parallel isolation

Concurrent implementation attempts must not share a mutable worktree.

Use isolated worktrees/branches or another existing Rack AI-controlled mechanism so that:

- workers cannot overwrite one another's changes;
- each attempt has independent Git evidence;
- path-policy checks apply to each unit;
- integration can inspect exact commits/diffs;
- failed work can be discarded without contaminating accepted work.

## Controlled integration

Add an explicit integration stage for accepted parallel work.

Integration must:

- respect declared dependencies/order;
- detect conflicting/overlapping changes;
- perform deterministic validation after combining work;
- invoke semantic review on the integrated state where required;
- never silently resolve semantic conflicts by taking one worker's version;
- route integration failures through PR7 recovery semantics;
- retain provenance linking integrated commits to originating attempts/workers.

## Review independence

A worker that authored a change must not simply self-approve it using its authoring context.

If model capacity means the same model weights must review work, the review must be a fresh isolated invocation with evidence only, following the existing independent-review contract.

## Adaptive reassignment

The scheduler may consume PR7/PR10 signals to reassign work, for example:

- local-coder -> local-primary for a diagnosed complexity/capability issue;
- defer/requeue when a required worker is unavailable;
- serialize previously parallel work when a conflict/dependency is discovered.

Reassignment remains bounded and must not widen task authority.

## Resource/concurrency controls

Configuration must explicitly bound:

- maximum concurrent workers;
- per-worker concurrency;
- GPU/resource admission where relevant;
- task/attempt timeouts;
- queue size or campaign-level active work;
- integration retries.

The system must remain useful in sequential mode when configured concurrency is one.

## Failure handling

Test and preserve correct behaviour for:

- worker unavailable;
- worker/model timeout;
- one parallel branch fails while another succeeds;
- dependency becomes invalid after upstream change;
- merge/integration conflict;
- deterministic acceptance failure after integration;
- semantic rejection after integration;
- campaign pause/cancel while multiple work units are active;
- restart/recovery with active isolated worktrees.

## Tests

Add deterministic tests for at least:

1. two independent disjoint-path work units execute concurrently and integrate successfully;
2. dependent work units remain ordered;
3. overlapping write scopes are serialized or explicitly conflict-blocked;
4. one worker failure does not corrupt accepted parallel work;
5. integration failure enters PR7 diagnosis/recovery rather than ad hoc retry;
6. capability-aware assignment chooses a stronger worker when required by policy/evidence;
7. final integrated acceptance and independent review are required before promotion;
8. restart reconstructs scheduler/integration state correctly;
9. pause/cancel stops new dispatch and safely settles active work;
10. concurrency=1 preserves correct sequential behaviour.

Add an opt-in live-rack proof demonstrating useful concurrent use of the available local model endpoints on genuinely independent bounded work.

## PR5 relationship

Review the closed PR5 design document for useful ideas around worker capability, isolated worktrees, dependency-aware batches, integration and multi-GPU utilisation.

However, PR5 is superseded. Where it conflicts with proven PR7–PR11 contracts, the newer architecture wins.

## Explicit non-goals

Do not add:

- uncontrolled swarm behaviour;
- reliance on broken JCode swarm provider rebinding;
- automatic cloud workers;
- hidden shared mutable worktrees;
- weakening deterministic gates for speed;
- automatic authority expansion;
- arbitrary objective planning unless separately implemented;
- database/vector platform unless clearly required by existing durable-state design.

## Merge gate

PR12 may merge only when:

- sequential autonomy remains fully functional;
- PR7 recovery/PR10 escalation/PR11 research interfaces are reused;
- isolated parallel execution is proven deterministic;
- integration has explicit acceptance and review;
- conflict/dependency tests pass;
- restart/pause/cancel safety remains green;
- live evidence demonstrates that parallelism increases useful rack utilisation without weakening authority or acceptance.
