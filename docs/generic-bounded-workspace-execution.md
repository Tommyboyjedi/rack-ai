# Generic Bounded Workspace Execution

## Decision

Rack AI is a generic rack execution and resource-control plane. It is not a raw prompt proxy, and it is not a client-specific software-engineering orchestrator.

Its repository-changing operation is a **bounded workspace transaction**:

> Execute one already-ready generic AI task against one exact repository revision, inside a controlled worktree, under machine-enforced limits, and return a candidate revision or durable terminal failure evidence.

The client owns what the work means. Rack AI owns how the work is safely placed and executed on the rack.

## Why this boundary exists

A raw prompt router is too weak. It would force every client to reimplement privileged concerns such as worker selection, GPU placement, worktree trust, path enforcement, timeout cleanup, network policy, Git evidence, and terminalization.

A client-aware orchestration engine is too broad. If Rack AI learns about Tester, Developer, scenarios, TDD, repairs, reviews, or another client's workflow, it becomes coupled to that client and cannot remain a reusable rack service.

The bounded workspace boundary keeps both responsibilities where they belong:

- clients retain domain meaning, readiness, dependencies, retries, and acceptance interpretation;
- Rack AI centralizes generic resource selection, isolation, execution, enforcement, and evidence.

This also prevents prompt prose from becoming an undocumented API.

> The prompt is advisory. The typed execution envelope is authoritative.

Routing, identity, permissions, timeouts, network access, writable paths, and acceptance must be represented and enforced as typed data, not merely requested in natural language.

## What Rack AI may understand

Rack AI may understand generic infrastructure concepts:

- broad model capabilities;
- generic complexity and context requirements;
- source priority and admission ceilings;
- registered model profiles;
- worker runtimes and harnesses;
- physical resources and leases;
- repository identity and exact base revision;
- trusted worktrees;
- allowed writable paths and authorized read-only resources;
- timeout, network, process, and artifact limits;
- deterministic commands and required artifacts;
- idempotency, cancellation, status, and terminal evidence.

These concepts are useful to many clients and do not reveal why the task exists.

## What Rack AI must not understand

Rack AI must not infer or encode client workflow semantics such as:

- Tester or Developer role;
- scenario authoring or repair;
- RED or GREEN;
- strict TDD frontier;
- behavior planning or review;
- Gatekeeper reconciliation;
- client dependency graphs;
- client escalation tiers;
- whether a failure consumes a client's semantic attempt budget.

The task objective may naturally mention domain words because a model needs to perform the work. Rack AI treats that text as opaque task payload, not as scheduling or state-machine data.

## The three-part request

A bounded workspace request has three separate responsibilities.

### 1. Generic routing header

Conceptually:

```text
source_system
required_capabilities[]
complexity
requires_large_context
priority
```

Version-1 broad capabilities are aligned with broad model classes:

```text
reasoning
coding
visual
audio
```

A request may require more than one capability, for example `[reasoning, coding]`.

Complexity remains:

```text
small
medium
large
```

Context remains an explicit boolean requirement where the current contract uses that class:

```text
requires_large_context = true | false
```

Global priority vocabulary is:

```text
low
medium
high
paramount
```

Priority affects queue order and resource policy. It never changes the requested capabilities or the semantic validity of a result.

### 2. Machine-enforced execution envelope

Conceptually:

```text
opaque work_id
unique submission_id
idempotency key
repository identity
exact base ref and SHA
allowed writable paths
authorized read-only resources
network and process policy
timeout
required artifacts
deterministic acceptance commands
cancellation policy
```

Rack AI validates and enforces this envelope independently of model behavior.

### 3. Model-facing task payload

Conceptually:

```text
bounded objective
relevant immutable context
expected artifact
prior generic failure evidence, when supplied
```

The objective should be concise and purposeful. It must not be used as the only place where access, timeout, routing, or acceptance rules live.

## Bounded workspace transaction lifecycle

```text
receive already-ready generic request
  -> validate source admission and request schema
  -> resolve trusted repository and exact base revision
  -> create isolated worktree
  -> determine eligible registered model/worker runtimes
  -> acquire required resource lease
  -> record generic selection decision
  -> invoke the registered harness
  -> enforce path/network/process/time limits
  -> inspect actual Git changes
  -> run caller-supplied deterministic acceptance
  -> materialize candidate revision when valid
  -> persist terminal packet and execution provenance
```

The operation terminalizes as accepted, rejected, failed, blocked, cancelled, or another bounded generic terminal status. It does not decide whether the candidate is a valid test, correct feature, semantic repair, or completed client workflow.

## Required capabilities versus internal eligibility metadata

The client sends only the capabilities required by the current job.

Example:

```text
required_capabilities = [coding]
```

Rack AI owns internal model eligibility profiles, conceptually containing:

```text
model profile identity
broad capabilities
qualified complexity envelope
large-context eligibility
qualification status and evidence
context/runtime constraints
profile version
```

The client does not transmit or author this metadata.

The request asks:

```text
What broad kind of model ability is required?
```

Rack AI's internal registry answers:

```text
Which healthy, available or queueable worker runtime is qualified to provide it?
```

This distinction matters when the same model profile can run on different GPUs. The model has the same intelligence capabilities; worker instances may differ in throughput, warm state, and availability.

## Model profile, worker runtime, and physical resource

Rack AI must keep these concepts separate.

### Model profile

Describes broad model ability and qualification:

```text
capabilities
complexity envelope
context eligibility
qualification evidence
runtime constraints
```

### Worker runtime

Describes one executable instance:

```text
worker ID
model profile
harness/profile
status
concurrency capacity
resource requirements
```

### Physical resource

Describes the rack asset:

```text
GPU or other resource ID
memory and health
lease state
supported runtime profiles
```

A 4060 Ti worker and 4080 Super worker running the same model profile expose the same broad intelligence capability even when one is faster.

## Generic worker selection

Selection must be deterministic and based only on generic request and rack state.

### Hard eligibility

A worker is eligible only when:

1. it supports every requested capability;
2. it is qualified for the requested complexity;
3. it satisfies the context requirement;
4. its model, harness, worker, and resource are healthy or validly queueable;
5. the execution envelope can be enforced;
6. source admission and priority policy permit the request.

### Ranking

Among eligible workers, Rack AI may use generic factors such as:

- global priority and queue age;
- least-scarce sufficient capability profile;
- current availability;
- warm/resident model state;
- measured throughput or expected completion time;
- resource pressure;
- deterministic tie-break.

For a small `[coding]` request, a coding-only qualified worker is normally less scarce than a worker that also supplies reasoning. For `[reasoning, coding]`, a coding-only worker is ineligible.

No ranking reason may depend on hidden client concepts such as scenario authoring or TDD phase.

## Selection evidence and execution provenance

Rack AI should return two linked generic records.

### Selection decision

Explains why a worker was chosen:

```text
requested generic capabilities
complexity and context requirement
priority and source policy
eligible workers
ineligible workers with generic reasons
selected worker
selection reason
resource/lease evidence
policy and profile versions
```

### Execution provenance

Proves what actually ran:

```text
worker
model profile
provider profile
resource
backend/harness profile
```

Selection and execution identities must agree. A mismatch fails closed.

The client may inspect this evidence, but it does not choose the concrete worker or author Rack AI's internal model profile.

## Source priority ceilings

Priority is global Rack AI vocabulary, while each source may have an admission ceiling.

For ATHBA:

```text
allowed priorities: low, medium
maximum priority: medium
```

ATHBA is continuous slow-burn work. A job that blocks an ATHBA project may be medium inside the global rack queue, but it is not automatically high or paramount.

High and paramount remain available for separately authorized interactive, operational, service-restoration, safety, or future media workloads.

The source connector should reject an invalid outbound priority, and Rack AI admission policy must independently reject a source request above its configured ceiling. Rack AI must not promote an ATHBA request above medium.

## Queue and dependency boundary

Rack AI accepts only work the client already considers ready.

Rack AI may queue an accepted job while eligible capacity is busy, but it must not receive or interpret the client's semantic dependency graph. It does not decide that one client job must precede another based on IDs or objective text.

A sequence number may be retained for audit. It is not a dependency mechanism.

Temporary lack of free capacity is a queued state. Absence of any qualified worker for the required capabilities is a capability-unavailable blocker. Those are different outcomes.

## Submission and invocation identity

A stable `work_id` may link several client-authorized submissions for the same logical work. Rack AI treats it as opaque.

A unique `submission_id` identifies one requested backend model invocation. One submission should not silently contain several semantic model submissions.

Rack AI may retry genuinely low-level infrastructure operations that do not invoke the model again, but infrastructure recovery and additional model invocation must remain distinguishable in evidence. Clients need this distinction for truthful attempt accounting.

## Why Rack AI owns deterministic acceptance execution

The client supplies command argv and required artifacts because the client owns their meaning. Rack AI executes them because it owns the trusted worktree and terminal evidence.

Rack AI may report that commands exited successfully or failed. It does not infer that a passing command proves a behavior, that a failure is valid RED, or that a candidate should advance a client workflow.

## Why tools and harness profiles remain internal

The client requests broad model capabilities and a bounded operation. Rack AI chooses a qualified worker runtime, including its harness and tool profile.

A client must not request a concrete JCode profile merely to make one model output pass. A model attempting an unavailable tool is not itself evidence that Rack AI should grant that tool.

Tool and harness changes require independent generic qualification and safety evidence.

## Dynamic workload and GPU changes

This boundary supports future competing workloads without exposing physical rack state to clients.

If a media workload leases one GPU:

- Rack AI stops assigning incompatible new jobs to that resource;
- other eligible workers may continue;
- generic jobs may remain queued;
- the client request does not change;
- terminal selection/provenance remains truthful.

Detailed ComfyUI arbitration, preemption, model residency, multi-GPU placement, and idle-worker optimization belong to a separate Rack AI scheduling specification. They must preserve this generic boundary.

## Change-control rule

Any boundary or harness change must demonstrate that it restores or extends a documented generic Rack AI contract. Moving one client fixture one step farther is not sufficient justification.

Specifically:

- do not add a tool merely because a model called it;
- do not add a client workflow field merely because one client can describe its work more easily that way;
- do not infer client dependencies or semantic state;
- do not encode access, timeout, or acceptance only in prompts;
- do not convert ordinary model failure into another hidden retry subsystem;
- do not move client-specific language/framework knowledge into Rack AI.

## Current and target contract

The current `rack-ai/work-unit/v1` contract is a deliberate MVP. It still contains `application-development` and singular `implementation` vocabulary. Those fields remain compatibility facts, not the desired long-term semantic boundary.

The immediate PR23-related generalization should be bounded to:

- broad capability sets;
- existing complexity and context requirements;
- global priority with source ceilings;
- internal generic model eligibility profiles;
- generic selection evidence linked to execution provenance;
- opaque work/submission identity;
- preservation of the existing bounded workspace transaction.

A universal inference or media execution framework is not required for this change.


## PR32 wire contract

`rack-ai/work-unit/v2` is the published generic routing schema. `work_unit.routing` is required only for v2 and has this exact shape:

```json
{"source_system":"athba","work_id":"opaque-work","submission_id":"opaque-submission","idempotency_key":"opaque-key","required_capabilities":["reasoning","coding"],"priority":"medium"}
```

`required_capabilities` is a non-empty, duplicate-free set from `reasoning`, `coding`, `visual`, and `audio`. Rack AI canonicalizes its persisted ordering. Complexity remains exactly `small`, `medium`, or `large`; `requires_large_context` remains a boolean eligibility constraint. Priority is exactly `low`, `medium`, `high`, or `paramount` and governs admission/scheduling only.

Source admission is typed Rack AI configuration. `athba` is capped at `medium`; high and paramount requests are rejected before worker selection or execution. The configuration includes an explicit wildcard default ceiling of `paramount`; without either a matching source policy or the wildcard default, admission fails closed.

Profiles are internal. The minimal `local-coder` profile is coding-only, small-only, non-large-context, and retains its minimal JCode constraint. `local-primary` is qualified for reasoning and coding at small, medium, and large complexity according to its configured profile. For a small coding request, least-scarce-sufficient selection chooses `local-coder`; reasoning plus coding at medium selects `local-primary`.

A selected v2 request persists a generic selection decision and must have execution provenance for the same worker. A mismatch fails closed. Selection evidence is retained for terminal accepted, rejected, timeout, protocol, and post-selection executor failures; failures before selection do not fabricate a decision. Resource-capable but busy workers produce the typed temporary-unavailable outcome, distinct from no capable worker.

`work_id`, `submission_id`, and `idempotency_key` are opaque. A submission-specific safe internal transaction identifier preserves distinct submissions for the same work ID. A repeat with the same persisted source/work/submission/idempotency identity is rejected before another execution.

V1 remains readable with its historical singular `implementation` routing semantics. Rack AI never interprets a v1 packet as v2. This compatibility window remains until an explicitly versioned deprecation change. PR32 does not add client dependency scheduling, universal inference/media execution forms, ComfyUI arbitration, preemption, idle-worker overflow, or three-GPU scheduling.
