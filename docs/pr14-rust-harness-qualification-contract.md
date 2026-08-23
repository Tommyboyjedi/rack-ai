# PR14 Contract — Rust Coding Harness Qualification

## Status

Architecture-reset qualification contract. This PR is intentionally documentation/experiment focused and must not turn into another bespoke coding-agent implementation.

## Strategic reset

Rack AI is the Rust control plane above coding harnesses and inference backends. It owns orchestration, authority, isolation, verification, recovery, evidence and promotion policy. It should not implement general-purpose coding-agent mechanics when a suitable Rust-native harness already provides them.

The selected coding harness will own model-facing source-code interaction: repository navigation, coding tool loops, edit/patch mechanics, implementation-time command use and harness-local context management.

Rack AI remains responsible for deciding whether harness output is acceptable.

## Candidate set

The initial qualification is deliberately narrow:

1. JCode direct/non-swarm execution.
2. Abacus.

JCode has incumbent advantage because it is already installed, Rust-native, and direct endpoint execution has worked on the rack. The previously observed JCode defect was in swarm provider/endpoint rebinding; PR14 must not depend on JCode swarm.

Abacus is the challenger because it is Rust-native, supports OpenAI-compatible/vLLM endpoints, and explicitly handles several open-weight textual tool-call formats client-side.

Do not broaden the bake-off unless both candidates fail a material requirement.

## Qualification principle

Use identical disposable target repositories, tasks, models, authority and acceptance wherever practical. The objective is to choose one production harness, not to prove that multiple harnesses can work.

## Required experiments

At minimum test both harnesses against:

- `local-coder` on port 8018;
- `local-primary` on port 8017;
- a small localized edit task;
- a multi-file compatibility-preserving task;
- malformed/textual tool-call behaviour representative of the current local-coder;
- compiler/test feedback and repair;
- a task requiring repository search/navigation;
- a bounded failure/timeout case;
- a network-disabled disposable worktree.

Where practical reuse the PR8 `semantic-contract` task as one neutral historical proving task, but PR14 does not implement PR8.

## Required measurements

Record for each candidate:

- whether a substantive diff was produced;
- correctness against deterministic acceptance;
- preservation of unrelated APIs/behaviour;
- model/tool protocol failures;
- number of model turns/tool calls;
- wall time;
- context/output truncation behaviour;
- ability to repair compiler/test failures;
- quality and structure of headless output/transcripts;
- process exit/timeout/cancellation behaviour;
- offline/network-disabled operation;
- ease of selecting a specific local vLLM endpoint/model;
- behaviour when two independent sessions target different endpoints;
- ability for Rack AI to inspect the final worktree/diff independently.

## Security and ownership requirements

A qualifying harness must be invokable without receiving Rack AI promotion authority. Rack AI must remain able to enforce the outer worktree/container/network/runtime boundary and independently run final path, Git, acceptance and review gates.

The harness must not require remote credentials or automatic push/merge authority for normal Rack AI operation.

## Rust requirement

The production coding harness must be Rust-native. Target-project compilers/interpreters may of course be non-Rust; this requirement applies to the trusted long-running Rack AI/harness software layer, not to languages Rack AI can develop.

## Decision gate

PR14 ends with one explicit result:

`SELECTED_HARNESS = jcode`

or

`SELECTED_HARNESS = abacus`

If neither qualifies, document the exact failed requirements before considering a third-party alternative or any new Rack AI-owned coding-agent functionality.

Do not choose based on familiarity alone. Abacus must materially beat JCode to displace the incumbent, but JCode must still satisfy the actual local-coder requirements.

## Non-goals

- no JCode swarm dependency;
- no new Rack AI edit tools;
- no Rack AI-owned LSP/semantic backend;
- no objective planning;
- no adaptive scheduling;
- no web research system;
- no frontend;
- no automatic remote Git promotion.

## Merge gate

Merge PR14 only when both candidates have been tested sufficiently to make the production decision and the evidence/decision is committed to the repository.