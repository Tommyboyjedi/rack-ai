# Adaptive Multi-Worker Execution — Design Specification

> **Status:** Draft design only. Do not implement from this document yet.
>
> This specification records the current agreed direction for the next major Rack AI execution model after P1. It is intentionally expected to change after real post-P1 usage and utilisation testing.

## Purpose

Rack AI currently uses an intentionally asymmetric execution model:

- `local-primary` on the RTX 4060 Ti acts mainly as coordinator, planner, verifier, semantic reviewer, and fallback implementer.
- `local-coder` on the RTX 2060 performs most implementation work.

That architecture was the correct choice for proving safe autonomous execution because it kept the trust model simple and made independent review explicit.

However, it under-utilises the stronger GPU and model during long implementation phases. The 2060 can be busy for most of a campaign while the 4060 Ti is idle between planning and review cycles.

The next logical evolution is therefore not simply "use the 4060 more". It is to make Rack AI capable of **adaptive, dependency-aware multi-worker execution** across all available model workers while preserving the safety and review guarantees established by P0/P1.

## Core Principle

Rack AI should schedule work by **capability, dependency, and availability**, not by permanently assigning one model to coding and another model to management.

A strong model may act as coordinator at one moment and as an implementation worker at another.

The intended high-level cycle is:

1. `local-primary` understands the overall goal.
2. It decomposes the goal into bounded, dependency-aware work batches.
3. Ready batches are allocated across available capable workers.
4. The 4060 Ti and 2060 may implement different batches concurrently in isolated worktrees.
5. Completed batches pass deterministic gates.
6. `local-primary` returns to coordinator/reviewer mode and independently reviews the completed work.
7. Accepted work proceeds to controlled integration.
8. Rejected work is repaired, reassigned, or escalated according to bounded policy.
9. The coordinator reassesses the plan and releases the next ready batches.
10. The process repeats until the campaign goal is complete or blocked.

## Why This Matters

The current model optimises primarily for safety and simplicity.

The adaptive model should additionally optimise for:

- GPU utilisation
- wall-clock completion time
- stronger-model attention on difficult work
- parallelism where dependencies allow it
- bounded escalation from weaker to stronger workers
- future expansion to additional GPUs/models without redesigning the orchestration model

The expected benefit is not merely faster coding. It is better allocation of reasoning capability across heterogeneous hardware.

## Worker Model

Workers should eventually be described by capabilities rather than fixed job titles.

Conceptually:

```text
local-primary
  reasoning: high
  planning: high
  coding: high
  review: high
  availability: dynamic

local-coder
  reasoning: moderate
  planning: limited
  coding: good
  review: moderate/limited
  availability: dynamic
```

The exact capability model is not yet specified and should be refined after measurements from the current P0/P1 architecture.

The scheduler should eventually answer:

> What work is ready, what resources are idle, and which available worker is sufficiently capable to perform each batch?

rather than:

> Is this coding work? Send it to `local-coder`.

## Coordinator and Worker Role Switching

`local-primary` should not remain permanently reserved for coordination.

When planning/coordination work is complete and there are independent ready batches, the stronger model should be eligible to act as an implementation worker.

When implementation batches finish, `local-primary` can return to coordinator/reviewer mode.

No model reload is required merely to change logical role. Role is an orchestration responsibility, not necessarily a different model instance.

This produces a repeating pattern:

```text
PLAN
  |
  +--> Batch A --> local-primary implementation
  |
  +--> Batch B --> local-coder implementation
  |
  v
GATES
  |
  v
COORDINATOR REVIEW
  |
  +--> accept/integrate
  +--> repair/reassign/escalate
  |
  v
REPLAN / RELEASE NEXT BATCHES
```

## Dependency-Aware Planning

Parallelism must be based on a DAG, not a flat task list.

The planner must distinguish between:

- independent batches that may execute concurrently
- batches that depend on accepted output from earlier batches
- batches that share files or integration boundaries and therefore should not run concurrently without an explicit merge strategy

Example of sensible parallel work:

- implement parser
- implement independent persistence adapter
- add tests against an already-defined interface

Example of work that may require ordering:

- change a core domain model
- implement consumers that depend on the new model

The planner should release only dependency-ready nodes.

## Isolated Worktrees

Concurrent implementation must never mean concurrent mutation of the same checkout.

Each implementation batch should operate in an isolated Git worktree based on a known accepted/base SHA.

Conceptually:

```text
campaign accepted base
    |
    +--> worktree/batch-a --> worker A
    |
    +--> worktree/batch-b --> worker B
```

Each worker produces its own evidence and local commit(s).

The existing rootless Podman mutation boundary remains mandatory.

## Integration Stage

Parallel implementation introduces a new first-class concern: integration.

Accepted batches must not simply be copied into the target repository in arbitrary completion order.

A future implementation needs an explicit integration stage that can:

- determine whether accepted batches are compatible
- apply/merge accepted commits in a controlled order
- detect conflicts
- rerun relevant deterministic checks after integration
- preserve provenance from each originating batch
- fail closed on ambiguous or conflicting integration

The exact integration mechanism is deliberately not fixed yet.

Potential approaches may include cherry-picking accepted commits, constructing an integration worktree, or another Git-native mechanism. This should be chosen after experimentation rather than assumed in advance.

## Complexity-Aware Scheduling

The stronger model should not automatically receive every ready task.

Routine work can be allocated to the smaller worker while difficult work is allocated to the stronger worker.

Examples of likely `local-coder` work:

- routine tests
- narrow mechanical changes
- straightforward typed data structures
- small bounded implementations with clear contracts

Examples of likely `local-primary` implementation work:

- difficult concurrency logic
- architectural changes
- subtle debugging
- state-machine design
- tasks with high ambiguity or high reasoning requirements

The exact classification mechanism remains open.

Possible inputs include:

- planner-assigned complexity
- file/scope size
- architectural risk
- previous attempt history
- rejection count
- worker performance history
- required domain/context depth

## Escalation

The scheduler should support bounded escalation.

A weaker worker failing does not necessarily mean the entire campaign should stop.

A possible policy is:

```text
local-coder attempt
    |
    v
review rejection
    |
    +--> bounded local-coder repair
    |
    v
second rejection / complexity escalation
    |
    v
local-primary implementation attempt
```

This is only an illustrative policy. Exact retry and escalation limits should be based on observed failure rates and the existing campaign safety contract.

All retries and escalations remain bounded.

## Review Independence

Every implementation attempt still requires:

1. deterministic gates
2. fresh semantic coordinator review

This remains true even when `local-primary` authored the batch.

When the same endpoint performs implementation and later review, review must be a fresh invocation with no authoring conversation context and only the evidence required for review.

The review invocation should receive:

- task/batch contract
- diff or resulting changes
- deterministic evidence
- required artifacts
- relevant repository context

It should not simply continue the implementation conversation and judge its own answer informally.

This is logical review independence, even though the underlying model weights may be the same.

If future hardware provides another strong independent reviewer, true cross-model review may be preferable.

## Safety Invariants That Must Not Regress

Adaptive parallelism must preserve all established Rack AI safety properties.

At minimum:

- external writes remain inside rootless Podman workspaces
- Rack AI cannot modify its own running repository
- all actions remain bounded
- pause/cancel remains durable and checked before commit/integration
- late worker completion cannot bypass operator intent
- deterministic gates run before semantic acceptance
- every implementation attempt gets a fresh review
- paths remain typed/normalized and fail closed
- campaign and lease state remains durable and race-safe
- retries/escalations are bounded
- evidence remains sufficient to reconstruct each batch, review, integration, and decision
- parallel execution must not introduce shared-checkout mutation

Performance must not be obtained by weakening the P0/P1 trust model.

## Scheduling Should Generalise Beyond Two GPUs

This design should not hard-code "4060 + 2060" as the permanent architecture.

The current two-GPU rack is the first deployment target, but the scheduling model should naturally accommodate future resources such as:

- another NVIDIA GPU
- RTX 3090 / 4090 / A6000-class workers
- specialised inference accelerators
- future vision/audio workers
- other model-serving endpoints

The important abstraction is a set of resources with capabilities, constraints, current availability, and health.

## Expected Operational Benefits

If a feature decomposes into multiple independent implementation batches, running two workers concurrently may substantially reduce elapsed time.

Perfect 2x scaling should not be assumed because campaigns also include:

- planning
- review
- integration
- dependency stalls
- retries
- GPU/model speed differences

A realistic objective should be measured experimentally.

The larger benefit may be quality as much as throughput: difficult work can reach the strongest available model earlier rather than only after repeated failure.

## What We Must Measure Before Finalising This Design

This specification should be revised after P1 completion and real baseline campaigns.

Collect at least:

- total campaign elapsed time
- time spent in planning
- time spent in implementation
- time spent in deterministic checks
- time spent in semantic review
- 2060 utilisation / busy percentage
- 4060 Ti utilisation / busy percentage
- time each GPU is idle while runnable campaign work exists
- implementation attempts per worker
- first-pass acceptance rate per worker
- semantic rejection rate per worker
- repair rate
- fallback/escalation rate
- typical task sizes and dependency structures
- observed quality difference between `local-coder` and `local-primary` implementation
- integration/conflict patterns in representative repositories

The measurements should answer whether the suspected utilisation imbalance is real and identify which scheduling policy would yield the most value.

## Validation Strategy for a Future Implementation

Before replacing the current execution model, run comparative benchmarks against the proven sequential/asymmetric baseline.

Candidate scenarios should include:

- several independent small batches
- mixed simple and difficult batches
- strongly dependency-ordered work
- overlapping-file work
- worker rejection and escalation
- one model endpoint temporarily unavailable
- pause/cancel during parallel execution
- restart/recovery with multiple in-flight batches
- integration conflict

Compare at least:

- correctness
- acceptance/rejection quality
- wall-clock completion time
- resource utilisation
- failure recovery
- evidence quality
- operator controllability

The current architecture should remain available as a known-safe baseline until the adaptive execution model has demonstrated equivalent safety and better operational value.

## Explicitly Out of Scope for This Draft

This document does not yet specify:

- exact scheduler algorithms
- exact complexity scoring
- exact capability schema
- exact integration/cherry-pick strategy
- maximum parallelism
- GPU utilisation thresholds
- queue fairness policy
- model replacement or new model selection
- front-end/user-interface changes
- autonomous self-starting goal generation

Those decisions should be informed by post-P1 measurements.

## Relationship to Current Rack AI Behaviour

This design does **not** reject the current architecture.

The existing `local-coder` implementation / `local-primary` coordination-review split was the correct architecture for establishing safe autonomous execution.

This document proposes the next optimisation after that baseline is operationally proven:

> Keep the same safety contract, but schedule available reasoning and coding capacity more intelligently.

## Decision Gate Before Implementation

Do not begin implementation merely because this PR exists.

Before implementation:

1. complete and merge P1
2. run representative real campaigns using the current architecture
3. collect utilisation, timing, quality, rejection, and fallback metrics
4. revisit this specification
5. refine scheduling, integration, capability, and review semantics based on evidence
6. explicitly approve the revised design

Only then should this document become an implementation contract.
