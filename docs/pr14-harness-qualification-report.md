# PR14 Harness Qualification Report

Date opened: 2026-08-23  
Latest qualification update: 2026-08-24  
Branch: `strategy/pr14-harness-qualification`

## Scope

This report executes PR14 only.
It qualifies the current Rust-native coding harness candidates against the live rack workers and records the evidence needed for an initial harness-routing policy for later PR15 integration.

This PR does **not** integrate a harness into Rack AI.
It does **not** revive JCode swarm.
It does **not** add a second bespoke Rack AI coding-agent implementation.

A material finding on 2026-08-24 changed the qualification problem: context capacity and context-management policy are now part of the routing/supervision contract. PR14 must not merge until the current workers are tested with their **true context limits advertised to the harnesses**, compaction/recovery behaviour is understood, and Rack AI has a defensible policy for stopping/escalating a small worker before hard context exhaustion.

## Candidate versions

- JCode currently installed during the 2026-08-24 Qwen3.5 tests: `v0.79.1` (`993da322e`)
- Earlier PR14 evidence used JCode `v0.78.1` (`03ddbcfc8`)
- Abacus installed version: `abacus-agent v0.6.1`
- Earlier Abacus source qualification checkout recorded SHA `6519766878c8667a4e9a2103f992b0f1b7ba109b`

Any final PR14 classification must record the exact harness revision used for the final matrix.

## Rack model/runtime state

### `local-primary`

- GPU: RTX 4060 Ti 16 GB
- endpoint: `http://127.0.0.1:8017/v1`
- model: `cyankiwi/gemma-4-12B-it-AWQ-INT4`
- served context: `131072`
- role: coordinator/planner/reviewer and stronger implementation/fallback worker

### `local-coder` — superseded model

Earlier PR14 evidence used:

- GPU: RTX 2060 6 GB
- endpoint: `http://127.0.0.1:8018/v1`
- model: `Qwen/Qwen2.5-Coder-3B-Instruct-AWQ`
- served context: `32768`

That model failed material harness tests through false-success/no-diff, raw tool JSON and incorrect claims. It is retained below only as historical evidence and is no longer the active coder candidate.

### `local-coder` — current Qwen3.5 candidate

Current validated serving combination:

- GPU: RTX 2060 6 GB
- endpoint: `http://127.0.0.1:8018/v1`
- model: `NotaMG/eqaq-v2`
- family: Qwen3.5 4B text-only
- quantization: 4-bit compressed-tensors
- vLLM: `0.27.1`
- served model name: `local-coder`
- `max_model_len`: `16368`
- `max_num_seqs`: `1`
- CUDA graphs enabled; `--enforce-eager` is not used
- tool-call parser: `qwen3_coder`
- reasoning parser: `qwen3`

This combination has independently demonstrated native OpenAI-compatible `tool_calls` and correct tool-result continuation through the vLLM endpoint.

## Historical Qwen2.5 qualification evidence

The original PR14 disposable fixture used `/tmp/pr14-fixture-base` at base commit `a9114bf2bd118ad50b1580b8145c14768e4d912c`, with evidence under `/tmp/pr14-evidence/`.

### Historical result matrix

| Harness | Worker/model | Task | Result | Duration | Evidence |
| --- | --- | --- | --- | ---: | --- |
| JCode | `local-primary` | localized additive change | pass | 95s | `/tmp/pr14-evidence/jcode_primary_t1.*` |
| JCode | old `local-coder` / Qwen2.5 | localized additive change | fail: false success, no source diff | 44s | `/tmp/pr14-evidence/jcode_coder_t1.*` |
| Abacus | `local-primary` | localized additive change | pass | 50s | `/tmp/pr14-evidence/abacus_primary_t1.*` |
| Abacus | old `local-coder` / Qwen2.5 | localized additive change | fail: false success, no source diff | 11s | `/tmp/pr14-evidence/abacus_coder_t1.*` |
| JCode | `local-primary` | structural multi-file change | pass | 27s | `/tmp/pr14-evidence/jcode_primary_t2.log` |
| Abacus | `local-primary` | structural multi-file change | fail: timeout, no source diff | 240s | `/tmp/pr14-evidence/abacus_primary_t2.log` |
| JCode | old `local-coder` / Qwen2.5 | read-only navigation | fail: raw tool-call JSON | 3s | `/tmp/pr14-evidence/jcode_coder_read.log` |
| Abacus | old `local-coder` / Qwen2.5 | read-only navigation | fail: incorrect answer and incorrect files-changed claim | 11s | `/tmp/pr14-evidence/abacus_coder_read.log` |

Historical classification from that model was:

```text
JCode:
  local-primary: qualified
  old local-coder/Qwen2.5: not_qualified

Abacus:
  local-primary: qualified_with_constraints
  old local-coder/Qwen2.5: not_qualified
```

That old `local-coder -> none` decision is **superseded as a current conclusion** by the Qwen3.5 requalification work below.

## 2026-08-24 Qwen3.5 local-coder requalification

The replacement coder model was tested directly against both Rust harnesses on disposable Rust repositories.

### Serving/tool protocol checks

`NotaMG/eqaq-v2` passed:

1. stable vLLM startup on the RTX 2060;
2. a practical 16K-class context (`16368` tokens);
3. native structured OpenAI-compatible function/tool calls;
4. correct continuation after a tool result.

This resolves the most serious protocol defect of the old Qwen2.5 coder.

### Simple real repository edit fixture

Repository: `/tmp/qwen35-jcode-smoke`

Initial defect:

```rust
pub fn add(a: i32, b: i32) -> i32 {
    a - b
}
```

Expected implementation: `a + b`.

Results:

| Harness | Result | Evidence/observation |
| --- | --- | --- |
| JCode + Qwen3.5 | PASS | inspected repo, edited real source, ran `cargo test`, tests passed, truthful completion |
| Abacus + Qwen3.5 | PASS | inspected repo, edited real source, ran `cargo test`, tests passed, truthful completion |

Independent Git inspection showed only the intended source mutation; `Cargo.lock` and `target/` were test-generated artifacts.

### Single-file repair fixture

Repository: `/tmp/qwen35-repair-smoke`

Initial defect:

```rust
.map(|name| name.trim().to_lowercase())
.filter(|name| name.is_empty())
```

Expected repair:

```rust
.filter(|name| !name.is_empty())
```

Results:

| Harness | Result | Evidence/observation |
| --- | --- | --- |
| JCode + Qwen3.5 | PASS | minimal implementation-only repair; tests passed |
| Abacus + Qwen3.5 | PASS | same minimal semantic repair; tests passed |

These results establish that the RTX 2060/Qwen3.5 worker is genuinely useful for bounded coding work. It must no longer be treated as an unqualified no-op worker simply because the superseded Qwen2.5 model failed.

### Multi-file compatibility/repair fixture

Repository: `/tmp/qwen35-multifile-smoke`

Shape:

- `src/lib.rs`
- `src/user.rs`
- `src/formatter.rs`
- public `User` API had to remain intact
- test expected `User::new(" Alice ", "SMITH")` to display as `"Alice Smith"`
- harness had to inspect relevant files, preserve the public API, make a correct implementation change and react to test feedback

#### JCode result

JCode successfully:

- found all Rust source files;
- ran and interpreted the initial failing test;
- inspected `user.rs`, `lib.rs` and `formatter.rs`;
- made real source edits;
- reran tests and observed a second meaningful failure.

However the run reached approximately:

- first tool call: `12587` input tokens;
- later repair turn: `15649` input tokens;
- model hard limit: `16368` tokens.

After the second failing test the trace repeatedly cycled through `sending request` / `waiting for response` without a new useful tool turn. The worktree remained partially modified and tests were still failing.

Observed JCode outcome:

```text
multi-file compatibility/repair = FAIL
failure mode = practical context exhaustion / no safe remaining repair runway
repository = partially modified, tests failing
```

#### Abacus result

Abacus successfully:

- inspected all three relevant files;
- reproduced the failing test;
- diagnosed whitespace/casing issues;
- made multiple source edits;
- reran tests and responded to additional failures.

It then over-expanded the repair, introduced invalid Rust (`std::iter::Itertools` and an invalid iterator-to-String conversion), and the provider stream timed out. The final worktree did not compile.

Observed Abacus outcome:

```text
multi-file compatibility/repair = FAIL
failure mode = repeated repair growth followed by provider timeout
repository = modified, does not compile
```

This result initially appeared to demonstrate the capability ceiling of the 4B worker. The subsequent diagnostics below show that interpretation was incomplete because neither harness was operating with the true `16368` context limit configured.

## Critical context-window configuration finding

This is now a first-class PR14 architectural finding.

### Abacus

`abacus doctor` on the active local profile reported:

```text
profile      local
model        local-coder
endpoint     http://127.0.0.1:8018/v1/chat/completions
limits       128000 context · auto output · default · compacts near 364185 chars
```

The real vLLM model limit is only `16368` tokens.

Therefore Abacus currently believes the coder has roughly 7.8x more token context than it actually has and schedules compaction far beyond the physical model limit. Its automatic context-management behaviour was not being exercised at the point where this worker actually needed it.

### JCode

The active JCode provider profile contains:

```toml
[providers.local-coder]
type = "open-ai-compatible"
base_url = "http://127.0.0.1:8018/v1"
default_model = "local-coder"
requires_api_key = false
provider_routing = false
model_catalog = false
allow_provider_pinning = false

[[providers.local-coder.models]]
id = "local-coder"
```

There is no explicit `context_window = 16368` declaration for the model.

The multi-file trace then reached `15649` input tokens without an observed successful compaction before the hard `16368` serving limit.

### Consequence

The 2026-08-24 multi-file failures are valid evidence that the **current configured systems fail under context pressure**, but they are **not yet valid evidence that the 4B model itself cannot complete the task under correctly configured context management**.

PR14 must therefore re-run the relevant pressure test after each harness is explicitly configured with the true worker context window.

## Architectural conclusion: context is a supervised resource

Rack AI must treat context capacity in the same class as VRAM, wall time and process budget: a deterministic resource owned by the control plane, not an informal property left to model self-awareness.

The small worker must not be expected to notice on its own that it is about to exhaust context.

The required architecture is conceptually:

```text
Rack AI
  |
  +-- task admission / complexity classification
  |
  +-- worker + harness selection
  |
  +-- context budget governor
        |
        +-- continue
        +-- compact / checkpoint
        +-- stop local worker
        +-- escalate / resume on stronger worker
```

Coding harnesses may own the mechanics of their supported local compaction, but Rack AI owns the policy and the decision about whether continuation remains safe.

## Context Budget Governor requirements

PR14 now requires a design/qualification result for the following behaviour. PR15 may implement the production adapter/control path later, but PR14 must prove the concept and define the contract.

### 1. Exact worker context registration

Every registered worker/model profile must carry its real served context capacity.

For the current coder:

```text
worker = local-coder
model = NotaMG/eqaq-v2
max_context_tokens = 16368
```

Harness configuration must not silently substitute a generic 128K/default window.

### 2. Reserved operating headroom

Rack AI must not intentionally consume the model's absolute maximum context before deciding what to do next.

Initial bands to test for the 16,368-token coder are:

```text
0-60%    NORMAL
60-72%   WATCH
72-80%   COMPACT
80-88%   CHECKPOINT / DECIDE
>88%     DO NOT START ANOTHER LARGE STEP
```

These percentages are hypotheses for qualification, not yet production constants. PR14 should tune them from observed harness behaviour.

Approximate token boundaries:

```text
60% ~= 9,821
72% ~= 11,785
80% ~= 13,094
88% ~= 14,404
```

The observed JCode first tool call at ~12,587 tokens demonstrates why harness overhead must be measured as part of task admission and routing.

### 3. Task admission / capability envelope

Routing must consider expected task shape, not simply `coding task -> local-coder`.

A starting classification to validate is:

```text
Tier A: local-coder preferred
- localized one-file changes
- known/narrow implementation points
- straightforward deterministic failures
- small mechanical edits

Tier B: local-coder conditional
- small multi-file changes
- limited repository discovery
- one bounded repair iteration expected
- compatibility changes with a narrow surface

Tier C: local-primary preferred
- architectural work
- broad repository discovery
- high ambiguity
- refactors
- many files/modules
- repeated compile/test repair expected
```

Routing must ultimately be based on measured worker/model/harness capability metadata, not hard-coded parameter-count rules.

### 4. Compaction before exhaustion

A qualifying harness route must demonstrate that compaction is triggered while sufficient output/tool-call runway remains.

Compaction must preserve the information needed for correct continuation and must not silently discard critical task constraints or recent failure evidence.

Built-in JCode/Abacus context management should be evaluated first. A third-party layer such as Headroom may be evaluated only if the built-in mechanisms are insufficient or if it provides a demonstrable operational advantage.

### 5. Structured checkpoint before stop/escalation

Compaction alone is not sufficient. Before a context-pressure stop or worker escalation, Rack AI must be able to persist a compact state equivalent to:

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

The exact schema may change, but the persisted checkpoint must be deterministic enough for another worker to continue without replaying the entire prior transcript.

### 6. Escalation is a normal recovery path

A small worker is not required to finish every task it starts.

Useful work may include:

- repository discovery;
- reproducing the bug;
- narrowing the cause;
- making a bounded first-pass patch;
- producing compiler/test feedback.

If context or repair complexity exceeds the worker's safe envelope, Rack AI should stop the local-coder cleanly and allow `local-primary` to continue from:

- original goal;
- structured checkpoint;
- current Git diff;
- latest deterministic compiler/test output.

This should be classified as controlled recovery/escalation, not as a mysterious provider crash or fabricated success.

## Context-management qualification track

Ordinary PR14 matrix expansion is paused until this track is resolved.

### C1 — correct harness context configuration

- configure JCode `local-coder` model metadata with the actual `16368` context window;
- configure Abacus local profile/model limits so diagnostics report the actual `16368` context window rather than `128000`;
- verify each harness reports/uses the intended value.

### C2 — built-in compaction pressure test

Re-run the same `/tmp/qwen35-multifile-smoke` task with:

- identical repository state;
- identical prompt;
- identical vLLM endpoint/model;
- correctly configured context metadata.

Record:

- token usage by turn where available;
- compaction trigger point;
- content retained/removed;
- whether task completion improves;
- final Git diff;
- final tests;
- truthful completion/failure state.

This is a new experiment, not an attempt to erase the previous failures.

### C3 — optional Headroom evaluation

Only if built-in compaction is insufficient or operationally poor, deploy Headroom on `gpurack` and repeat the same fixture. Prefer transparent/automatic compression over requiring the 4B model to remember to invoke an MCP tool when context is already scarce.

Measure at minimum:

- input tokens/context occupancy;
- tool turns;
- compression events;
- retrieval events;
- wall time;
- final correctness.

Sequential-thinking MCP is not considered a context-exhaustion solution. It may later be tested for reasoning quality, but additional reasoning/tool turns can increase context consumption.

### C4 — forced safe escalation

Create a task intentionally beyond the 2060 worker's safe envelope and prove a control flow equivalent to:

```text
local-coder begins bounded work
-> context/repair budget threshold reached
-> worker stops without claiming success
-> structured checkpoint + diff + latest test evidence persisted
-> local-primary resumes from compact state
-> Rack AI independently accepts/rejects final result
```

PR14 need not implement the final production scheduler, but the contract, evidence and minimum reproducible mechanism must be sufficient for PR15/PR7 recovery integration.

## Context-management acceptance standard

Do not consider this problem solved until evidence supports all of the following:

1. Rack AI knows every active worker's real context capacity.
2. Harnesses are configured with that capacity rather than a generic default.
3. A worker is not allowed to consume the absolute hard limit without a prior control decision.
4. Compaction occurs before hard exhaustion when continuation is appropriate.
5. Compaction preserves enough task state for correct continuation.
6. Repeated repair loops cannot silently burn through the context window indefinitely.
7. Context pressure can trigger a controlled stop rather than false success or an opaque hang.
8. A structured checkpoint is persisted before escalation.
9. Another qualified worker can resume from that checkpoint without the complete transcript.
10. Context exhaustion is represented as a normal recoverable Rack AI control-plane event.

## Current provisional capability classification

These are **provisional** until the context-management track is complete.

### `local-primary`

- JCode: `qualified`
- Abacus: `qualified_with_constraints`

### `local-coder` / Qwen3.5

- JCode: `provisionally qualified_with_constraints` for bounded/localized work
- Abacus: `provisionally qualified_with_constraints` for bounded/localized work
- neither harness is yet qualified for longer multi-step repair work
- the multi-file failures must be reinterpreted after correct context configuration and compaction testing

## Provisional routing direction

Do **not** freeze the final PR15 routing policy yet.

The evidence currently supports this architectural direction:

```text
local-coder
  -> bounded/localized implementation work
  -> harness preference TBD from context-corrected PR14 tests
  -> compact/checkpoint before context exhaustion
  -> escalate to local-primary when safe envelope is exceeded

local-primary
  -> JCode preferred for stronger/broader implementation and review work
  -> Abacus remains a possible constrained fallback
```

The final routing policy must combine:

- worker/model capability envelope;
- harness qualification;
- true context capacity;
- task shape/complexity;
- recovery/escalation policy.

## Residual PR14 gaps

Still required before merge:

- context-corrected C1/C2 tests;
- context checkpoint/escalation proof or sufficiently concrete PR14 mechanism/fixture;
- truthful no-change behaviour for the new Qwen3.5 coder;
- hard network-disabled relevant harness execution;
- simultaneous dual-endpoint sessions proving no cross-binding;
- final capability matrix and deterministic routing policy based on the current Qwen3.5 coder rather than the superseded Qwen2.5 evidence.

## PR15 gate

Do not begin production PR15 harness integration until PR14 has resolved the context-budget problem sufficiently to define:

- exact worker context metadata;
- safe continuation/compaction/stop semantics;
- checkpoint artifact requirements;
- escalation/fallback expectations;
- final harness route for each current worker/model profile.

## Machine-readable summary — provisional

```text
PR14_STATUS = CONTEXT_MANAGEMENT_QUALIFICATION_IN_PROGRESS
QUALIFIED_HARNESSES = jcode,abacus
LOCAL_CODER_MODEL = NotaMG/eqaq-v2
LOCAL_CODER_CONTEXT = 16368
LOCAL_CODER_PREFERRED_HARNESS = pending_context_corrected_tests
LOCAL_PRIMARY_PREFERRED_HARNESS = jcode
CONTEXT_GOVERNOR_REQUIRED = true
PR15_READY = false
```
