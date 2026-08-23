# PR7 Implementation Contract — Failure Diagnosis and Bounded Replanning

## Status

Implementation contract for PR #7. This PR is the next active engineering milestone after PR #6. It is intentionally narrow: make the original PR6 substantive acceptance scenario solvable without broadening that acceptance test.

## Baseline

Implement from current `main` after PR #6, baseline commit `f055358621d2e282900853d2c2e924a37c62c1af` unless `main` has advanced with approved Rack AI work. Preserve all PR3/PR4/PR6 safety and evidence guarantees.

## Problem to solve

The PR6 AdaptOS proving campaign (`adaptos-accessibility-findings-v6`) exposed a control-loop defect rather than a safety-boundary defect.

Observed failure pattern:

1. The implementation changed `AssessmentService` inside permitted source paths.
2. An existing caller in `src/main.rs` then failed to compile.
3. `src/main.rs` was outside the step's `allowed_paths`.
4. The worker attempted to modify `src/main.rs` and was correctly blocked by path policy.
5. Subsequent repair/fallback behaviour continued to focus on the local compiler error instead of inferring that the permitted implementation strategy itself needed to change.

The correct reasoning is:

- the task requires preserving existing caller/CLI behaviour;
- the caller is a compatibility constraint even though it is not writable;
- the caller cannot be changed under current authority;
- therefore the implementation inside permitted paths must be reconsidered;
- attempting the forbidden caller again is not a useful repair strategy.

Current Rack AI generally maps `rejected_retryable` directly into another implementation attempt. The stronger `local-primary` semantic reviewer is normally called only after deterministic gates are already acceptable. That means an acceptance failure can skip the strongest reasoning step exactly when diagnosis is most needed.

## Goal

Insert a first-class, bounded recovery-diagnosis decision between a substantive failed attempt and the next implementation attempt.

The result must let Rack AI distinguish at least:

- simple/local repair;
- implementation-strategy failure requiring bounded replanning;
- compatibility regression;
- repeated/stagnant failure;
- genuinely insufficient authority/allowed paths;
- transient worker/model/tool/executor failure where the existing mechanical recovery path is appropriate.

The system must remain fail-closed. Diagnosis never grants new authority.

## Required behaviour

### 1. Typed recovery context

Add an explicit typed input for recovery diagnosis. Names are not mandated, but the design should make the concept first-class rather than embedding another free-form string in `campaign_runner.rs`.

The context should contain enough bounded evidence to reason about the failure, including where available:

- campaign ID and step ID;
- original step task;
- campaign `permitted_paths`;
- step `allowed_paths` and `required_changed_paths`;
- required artifacts and acceptance commands;
- current Git diff/diff stat/status/changed paths;
- failed deterministic command evidence, including exit code and bounded stdout/stderr excerpts;
- current worker/tool transcript summary;
- previous attempt dispositions and classifications for the same step;
- prior repair/replan instructions;
- paths a worker attempted or requested to mutate when that is observable;
- relevant previous semantic-review rationale.

Keep context bounded and persistable. Do not dump unbounded repository contents or transcripts into the model prompt.

### 2. Typed recovery decision

Add a machine-readable decision produced by the coordinator/recovery layer. Exact enum/type names are open, but it must distinguish at least:

- `repair` — same overall strategy is still valid; issue a focused bounded correction;
- `replan` — current implementation strategy is invalid; issue a materially different bounded strategy inside existing authority;
- `block_insufficient_authority` — task cannot be completed inside immutable campaign/step authority;
- `block_terminal` — evidence proves continuation is unsafe or nonsensical;
- `retry_transient` — use existing transient recovery semantics where appropriate.

The decision should record:

- decision kind;
- failure/root-cause classification;
- concise rationale;
- evidence references;
- next bounded instruction when applicable;
- whether the next attempt should remain on the same worker or use the existing fallback worker policy.

Do not silently coerce malformed model output into success. Parser failures must fail closed or use a clearly defined bounded fallback classification.

### 3. Invoke local-primary for substantive diagnosis

For substantive retryable implementation failures, Rack AI must give `local-primary` a fresh diagnosis/recovery reasoning turn before choosing another coding attempt.

At minimum this includes:

- deterministic acceptance failure;
- inadequate implementation;
- repeated no-change where a previous real attempt existed;
- path-policy-related evidence that indicates a worker is trying to solve the task by changing non-authorized callers/dependencies, without weakening the path-policy terminal gate itself.

Purely transient failures such as endpoint disconnects/timeouts may continue through existing bounded health/recovery logic without a semantic diagnosis every time.

The diagnosis invocation must be read-only. It must not receive mutation tools or promotion authority.

### 4. Do not weaken deterministic/path gates

Preserve these invariants:

- `allowed_paths` and campaign `permitted_paths` are immutable during automatic recovery;
- acceptance commands cannot be broadened or replaced by the model;
- campaign/step duration and attempt budgets cannot be enlarged automatically;
- no remote push/merge capability is introduced;
- Podman/rootless mutation boundary remains unchanged;
- a path-policy violation never becomes accepted because the recovery model approves it;
- all existing pause/cancel/lease/continuity checks remain authoritative.

### 5. Replanning means strategy change, not campaign expansion

PR7 is NOT autonomous campaign generation.

`replan` in this PR means: within the same predeclared step, produce a materially different implementation strategy that still satisfies the original task, original allowed paths, required changed paths, acceptance commands, resource limits, and campaign authority.

It must not invent new campaign steps, broaden scope, or append operator revisions automatically.

### 6. Cross-attempt history and stagnation

The recovery decision must see enough previous-attempt history to detect repeated failure.

Add bounded stagnation detection. It does not need sophisticated embeddings. A deterministic fingerprint/summary based on failure classification, failing command/diagnostic, changed paths and prior decision/strategy is acceptable.

The important property is that Rack AI must not indefinitely convert the same failure into slightly reworded repair prompts. Within the existing attempt budget, repeated equivalent failure should force either `replan`, fallback/escalation already permitted by worker policy, or safe blocking.

### 7. Evidence and state

Persist recovery diagnosis evidence alongside existing attempt evidence. A future operator or test must be able to reconstruct:

- why the previous attempt failed;
- what evidence the coordinator considered;
- why it chose repair vs replan vs block;
- the exact next instruction;
- which previous attempt the decision relates to.

Extend state/event/inspect output as needed without breaking compatibility with existing persisted state. Follow existing compatible-state migration patterns.

## Mandatory PR6 regression fixture

Add a deterministic repository fixture/test reproducing the essential PR6 failure, without requiring AdaptOS itself.

The fixture must have:

- a permitted implementation module/service file;
- an existing caller in a separate file outside `allowed_paths`;
- an initial implementation strategy that compiles incorrectly or breaks the caller contract;
- a failing acceptance command that points at the out-of-scope caller;
- enough repository evidence for a correct implementation inside allowed paths to preserve compatibility.

The test must prove all of the following:

1. The first attempt fails acceptance because the external caller is broken.
2. Rack AI does not broaden `allowed_paths`.
3. A worker attempt to write the caller remains rejected.
4. The coordinator produces a persisted diagnosis identifying a compatibility/strategy problem rather than merely `acceptance command failed`.
5. The coordinator chooses a bounded replan or equivalent strategy-changing decision.
6. The next implementation instruction explicitly preserves the out-of-scope caller contract and directs changes only inside permitted paths.
7. A subsequent valid implementation can pass acceptance and normal semantic review.
8. The final accepted commit contains no change to the out-of-scope caller.

The test should be deterministic at the application layer. A separate opt-in live-model proof may additionally exercise the local endpoints, but unit/fixture acceptance must not depend solely on model nondeterminism.

## Live proof before merge

PR7 is not complete merely because unit tests pass.

Run an opt-in live-rack proving scenario using the actual `local-coder` and `local-primary` endpoints through the real Podman-backed campaign path. The test may use the deterministic fixture repository rather than AdaptOS.

Required live evidence:

- local-coder makes the initial incorrect/bounded attempt or is driven into an equivalent failing state;
- local-primary receives the recovery context;
- a strategy-changing recovery instruction is recorded;
- the eventual mutation stays inside allowed paths;
- deterministic acceptance passes;
- semantic review passes;
- a local campaign commit is produced;
- evidence packets show the diagnosis/replan chain.

If the exact small-model behaviour makes the first-error setup unreliable, the test may deterministically seed the initial failed state, but the diagnosis/replan decision itself must exercise the real local-primary model before this PR is considered live-proven.

## Likely code areas

Inspect and extend existing abstractions rather than creating a competing runner. Likely areas include:

- `crates/rack_ai_application/src/campaign.rs`
- `campaign_runner.rs`
- `campaign_review.rs`
- `campaign_model_review.rs`
- `implement_change_request.rs`
- campaign state compatibility/evidence code
- `crates/rack_ai_infrastructure/src/local_primary_reviewer.rs`
- campaign tests and live smoke fixtures

Do not assume these are the only files; follow existing architecture and `AGENTS.md`.

## Non-goals

Do NOT add in PR7:

- objective-to-campaign planning;
- automatic new step generation;
- PR5 adaptive multi-worker scheduling/parallel execution;
- web/SearXNG research;
- Serena/LSP semantic code tools (reserved for PR8 unless a tiny internal helper is strictly required for the regression fixture);
- cloud services/API keys;
- OpenCodex/another agent framework;
- GitHub push/PR creation/merge automation;
- broader escalation UI/state beyond what is required for bounded diagnosis.

## Required validation

At minimum:

```bash
cargo test --workspace --offline
bash tests/rack_change_executor_smoke.sh
bash tests/rack_change_implement_smoke.sh
bash tests/rack_change_path_policy_smoke.sh
bash tests/rack_campaign_smoke.sh
```

Run any new PR7-specific fixture smoke plus the opt-in live-rack diagnosis/replan proof.

Do not weaken or delete existing tests to make PR7 pass.

## Merge gate

Merge PR7 only when:

- all existing tests pass;
- the deterministic PR6 regression fixture passes;
- live local-primary diagnosis/replanning has been demonstrated through the real campaign path;
- no safety/path/authority invariant has been weakened;
- evidence clearly distinguishes diagnosis/replan from ordinary repair;
- the implementation remains within the original PR6 acceptance scope.

After PR7 merges, run the PR6 regression fixture again from clean `main`. If it is robust, proceed to PR8 rather than expanding PR6 acceptance criteria.