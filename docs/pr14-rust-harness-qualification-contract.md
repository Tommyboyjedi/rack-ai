# PR14 Contract — Rust Coding Harness Qualification and Routing Policy

## Status

Architecture-reset qualification contract. This PR is documentation/experiment focused and must not become another bespoke Rack AI coding-agent implementation.

PR14 is the first active PR after the architecture reset. Its purpose is to qualify the Rust-native coding harnesses Rack AI can supervise, determine the capability envelope of each harness against the actual rack workers/models, and establish the initial deterministic harness-routing policy.

PR14 is no longer winner-takes-all. Rack AI may deliberately use different coding harnesses for different registered workers/models when evidence shows that this is materially better.

## Strategic boundary

Rack AI is the Rust control plane above coding harnesses and inference backends.

Rack AI owns:

- orchestration and campaign/task state;
- worker/model/GPU registration and placement;
- target repository/worktree authority;
- isolation/network/process policy;
- deterministic acceptance and no-change rejection;
- independent review;
- recovery/replan/fallback;
- durable evidence;
- Git/commit/promotion policy;
- selection of the appropriate qualified coding harness for a worker/task.

A coding harness owns model-facing implementation mechanics inside its bounded workspace:

- coding-agent loop;
- source navigation/search;
- edit/patch mechanics;
- implementation-time command/tool use;
- harness-local context management;
- model/tool-call parsing/correction provided by that harness.

The harness may report success. Rack AI remains responsible for deciding whether work is accepted.

## Rust requirement

Production coding harnesses in the trusted long-running Rack AI stack must be Rust-native.

Target-project compilers/interpreters may be non-Rust. The requirement applies to Rack AI and its coding-harness layer, not to the languages Rack AI can develop.

## Candidate set

Qualify initially:

1. **JCode direct/non-swarm execution**.
2. **Abacus**.

Do not introduce a third harness unless both fail a documented material requirement that cannot reasonably be solved through configuration or upstream capability.

### JCode hypothesis

JCode is expected to be especially useful for stronger models because it is mature, already installed on the rack, has rich coding/navigation capabilities and direct local-provider execution has worked.

Do not use JCode swarm as part of PR14. The previously observed swarm provider/endpoint rebinding defect is not required to be fixed for the direct-harness architecture.

### Abacus hypothesis

Abacus is expected to be especially useful for smaller/weaker open-weight models because it is Rust-native, supports OpenAI-compatible/vLLM endpoints and explicitly handles several textual/open-weight tool-call formats client-side.

This is a hypothesis to test, not a hard-coded parameter-count rule.

## Preconditions

Before starting PR14:

- current `main` includes merged PR7;
- vLLM remains the inference runtime;
- local endpoints are available:
  - `local-primary` on `127.0.0.1:8017`;
  - `local-coder` on `127.0.0.1:8018`;
- use disposable target worktrees/clones;
- never experiment against the live executing Rack AI checkout;
- preserve existing Rack AI path, Git, Podman, timeout and no-remote-promotion safety rules;
- do not modify Rack AI to compensate for a candidate weakness during comparison except neutral instrumentation/reproducibility support that does not bias one harness.

## Qualification objective

The result of PR14 is not one global winner. The result is a **capability matrix and initial routing policy**.

For each harness, establish which worker/model/task combinations are qualified, conditionally qualified or not qualified.

A result may legitimately look like:

```text
JCode:
  local-primary: qualified
  local-coder: not qualified

Abacus:
  local-primary: qualified
  local-coder: qualified

Initial routing:
  local-coder -> abacus
  local-primary -> jcode
```

If both harnesses perform well for a worker, choose one preferred harness and optionally one fallback based on evidence and operational simplicity.

Do not route solely from raw GPU size or model parameter count. Route from registered worker/model capability evidence.

## Required experiments

Run both JCode direct and Abacus against both current model roles wherever each harness can be configured:

- `local-coder` on port 8018;
- `local-primary` on port 8017.

At minimum exercise:

1. a small localized edit;
2. an additive edit to an existing Rust module/export surface;
3. a multi-file compatibility-preserving task;
4. repository search/navigation;
5. compiler/test feedback and repair;
6. malformed or textual tool-call behaviour representative of current `local-coder`;
7. a bounded failure/timeout case;
8. network-disabled disposable worktree operation;
9. explicit endpoint/model binding;
10. two independent sessions targeting different local endpoints without cross-binding;
11. final worktree/diff inspection independent of the harness.

Where practical reuse the historical PR8 `semantic-contract` step as one neutral proving task, but PR14 does not implement PR8.

At least one task must require inspecting existing code, preserving an existing contract, making substantive edits and reacting correctly to compiler/test feedback.

## Fairness rules

For comparable experiments, keep these equal wherever practical:

- target base SHA;
- task wording;
- acceptance commands;
- model endpoint;
- context/output limits;
- network policy;
- runtime/resource limits;
- target authority.

Candidate-specific configuration that is part of the harness's normal supported operation is allowed and must be recorded.

Do not give one harness bespoke Rack AI repair logic that the other does not receive.

## Required measurements

For each `(harness, worker/model, task)` combination record:

- substantive diff produced: yes/no;
- deterministic acceptance result;
- preservation of unrelated APIs/behaviour;
- tool/protocol failures;
- textual/malformed tool-call handling;
- turns/tool calls where available;
- wall time;
- context/output truncation behaviour;
- compiler/test repair quality;
- repository navigation quality;
- headless transcript/evidence quality;
- timeout/cancel/process behaviour;
- network-disabled operation;
- endpoint/model-binding reliability;
- installation/configuration/upgrade complexity;
- ability for Rack AI to inspect the final worktree independently;
- amount of Rack AI-native coding-agent functionality that the harness can replace.

## Capability classification

Classify each relevant harness/worker pairing as:

- `qualified` — suitable for production use for the tested class of work;
- `qualified_with_constraints` — usable with clearly documented bounded configuration/limitations;
- `not_qualified` — fails a material requirement.

Do not hide failures behind aggregate scores.

## Initial routing policy

PR14 must commit an explicit initial routing policy suitable for PR15 implementation.

The routing policy should be data/config driven and expressed in terms of registered worker/model capabilities, not embedded rules such as `parameter_count < 7B`.

It should support at least:

- preferred harness per worker/model profile;
- optional fallback harness when qualified;
- explicit `none` when no harness is qualified;
- reason/evidence reference for the route.

For the current rack, the expected-but-unproven starting hypothesis is:

```text
local-coder   -> Abacus preferred
local-primary -> JCode preferred
```

PR14 evidence is allowed to overturn that hypothesis.

## Required repository artifacts

Commit a qualification report under `docs/` containing:

- exact JCode version/SHA;
- exact Abacus version/SHA;
- exact Rack AI SHA used;
- exact local model names/configuration;
- task definitions and acceptance commands;
- result matrix for every required test;
- evidence/transcript references;
- material failures and candidate-specific constraints;
- capability classification for each harness/worker pairing;
- initial routing policy and rationale.

The report must end with a machine-readable summary equivalent to:

```text
QUALIFIED_HARNESSES = jcode,abacus
LOCAL_CODER_PREFERRED_HARNESS = <jcode|abacus|none>
LOCAL_PRIMARY_PREFERRED_HARNESS = <jcode|abacus|none>
```

If a harness is not qualified anywhere, state that explicitly rather than forcing dual-harness adoption.

## Security and authority requirements

A qualifying harness must be invokable without receiving Rack AI promotion authority.

Rack AI must remain able to enforce the outer worktree/container/network/runtime boundary and independently run final Git/path, acceptance and review gates.

A harness must not require remote Git credentials or automatic push/merge authority for normal Rack AI operation.

## Non-goals

- no JCode swarm dependency;
- no production PR15 integration yet;
- no new Rack AI edit tools;
- no Rack AI-owned LSP/semantic coding backend;
- no objective planning;
- no adaptive task scheduling;
- no web research system;
- no frontend;
- no automatic remote Git promotion;
- no dynamic self-learning harness scheduler yet.

## Implementation-agent handoff

An agent assigned PR14 should:

1. read this contract and current architecture/engineering docs;
2. inspect current worker/model endpoint configuration;
3. identify exact JCode and Abacus versions to test;
4. prepare only the minimum tooling/configuration needed for reproducible comparison;
5. create disposable fixtures/tasks from known clean SHAs;
6. run the same matrix against both harnesses and both worker roles;
7. preserve raw failure evidence;
8. classify each harness/worker pairing;
9. commit the comparison report and initial routing policy;
10. stop without implementing PR15.

## Merge gate

Merge PR14 only when the current worker/model roles have been tested sufficiently to establish a defensible capability matrix and initial harness-routing policy.

It is acceptable for one harness to be unqualified for one or all worker profiles. It is not acceptable to guess routing from model size without evidence.
