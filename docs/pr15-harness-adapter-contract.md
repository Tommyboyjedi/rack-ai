# PR15 Contract — Selected Harness Adapter

## Status

Post-PR14 implementation contract. Do not implement until PR14 has selected exactly one Rust coding harness.

## Goal

Replace direct model-facing coding execution with a small Rack AI-owned adapter around the selected harness.

Rack AI must remain the authority boundary. The harness performs coding work inside the bounded target workspace; Rack AI owns campaign state, worker/model placement, isolation, acceptance, review, recovery, Git and promotion.

## Required architecture

Introduce a small application-level abstraction representing a coding harness run. The exact names are open, but it should be equivalent to:

```rust
trait CodingHarness {
    fn execute(&self, request: HarnessRequest) -> Result<HarnessRun, HarnessError>;
}
```

The request should contain only the information needed to launch bounded work, such as:

- target workspace/worktree;
- task/instruction;
- selected local model/provider profile;
- timeout/resource envelope;
- approved environment/configuration;
- any explicit harness mode required by the PR14 decision.

The result should expose enough structured evidence for Rack AI to supervise the run, such as:

- process exit/termination status;
- harness transcript or structured output reference;
- model/harness identity;
- timing/usage where available;
- bounded error/termination reason.

It must not expose Rack AI's Git promotion or campaign authority to the harness.

## JCode-specific rule if selected

Use direct/non-swarm execution. Rack AI selects the endpoint/model/session. Do not rely on JCode swarm provider rebinding unless independently proven fixed later.

## Abacus-specific rule if selected

Use its supported headless/local OpenAI-compatible path and preserve any useful open-weight tool-call parsing rather than recreating it in Rack AI.

## Process and isolation

- launch the harness as a bounded child process or equally narrow integration;
- keep execution inside the target worktree and existing isolation boundary;
- mutation work remains network-disabled unless a future explicit policy says otherwise;
- no home/SSH/remote Git credentials exposed to the harness;
- timeout/cancel must terminate the harness and descendants predictably;
- capture evidence before cleanup;
- do not let harness success bypass Rack AI acceptance/review.

## Required integration

At minimum integrate the selected harness into the current implementation-worker path while preserving:

- local-coder primary assignment;
- local-primary fallback where policy permits;
- deterministic no-change rejection;
- path/Git inspection;
- deterministic acceptance commands;
- fresh independent review;
- PR7-style diagnosis/replan/fallback above the harness;
- durable campaign evidence/state.

## Explicit non-goal

Do not implement general-purpose coding-agent tools in Rack AI. No new agent-facing read/write/edit/replace/insert/LSP/search tools should be added unless PR14 proved the selected harness cannot supply a material requirement and the architectural decision is documented first.

## Tests

Add deterministic coverage for:

- request-to-process/config mapping;
- selected model/endpoint mapping;
- successful harness run evidence capture;
- non-zero harness failure;
- timeout/cancellation and child cleanup;
- no-change rejection after harness completion;
- acceptance failure after harness completion;
- independent reviewer remains separate from implementer;
- path authority remains unchanged;
- no automatic push/merge/default-branch mutation.

Add at least one live-rack proof using the selected harness and actual local vLLM endpoint.

## Merge gate

PR15 merges only when Rack AI can supervise a real selected-harness implementation run end-to-end without relying on the legacy direct model coding loop for that path.