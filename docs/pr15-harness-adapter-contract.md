# PR15 Contract — Selected Harness Adapter

## Status

Post-PR14 implementation contract. Do not implement until PR14 has selected exactly one Rust coding harness and committed the qualification evidence.

## Purpose

Integrate the selected harness into Rack AI with the smallest practical adapter so Rack AI can supervise coding work without owning coding-agent internals.

PR15 is not a harness-development PR. Its purpose is to replace the production direct model-facing implementation path with a narrow, testable harness boundary while preserving Rack AI's existing control-plane guarantees.

## Architectural boundary

Rack AI owns:

- campaign/task state;
- worker/model/GPU placement;
- repository/worktree registration;
- process/isolation/network policy;
- timeout/cancel/cleanup;
- path/Git authority;
- deterministic acceptance;
- no-change detection;
- fresh independent review;
- PR7 recovery/replan/fallback;
- durable evidence/state;
- commit/promotion policy.

The selected Rust harness owns:

- model-facing coding loop;
- source-code navigation;
- implementation-time edit/patch operations;
- implementation-time repository search;
- harness-local context management;
- model/tool-call parsing and correction provided by the harness;
- implementation-time command use inside its bounded workspace.

The harness may report success. Only Rack AI may accept the attempt.

## Preconditions

Before implementation:

- PR14 must be merged or its exact selected-harness decision must be incorporated onto the PR15 branch;
- use exactly the selected harness unless new evidence proves the PR14 decision invalid;
- do not rely on JCode swarm if JCode is selected;
- preserve vLLM as the inference runtime;
- preserve the current target-repository isolation model;
- inspect current `DirectCoderWorker`, campaign runner, workspace executor, review, recovery and process-cleanup paths before changing them.

## Required application boundary

Introduce one small application-level abstraction equivalent in responsibility to:

```rust
trait CodingHarness {
    fn execute(&self, request: HarnessRequest) -> Result<HarnessRun, HarnessError>;
}
```

Exact type names may follow existing repository conventions.

`HarnessRequest` should contain only bounded launch information such as:

- target workspace/worktree;
- task/instruction;
- selected local model/provider profile;
- timeout/resource envelope;
- approved environment/configuration;
- explicit harness mode/configuration selected by PR14.

`HarnessRun` should expose structured supervision evidence such as:

- process exit/termination status;
- harness transcript or structured-output reference;
- harness/model identity;
- timing/usage where available;
- bounded error/termination reason;
- enough information to correlate the run with Rack AI attempt evidence.

Do not model harness-internal edit tools in the Rack AI application interface.

## Selected-harness implementation rule

### If JCode was selected

Use direct/non-swarm execution. Rack AI selects the endpoint/model/session explicitly. Do not depend on native swarm provider rebinding unless a later qualification separately proves it fixed.

Prefer JCode's own source navigation/edit/tool loop rather than recreating those tools in Rack AI.

### If Abacus was selected

Use its supported headless/local OpenAI-compatible execution path. Preserve useful built-in open-weight tool-call parsing and edit behaviour rather than recreating them in Rack AI.

## Process and isolation requirements

- launch the harness as a bounded child process or equally narrow integration;
- run against the target worktree only;
- keep mutation work network-disabled unless an explicit later policy changes this;
- expose no home directory, SSH credentials, GitHub tokens, host sockets or unrelated host filesystem data;
- timeout/cancel must terminate the harness and descendants predictably;
- retain useful transcript/process evidence before cleanup;
- the harness must not commit, push or merge merely because it considers itself finished unless Rack AI explicitly owns and authorizes that exact operation;
- final Git/path inspection remains Rack AI-owned.

## Required integration behaviour

Integrate the harness into the real implementation-worker path while preserving all of the following:

1. `local-coder` remains the normal primary implementation role unless configuration says otherwise.
2. `local-primary` remains available as bounded fallback where existing policy permits.
3. A harness run that produces no substantive source diff is rejected.
4. Rack AI inspects changed paths/Git state independently of the harness.
5. Rack AI runs deterministic acceptance independently of the harness.
6. A fresh reviewer evaluates accepted-looking work independently of the implementation session.
7. PR7 recovery can diagnose/replan/reassign after harness-backed failures without broadening authority.
8. Attempt/campaign state remains durable and restart-safe.
9. Harness output/evidence is attached to the same attempt evidence model rather than creating a parallel orchestration architecture.

## Legacy path rule

Do not delete the old direct model-facing implementation code in PR15 unless removal is trivially necessary for correctness. PR16 owns broad cleanup/deletion.

However, the production path being qualified at PR15 merge must actually use the selected harness. Do not leave the harness as an unused optional prototype while the legacy path remains authoritative.

## Explicit non-goals

- no new general-purpose Rack AI coding tools;
- no Rack AI-owned read/edit/replace/insert/LSP/search agent surface;
- no objective planning;
- no adaptive multi-worker scheduling;
- no web research;
- no frontend;
- no automatic cloud/frontier escalation;
- no remote Git promotion;
- no large cleanup/refactor of legacy harness internals beyond what integration requires.

## Required tests

Add deterministic coverage for at least:

- request-to-harness-process/config mapping;
- exact selected model/endpoint mapping;
- successful harness-run evidence capture;
- non-zero harness failure;
- timeout/cancellation and descendant cleanup;
- no-change rejection after harness completion;
- path-policy violation remains rejected;
- acceptance failure after harness completion;
- independent reviewer remains separate from implementer;
- recovery receives harness-backed failure evidence;
- no automatic push/merge/default-branch mutation;
- restart/durable state does not lose the harness attempt outcome.

Run the existing relevant workspace/campaign/isolation tests as well.

## Required live proof

Run at least one real local-vLLM implementation through the selected harness under Rack AI supervision.

The retained evidence must show:

- selected harness/version;
- selected local model/endpoint;
- target worktree;
- substantive diff;
- Rack AI-owned acceptance results;
- independent review result;
- final accepted or safely rejected disposition.

A shell command that invokes the harness outside the Rack AI path is not sufficient proof.

## Required documentation

Update architecture/operations documentation with:

- the selected harness;
- how Rack AI launches/configures it;
- endpoint/model mapping;
- ownership boundary;
- how to inspect harness evidence;
- what remains legacy until PR16.

## Implementation-agent handoff

An agent assigned PR15 should:

1. read PR14's decision report and this contract;
2. inspect the existing implementation/review/recovery/process boundaries before editing;
3. add the smallest typed harness abstraction and selected-harness adapter;
4. route the real implementation-worker path through it;
5. preserve existing Rack AI gates rather than delegating them into the harness;
6. add deterministic tests and a real-rack proof;
7. document exact residual legacy code for PR16.

Do not implement PR16 cleanup or PR17 qualification in this PR.

## Merge gate

PR15 merges only when Rack AI can supervise a real selected-harness implementation run end-to-end, using the real campaign/worker path, while all Rack AI-owned authority, acceptance, review, recovery and evidence guarantees remain intact.