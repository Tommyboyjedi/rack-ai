# PR15 Contract — Multi-Harness Adapter and Worker Routing

## Status

Post-PR14 implementation contract. Do not implement until PR14 has committed the harness capability matrix and initial routing policy.

## Purpose

Integrate the qualified Rust coding harnesses into Rack AI behind one small application boundary and route each registered worker/model profile through its preferred qualified harness.

PR15 is not a harness-development PR. Rack AI should supervise JCode and/or Abacus rather than recreate their coding-agent internals.

## Architectural boundary

Rack AI owns:

- campaign/task state;
- worker/model/GPU registry and placement;
- harness selection policy;
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

Each qualified Rust harness owns its model-facing coding loop, source navigation/search, edit/patch mechanics, implementation-time tool use, context management and built-in model/tool-call handling.

The harness may report success. Only Rack AI may accept an attempt.

## Preconditions

Before implementation:

- PR14 capability matrix/routing policy is available;
- use only harness/worker pairings that PR14 qualified;
- preserve vLLM as inference runtime;
- preserve current target-repository isolation and no-remote-promotion rules;
- JCode must use direct/non-swarm execution unless separately proven otherwise;
- inspect current worker registry, model registry, campaign runner, `DirectCoderWorker`, workspace/process execution, review, recovery and evidence paths before editing.

## Required application boundary

Introduce one harness-neutral abstraction equivalent in responsibility to:

```rust
trait CodingHarness {
    fn execute(&self, request: HarnessRequest) -> Result<HarnessRun, HarnessError>;
}
```

Exact names should follow repository conventions.

`HarnessRequest` should contain only bounded launch information such as:

- target workspace/worktree;
- task/instruction;
- selected worker/model/provider profile;
- selected harness identity;
- timeout/resource envelope;
- approved environment/configuration.

`HarnessRun` should expose supervision evidence such as:

- harness identity/version;
- model/worker identity;
- process exit/termination status;
- transcript or structured-output reference;
- timing/usage where available;
- bounded error/termination reason;
- attempt correlation information.

Do not expose harness-internal read/edit/search tools through the Rack AI application interface.

## Required harness adapters

Implement a thin adapter for every PR14-qualified production harness needed by the initial routing policy.

### JCode adapter

If JCode is qualified for any worker profile:

- use direct/non-swarm execution;
- bind endpoint/model explicitly;
- prefer its supported headless/structured output interface;
- preserve its own source navigation/edit/tool loop;
- do not recreate JCode tools inside Rack AI.

### Abacus adapter

If Abacus is qualified for any worker profile:

- use its supported headless/local OpenAI-compatible path;
- bind endpoint/model explicitly;
- preserve its built-in open-weight/textual tool-call handling;
- do not recreate Abacus editing/parsing logic inside Rack AI.

If PR14 disqualifies one harness entirely, do not implement a dead production adapter merely to satisfy symmetry.

## Harness routing

Rack AI must select a harness from registered worker/model policy, not from ad-hoc prompt logic and not directly from GPU size.

Initial routing should support at least:

- `preferred_harness` per worker/model profile;
- optional `fallback_harness` where PR14 qualified one;
- explicit failure when no harness is qualified;
- versioned configuration suitable for future hardware/model changes.

The expected shape may be similar to:

```text
local-coder:
  preferred_harness: abacus
  fallback_harness: null

local-primary:
  preferred_harness: jcode
  fallback_harness: abacus
```

Use the actual PR14 result, not this example.

PR15 should remain deterministic. Dynamic performance-learning or task-specific adaptive harness selection belongs in PR18/future work.

## Process and isolation requirements

For every harness:

- launch as a bounded child process or equally narrow integration;
- run against the target worktree only;
- keep mutation work network-disabled unless explicit later policy changes it;
- expose no home/SSH/GitHub credentials, host sockets or unrelated filesystem data;
- timeout/cancel must terminate the harness and descendants predictably;
- capture useful evidence before cleanup;
- no harness may automatically push/merge/default-branch mutate;
- final Git/path inspection remains Rack AI-owned.

## Required production behaviour

Integrate routing into the real implementation-worker path while preserving:

1. Rack AI chooses the registered worker/model according to existing campaign policy.
2. Rack AI then chooses that worker's preferred qualified harness.
3. The selected harness executes the implementation task.
4. A no-change result is rejected regardless of harness claims.
5. Rack AI inspects changed paths/Git independently.
6. Rack AI runs deterministic acceptance independently.
7. A fresh reviewer evaluates accepted-looking work independently of the implementation session/harness.
8. PR7 recovery can diagnose/replan/reassign after harness-backed failure without widening authority.
9. Durable campaign evidence records both worker/model and harness identity.
10. Optional harness fallback may occur only where policy explicitly permits and the fallback pairing was qualified in PR14.

## Cross-harness recovery rule

Do not automatically treat every implementation failure as a reason to switch harnesses.

Existing recovery should first classify the failure. Harness switching is appropriate only for bounded conditions such as harness/process/protocol incompatibility or an explicitly configured fallback strategy.

A coding defect should normally remain a coding repair/replan problem unless evidence indicates the selected harness itself is the limiting capability.

## Legacy path rule

PR15 should not perform broad deletion of the old direct model-facing coding loop; PR16 owns cleanup.

However, the production path proven at PR15 merge must genuinely route through the qualified external harness adapters. The old native loop must not remain the authoritative default while the new adapters sit unused.

## Required tests

Add deterministic coverage for at least:

- worker/model -> preferred harness mapping;
- optional qualified fallback mapping;
- no-qualified-harness failure;
- JCode request/process/config mapping where used;
- Abacus request/process/config mapping where used;
- exact endpoint/model binding for each active adapter;
- successful harness evidence capture;
- non-zero harness failure;
- timeout/cancellation and descendant cleanup;
- no-change rejection after harness completion;
- path-policy violation rejection;
- acceptance failure after harness completion;
- independent reviewer remains separate;
- recovery receives harness identity/failure evidence;
- no automatic push/merge/default-branch mutation;
- durable state retains harness identity/outcome;
- two worker profiles can use different harnesses without endpoint cross-binding.

Run existing relevant workspace/campaign/isolation tests as well.

## Required live proof

Run real local-vLLM implementations under Rack AI supervision for each production routing path established by PR14.

At minimum, if the expected current policy is confirmed, prove:

- `local-coder -> Abacus` on port 8018;
- `local-primary -> JCode` on port 8017.

If PR14 establishes different routing, prove that instead.

Evidence must show harness/version, worker/model/endpoint, target worktree, substantive diff, Rack AI acceptance and independent review outcome.

Manual harness invocation outside Rack AI is not sufficient.

## Required documentation

Update architecture/operations documentation with:

- qualified harnesses;
- worker/model routing configuration;
- adapter launch/configuration;
- evidence inspection;
- ownership boundary;
- fallback rules;
- residual legacy code awaiting PR16.

## Non-goals

- no new general-purpose Rack AI coding tools;
- no Rack AI-owned editor/search/LSP surface;
- no JCode swarm dependency;
- no dynamic learning scheduler;
- no objective planning;
- no adaptive multi-worker task scheduling;
- no web research;
- no frontend;
- no cloud/frontier escalation;
- no remote Git promotion;
- no broad legacy cleanup beyond what integration requires.

## Implementation-agent handoff

An agent assigned PR15 should:

1. read PR14's capability/routing report and this contract;
2. inspect current worker/model registry and execution boundaries;
3. implement the smallest harness-neutral abstraction;
4. implement only the adapters required by PR14-qualified production routes;
5. make worker/model -> harness routing explicit and versioned;
6. route the real implementation-worker path through it;
7. preserve all Rack AI gates;
8. add deterministic tests and real-rack proofs for each production route;
9. document residual native coding-loop code for PR16;
10. stop without implementing PR16 cleanup or PR17 qualification.

## Merge gate

PR15 merges only when Rack AI can route real production implementation attempts through the PR14-qualified harness/worker combinations end-to-end while retaining Rack AI-owned authority, acceptance, review, recovery and evidence guarantees.
