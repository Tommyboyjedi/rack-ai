# Rack AI Agent Instructions

These instructions apply to all AI coding agents working in the Rack AI repository.

## Mandatory reading before work

Before inspecting, planning, editing, testing, or committing code, read and obey:

1. `coding_principles.MD`
2. `AGENTS.md`
3. the Rack AI architecture/boundary documentation relevant to the task
4. the current PR description and any source-controlled implementation contract relevant to the task

The coding principles are mandatory application-level rules. Do not treat them as style suggestions.

## Repository boundary

Work only in the Rack AI repository.

Do not modify ATHBA source or configuration. If a task exposes an ATHBA planning/development-semantics defect, stop and hand back evidence rather than fixing ATHBA from the Rack AI task.

## Architecture boundary

Rack AI owns generic rack execution/resource behavior: model/worker selection, JCode integration, trusted repositories/worktrees, bounded host or Podman process execution, timeout/resource/network/path policy, evidence, recovery, and accepted candidate revision materialization.

ATHBA owns software-development semantics such as project/environment meaning, architecture/specification, Behavior Contracts, TDD planning, Tester/Developer/Senior Reviewer orchestration, semantic replanning, and project progression.

Rack AI must remain language/framework agnostic.

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

- Fail closed on trust, policy, evidence, timeout, resource, revision, and path ambiguity.
- Preserve exact execution and Git evidence.
- Do not weaken safeguards to make a demonstration pass.
- Keep host and Podman execution as generic executor backends; do not add language-specific execution semantics.
- Inspect existing architecture and tests before editing.
- Add tests for behavior and important failure paths.
- Run focused tests, then full applicable tests and `git diff --check`.
- Do not merge unless explicitly instructed.
- Report residual risk and incomplete behavior clearly.
