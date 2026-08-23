# PR16 Contract — Remove Superseded Native Coding Harness

## Status

Post-PR15 cleanup contract. Do not implement until the qualified Rust harnesses are integrated and the production worker/model routing path is proven.

## Goal

Make the architecture reset real by deleting or simplifying Rack AI code whose responsibility now belongs to JCode, Abacus, or another PR14-qualified external Rust coding harness.

Code reduction is an explicit success criterion.

PR16 is not allowed to collapse the new multi-harness architecture back into one bespoke Rack AI coding loop.

## Architectural boundary after PR16

Rack AI owns:

- registered target repositories/worktrees;
- rootless isolation and network policy;
- campaign/task durable state;
- worker/model/GPU registry and placement;
- qualified-harness routing policy;
- timeout/cancel/process cleanup;
- Git/path authority and final change inspection;
- deterministic acceptance;
- no-change detection;
- fresh independent review;
- PR7 diagnosis/replan/fallback;
- durable evidence/restart behaviour;
- commit/promotion policy.

External Rust coding harnesses own:

- model-facing coding loops;
- model tool-call parsing/correction;
- source navigation/search;
- edit/patch mechanics;
- implementation-time context/tool-choice strategy;
- implementation-time command use inside the bounded workspace.

Rack AI may contain thin adapters and routing policy, but not duplicate general-purpose implementations of the harness capabilities.

## Preconditions

Before implementation:

- PR14 capability matrix/routing policy exists;
- PR15 integrates every production-qualified route required for current workers/models;
- the real production implementation path uses external harness adapters;
- all current safety/review/recovery tests are green;
- inspect actual call graphs/usages before deleting code.

## Candidate removal/simplification areas

Inspect actual usage rather than deleting by name alone. Likely candidates include:

- legacy `DirectCoderWorker` model/tool loop;
- `WorkspaceCoderToolRunner` agent-facing coding tools;
- Rack AI-owned `write`/`replace`/`insert_after` tool advertisement/correction logic;
- bespoke textual/tool-call parser workarounds now owned by Abacus/JCode;
- coding-agent prompting/tool-choice strategy;
- planned Rack AI-owned LSP/semantic coding backend work;
- tests/configuration that exist only for removed native agent internals.

Preserve lower-level infrastructure when it remains a genuine Rack AI authority/isolation primitive used independently of the legacy coding loop.

## Required inventory method

Before deleting, classify relevant components as:

- `rack_control_plane` — must remain;
- `external_harness_responsibility` — should be removed from Rack AI;
- `shared_boundary` — keep only the minimum adapter/process/evidence structure;
- `legacy_test_or_migration_only` — retain only if explicitly justified.

Commit this inventory or include it in the cleanup report.

## Multi-harness preservation rule

Do not remove the harness-neutral abstraction or the worker/model -> harness routing introduced in PR15.

The intended production shape remains:

```text
worker/model profile
    -> Rack AI harness routing
        -> JCode or Abacus (qualified route)
            -> local vLLM model
```

It is acceptable for current routing to be static/versioned. Dynamic empirical routing belongs to future work.

## Compatibility rule

Do not remove a component merely because its name sounds like coding-agent infrastructure. Trace whether it is still used for:

- Rack AI outer safety boundary;
- acceptance/review/recovery;
- process cleanup;
- evidence capture;
- worker/model placement;
- non-coding rack workloads.

If yes, simplify rather than delete where appropriate.

## Documentation

Update architecture and engineering docs so the production boundary is unambiguous:

- Rack AI = orchestration, authority, routing, isolation, verification, recovery, evidence;
- JCode/Abacus = qualified source-code implementation harnesses according to worker/model profile;
- vLLM = inference runtime.

Record the current routing policy and the rule that Rack AI should prefer harness configuration/upstream contribution over recreating mature coding-agent functionality.

## Required tests

Keep green:

- workspace tests;
- each production-qualified harness live integration route;
- worker/model -> harness routing tests;
- external repository isolation/path policy;
- campaign acceptance/no-change rejection;
- independent review;
- PR7 recovery/replan/fallback;
- timeout/cancel/descendant cleanup;
- durable evidence/restart behaviour;
- no remote push/merge/default-branch mutation.

Add a regression proving the normal production flow does not invoke the removed native coding-agent path.

## Required cleanup report

Commit a report recording:

- major components deleted;
- components simplified;
- responsibilities transferred to JCode;
- responsibilities transferred to Abacus;
- shared adapter/routing code retained;
- any legacy code intentionally retained and why;
- before/after simplification measure (for example file/LOC/module count where meaningful);
- residual risks.

## Non-goals

- no new product functionality;
- no new Rack AI agent tools;
- no changes to PR14 routing merely for cleanup convenience;
- no dynamic harness scheduler;
- no objective planning;
- no adaptive task scheduling;
- no research system;
- no remote Git promotion;
- no PR17 substantive qualification run.

## Implementation-agent handoff

An agent assigned PR16 should:

1. read PR14 and PR15 reports/contracts;
2. identify the actual production multi-harness paths;
3. inventory legacy native coding-agent components by responsibility;
4. remove external-harness duplication;
5. minimize shared adapter code;
6. preserve Rack AI control-plane and routing responsibilities;
7. update tests/docs;
8. run live regression on every current production harness route;
9. commit the cleanup report;
10. stop before PR17.

If cleanup requires rebuilding significant coding-agent functionality inside Rack AI, stop and report the architectural conflict instead of recreating the layer.

## Merge gate

PR16 should leave Rack AI materially smaller/simpler than the post-PR15 state while preserving working multi-harness routing and every Rack AI-owned safety, acceptance, review, recovery and evidence invariant.
