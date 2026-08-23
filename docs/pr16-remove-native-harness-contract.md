# PR16 Contract — Remove Superseded Native Coding Harness

## Status

Post-PR15 cleanup contract. Do not implement until the selected Rust harness is integrated and proven through the PR15 path.

## Goal

Make the architectural reset real by deleting or simplifying Rack AI code whose responsibility now belongs to the selected coding harness.

Code deletion is an explicit success criterion.

## Candidate removal/simplification areas

Inspect actual usage after PR15 rather than deleting by name alone. Likely candidates include:

- `DirectCoderWorker` model/tool loop;
- `WorkspaceCoderToolRunner` agent-facing coding tools;
- Rack AI-owned `write`/`replace`/`insert_after` tool advertisement and correction logic;
- bespoke textual/tool-call parser workarounds that the selected harness owns;
- coding-agent prompting/tool-choice strategy;
- planned Rack AI-owned LSP/semantic coding backend work;
- tests that exist only to validate removed coding-harness internals.

Preserve lower-level infrastructure if it remains a genuine Rack AI authority/isolation primitive used independently of the harness.

## Keep

PR16 must preserve Rack AI-owned responsibilities:

- registered target repositories/worktrees;
- rootless isolation and network policy;
- campaign/task state;
- model/GPU/worker registry and placement;
- timeout/cancel/process cleanup;
- Git/path inspection and mutation authority boundaries;
- deterministic acceptance;
- no-change detection;
- fresh independent review;
- PR7-style diagnosis/replan/fallback above harness execution;
- durable evidence and restart behaviour;
- no remote push/merge unless explicitly added in a future approved capability.

## Compatibility rule

Do not remove a component merely because its name sounds like coding-agent infrastructure. Trace whether it is used for Rack AI's outer safety boundary, acceptance path, campaign engine or non-coding workloads.

## Documentation

Update architecture/engineering documentation so the production boundary is unambiguous:

- Rack AI = orchestration, authority, isolation, verification, recovery, evidence;
- selected Rust harness = source-code interaction and implementation agent loop;
- vLLM = inference runtime.

Record the selected harness and explicitly state that Rack AI should prefer upstream contribution/configuration over recreating mature harness functionality.

## Tests

- workspace tests remain green;
- selected-harness live integration remains green;
- external repository isolation/path policy remains green;
- campaign acceptance/review/recovery remains green;
- no legacy direct coding path is accidentally exercised in the production harness-backed flow.

## Merge gate

PR16 should leave the codebase materially smaller or simpler than the post-PR15 state. If integrating an external harness results in a larger duplicate coding-agent implementation, stop and re-evaluate the boundary instead of merging.