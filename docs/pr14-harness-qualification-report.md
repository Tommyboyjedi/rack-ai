# PR14 Harness Qualification Report

Date: 2026-08-23
Branch: `strategy/pr14-harness-qualification`
Rack AI SHA under test: `e1a24f0`

## Scope

This report executes PR14 only.
It qualifies the current Rust-native coding harness candidates against the live rack workers and records the initial harness-routing policy for later PR15 integration.

This PR does **not** integrate a harness into Rack AI.
It does **not** revive JCode swarm.
It does **not** add new Rack AI coding-agent functionality.

## Candidate versions

- JCode: `v0.78.1` (`03ddbcfc8`)
- Abacus repository SHA: `6519766878c8667a4e9a2103f992b0f1b7ba109b`
- Abacus build result: `abacus-agent v0.6.1` built locally on `gpurack` from the SHA above

## Rack model/runtime state

From the live vLLM endpoints on 2026-08-23:

- `local-primary`
  - endpoint: `http://127.0.0.1:8017/v1`
  - root model: `cyankiwi/gemma-4-12B-it-AWQ-INT4`
  - max context reported: `131072`
- `local-coder`
  - endpoint: `http://127.0.0.1:8018/v1`
  - root model: `Qwen/Qwen2.5-Coder-3B-Instruct-AWQ`
  - max context reported: `32768`

## Test method

All mutation tests used the same disposable Rust repository fixture at base SHA:

- fixture repo: `/tmp/pr14-fixture-base`
- fixture base commit: `a9114bf2bd118ad50b1580b8145c14768e4d912c`

Each run copied that fixture to a fresh disposable worktree under `/tmp/pr14-runs/<case>`.
Raw transcripts, diffs, acceptance logs and metadata were written under `/tmp/pr14-evidence/`.

## Tasks

### T1: localized additive change

Prompt summary:
- add `double_score(a, b)`
- update tests
- run `cargo test --offline`

Acceptance:
- non-empty source diff
- `cargo test --offline` passes

### T2: exported module / multi-file structural change

Prompt summary:
- export new module `semantic_contract`
- create `src/semantic_contract.rs`
- implement `semantic_contract() -> &'static str`
- add tests
- run `cargo test --offline`

Acceptance:
- substantive source diff in `src/`
- `cargo test --offline` passes

### R1: read-only navigation check

Prompt summary:
- report exported modules from `src/lib.rs`
- report what `compute_score` returns
- do not modify files

Acceptance:
- zero source changes
- answer matches repository contents

## Exact harness invocations

### JCode direct

- primary:
  - `/home/tomp/.local/bin/jcode --provider-profile local-primary --model local-primary -C <worktree> run <prompt>`
- coder:
  - `/home/tomp/.local/bin/jcode --provider-profile local-coder --model local-coder -C <worktree> run <prompt>`

### Abacus

- primary mutation runs:
  - `/tmp/abacus-pr14-test/target/release/abacus --mode build --model local-primary --base-url http://127.0.0.1:8017/v1 --protocol chat-completions --no-session --always-approve -p <prompt>`
- coder mutation runs:
  - `/tmp/abacus-pr14-test/target/release/abacus --mode build --tool-format qwen --model local-coder --base-url http://127.0.0.1:8018/v1 --protocol chat-completions --no-session --always-approve -p <prompt>`
- coder read-only run:
  - `/tmp/abacus-pr14-test/target/release/abacus --model local-coder --tool-format qwen --base-url http://127.0.0.1:8018/v1 --protocol chat-completions --no-session -p <prompt>`

## Result matrix

| Harness | Worker/model | Task | Result | Duration | Evidence |
| --- | --- | --- | --- | ---: | --- |
| JCode | `local-primary` | T1 | pass | 95s | `/tmp/pr14-evidence/jcode_primary_t1.*` |
| JCode | `local-coder` | T1 | fail: false success, no source diff | 44s | `/tmp/pr14-evidence/jcode_coder_t1.*` |
| Abacus | `local-primary` | T1 | pass | 50s | `/tmp/pr14-evidence/abacus_primary_t1.*` |
| Abacus | `local-coder` | T1 | fail: false success, no source diff | 11s | `/tmp/pr14-evidence/abacus_coder_t1.*` |
| JCode | `local-primary` | T2 | pass | 27s | `/tmp/pr14-evidence/jcode_primary_t2.log` plus live diff in `/tmp/pr14-runs/jcode_primary_t2` |
| Abacus | `local-primary` | T2 | fail: timed out, no source diff | 240s | `/tmp/pr14-evidence/abacus_primary_t2.log` |
| JCode | `local-coder` | R1 | fail: emitted raw tool-call JSON instead of a repository answer | 3s | `/tmp/pr14-evidence/jcode_coder_read.log` |
| Abacus | `local-coder` | R1 | fail: zero source changes but incorrect content answer and incorrect `files_changed: yes` | 11s | `/tmp/pr14-evidence/abacus_coder_read.log` |

## Material observations

### JCode on `local-primary`

Strengths observed:
- produced substantive diffs on both mutation tasks
- successfully used repository navigation and test feedback
- preserved existing behaviour while making bounded changes
- no endpoint rebinding was observed in direct single-agent mode

Weaknesses observed:
- tool usage was noisy on T1
- transcript showed failed `multiedit` attempts before recovering with `write`
- T2 added a unit test inside the new module instead of extending the integration test file; acceptable for coverage, but not the exact preferred shape

Conclusion:
- currently the strongest primary-worker harness on this rack

### Abacus on `local-primary`

Strengths observed:
- T1 was clean and correct
- tool transcript was concise and readable
- normal supported `--mode build` invocation worked against the vLLM endpoint

Weaknesses observed:
- T2 consumed the full 240-second bound and exited `124`
- no repo mutation was made on T2 before timeout
- current build/install complexity is materially higher than JCode because Abacus is not yet installed on the rack and required a source build

Conclusion:
- promising, but not yet strong enough to be the preferred primary harness for the current rack state

### JCode on `local-coder`

Observed failures:
- T1 returned `COMPLETE` with a fully fabricated change summary
- no source files changed
- R1 returned a raw JSON-like tool-call block instead of the requested answer

Conclusion:
- not qualified for production supervision against the current `local-coder` model

### Abacus on `local-coder`

Configuration tested:
- `--mode build`
- `--tool-format qwen`
- direct OpenAI-compatible vLLM endpoint binding

Observed failures:
- T1 returned success text with no source diff
- R1 performed a read-only tool call but still answered incorrectly
- it claimed `files_changed: yes` despite a clean Git status

Conclusion:
- not qualified for production supervision against the current `local-coder` model

## Capability classification

### JCode

- `local-primary`: `qualified`
- `local-coder`: `not_qualified`

### Abacus

- `local-primary`: `qualified_with_constraints`
  - acceptable for smaller bounded primary-worker tasks
  - not preferred because the structural multi-file task timed out with no diff
- `local-coder`: `not_qualified`

## Initial routing policy

The evidence does **not** support the starting hypothesis `local-coder -> abacus preferred`.
With the current 2060 model, neither Rust harness produced a reliable supervised mutation path.

Initial routing policy for PR15 implementation should therefore be:

```text
local-primary -> jcode preferred, abacus optional fallback
local-coder   -> none
```

Expressed as a config-ready shape:

```json
{
  "routes": [
    {
      "worker_id": "local-primary",
      "preferred_harness": "jcode",
      "fallback_harness": "abacus",
      "classification": "qualified",
      "reason": "JCode completed both mutation tasks; Abacus timed out on the structural task."
    },
    {
      "worker_id": "local-coder",
      "preferred_harness": "none",
      "fallback_harness": "none",
      "classification": "not_qualified",
      "reason": "Both JCode and Abacus returned false-success or incorrect read-only results against the current Qwen2.5-Coder-3B-AWQ worker."
    }
  ]
}
```

## Operational notes

- JCode was already installed and immediately usable.
- Abacus was absent from the rack at the start of PR14 and had to be cloned and built from source.
- Abacus build time on `gpurack`: about 1m39s for the tested checkout.
- No JCode swarm functionality was used.
- No Rack AI code was modified to compensate for harness weaknesses during comparison.

## Residual gaps

This report is enough to set the initial routing recommendation, but it did **not** separately prove every PR14 matrix item.

Not separately validated in a dedicated experiment:

- hard network-disabled execution for each harness;
- simultaneous dual-endpoint sessions used specifically to prove no cross-binding.

Those gaps do not change the current routing decision because:

- both `local-coder` pairings already failed on simpler mutation or read-only checks;
- `local-primary` already separated the candidates materially on task correctness and timeout behaviour.

If PR14 is held to the full contract literally before merge, those two experiment classes should be added as follow-up evidence on this branch.

## Recommendation for PR15

Do **not** assume the current `local-coder` model has a qualified Rust harness.
PR15 should only integrate routes that this report actually qualifies.
That means:

1. integrate `local-primary -> jcode` first;
2. keep Abacus as an optional, explicit fallback candidate only for `local-primary`;
3. do not wire `local-coder` to either Rust harness as a production path until the model or harness evidence materially changes.

## Machine-readable summary

```text
QUALIFIED_HARNESSES = jcode,abacus
LOCAL_CODER_PREFERRED_HARNESS = none
LOCAL_PRIMARY_PREFERRED_HARNESS = jcode
```
