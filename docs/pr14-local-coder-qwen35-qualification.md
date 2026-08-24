# PR14 Addendum — Qwen3.5 Local-Coder Harness Qualification

Date: 2026-08-24  
Branch: `strategy/pr14-harness-qualification`

## Purpose

This addendum records the local-coder-specific evidence behind the final PR14 routing decision. The authoritative consolidated result is in `docs/pr14-harness-qualification-report.md`.

## Local-coder runtime

```text
worker = local-coder
GPU = RTX 2060 6 GB
endpoint = http://127.0.0.1:8018/v1
served_model = local-coder
model = NotaMG/eqaq-v2
family = Qwen3.5 4B text-only
vLLM = 0.27.1
context_window = 16368
max_num_seqs = 1
```

Native OpenAI-compatible tool calls and tool-result continuation were confirmed.

## Context/profile finding

Initial comparison results were distorted by incorrect harness context assumptions.

- JCode originally had no explicit `context_window` for `local-coder`.
- Abacus reported 128K against the real 16,368-token serving limit.

After correcting JCode to `context_window = 16368`, reactive context enforcement became visible.

The decisive JCode result was tool-profile overhead:

```text
JCode full:
  first useful turn ~12.8k input tokens
  too little practical repair runway

JCode minimal:
  first useful turn ~2.9k input tokens
  bounded multi-step repair completed
  strengthened rerun completed around ~6.8k
```

The small worker therefore qualifies only with the lightweight JCode profile currently tested.

## Multi-file repair result

JCode `minimal` successfully inspected the relevant Rust files, reproduced failures, iterated on compiler/test feedback and reached green tests.

A strengthened rerun explicitly prohibited changes to the signatures of `User::new`, `User::first_name` and `User::last_name`; the final implementation preserved those method signatures and passed tests.

One successful run left `src/user.rs.bak` and `src/user_debug.rs`. This is why Rack AI must independently inspect final Git status/diff and reject unexpected files. The route remains `qualified_with_constraints`.

## Truthful no-change

PASS.

On a known-correct fixture JCode `minimal`:

- inspected relevant source;
- ran Rust tests;
- correctly concluded no implementation change was needed;
- explicitly reported that conclusion;
- left an empty tracked Git diff.

Only test-generated `Cargo.lock` and `target/` artifacts were present.

## Network-disabled operation

PASS.

A qualification network guard blocked normal external IPv4/IPv6 connections while allowing loopback.

Independent guard proof:

```text
127.0.0.1:8018/v1/models -> reachable
https://example.com       -> blocked
```

JCode still completed local repository/test work through the local vLLM endpoint with no tracked source change.

## Dual-endpoint isolation

PASS.

Two JCode sessions ran concurrently:

```text
local-primary -> 8017 -> served model local-primary
local-coder   -> 8018 -> served model local-coder
```

Both completed successfully, ran tests and made no tracked changes. Because the endpoints expose different served model IDs, endpoint cross-binding would have produced a model-not-found failure; none occurred.

## Abacus result

Abacus was retested with the real 16,368-token limit and `--no-session`.

It demonstrated basic repository/tool capability but repeatedly failed the bounded multi-step repair qualification through provider-stream timeout, repair drift, a failing final repository and unexpected `AGENTS.md` creation.

Classification:

```text
Abacus = not_qualified
```

Abacus was uninstalled after local evidence preservation. It may be reconsidered after upstream improvements.

## Final local-coder decision

```text
preferred_harness = JCode
JCode_tool_profile = minimal
context_window = 16368
max_num_seqs = 1
classification = qualified_with_constraints
fallback_harness = none
```

The local-coder is suitable initially for bounded implementation work with deterministic acceptance and independent final worktree inspection.

## Swarm decision

JCode swarm is deferred.

With `max_num_seqs = 1` on the current coder and only two useful concurrent worker endpoints, swarm-lite offers little advantage over Rack AI directly scheduling one task per worker. Deep/DAG swarm also overlaps with responsibilities intentionally owned by Rack AI.

Future multi-GPU optimisation remains a Rack AI scheduling concern.
