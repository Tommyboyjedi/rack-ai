# PR14 Harness Qualification Report

Date opened: 2026-08-23  
Final qualification update: 2026-08-24  
Branch: `strategy/pr14-harness-qualification`

## Purpose

PR14 qualifies the Rust-native coding harness layer against the actual rack workers and establishes the initial production routing policy for later Rack AI integration.

This PR does **not** integrate the harness into Rack AI. It records the evidence and the architectural boundary that the implementation PR must follow.

## Final decision

The qualification outcome is:

```text
JCode direct/non-swarm:
  local-primary: qualified
  local-coder: qualified_with_constraints

Abacus:
  not_qualified for Rack AI production use at this time

Initial routing:
  local-primary -> JCode
  local-coder   -> JCode minimal
```

JCode swarm is not part of the current production path. The current two-card rack already has only one active sequence on the RTX 2060 coder (`max_num_seqs = 1`), so swarm-lite does not provide meaningful additional parallelism over Rack AI directly scheduling one task per worker. Swarm may be re-evaluated when the rack has more genuinely concurrent workers or a concrete capability that Rack AI cannot provide more cleanly itself.

## Architectural boundary

Rack AI remains the control plane.

Rack AI owns:

- campaign and task lifecycle;
- worker/model/GPU registration and placement;
- deciding which worker receives which task;
- target repository/worktree authority;
- filesystem, network and process isolation;
- timeout/cancellation policy;
- deterministic acceptance;
- independent semantic review;
- no-change rejection;
- retry/replan/fallback/escalation;
- Git commit/promotion authority;
- final evidence and audit state.

JCode owns model-facing coding mechanics inside the bounded execution environment:

- source navigation/search;
- editing and patching;
- tool-call handling;
- implementation-time command execution;
- compiler/test feedback loops;
- harness-local context management.

Harness-reported success is evidence, not acceptance. Rack AI must independently inspect the final worktree and acceptance results.

## Runtime under qualification

### `local-primary`

- GPU: RTX 4060 Ti 16 GB
- endpoint: `http://127.0.0.1:8017/v1`
- served model: `local-primary`
- model root: `cyankiwi/gemma-4-12B-it-AWQ-INT4`
- served context: `131072`

### `local-coder`

- GPU: RTX 2060 6 GB
- endpoint: `http://127.0.0.1:8018/v1`
- served model: `local-coder`
- model root: `NotaMG/eqaq-v2`
- family: Qwen3.5 4B text-only
- vLLM: `0.27.1`
- quantization: 4-bit compressed-tensors
- served context: `16368`
- `max_num_seqs = 1`
- CUDA graphs enabled
- tool-call parser: `qwen3_coder`
- reasoning parser: `qwen3`

The local-coder model independently demonstrated native structured OpenAI-compatible tool calls and correct continuation after tool results.

## Harness versions

Final JCode qualification was performed with:

```text
JCode v0.79.1 (993da322e)
```

Earlier evidence also used JCode v0.78.1 and is retained only as historical context.

Abacus qualification used:

```text
abacus-agent v0.6.1
```

Abacus was removed from the rack after qualification evidence was preserved under local PR14 evidence storage.

## Historical Qwen2.5 result

The original `local-coder` model was `Qwen/Qwen2.5-Coder-3B-Instruct-AWQ`.

It failed important tool/protocol and truthful-completion tests. That historical `local-coder -> none` conclusion is superseded by the Qwen3.5 qualification below.

## Qwen3.5 local-coder qualification

### Native tool protocol

PASS.

The model emitted real OpenAI-compatible `tool_calls` and correctly continued from tool results through vLLM.

### Simple real repository edit

PASS through JCode.

The model inspected source, fixed a real implementation defect, ran `cargo test`, observed passing tests and reported completion truthfully.

### Single-file compiler/test repair

PASS through JCode.

The model repaired a semantic defect with a small implementation change and passed deterministic tests.

### Multi-file compatibility/repair

This fixture was the critical qualification task.

The repository contained:

- `src/lib.rs`;
- `src/user.rs`;
- `src/formatter.rs`;
- a public `User` API that had to remain intact;
- a failing test expecting `User::new(" Alice ", "SMITH")` to display as `"Alice Smith"`.

#### Initial JCode `full` result

The first useful model/tool turn was approximately 12.8k input tokens against a hard 16,368-token model window.

After correcting JCode model metadata to explicitly declare:

```toml
context_window = 16368
```

reactive compaction became visible, proving the original configuration had been incorrect. However the `full` tool profile still consumed too much static context overhead for the small worker and left insufficient repair runway.

`JCode full` is therefore not the approved local-coder profile.

#### JCode `minimal` result

The same worker with:

```text
tool_profile = minimal
context_window = 16368
```

reduced the first useful interaction to roughly 2.9k input tokens.

The worker then had enough context to:

- inspect the repository;
- run tests;
- react to repeated compiler/test feedback;
- make substantive edits;
- complete a passing test run.

A strengthened rerun explicitly prohibited changing the signatures of `User::new`, `User::first_name` and `User::last_name`. That run preserved the public method signatures and completed with passing tests at roughly 6.8k input tokens.

One successful run left unnecessary `src/user.rs.bak` and `src/user_debug.rs` artifacts. This is the reason the route remains `qualified_with_constraints` rather than unconditional: Rack AI must independently reject unexpected files and inspect final Git state.

### Truthful no-change test

PASS.

A known-correct fixture was presented to JCode `minimal` on `local-coder`.

JCode:

- inspected the relevant implementation;
- ran the relevant Rust tests;
- correctly concluded the implementation was already correct;
- explicitly reported that no implementation change was required;
- made no tracked source changes.

Independent result:

```text
git diff: empty
cargo test: pass
```

Only generated `Cargo.lock` and `target/` artifacts were present after test execution.

### Network-disabled execution

PASS.

For qualification, JCode and its child tools were launched with an `LD_PRELOAD` network guard that blocked normal external IPv4/IPv6 connections while allowing loopback.

The guard was independently verified before the harness run:

```text
127.0.0.1:8018/v1/models -> reachable
https://example.com       -> blocked
```

JCode then successfully used the local-coder endpoint, inspected the repository, ran local tests and left no tracked source diff while external network access was blocked.

This proves the harness can operate under a network-denied outer policy while retaining access to the local vLLM endpoint. Production Rack AI isolation should use its normal kernel/container/process boundary rather than relying on `LD_PRELOAD`; the preload guard was only the PR14 qualification mechanism.

### Dual-endpoint isolation

PASS.

Two independent JCode sessions were launched concurrently:

```text
session A -> local-primary -> 127.0.0.1:8017
session B -> local-coder   -> 127.0.0.1:8018
```

The endpoints advertised distinct served model IDs:

```text
8017 -> local-primary -> cyankiwi/gemma-4-12B-it-AWQ-INT4
8018 -> local-coder   -> NotaMG/eqaq-v2
```

Both concurrent sessions successfully completed repository inspection and Rust test execution and truthfully reported that no changes were required.

Because the two vLLM endpoints expose different served model IDs, cross-binding would have produced a model-not-found failure of the same class as the previously observed JCode swarm rebinding defect. Neither session produced such a failure.

Independent final result:

```text
tracked git diff: empty
both test executions: pass
```

Direct JCode provider/model routing is therefore qualified for simultaneous use of the two local endpoints.

## Abacus qualification result

Abacus was tested against the rack and initially passed small localized tasks.

However, once the Qwen3.5 multi-step repair fixture was used, repeated material failures were observed even after correcting its context assumption to the real 16,368-token limit and using `--no-session`:

- provider-stream timeouts before successful completion;
- repair loops that drifted into unnecessary or incorrect implementation approaches;
- final repository left failing on the bounded multi-file task;
- unexpected `AGENTS.md` creation;
- no operational advantage over JCode `minimal` for the small-worker role.

The key Rack AI requirement is not whether a harness can perform any coding action; it is whether the harness qualifies for a defensible production role in the rack.

Current classification:

```text
Abacus = not_qualified
```

Rack AI will not carry a second production harness merely to preserve optionality. Abacus may be reconsidered after upstream improvements. Other Rust-native harnesses may be qualified in the future using the same evidence-driven process.

## Context-management conclusion

The early PR14 investigation temporarily suggested Rack AI needed a new mandatory context-budget governor/checkpoint subsystem before the small worker could be considered usable.

The corrected evidence does **not** support making that a PR14 merge blocker.

The material findings are simpler:

1. exact served context capacity is worker/model metadata and must be configured correctly;
2. harness/tool-profile overhead materially affects effective usable context;
3. JCode `minimal` gives the 16,368-token worker sufficient practical runway for the tested bounded task class;
4. Rack AI should still route broader/harder work to a stronger qualified worker when appropriate;
5. future recovery/checkpoint sophistication may be added from measured production need rather than invented prematurely.

There is therefore no requirement in PR14 to implement or prove a bespoke context-budget governor or automatic checkpoint/escalation subsystem.

## Initial routing policy

### `local-coder`

```text
worker: local-coder
GPU: RTX 2060 6 GB
model: NotaMG/eqaq-v2
harness: JCode
JCode tool profile: minimal
context_window: 16368
max_num_seqs: 1
classification: qualified_with_constraints
```

Initial task envelope:

- localized implementation work;
- small mechanical changes;
- bounded compiler/test repair;
- narrow multi-file compatibility changes where deterministic acceptance exists.

Rack AI must independently inspect final Git state and deterministic acceptance before accepting work.

### `local-primary`

```text
worker: local-primary
GPU: RTX 4060 Ti 16 GB
model: cyankiwi/gemma-4-12B-it-AWQ-INT4
harness: JCode
classification: qualified
```

The primary remains the stronger route for broader reasoning, planning, review and harder implementation work.

## Multi-GPU scheduling

PR14 does not implement adaptive routing between the two GPUs.

The later scheduling work should let Rack AI decide when:

- a bounded implementation belongs on the 2060;
- harder implementation belongs on the 4060 Ti;
- two independent tasks can execute concurrently, one per worker;
- work should be reassigned from the small worker to the stronger worker.

That scheduling responsibility remains in Rack AI rather than JCode swarm.

## Final capability matrix

| Harness | Worker | Result | Production role |
| --- | --- | --- | --- |
| JCode direct | `local-primary` | `qualified` | preferred stronger worker harness |
| JCode `minimal` | `local-coder` | `qualified_with_constraints` | preferred bounded small-worker harness |
| JCode `full` | `local-coder` | not approved for this route | excessive static context overhead |
| Abacus | current rack | `not_qualified` | none |
| JCode swarm | current rack | deferred | no present value over Rack AI direct scheduling |

## PR15 gate

PR15 may now implement the production JCode adapter/control path.

PR15 must preserve the architectural boundary:

```text
Rack AI
  -> selects worker/model/task
  -> invokes JCode with the approved worker profile
  -> enforces outer workspace/network/process/time limits
  -> runs independent acceptance/review
  -> owns Git/promotion/evidence
```

PR15 should not reintroduce Abacus, JCode swarm dependency, or another bespoke Rack AI coding-agent loop unless new qualification evidence justifies it.

## Machine-readable summary

```text
QUALIFIED_HARNESSES = jcode
LOCAL_PRIMARY_PREFERRED_HARNESS = jcode
LOCAL_CODER_PREFERRED_HARNESS = jcode
LOCAL_CODER_JCODE_TOOL_PROFILE = minimal
LOCAL_CODER_CONTEXT_WINDOW = 16368
LOCAL_CODER_MAX_NUM_SEQS = 1
ABACUS = not_qualified
JCODE_SWARM = deferred
CONTEXT_GOVERNOR_REQUIRED_FOR_PR15 = false
PR15_READY = true
```
