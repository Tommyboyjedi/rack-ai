# rack-ai Engineering Contract

This document expands the standing rules in `../AGENTS.md`.

`AGENTS.md` is the concise agent-facing contract. This document records the reasoning, architecture boundaries, safety invariants, and expected evidence behind those rules.

# Purpose

`rack-ai` is the orchestration and control layer for the GPU rack.

Its purpose is not merely to make local models run. It must make autonomous work:

- bounded
- observable
- reviewable
- recoverable
- isolated
- durable
- explainable after the fact

The system should be safe to leave running without relying on undocumented model behaviour or continuous human supervision.

## August 22, 2026 Progress

Implemented in the current `feat/campaign-live-supervision` branch:
- P0 regression coverage proving a background state heartbeat cannot resurrect paused or cancelled campaign state
- versioned unattended operations config in `config/operations.json`
- `campaign supervise` reconciliation loop for campaigns left in `running` state
- stale campaign-container tracking and cleanup before bounded resume
- bounded retention pruning for old terminal campaign state/worktrees
- bounded retention pruning for auxiliary logs/history/change artifacts
- stale orphan repository-lease cleanup
- headless operating documentation and supervision smoke coverage

Still outstanding for full unattended confidence:
- longer soak and repeated crash/reboot recovery runs on the live rack
- more live evidence for extended endpoint degradation and unattended recovery over multi-day operation

# Runtime Architecture

Host: `gpurack`

Model serving uses vLLM through OpenAI-compatible endpoints.

## local-primary

Endpoint:

`http://127.0.0.1:8017/v1`

Primary roles:

- coordinator
- planner
- verifier
- semantic reviewer
- fallback implementation worker

## local-coder

Endpoint:

`http://127.0.0.1:8018/v1`

Primary role:

- implementation worker

## JCode

Direct single-agent JCode runs are usable.

Native JCode swarm is not trusted for cross-provider delegation because JCode v0.78.1 demonstrated provider/endpoint rebinding failures: a worker assigned to `local-coder` could inherit the coordinator endpoint and send the request to `8017`.

Therefore:

- do not use JCode swarm as the primary orchestration mechanism
- do not assume a future version fixes the issue without explicit rack-side verification
- direct/manual/debug JCode use may remain available where appropriate

The authoritative autonomous orchestration path is the repo-controlled campaign system.

# Trust Boundaries

## External Repository Writes

External repository mutation must happen through the rootless Podman workspace executor.

This rule applies regardless of which model performs the implementation.

In particular, `local-primary` acting as fallback implementer does not gain extra trust.

Workers must not mutate external repositories via:

- host shell
- direct host filesystem writes
- JCode shell execution
- privileged helper processes
- alternate write routes that bypass the workspace executor

The purpose of this boundary is to make implementation capability explicit and constrained.

# Fail-Closed Behaviour

Safety-sensitive uncertainty is failure, not success.

Fail closed when there is uncertainty or failure involving:

- path authorization
- deterministic checks
- semantic review
- reviewer protocol
- model protocol
- model timeout
- container timeout
- command timeout
- lease ownership
- stale state
- campaign state integrity
- required evidence
- Git evidence
- malformed campaign configuration

Do not reinterpret an absent, malformed, or timed-out safety decision as acceptance.

# Bounded Execution

Potentially blocking work must have explicit wall-clock bounds.

This includes:

- model connection
- model response/read
- overall worker action
- per-turn model generation
- semantic review
- Podman command execution
- acceptance commands
- subprocess cleanup
- retries
- fallback attempts

No retry policy may be infinite.

A hung model or process must eventually resolve into a classified failure/retry/fallback/block condition rather than leaving the campaign indefinitely active.

# Durable State

Campaign control state is safety-critical.

Important state-changing paths include:

- campaign creation
- runner snapshots
- pause
- resume
- cancel
- revise
- attempt transitions
- progress/heartbeat
- lease acquire
- lease heartbeat
- lease release
- stale lease handling
- blocking
- completion

Required properties:

- valid state survives interrupted writes
- no torn JSON
- no stale runner save may erase a newer cancel
- no stale runner save may erase a newer pause
- revisions must not disappear
- cancel must not be resurrected into running
- active leases must not be stolen
- stale leases may only be reclaimed according to explicit policy
- concurrent control operations must be serialized or otherwise race-safe

## Atomic Writes

The durable-write implementation should use the repository atomic-write mechanism rather than plain overwrite.

The intended durability sequence is:

1. write temporary file
2. flush data
3. fsync temporary file
4. atomic rename
5. where required for the durability guarantee, fsync the parent directory

Changes to this mechanism must be tested against interrupted-write behaviour.

# Pause and Cancel Semantics

Pause and cancel belong to the operator.

Worker completion does not override operator control.

Requirements:

- pause/cancel become durable promptly
- stale runner state does not erase them
- the runner checks them at safe progression boundaries
- there is a checkpoint immediately before commit
- a result arriving after pause/cancel cannot be committed merely because the worker succeeded

Tests should exercise races close to the commit boundary.

# Heartbeats, Progress, and Stale Work

An active campaign must provide durable evidence that it is alive.

The intended maximum heartbeat gap is 30 seconds while long-running work is active.

Operators should be able to distinguish:

- active healthy work
- slow but alive work
- stale/hung work
- retry/fallback
- blocked work
- completed work

Where possible, status/events/inspect should expose:

- current step
- current attempt
- worker identity
- current action
- recent heartbeat
- last progress time
- rejection/fallback
- blocked/completed state

A stale action or lease must be detected, classified, evidenced, and handled through bounded policy.

# Independent Review

Every implementation attempt requires two stages.

## Stage 1 — deterministic checks

Examples include:

- path restrictions
- required changed paths
- command policy
- acceptance commands
- Git evidence
- repository invariants

## Stage 2 — semantic coordinator review

The semantic reviewer produces structured output such as:

- `accepted`
- `rejected_retryable`
- `rejected_terminal`

Evidence should preserve:

- disposition
- classification
- rationale
- evidence references
- raw request where required
- raw model output where required

Reviewer errors, malformed output, or timeout fail closed.

Fallback implementation still requires a fresh review.

If `local-primary` performs fallback implementation, it must not be auto-accepted merely because the reviewer also uses the `local-primary` endpoint.

Review must remain logically independent.

The reviewer must not be given write-capable workspace tools.

# Path Authorization

Filesystem authorization must use validated path semantics.

Unsafe examples include naïve raw string-prefix authorization because strings such as:

- `src/foo`
- `src/foobar`

share a prefix while representing different path boundaries.

Tests should cover:

- `..`
- absolute paths
- dot components
- repeated separators
- prefix collisions
- malformed allowed paths
- malformed permitted paths
- malformed required-change paths
- workspace escape
- symlink/path escape where relevant to the actual executor design

Authorization failures are fail-closed.

# Rust Engineering Principles

The original project programming rules came from C# development. Their intent remains useful, but they should be applied idiomatically in Rust.

## Size and Responsibility

A class-equivalent implementation unit should normally remain under approximately 100 lines.

This is not intended as a cosmetic line-count target.

It encodes the expectation that a large unit probably contains multiple responsibilities and should be decomposed.

Agents should refactor automatically rather than repeatedly asking for permission solely because a unit crosses the threshold.

Legitimate exceptions are mainly declarative rather than behavioural, such as:

- exhaustive enum matching
- serialization/schema definitions
- generated/external boilerplate
- compact single-purpose trait implementations

## Composition

Prefer composition.

Rust naturally supports this through:

- structs containing structs
- enums for closed variants
- narrow traits for real boundaries
- explicit dependencies

Do not construct inheritance-like trait hierarchies merely to reproduce an OO structure.

## Function Signatures

The old C# "maximum two parameters" rule should not be applied literally.

Instead:

- group values that form one concept
- use request/config/domain structs when several related values travel together
- avoid long lists of loosely related primitives
- allow a small direct signature when it is clearer than an artificial wrapper

## Typed Contracts

Typed structures are preferred over loosely structured bags of data.

Avoid using `HashMap<String, Value>`, `serde_json::Value`, or opaque primitive collections as internal contracts when a struct, enum, or newtype expresses the domain more safely.

A map remains valid when the domain itself is genuinely map-shaped.

## Magic Values

Avoid unexplained magic numbers and strings.

Use:

- constants
- enums
- typed limits
- configuration
- descriptive helper functions

Timeouts, limits, protocol names, states, and policy values should have explicit meaning.

## Branching

The old C# "no more than three branches" rule is not literal in Rust.

Exhaustive `match` is idiomatic and often desirable.

The real concern is substantial behavioural complexity within branches.

Extract complex branch bodies into focused functions or composed objects.

Prefer enums over string-based dispatch.

## Error Handling

Use `Result` and preserve useful error context.

Do not silently swallow errors.

Production code should avoid `unwrap()` and `expect()` unless a documented invariant makes failure genuinely impossible.

Tests may use them for readability.

Panics are not ordinary control flow.

## Unsafe

Repository-maintained Rust code should not use `unsafe`.

Prohibited by default:

- `unsafe` blocks
- `unsafe fn`
- `unsafe impl`
- raw pointer manipulation
- unchecked memory operations

Third-party dependencies may internally use unsafe.

Any repository exception requires explicit human approval before implementation and must document:

- why safe Rust is insufficient
- the exact safety invariant
- the minimum unsafe surface
- tests around the boundary
- concurrency/failure implications

Do not introduce unsafe for speculative performance.

## Ownership and Shared Mutation

Prefer clear ownership and explicit state flow.

Shared mutation should be deliberate.

Use:

- `Mutex`
- `RwLock`
- atomics
- interior mutability
- shared ownership

only where the design genuinely requires them.

Concurrency should have:

- clear ownership
- bounded execution
- explicit invariants
- race tests where safety matters
- deadlock-aware lock ordering

## Traits and Abstraction

Traits should represent real boundaries.

Examples:

- persistence
- external runtime
- execution
- review
- repository access
- clock/time

Do not create traits solely because every implementation "should have an interface".

Avoid premature abstraction.

## Macros

Prefer ordinary functions, structs, enums, and traits.

Macros are appropriate only when they reduce meaningful repetition without hiding behaviour or safety logic.

# Testing Contract

Behavioural changes need tests.

Safety and concurrency work should test the actual failure mode.

Relevant examples:

- interrupted atomic write
- concurrent runner/operator state updates
- lost pause prevention
- lost cancel prevention
- revision preservation
- pause immediately before commit
- cancel immediately before commit
- late worker completion
- stale lease handling
- active lease protection
- path traversal rejection
- semantic reviewer rejection
- reviewer timeout/error
- bounded primary/fallback attempts
- model timeout
- Podman timeout
- timeout cleanup
- executor identity evidence
- host-shell prohibition

Do not weaken or delete safety tests merely to achieve a green run.

A bug fix should normally include a regression test.

# Verification

Baseline:

```bash
cargo test --workspace --offline
```

Also run smoke tests relevant to the changed subsystem.

For campaign/executor work, run the rootless Podman smoke scripts present under `tests/`.

For live-model campaign work, when model endpoints are available:

```bash
RACK_AI_LIVE_SMOKE=1 bash tests/rack_campaign_live_model_smoke.sh
```

Live success requires exit code zero and:

```text
rack_campaign_live_model_smoke: ok
```

Compiler warnings introduced by branch changes should be cleaned up before completion.

# Evidence Contract

Evidence should make campaign decisions reconstructable.

Depending on the attempt, retain:

- worker identity
- attempt kind
- executor kind
- tool transcript
- command stdout/stderr
- Git evidence
- changed paths
- deterministic review
- semantic review request
- raw review output
- structured review disposition
- retry/fallback classification
- heartbeat/progress evidence
- final campaign status

A human should be able to determine:

- what ran
- which model ran it
- how it was executed
- what changed
- why it was accepted/rejected
- whether fallback occurred
- why execution stopped

# Git and Repository Hygiene

Keep changes narrow.

Avoid unrelated formatting or refactoring.

Do not commit:

- `.idea/`
- editor state
- temporary live-smoke directories
- generated logs/evidence
- model files
- build artifacts
- unrelated local config

Before commit:

```bash
git status
git diff
git diff --check
```

Do not merge unless explicitly instructed.

Do not rewrite published history unless explicitly instructed.

# Human Approval Boundaries

Explicit human approval is required before:

- introducing repository `unsafe`
- weakening a safety boundary
- enabling host-shell mutation
- replacing vLLM
- restoring JCode swarm as primary cross-provider delegation
- removing deterministic review
- removing semantic review
- reducing evidence required by the campaign contract
- changing the fundamental trust model
- merging a PR when the task only requests implementation/review

# Working Strategy

Agents should:

1. inspect before changing
2. understand the current tests/contracts
3. identify the actual defect
4. make the smallest coherent fix
5. add/update tests
6. run targeted tests
7. run workspace tests
8. run applicable smoke/live tests
9. inspect the final diff
10. report residual risk honestly

Do not start by rewriting working subsystems.

Prefer boring, explicit, typed, bounded, observable, recoverable code.
