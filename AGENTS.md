# AGENTS.md — rack-ai Agent Rules

These rules apply to all human and AI changes to `rack-ai`.

Before inspecting, planning, editing, testing, or committing code, read and obey:

1. `coding_principles.MD`
2. `agent.MD`
3. `docs/engineering-contract.md`
4. `docs/generic-bounded-workspace-execution.md`
5. `docs/athba-runtime-boundary.md` for ATHBA-facing work
6. the current PR description and any source-controlled implementation contract relevant to the task

`coding_principles.MD` is mandatory application-level policy, not optional style guidance. Exceptions to its class-size, parameter-count, inheritance, or other architectural rules require explicit user approval and documented rationale.

For architecture detail, safety invariants, and verification guidance, see:

- `docs/engineering-contract.md`
- `docs/generic-bounded-workspace-execution.md`

## Core Boundary

Rack AI is a generic rack execution and resource-control plane. It is neither a raw prompt proxy nor a client-specific software-engineering orchestrator.

Rack AI owns the bounded workspace transaction: generic admission, model/worker/resource selection, leases, trusted worktrees, harness execution, path/network/process/time enforcement, deterministic command evidence, candidate revisions, and durable terminal packets.

Clients own what work means, whether it is ready, their dependencies, semantic attempts, repair/escalation policy, and interpretation of results.

Do not add client workflow concepts such as Tester, Developer, scenario, RED, GREEN, frontier, review, or Gatekeeper to Rack AI routing or state.

The prompt is advisory. The typed execution envelope is authoritative. Never rely on prompt prose as the only security, routing, timeout, path, or acceptance control.

## Generic Routing Boundary

Generic request fields may describe:

- broad capabilities: `reasoning`, `coding`, `visual`, `audio`;
- complexity: `small`, `medium`, `large`;
- large-context requirement;
- source priority and admission ceiling;
- opaque work/submission identity;
- repository/base/path/resource/timeout/network/acceptance constraints.

The client sends only the capabilities required by a job. Rack AI owns internal model eligibility and qualification profiles. Clients do not author those profiles and must not choose concrete workers, models, GPUs, endpoints, or JCode profiles.

For ATHBA-originated work, allowed priorities are only `low` and `medium`. Rack AI must reject ATHBA requests above the configured medium ceiling and must not promote ATHBA work to `high` or `paramount`.

One client submission should correspond to one model invocation. Low-level infrastructure recovery must remain distinguishable from another semantic model submission.

A model calling an unavailable tool is not, by itself, evidence that the tool should be granted. Tool/profile changes require independent generic qualification and safety evidence.

## Current Runtime

- Model runtime: **vLLM**
- `local-primary` — coordinator, planner, verifier, semantic reviewer, fallback implementer
    - `http://127.0.0.1:8017/v1`
- `local-coder` — primary implementation worker
    - `http://127.0.0.1:8018/v1`
- Do not replace vLLM with Ollama unless explicitly instructed.
- Do not use JCode swarm for cross-provider delegation until its provider-rebinding bug has been explicitly proven fixed on this rack.

Current worker names and GPU placements are deployment details. Generic clients request broad capabilities and constraints; Rack AI selects the concrete runtime internally.

## Safety Boundaries

- External-repository work must stay inside a Rack AI managed isolated Git worktree.
- Qualified JCode direct execution is the production model-facing coding harness for external repositories.
- Deterministic acceptance and build/test execution must remain bounded through the configured generic executor backend. Trusted host execution and rootless Podman are both supported backends; do not force caller-owned host environments through Podman when administrator configuration selects host execution.
- Workers must not mutate the source/default repository, bypass Rack AI worktree management, or bypass post-run Git/path/acceptance review.
- The same rule applies to `local-primary` when acting as fallback implementer.
- Fail closed on safety, timeout, review, path, lease, state-integrity, protocol, evidence, source-admission, selection, or provenance failures.
- All model, executor, command, review, and retry operations must be bounded.
- Preserve durable campaign state and operator intent.
- Pause/cancel must be checked immediately before commit; late worker completion must not bypass them.
- Active long-running work must leave durable liveness evidence, with no intended heartbeat gap over 30 seconds.
- Do not weaken established safety boundaries for convenience.
- Do not infer client dependencies, readiness, attempts, or semantic progression from objective text, IDs, or sequence numbers.

## Review Contract

Every implementation attempt must pass:

1. deterministic checks
2. independent semantic coordinator review

Rules:
- deterministic checks run first
- rejected review must block acceptance
- malformed/error/timeout review fails closed
- fallback implementations receive a fresh review
- `local-primary` fallback work is not auto-trusted
- reviewers must not receive write-capable workspace tools
- preserve required review request/output/evidence

The review mechanism must remain generic. Rack AI may determine whether its own execution/review contract was satisfied; it must not claim that a client behavior, TDD stage, or product requirement is semantically complete.

## Path Safety

- Use parsed/normalized path semantics.
- Do not authorize raw filesystem paths with naïve string-prefix checks.
- Reject traversal, absolute paths, malformed paths, prefix collisions, and workspace escape.
- Fail closed on ambiguous path authorization.

# Rust Programming Standards

The repository-wide principles in `coding_principles.MD` apply to Rust as well as Python-oriented examples in that document. Use Rust structs/enums/request objects as the equivalent typed parameter/value objects.

## Size and Responsibility

Keep structs, enums, implementation blocks, and major modules small and focused.
A class-equivalent application-owned implementation unit must remain under 100 executable lines unless the user explicitly approves an exception and that exception is documented in an application-level Markdown file.

The intent is to prevent large multi-responsibility units, not to satisfy a line counter mechanically.

## General Design

- Prefer composition.
- Prefer small, cohesive functions and types.
- Prefer explicit typed domain/request/config objects over unstructured maps or loosely related primitives.
- If more than two conceptual inputs are required, group them into a typed request/context/config object rather than growing method signatures.
- Avoid unexplained magic numbers and magic strings; use constants, enums, configuration, or domain types.
- Rust `match` may be exhaustive; extract large behavioural branches into responsible objects/modules rather than letting one coordinator own the entire state machine.
- Prefer enums over stringly typed state.
- Keep public APIs minimal.
- Keep dependencies explicit.
- Avoid hidden global state.
- Avoid premature abstraction.
- Prefer a little obvious duplication over a complicated abstraction when the shared concept is not stable.

## Errors and Ownership

- Prefer `Result` propagation with useful context.
- Do not silently swallow errors.
- Avoid `unwrap()` / `expect()` in production paths unless a clear invariant makes failure genuinely impossible.
- Prefer clear ownership and explicit state flow.
- Use shared mutable state, locks, atomics, or interior mutability only where genuinely required.
- Keep concurrency explicit, bounded, and tested.
- Avoid unnecessary macros or metaprogramming.

## `unsafe`

Rust `unsafe` is prohibited by default.

Do not introduce:
- `unsafe` blocks
- `unsafe fn`
- `unsafe impl`
- raw-pointer manipulation
- unchecked memory access

Any exception requires explicit human approval before implementation, with a documented safety invariant and tests.

Third-party dependencies may internally use `unsafe`; this prohibition applies to code maintained in this repository.

# Testing and Change Discipline

- Add tests for behavioural, bug-fix, safety, timeout, concurrency, state, path, selection, source-priority, provenance, and review changes.
- Do not weaken, delete, or skip safety tests to make code pass.
- Important failure modes must be tested directly.
- Keep changes narrowly scoped.
- Avoid unrelated refactors and formatting churn.
- Do not commit `.idea/`, editor state, temporary evidence, logs, model files, build artifacts, or unrelated local configuration.
- Do not merge PRs unless explicitly instructed.
- Do not rewrite published history unless explicitly instructed.
- A boundary change must demonstrate a generic Rack AI requirement, not merely move one client fixture one step farther.

Before declaring work complete:

```bash
cargo test --workspace --offline
git status
git diff
git diff --check
```

Run all smoke/live tests applicable to the changed behaviour.

For live campaign work, when endpoints are available:

```bash
RACK_AI_LIVE_SMOKE=1 bash tests/rack_campaign_live_model_smoke.sh
```

Do not claim live success unless it exits zero and prints:

```text
rack_campaign_live_model_smoke: ok
```

## Human Approval Required

Get explicit human approval before:
- introducing repository `unsafe`
- approving an exception to `coding_principles.MD`
- weakening a safety boundary
- enabling unbounded host-shell mutation of external repos
- replacing vLLM
- re-enabling JCode swarm as the primary cross-provider mechanism
- removing deterministic or semantic review gates
- reducing required evidence retention
- changing the fundamental trust model
- adding client-specific workflow semantics to Rack AI
- raising a source-system priority ceiling
- merging when the task only requested implementation/review

## Default Working Method

1. Read `coding_principles.MD`, `agent.MD`, and relevant architecture docs.
2. Inspect the existing code and tests.
3. Identify the real defect/gap and affected responsibility boundaries.
4. Confirm the change belongs to generic rack execution rather than a client's semantic domain.
5. Refactor first if the change would extend an overgrown object or violate coding principles.
6. Make the smallest coherent architectural fix.
7. Add/update tests.
8. Run targeted tests.
9. Run workspace tests.
10. Run applicable smoke/live tests.
11. Inspect the final diff.
12. Report residual risk honestly.

Prefer boring, explicit, typed, bounded, observable, recoverable code.
