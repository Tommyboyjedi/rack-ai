# Rack AI Agent Instructions

These instructions apply to all AI coding agents working in the Rack AI repository.

## Mandatory reading before work

Before inspecting, planning, editing, testing, or committing code, read and obey:

1. `coding_principles.MD`
2. `AGENTS.md`
3. `docs/generic-bounded-workspace-execution.md`
4. `docs/athba-runtime-boundary.md` for ATHBA-facing work
5. the Rack AI architecture/boundary documentation relevant to the task
6. the current PR description and any source-controlled implementation contract relevant to the task

The coding principles are mandatory application-level rules. Do not treat them as style suggestions.

## Repository boundary

Work only in the Rack AI repository.

Do not modify ATHBA source or configuration. If a task exposes an ATHBA planning/development-semantics defect, stop and hand back evidence rather than fixing ATHBA from the Rack AI task.

## Architecture boundary

Rack AI owns generic rack execution/resource behavior: source admission, broad model capability eligibility, model/worker/resource selection, JCode integration, trusted repositories/worktrees, bounded host or Podman process execution, timeout/resource/network/path policy, deterministic command evidence, recovery, candidate revisions, and durable terminal packets.

Clients such as ATHBA own the meaning of work, semantic readiness, dependencies, model-attempt policy, repair/escalation decisions, and interpretation of returned results.

Rack AI must remain language-, framework-, and client-workflow agnostic.

## Why this boundary exists

Rack AI must be more than a raw prompt router because privileged workspace execution, model placement, GPU leases, sandboxing, timeouts, Git evidence, and terminalization should be implemented once for the rack.

Rack AI must be less than a client orchestration engine because learning Tester, Developer, scenario, RED/GREEN, frontier, review, or another client's state machine would make the control plane non-generic and tightly coupled.

The bounded workspace transaction is the middle ground:

```text
client supplies an already-ready generic task and typed limits
Rack AI selects and executes it safely
Rack AI returns generic evidence
client interprets the result in its own domain
```

The prompt is advisory. The typed execution envelope is authoritative.

Do not place access, timeout, routing, identity, or acceptance solely in model instructions.

## Generic request boundary

A generic request may contain:

- broad required capabilities: `reasoning`, `coding`, `visual`, `audio`;
- complexity: `small`, `medium`, `large`;
- large-context requirement;
- source priority;
- opaque work and submission identities;
- exact repository/base revision;
- allowed writable paths and authorized read-only resources;
- timeout, network, process, artifact, and deterministic acceptance constraints;
- a bounded task objective and immutable context.

Rack AI must not accept client workflow concepts as routing fields. In particular, do not add Tester, Developer, scenario authoring, repair, RED, GREEN, frontier, review, Gatekeeper, client dependency, or client escalation-tier values to Rack AI's domain.

The objective text may mention such concepts because the model needs the task, but Rack AI must treat that text as opaque payload rather than parse it into scheduling or workflow state.

## Capability requirement versus internal eligibility profile

The client sends only the capabilities required by the current job.

Rack AI owns its internal model eligibility profiles, qualification evidence, complexity/context envelope, worker runtime, harness profile, physical resource placement, health, throughput, and lease state.

Do not require the client to send or maintain Rack AI's model metadata. Do not let a client select a concrete worker, model, GPU, endpoint, or JCode profile through the generic contract.

Selection evidence may be returned to the client for audit. It explains why Rack AI selected a worker. Execution provenance proves what actually ran. A mismatch fails closed.

## Priority admission

Rack AI's global priority vocabulary may include `low`, `medium`, `high`, and `paramount`.

Source systems have explicit ceilings. ATHBA may submit only `low` or `medium`; reject an ATHBA request above medium and never promote it internally above its source ceiling.

Priority controls queue order and resource policy. It does not add capabilities, alter semantic validity, or bypass safety.

## Queue and dependency boundary

Rack AI queues only jobs a client has already declared ready.

Do not receive, infer, or own a client's semantic dependency graph. Do not interpret work IDs, submission sequence, or objective prose as dependency ordering.

Temporary lack of capacity is queued work. Absence of any qualified worker is a capability-unavailable result. Preserve that distinction.

## Submission identity

A stable opaque work ID may link multiple client-authorized attempts. A unique submission ID represents one requested model invocation.

Do not silently hide multiple semantic model invocations behind one submission ID. Low-level infrastructure recovery is allowed only when it remains distinguishable from another model invocation in the evidence.

## Tool and harness discipline

Rack AI selects the qualified harness and tool profile internally.

A model attempting an unavailable tool is not, by itself, evidence that Rack AI should expose that tool. Tool/profile changes require generic qualification and safety evidence independent of a single client fixture.

Do not weaken a tool boundary, path boundary, or executor policy merely to allow one live proof to progress.

## Coding discipline

All new or changed application-owned code must conform to `coding_principles.MD`.

In particular:

- application-owned class-equivalent units are limited to 100 executable lines unless the user explicitly approves and the exception is documented;
- constructors and methods should not accept more than two conceptual inputs; group larger inputs into typed request/context/config objects;
- use composition rather than application-owned inheritance hierarchies;
- avoid magic numbers and strings; use typed configuration, enums, constants, and value objects;
- refactor overgrown objects before adding responsibilities;
- make state transitions explicit and typed rather than encoding a whole state machine in one large coordinator.

Do not introduce an exception to these rules without explicit user approval and documented rationale.

## Safety and change discipline

- Fail closed on trust, policy, evidence, timeout, resource, revision, path, source-admission, selection, and provenance ambiguity.
- Preserve exact execution and Git evidence.
- Do not weaken safeguards to make a demonstration pass.
- Keep host and Podman execution as generic executor backends; do not add language-specific execution semantics.
- Keep client workflow semantics outside Rack AI.
- Inspect existing architecture and tests before editing.
- Add tests for behavior and important failure paths.
- Run focused tests, then full applicable tests and `git diff --check`.
- Do not merge unless explicitly instructed.
- Report residual risk and incomplete behavior clearly.

Any boundary or harness change must demonstrate that it restores or extends a documented generic Rack AI contract rather than merely moving one client fixture one step farther.
