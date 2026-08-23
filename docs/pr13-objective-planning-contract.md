# PR13 — Objective-to-Campaign Planning and Plan Revision

## Status

This is a post-PR12 implementation contract. It addresses the known limitation in Campaign v1: campaigns are predeclared and Rack AI does not yet turn a software objective into its own validated executable campaign.

This capability is intentionally outside the frozen PR6 qualification and must not be used to rewrite PR9 success criteria.

## Goal

Allow a user/operator to provide a software objective and have Rack AI produce a bounded, reviewable, executable campaign plan that can then run through the existing campaign machinery.

Target flow:

`software objective -> local-primary planning -> typed PlanProposal -> deterministic Rack AI validation -> executable campaign -> execution/recovery -> bounded plan revision when evidence invalidates unexecuted work`

## Core principle

The model proposes. Rack AI authorises.

A model-generated plan must never directly become execution authority without deterministic validation against repository registration, path policy, limits, acceptance policy and campaign invariants.

## Typed plan proposal

Represent planning output with typed fields equivalent to:

- objective;
- assumptions;
- relevant repository evidence;
- proposed ordered/dependency-aware work units;
- task description per unit;
- proposed allowed write paths;
- required changed paths where appropriate;
- proposed acceptance commands/evidence;
- dependencies;
- capability/worker requirements where relevant;
- rationale for decomposition;
- unresolved risks/questions.

Do not store raw chain-of-thought. Persist concise assumptions, evidence, decisions and rationale.

## Plan validation

Rack AI must deterministically validate or reject the proposal before execution.

Validation must cover at least:

- target repository is registered and allowed;
- no live-controller self-modification exception;
- every proposed writable path is within operator-granted campaign authority;
- commands conform to executor policy;
- runtime/resource/attempt limits are bounded;
- acceptance criteria are executable and meaningful;
- dependencies are acyclic;
- required artifacts/changed paths are coherent;
- worker/tool requirements exist or produce an explicit planning/escalation failure;
- plan cannot silently introduce network/cloud/credential authority.

Operator-supplied top-level authority remains immutable unless a separate explicit operator action changes it outside the planner.

## Repository investigation for planning

The planner may use PR8 semantic intelligence and normal bounded read operations across the registered repository to understand architecture before proposing work.

It may use PR11 research only when a typed research need is justified.

Planning must not mutate the target repository.

## Sequential-first correctness

Even though PR12 may provide adaptive scheduling, the planning abstraction must remain valid when executed sequentially.

Correct decomposition and recoverability are more important than parallelism.

## Plan revision

Execution evidence can invalidate assumptions. Add bounded revision semantics for unexecuted/future work.

Rules:

- accepted/committed history is immutable evidence;
- completed steps are not rewritten retrospectively;
- a revision may change future strategy/dependencies/work units only within original operator-granted authority;
- revision must be triggered by explicit evidence such as PR7 recovery diagnosis, changed dependency state, integration result, or discovered repository constraint;
- revised plans pass the same deterministic validator before execution;
- plan revision count/budget is bounded;
- inability to produce a valid revised plan routes through PR10 escalation.

## Authority relationship

The initial objective is not sufficient authority by itself.

Define an explicit operator-granted planning envelope such as:

- target repository;
- maximum permitted path scope (possibly repository-wide if deliberately granted);
- forbidden paths;
- allowed command/executor classes;
- time/resource limits;
- whether research is allowed;
- whether parallel execution is allowed;
- promotion rules.

The planner may choose a narrower per-step scope but never exceed the envelope.

## Acceptance design

The planner should propose deterministic acceptance wherever reasonably inferable from the repository, such as existing test/build/lint commands, but Rack AI must validate commands before use.

Where acceptance cannot be established safely, planning should fail/escalate rather than fabricate a success criterion.

Semantic review remains independent and does not replace deterministic acceptance.

## Tests

Add deterministic tests for at least:

1. simple objective -> valid two-step campaign proposal -> validation -> execution;
2. proposal attempts path outside planning envelope -> rejected before any mutation;
3. cyclic dependency plan -> rejected;
4. nonexistent worker/tool requirement -> explicit planning failure/escalation;
5. repository evidence causes planner to choose narrower correct path scope;
6. execution evidence invalidates a future step -> bounded plan revision succeeds;
7. revision attempts authority expansion -> rejected;
8. accepted completed history is preserved across plan revision;
9. repeated invalid revisions terminate/escalate finitely;
10. planner crash/timeout leaves no target mutation and durable evidence;
11. separate-clone Rack AI target remains allowed while executing live checkout remains protected.

Add an opt-in live-rack proof in which the operator supplies a bounded software objective rather than a prebuilt campaign and Rack AI produces, validates and completes the campaign using local models only.

## Next acceptance target

PR13 should define or enable a NEW autonomy qualification after the frozen PR6/PR9 milestone. That later qualification should explicitly test objective-to-plan autonomy and bounded plan revision rather than silently adding these requirements to PR6.

## Explicit non-goals

Do not add:

- unrestricted self-directed goals;
- authority inferred solely from natural-language objective;
- automatic expansion of writable repository scope;
- automatic deployment/promotion of Rack AI's own live controller;
- cloud/frontier fallback by default;
- another agent framework;
- hidden chain-of-thought persistence.

## Merge gate

PR13 may merge only when:

- the PR9 frozen PR6 qualification remains unaffected/passed;
- PR7–PR12 interfaces are reused rather than duplicated;
- model plans require deterministic validation;
- authority expansion tests fail closed;
- bounded plan revision is durable and finite;
- an end-to-end local-model objective-to-campaign proof succeeds on a disposable/external target;
- a new post-PR6 qualification contract is written separately rather than modifying the old one.
