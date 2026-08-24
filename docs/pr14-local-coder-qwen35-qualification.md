# PR14 Addendum — Qwen3.5 Local-Coder Harness Qualification

Date: 2026-08-24
Branch: `strategy/pr14-harness-qualification`

## Purpose

This addendum records the qualification evidence gathered after replacing the original `local-coder` model with `NotaMG/eqaq-v2` (Qwen3.5 4B text-only) on the RTX 2060 6 GB worker.

The purpose is not to select multiple harnesses for their own sake. A harness must independently qualify for Rack AI. Rack AI may support more than one harness in the future, but only if each candidate demonstrates a defensible production role.

## Local-coder runtime under test

- worker: `local-coder`
- GPU: RTX 2060 6 GB
- endpoint: `http://127.0.0.1:8018/v1`
- model root: `NotaMG/eqaq-v2`
- architecture: Qwen3.5 4B text-only
- serving runtime: vLLM 0.27.1
- model context limit: 16,368 tokens
- native OpenAI-style tool calls: confirmed
- tool-result continuation: confirmed

## Critical context-window configuration finding

Initial harness comparison was invalidated in part by incorrect harness assumptions about the local-coder context size.

Observed configuration before correction:

- Abacus `doctor` reported `128000 context` and compaction near roughly 364k characters.
- JCode had no explicit `context_window` on the `local-coder` model entry.
- the actual vLLM serving limit was 16,368 tokens.

After correction:

- JCode was configured with `context_window = 16368`.
- Abacus was invoked with `--context-window 16368`.

This materially changed JCode behaviour: it began enforcing the true context ceiling and reactive compaction became visible.

## JCode profile-size finding

The most important local-coder harness result was the difference between JCode tool profiles.

### JCode `full`

On the multi-file repair fixture, the first useful model/tool turn was approximately 12.8k input tokens. This left too little practical runway inside a 16,368-token context window. Reactive compaction occurred once the correct context window was configured, but the run remained context-constrained.

### JCode `minimal`

On the same worker and same task class, the first useful tool turn dropped to approximately 2.9k input tokens.

This provided sufficient working room for:

- repository inspection;
- repeated compiler/test feedback;
- multiple repair attempts;
- final green `cargo test` execution.

A later constraint-strengthened rerun completed around 6.8k input tokens while preserving the required public method signatures.

The run did leave unnecessary source-adjacent artifacts (`src/user.rs.bak` and `src/user_debug.rs`), which Rack AI final acceptance must reject or clean. This is a harness/model quality issue, but not a context-exhaustion failure.

### JCode local-coder conclusion

`JCode + minimal tool profile` is the current preferred harness configuration for the Qwen3.5 local-coder worker.

Classification: `qualified_with_constraints`.

Current constraints include:

- use the `minimal` tool profile on the 16,368-token local-coder profile;
- Rack AI must independently validate final Git status/diff and reject unexpected artifacts;
- Rack AI must independently enforce task constraints and deterministic acceptance;
- broader/harder task classes still require qualification before being routed to the small worker.

## Abacus evidence

Abacus was retested after correcting the context limit to 16,368 and disabling session persistence with `--no-session`.

Observed positive behaviour:

- repository discovery worked;
- source files were inspected;
- failing tests were reproduced;
- edits were attempted;
- test feedback was interpreted;
- Abacus built-in compaction was visible in traces.

Observed material failures across repeated bounded repair runs:

- repeated provider-stream timeouts before successful completion;
- unnecessary or incorrect implementation directions after compiler/test feedback;
- final repository left failing on the multi-file repair fixture;
- unexpected workspace artifact creation (`AGENTS.md`);
- no demonstrated operational advantage over JCode `minimal` on the small worker.

The issue is not that Abacus cannot perform any coding actions. The qualification question is whether it is reliable enough for Rack AI's production worker role. On the RTX 2060 / Qwen3.5 4B / 16,368-token worker, current evidence says no.

### Abacus local-coder conclusion

Classification: `not_qualified` for the current `local-coder` worker.

Rack AI should not integrate Abacus as a production harness merely to preserve multi-harness optionality. Multi-harness support remains a future capability only when another Rust-native harness independently meets Rack AI qualification requirements.

## Product-level interpretation

A core Rack AI product goal is to make useful smaller GPUs viable as bounded workers.

The Qwen3.5 4B local-coder results support that goal when the harness is appropriately matched to the worker:

- incorrect context metadata can make a viable small worker appear unusable;
- excessive harness/tool-schema overhead can consume most of a small context window before useful work begins;
- a lightweight harness profile materially improves the practical capability of the same model and GPU;
- small-worker harness qualification must therefore include effective prompt/tool overhead, not merely model context size.

For the current rack, the evidence supports JCode `minimal` as the practical local-coder path.

## Revised local-coder routing decision

```text
local-coder -> JCode minimal preferred
Abacus      -> not qualified for local-coder
```

This does not preclude future Abacus reconsideration after upstream improvements. It also does not preclude qualifying another Rust-native harness later.

## Rack AI acceptance implication

The JCode runs demonstrate why Rack AI must remain the outer acceptance authority even when the harness completes successfully.

Rack AI final acceptance must independently inspect at least:

- deterministic tests/checks;
- Git diff;
- Git status;
- unexpected/unapproved files;
- preservation of required public contracts;
- truthful no-change behaviour;
- final repository validity.

Harness-reported success is evidence, not acceptance.

## Current PR14 position

For `local-coder`:

```text
JCode minimal: qualified_with_constraints; preferred
Abacus:        not_qualified
```

For `local-primary`, retain the existing PR14 evidence pending final report consolidation.

Abacus should not be included in PR15 production integration for the local-coder route. The project should keep monitoring Rust-native harnesses and may re-open qualification when a candidate materially improves the small-worker story.
