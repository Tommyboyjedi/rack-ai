# PR14 Contract — Rust Coding Harness Qualification

## Status

Architecture-reset qualification contract. This PR is documentation/experiment focused and must not turn into another bespoke Rack AI coding-agent implementation.

PR14 is the first active PR after the architecture reset. It exists to make one production decision: which Rust-native coding harness Rack AI will supervise going forward.

## Strategic reset

Rack AI is the Rust control plane above coding harnesses and inference backends. It owns orchestration, authority, isolation, verification, recovery, evidence and promotion policy. It should not implement general-purpose coding-agent mechanics when a suitable Rust-native harness already provides them.

The selected coding harness owns model-facing source-code interaction: repository navigation, coding tool loops, edit/patch mechanics, implementation-time command use and harness-local context management.

Rack AI remains responsible for deciding whether harness output is acceptable.

## Preconditions

Before starting PR14 work:

- current `main` must include merged PR7;
- the production comparison is Rust-native only;
- JCode swarm must not be used as a dependency or workaround;
- existing local vLLM endpoints remain the model runtime:
  - `local-primary` on `127.0.0.1:8017`;
  - `local-coder` on `127.0.0.1:8018`;
- use disposable target worktrees/clones; do not experiment against the live Rack AI checkout;
- preserve existing Rack AI path, Git, Podman, timeout and no-remote-promotion safety rules.

## Candidate set

The qualification is deliberately narrow:

1. **JCode direct/non-swarm execution**.
2. **Abacus**.

JCode has incumbent advantage because it is already installed, Rust-native, and direct endpoint execution has worked on the rack. The previously observed JCode defect was in swarm provider/endpoint rebinding; PR14 must not depend on JCode swarm.

Abacus is the challenger because it is Rust-native, supports OpenAI-compatible/vLLM endpoints, and explicitly handles several open-weight textual tool-call formats client-side.

Do not broaden the bake-off unless both candidates fail a material requirement. If another candidate is considered, first document exactly which required property both JCode and Abacus failed.

## Qualification principle

Use identical disposable target repositories, tasks, models, authority and acceptance wherever practical. The objective is to choose one production harness, not to prove that multiple harnesses can work.

Do not modify Rack AI to compensate for candidate shortcomings during the comparison unless the change is purely experimental instrumentation and cannot bias one harness over the other.

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
- a network-disabled disposable worktree;
- two independent sessions targeting different local endpoints without cross-binding.

Where practical reuse the historical PR8 `semantic-contract` task as one neutral proving task, but PR14 does not implement PR8.

At least one task should be non-trivial enough that a successful run requires inspecting existing code, preserving an existing contract and repairing a compiler/test failure if the initial change is imperfect.

## Required measurements

Record for each candidate:

- whether a substantive diff was produced;
- correctness against deterministic acceptance;
- preservation of unrelated APIs/behaviour;
- model/tool protocol failures;
- number of model turns/tool calls where available;
- wall time;
- context/output truncation behaviour;
- ability to repair compiler/test failures;
- quality and structure of headless output/transcripts;
- process exit/timeout/cancellation behaviour;
- offline/network-disabled operation;
- ease of selecting a specific local vLLM endpoint/model;
- behaviour when two independent sessions target different endpoints;
- ability for Rack AI to inspect the final worktree/diff independently;
- operational complexity of installation, configuration and upgrades;
- amount of Rack AI-native coding-agent code that the candidate would allow us to delete.

## Required repository artifacts

Commit a qualification report under `docs/` containing:

- exact JCode and Abacus versions/SHAs tested;
- exact Rack AI SHA used as controller/reference;
- exact local model names/configuration;
- the tasks and acceptance commands;
- concise result table for every required measurement;
- important raw evidence locations/transcript references;
- material failures and workarounds;
- final decision and rationale.

The report must end with exactly one of:

`SELECTED_HARNESS = jcode`

`SELECTED_HARNESS = abacus`

or, only if both materially fail:

`SELECTED_HARNESS = none`

## Selection rule

Do not choose based on familiarity alone. JCode is the incumbent, so Abacus should displace it only when it is materially better for the actual rack requirements. Conversely, JCode does not win merely because it is already installed: it must work reliably with the real local-coder and local-primary paths.

The most important selection criteria are:

1. reliable operation with the small local coder;
2. clean programmatic/headless integration;
3. predictable endpoint/model binding;
4. robust source editing and repository navigation;
5. bounded process control and evidence capture;
6. compatibility with Rack AI's outer isolation/acceptance/review boundary;
7. ability to reduce, not expand, Rack AI's own coding-agent implementation.

## Security and ownership requirements

A qualifying harness must be invokable without receiving Rack AI promotion authority. Rack AI must remain able to enforce the outer worktree/container/network/runtime boundary and independently run final path, Git, acceptance and review gates.

The harness must not require remote credentials or automatic push/merge authority for normal Rack AI operation.

## Rust requirement

The production coding harness must be Rust-native. Target-project compilers/interpreters may of course be non-Rust; this requirement applies to the trusted long-running Rack AI/harness software layer, not to languages Rack AI can develop.

## Explicit non-goals

- no JCode swarm dependency;
- no new Rack AI edit tools;
- no Rack AI-owned LSP/semantic backend;
- no objective planning;
- no adaptive scheduling;
- no web research system;
- no frontend;
- no automatic remote Git promotion;
- no production integration of the winner yet — that is PR15.

## Implementation-agent handoff

An implementation/research agent assigned PR14 should:

1. read this contract and the current Rack AI architecture/engineering docs;
2. inspect how local model endpoints are configured today;
3. install or prepare only the minimum candidate tooling required for the comparison;
4. create reproducible disposable fixtures/tasks;
5. run the same qualification matrix against JCode direct and Abacus;
6. preserve evidence rather than hand-waving around failures;
7. commit the comparison report and any neutral test scripts/fixtures needed to reproduce it;
8. make exactly one harness selection.

Do not implement PR15 while doing PR14.

## Merge gate

Merge PR14 only when both candidates have been tested sufficiently to make the production decision, the evidence is committed, and exactly one production harness is selected unless both fail a documented material requirement.