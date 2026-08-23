# PR16 Contract — Remove Superseded Native Coding Harness

## Status

Post-PR15 cleanup contract. Do not implement until the selected Rust harness is integrated and proven through the PR15 production path.

## Purpose

Make the architectural reset real by deleting or simplifying Rack AI code whose responsibility now belongs to the selected coding harness.

PR16 is not a generic refactor. It is a responsibility-transfer cleanup: remove duplicate coding-agent mechanics while preserving every control-plane invariant that Rack AI still owns.

Code deletion and reduced conceptual surface area are explicit success criteria.

## Preconditions

Before implementation:

- PR14 has selected exactly one Rust harness;
- PR15 has integrated that harness into the real implementation-worker path;
- at least one harness-backed live run has been retained as evidence;
- the legacy direct coding path is no longer required for the production flow being qualified;
- current relevant tests are green before cleanup starts.

## Required ownership boundary

After PR16, Rack AI must still own:

- registered target repositories/worktrees;
- rootless isolation and network policy;
- campaign/task state and durable restart behaviour;
- model/GPU/worker registry and placement;
- timeout/cancel/process cleanup;
- Git/path authority and final change inspection;
- deterministic acceptance;
- no-change detection;
- fresh independent review;
- PR7-style diagnosis/replan/fallback above harness execution;
- durable evidence;
- local commit/promotion policy;
- prevention of unauthorized remote push/merge/default-branch mutation.

The selected harness should own:

- model-facing coding loop;
- model tool-call parsing/correction;
- repository navigation for implementation;
- source edit/patch mechanics;
- implementation-time search/context handling;
- implementation-time coding prompts/tool-choice logic.

## Candidate removal/simplification areas

Inspect actual references and runtime paths after PR15 rather than deleting by name alone. Likely candidates include:

- `DirectCoderWorker` model/tool loop;
- `WorkspaceCoderToolRunner` agent-facing coding tools;
- Rack AI-owned `write`/`replace`/`insert_after` tool advertisement and malformed-call correction logic;
- bespoke textual/tool-call parser workarounds that the selected harness now owns;
- coding-agent prompting/tool-choice strategy;
- planned Rack AI-owned LSP/semantic coding backend work;
- tests that exist only to validate removed coding-harness internals;
- configuration fields used solely by the removed legacy coding loop.

Preserve lower-level types/adapters if they remain genuine Rack AI authority, process, filesystem, evidence or isolation primitives used independently of the coding harness.

## Required method

For each candidate component:

1. trace production references and tests;
2. classify the responsibility as `rack-control-plane`, `selected-harness`, or `shared-boundary`;
3. delete selected-harness duplication;
4. simplify shared-boundary code to the minimum adapter contract;
5. preserve Rack AI control-plane code;
6. update tests/documentation to reflect the final ownership model.

Do not perform unrelated style refactors while deleting legacy code.

## Legacy removal target

At merge, the normal harness-backed production execution path must not invoke the old Rack AI-native model/tool coding loop.

If a legacy path is intentionally retained temporarily, document:

- exact code retained;
- why it is still required;
- whether it is production-active, test-only or migration-only;
- the condition/date/PR that should remove it.

Do not retain duplicate functionality merely as a comforting fallback if it is no longer part of the architecture.

## Documentation requirements

Update architecture/engineering/operations documentation so the production boundary is unambiguous:

- **Rack AI** = orchestration, authority, isolation, verification, recovery, evidence and promotion policy;
- **selected Rust harness** = source-code interaction and implementation-agent loop;
- **vLLM** = inference runtime.

Record the selected harness and explicitly state that Rack AI should prefer harness configuration, existing upstream capability or upstream contribution over recreating mature coding-agent functionality.

Remove or clearly mark documentation that describes superseded Rack AI-native agent tools as the production design.

## Required tests

At minimum prove:

- workspace tests remain green;
- selected-harness live integration remains green;
- external-repository isolation/path policy remains green;
- campaign deterministic acceptance remains green;
- independent review remains green;
- PR7 recovery remains green above the harness adapter;
- timeout/cancel cleanup remains green;
- no-change rejection remains green;
- no remote push/merge/default-branch mutation is introduced;
- the normal production harness-backed flow cannot accidentally select the removed legacy direct coding path.

## Required cleanup report

Commit a short architectural cleanup report under `docs/` recording:

- major components deleted/simplified;
- responsibilities moved to the selected harness;
- Rack AI responsibilities preserved;
- any intentional residual legacy code;
- before/after source/test footprint or another concrete measure showing simplification.

The goal is not a vanity line-count target, but the report should demonstrate that the duplicate coding-agent layer genuinely became smaller.

## Explicit non-goals

- no new product functionality;
- no new agent tools;
- no objective planning;
- no adaptive scheduling;
- no web research;
- no remote Git promotion;
- no PR17 substantive qualification run except small checks needed to prove cleanup has not broken PR15 integration;
- no rewriting merged Git history.

## Implementation-agent handoff

An agent assigned PR16 should:

1. read the PR14 selection, PR15 integration docs and this contract;
2. identify the actual production harness-backed path;
3. inventory duplicate legacy coding-agent components;
4. remove/simplify them responsibility-by-responsibility;
5. preserve and rerun Rack AI safety/control tests;
6. update architecture docs;
7. commit a cleanup report explaining what remains and why.

If cleanup requires rebuilding substantial agent functionality in Rack AI, stop and report the architectural conflict rather than implementing it.

## Merge gate

PR16 should leave the codebase materially smaller or simpler than the post-PR15 state while keeping the selected-harness production path and all Rack AI control-plane invariants green. If integration with the external harness leaves a larger duplicate coding-agent implementation, stop and re-evaluate the boundary instead of merging.