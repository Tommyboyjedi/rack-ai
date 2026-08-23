# PR10 — Recovery Escalation and Capability Exhaustion

## Status

This is a post-PR9 implementation contract. Do not implement or merge it until the frozen PR6 qualification in PR9 has completed and its evidence has been reviewed.

PR10 must extend the recovery model introduced by PR7. It must not introduce a second recovery/escalation architecture.

## Goal

Add explicit, durable escalation outcomes for cases where Rack AI has correctly diagnosed a development problem but cannot safely or effectively complete it with the currently available local workers, authority, tools, or budget.

The purpose is not to make every failure succeed. The purpose is to distinguish:

- work that should be retried locally;
- work that should be reassigned to a stronger local worker;
- work that is impossible under current authority;
- work that exceeds currently available local capability;
- work that requires operator/frontier assistance;
- work that should terminate safely.

## Required architectural principle

Escalation is a decision produced from the existing PR7 recovery context and evidence. It is not an excuse to broaden authority automatically.

No escalation path may automatically widen:

- campaign permitted paths;
- step allowed paths;
- acceptance commands;
- repository registration;
- network policy;
- runtime/resource limits beyond explicit policy;
- Git promotion authority;
- campaign objective or step list;
- credentials or external services.

## Required model

Introduce typed escalation semantics that can represent at least:

1. `reassign_stronger_local_worker`
2. `local_capability_exhausted`
3. `insufficient_authority`
4. `operator_decision_required`
5. `external_expertise_required`
6. `terminal_failure`

Names may differ, but these meanings must be represented explicitly and durably.

Each escalation record must include concise, inspectable fields equivalent to:

- campaign and step identity;
- failure/recovery classification;
- root-cause summary;
- evidence references;
- attempted strategies/workers;
- reason further local retry is not justified;
- required capability or authority;
- whether continuation is safe;
- recommended next action;
- timestamp and worker/reasoner identity.

Do not store raw private chain-of-thought. Persist concise diagnosis, evidence, decision and rationale.

## State model

Evaluate whether the existing `Blocked` state plus an escalation artifact is sufficient. Add a dedicated `Escalated` state only if it materially improves correctness and lifecycle semantics.

Whatever representation is chosen must be durable across restart and visible in campaign status/evidence.

The system must never present `local_capability_exhausted` or `operator_decision_required` as successful campaign completion.

## Stronger-local-worker escalation

Where policy allows, PR10 may direct a failed implementation attempt from `local-coder` to `local-primary` or another explicitly registered local capability.

Requirements:

- reassignment must be justified by the recovery decision;
- worker identity must be recorded;
- attempt budget must remain bounded;
- final semantic review must be a fresh isolated invocation, even if the same model weights implemented the change;
- reassignment must not reuse hidden authoring conversation state as independent review evidence.

## Operator/frontier escalation artifact

Rack AI should be able to produce a compact handoff packet that allows an operator or frontier model to understand the problem without reconstructing the campaign from scratch.

The packet must contain only bounded, useful evidence such as:

- objective/step task;
- immutable authority constraints;
- changed paths and diff summary;
- failing commands and diagnostics;
- recovery diagnoses;
- attempted strategies;
- relevant semantic-code evidence where available;
- explicit unresolved question/capability gap.

Do not automatically send this packet anywhere in PR10. Automatic GitHub/Gmail/webhook/cloud escalation is out of scope.

## Stagnation and capability exhaustion

Build on PR7 stagnation/failure fingerprints. Repeated materially equivalent failures must not consume the entire attempt budget by replaying the same strategy.

Tests must prove that after bounded equivalent failures Rack AI either:

- changes strategy;
- reassigns according to policy;
- or produces an explicit capability/authority escalation outcome.

## Safety and lifecycle invariants

All existing invariants remain mandatory:

- external-target-only mutation;
- rootless Podman isolation where currently required;
- network policy;
- path policy;
- Git continuity;
- deterministic acceptance;
- semantic review;
- timeouts and attempt limits;
- durable evidence;
- pause/cancel/lease handling;
- fail closed.

## Tests

Add deterministic tests for at least:

1. local-coder failure diagnosed as suitable for stronger local worker, followed by bounded reassignment;
2. repeated equivalent failure resulting in `local_capability_exhausted` rather than endless repair;
3. problem requiring a write outside immutable allowed paths resulting in `insufficient_authority` with no widening;
4. operator-required escalation survives restart and remains inspectable;
5. escalation packet contains referenced evidence and does not contain unrestricted transcripts/secrets;
6. semantic review independence remains intact after stronger-worker implementation.

Add an opt-in live-rack proof using real local endpoints for at least one stronger-worker or capability-exhaustion path.

## Explicit non-goals

Do not add:

- autonomous objective-to-campaign planning;
- web/SearXNG research;
- adaptive parallel scheduling;
- automatic remote PR creation/merge;
- automatic frontier/cloud invocation;
- another agent framework;
- self-modification exceptions;
- broad memory/RAG infrastructure.

## Merge gate

PR10 may merge only when:

- PR9 has completed;
- PR7 recovery behaviour remains green;
- PR8 semantic tooling remains bounded/read-only;
- escalation is typed and durable;
- authority cannot be widened automatically;
- repeated-failure tests prove finite behaviour;
- at least one real local-model escalation path is evidenced.
