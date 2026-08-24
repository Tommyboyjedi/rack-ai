# PR14 Contract — Rust Coding Harness Qualification and Routing Policy

## Status

Architecture-reset qualification contract. This PR is documentation/experiment focused and must not become another bespoke Rack AI coding-agent implementation.

PR14 is the first active PR after the architecture reset. Its purpose is to qualify the Rust-native coding harnesses Rack AI can supervise, determine the capability envelope of each harness against the actual rack workers/models, and establish the initial deterministic harness-routing policy.

PR14 is no longer winner-takes-all. Rack AI may deliberately use different coding harnesses for different registered workers/models when evidence shows that this is materially better.

A 2026-08-24 qualification finding adds a mandatory context-management gate: worker context capacity, harness compaction behaviour, safe stop/checkpoint semantics and escalation must be treated as supervised resources before PR14 may merge. A small worker must not be allowed to run blindly into its hard context limit.

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
- selection of the appropriate qualified coding harness for a worker/task;
- worker context-capacity registration and safe context-budget policy;
- context-pressure stop/checkpoint/escalation policy.

A coding harness owns model-facing implementation mechanics inside its bounded workspace:

- coding-agent loop;
- source navigation/search;
- edit/patch mechanics;
- implementation-time command/tool use;
- harness-local context management/compaction mechanisms;
- model/tool-call parsing/correction provided by that harness.

The harness may report success. Rack AI remains responsible for deciding whether work is accepted and whether continuing the current worker remains safe.

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

Before starting or continuing PR14:

- current `main` includes merged PR7;
- vLLM remains the inference runtime;
- local endpoints are available:
  - `local-primary` on `127.0.0.1:8017`;
  - `local-coder` on `127.0.0.1:8018`;
- use disposable target worktrees/clones;
- never experiment against the live executing Rack AI checkout;
- preserve existing Rack AI path, Git, Podman, timeout and no-remote-promotion safety rules;
- do not modify Rack AI to compensate for a candidate weakness during comparison except neutral instrumentation/reproducibility support that does not bias one harness;
- every worker/model profile used in context-sensitive qualification must have its **actual served context limit** recorded and supplied to the harness when the harness supports such configuration.

## Qualification objective

The result of PR14 is not one global winner. The result is a **capability matrix and initial routing policy**.

For each harness, establish which worker/model/task combinations are qualified, conditionally qualified or not qualified.

A result may legitimately look like:

```text
JCode:
  local-primary: qualified
  local-coder: qualified_with_constraints

Abacus:
  local-primary: qualified_with_constraints
  local-coder: qualified_with_constraints

Initial routing:
  local-coder -> harness selected from measured bounded-task/context evidence
  local-primary -> jcode
```

If both harnesses perform well for a worker, choose one preferred harness and optionally one fallback based on evidence and operational simplicity.

Do not route solely from raw GPU size or model parameter count. Route from registered worker/model/task capability evidence.

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
6. malformed or textual tool-call behaviour representative of the active `local-coder`;
7. a bounded failure/timeout case;
8. network-disabled disposable worktree operation;
9. explicit endpoint/model binding;
10. two independent sessions targeting different local endpoints without cross-binding;
11. final worktree/diff inspection independent of the harness;
12. context-pressure behaviour with the harness configured to the worker's true context limit;
13. compaction/checkpoint behaviour before hard context exhaustion;
14. a controlled stop/escalation experiment for a task that exceeds the small worker's safe envelope.

Where practical reuse the historical PR8 `semantic-contract` step as one neutral proving task, but PR14 does not implement PR8.

At least one task must require inspecting existing code, preserving an existing contract, making substantive edits and reacting correctly to compiler/test feedback.

## Fairness rules

For comparable experiments, keep these equal wherever practical:

- target base SHA;
- task wording;
- acceptance commands;
- model endpoint;
- true context/output limits;
- network policy;
- runtime/resource limits;
- target authority.

Candidate-specific configuration that is part of the harness's normal supported operation is allowed and must be recorded.

Do not give one harness bespoke Rack AI repair logic that the other does not receive.

Do not treat a run as evidence of a model capability ceiling when the harness was configured with a materially incorrect context limit. Preserve such a run as evidence of the current configured system failure, then repeat after the context metadata is corrected.

## Required measurements

For each `(harness, worker/model, task)` combination record:

- substantive diff produced: yes/no;
- deterministic acceptance result;
- preservation of unrelated APIs/behaviour;
- tool/protocol failures;
- textual/malformed tool-call handling;
- turns/tool calls where available;
- wall time;
- configured context limit seen by the harness;
- input/context usage by turn where available;
- compaction trigger/behaviour where available;
- output/context truncation behaviour;
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

A `qualified_with_constraints` small worker may be intentionally limited to localized/bounded work with escalation to a stronger worker for broader or context-heavy tasks.

Do not hide failures behind aggregate scores.

## Context capacity is a control-plane resource

Context capacity is part of worker registration and must be treated like VRAM, runtime and process budget.

Rack AI must not depend on a model recognizing from natural-language introspection that it is about to run out of context.

The control-plane contract is:

```text
Rack AI
  -> task admission / complexity estimate
  -> worker + harness selection
  -> context-budget supervision
       -> continue
       -> compact/checkpoint
       -> stop current worker
       -> escalate/resume on another qualified worker
```

Harness-native context compaction remains a harness responsibility. Rack AI owns the outer policy, thresholds, evidence and escalation decision.

## Context Budget Governor qualification contract

PR14 is not required to implement the final PR15 production adapter, but it must define and prove enough of this contract to make PR15 unambiguous.

### Exact context registration

Every active worker/model profile must record its real served context capacity.

For the current `local-coder` candidate the validated serving limit is:

```text
model = NotaMG/eqaq-v2
max_context_tokens = 16368
```

Harness diagnostics/configuration must be checked to ensure they use that real value rather than a generic provider/model default.

### Reserved headroom

A worker must not intentionally consume the entire hard model window before Rack AI/harness policy makes a continuation decision.

PR14 should test initial operating bands such as:

```text
0-60%    NORMAL
60-72%   WATCH
72-80%   COMPACT
80-88%   CHECKPOINT / DECIDE
>88%     DO NOT START ANOTHER LARGE STEP
```

These are experimental starting values, not final production constants. The final report must state what was observed and what thresholds PR15 should implement/configure.

### Task admission classes

PR14 should establish an evidence-backed initial task-shape envelope. A starting hypothesis is:

```text
Tier A: local-coder preferred
- localized one-file work
- known/narrow implementation point
- simple deterministic failures
- mechanical edits

Tier B: local-coder conditional
- small multi-file surface
- limited repository discovery
- bounded repair loop expected
- narrow compatibility work

Tier C: local-primary preferred
- broad discovery
- architecture/refactor work
- high ambiguity
- many modules/files
- repeated compiler/test repair expected
```

The final routing representation must use capability metadata rather than rules based directly on parameter count.

### Compaction requirement

A qualifying route must prove that context compaction occurs before hard exhaustion when continuation remains appropriate.

The experiment must record, where observable:

- trigger point;
- token/context usage;
- what information was summarized/discarded;
- whether task constraints survived;
- whether recent compiler/test evidence survived;
- whether the model could continue correctly.

Built-in JCode/Abacus compaction must be tested before adding a third-party context layer.

### Optional Headroom evaluation

Headroom or an equivalent compression layer may be evaluated on `gpurack` if built-in harness compaction remains inadequate or a transparent proxy materially improves the context envelope.

Prefer deterministic/automatic compression over requiring the small model to notice context pressure and explicitly invoke an MCP compression tool.

A Headroom experiment must use the same fixture/prompt/model and record context use, compression/retrieval events, wall time and final correctness.

Sequential-thinking MCP is not a substitute for context-budget control and should not be introduced as part of this gate merely to increase reasoning steps.

### Structured checkpoint requirement

Before a context-pressure stop/escalation, Rack AI must be able to persist a compact handoff artifact containing at least the semantic equivalent of:

```json
{
  "objective": "...",
  "files_inspected": [],
  "observed_failures": [],
  "changes_attempted": [],
  "current_repo_state": "...",
  "remaining_issue": "...",
  "recommended_next_step": "..."
}
```

The exact schema is a PR15 implementation concern, but PR14 must prove that the information is sufficient for a stronger worker to resume without replaying the full small-worker transcript.

### Controlled escalation requirement

At least one qualification case must intentionally exceed the `local-coder` safe envelope and demonstrate an evidence-preserving control flow equivalent to:

```text
local-coder performs useful bounded work
-> context/repair threshold reached
-> stop without success claim
-> persist checkpoint + Git diff + latest deterministic test/compiler output
-> local-primary resumes from compact handoff
-> Rack AI independently accepts/rejects final result
```

A context-pressure stop followed by successful controlled escalation is a valid designed outcome. An opaque provider timeout, silent hang or fabricated completion is not.

## Context-management merge acceptance

Do not consider context management qualified until evidence supports all of the following:

1. the real context capacity of each active worker is registered;
2. each harness route is configured with that real capacity;
3. the worker cannot silently consume the hard limit without a control decision;
4. compaction occurs before hard exhaustion when continuation is appropriate;
5. compaction preserves critical task state;
6. repeated repair loops are bounded by context/repair policy;
7. context pressure can cause a clean stop rather than an opaque timeout/hang;
8. a structured checkpoint is preserved;
9. a qualified stronger worker can resume from the checkpoint without replaying the entire transcript;
10. Rack AI can represent context exhaustion/pressure as a normal recoverable control-plane event.

## Initial routing policy

PR14 must commit an explicit initial routing policy suitable for PR15 implementation.

The routing policy should be data/config driven and expressed in terms of registered worker/model capabilities, task shape and resource limits, not embedded rules such as `parameter_count < 7B`.

It should support at least:

- preferred harness per worker/model profile;
- optional fallback harness when qualified;
- explicit `none` when no harness is qualified;
- task/capability envelope for constrained workers;
- context-capacity metadata;
- escalation target/policy where applicable;
- reason/evidence reference for the route.

For the current rack, the expected-but-unproven direction after the Qwen3.5 replacement is:

```text
local-coder   -> bounded implementation route, preferred harness TBD after context-corrected tests
local-primary -> JCode preferred
```

PR14 evidence is allowed to overturn that hypothesis.

## Required repository artifacts

Commit a qualification report under `docs/` containing:

- exact JCode version/SHA;
- exact Abacus version/SHA;
- exact Rack AI SHA used;
- exact local model names/configuration;
- exact context limits advertised to each harness;
- task definitions and acceptance commands;
- result matrix for every required test;
- evidence/transcript references;
- material failures and candidate-specific constraints;
- context compaction/checkpoint/escalation evidence;
- capability classification for each harness/worker pairing;
- initial routing policy and rationale.

The report must end with a machine-readable summary equivalent to:

```text
QUALIFIED_HARNESSES = jcode,abacus
LOCAL_CODER_PREFERRED_HARNESS = <jcode|abacus|none>
LOCAL_PRIMARY_PREFERRED_HARNESS = <jcode|abacus|none>
CONTEXT_GOVERNOR_REQUIRED = true
```

If a harness is not qualified anywhere, state that explicitly rather than forcing dual-harness adoption.

## Security and authority requirements

A qualifying harness must be invokable without receiving Rack AI promotion authority.

Rack AI must remain able to enforce the outer worktree/container/network/runtime boundary and independently run final Git/path, acceptance and review gates.

A harness must not require remote Git credentials or automatic push/merge authority for normal Rack AI operation.

Context compression/checkpoint infrastructure must not weaken repository, network, credential or promotion boundaries.

## Non-goals

- no JCode swarm dependency;
- no production PR15 integration yet;
- no new Rack AI edit tools;
- no Rack AI-owned LSP/semantic coding backend;
- no objective planning beyond what is necessary to classify/route the current bounded qualification tasks;
- no general adaptive/learned task scheduler;
- no web research system;
- no frontend;
- no automatic remote Git promotion;
- no dynamic self-learning harness scheduler yet.

A minimal deterministic task-capability/context admission policy required to prevent known worker exhaustion is **not** excluded by the adaptive-scheduling non-goal; it is now part of the PR14 routing contract.

## Implementation-agent handoff

An agent assigned PR14 should:

1. read this contract and current architecture/engineering docs;
2. inspect current worker/model endpoint configuration;
3. identify exact JCode and Abacus versions to test;
4. verify exact context metadata seen by each harness;
5. prepare only the minimum tooling/configuration needed for reproducible comparison;
6. create disposable fixtures/tasks from known clean SHAs;
7. run the same matrix against both harnesses and both worker roles;
8. preserve raw failure evidence, including context-pressure failures;
9. run the context-corrected compaction and controlled-escalation qualification track;
10. classify each harness/worker/task-envelope pairing;
11. commit the comparison report and initial routing/context policy;
12. stop without implementing PR15.

## Merge gate

Merge PR14 only when the current worker/model roles have been tested sufficiently to establish:

- a defensible harness capability matrix;
- an initial deterministic harness-routing policy;
- correct context-capacity configuration for active routes;
- a proven or sufficiently reproducible compaction/checkpoint/escalation contract for context pressure.

It is acceptable for one harness to be unqualified for one or all worker profiles. It is acceptable for the 2060 worker to be qualified only for bounded/localized work with escalation. It is not acceptable to guess routing from model size, allow a worker to run blindly into its context ceiling, or treat an opaque context-related timeout as a normal successful completion.
