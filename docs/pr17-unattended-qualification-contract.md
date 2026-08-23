# PR17 Contract — Harness-Backed Unattended Qualification

## Status

Post-PR16 qualification contract. Do not run until the selected Rust harness is integrated and legacy native coding-agent duplication has been removed/simplified.

PR17 is a qualification PR, not another feature PR. Its job is to prove that the architecture created by PR14–PR16 is actually good enough for unattended local software development.

## Goal

Prove the new architecture can complete substantive software development unattended using local models while Rack AI independently supervises and verifies the result.

This replaces the old PR9 qualification path because the underlying architecture has changed.

## Stack under test

```text
Rack AI control plane
    -> selected Rust coding harness
        -> local vLLM model(s)
            -> isolated target worktree
    -> Rack AI deterministic acceptance
    -> independent review
    -> bounded recovery/replan/fallback
    -> controlled local commit
```

The harness may claim success, but only Rack AI can accept the attempt.

## Preconditions

Before PR17 execution:

- PR14 has selected exactly one Rust harness;
- PR15 has integrated that harness into the real Rack AI implementation-worker path;
- PR16 has removed/simplified superseded Rack AI-native coding-agent duplication;
- current workspace/isolation/campaign/recovery tests are green;
- selected harness/version/configuration is documented;
- local vLLM endpoints are healthy;
- qualification target uses a clean, disposable worktree/clone and known base SHA;
- no human/frontier model is needed to operate the normal run once launched.

If any precondition is false, PR17 should not be declared started.

## Qualification scenario

Use two layers of proof:

### 1. Deterministic regression fixture

Keep a small repeatable fixture that proves the core control behaviour, including at least:

- a substantive implementation change;
- an existing compatibility constraint/caller outside the immediate edit focus;
- deterministic acceptance;
- independent review;
- at least one rejected/failure path that exercises bounded recovery/replan/fallback;
- no unauthorized mutation.

### 2. Real substantive target task

Run at least one clean, real external-repository task that we would genuinely keep if completed correctly. It must be large enough to require:

- repository inspection/navigation;
- implementation across meaningful existing code;
- compiler/test feedback;
- preservation of existing behaviour/API constraints;
- deterministic acceptance;
- independent semantic review.

Do not use a toy one-line edit as the primary qualification.

## No-routine-supervision rule

Once the substantive qualification run begins, no human, ChatGPT, Codex, Grok or other frontier model may supply development decisions needed to complete the task.

Observation is allowed. Genuine infrastructure recovery may occur only if it does not provide implementation strategy, code changes, path widening, acceptance weakening or manual repair instructions. Any intervention must be recorded in the qualification report.

If manual development guidance is needed, the run is a qualification FAIL even if the final code later works.

## Required behaviours

The qualification must prove all of the following:

1. The selected harness performs real implementation work through the real Rack AI worker path.
2. Rack AI rejects no-change or invalid work even if the harness reports completion.
3. Rack AI independently inspects Git/changed paths.
4. Rack AI runs deterministic acceptance independently of harness-local tests.
5. A fresh reviewer independently evaluates accepted-looking work.
6. Rejected implementation can enter the bounded PR7 recovery/replan/fallback path without broadening authority.
7. Model/worker fallback remains bounded and explicit.
8. Accepted local commits are created only after Rack AI gates pass.
9. Process timeout/cancel/liveness behaviour remains bounded.
10. Durable campaign state can survive at least the restart/recovery scenario already promised by the production architecture.
11. The run completes or safely blocks without routine external supervision.
12. No remote push/merge/default-branch mutation occurs.
13. No unauthorized path mutation occurs.
14. Final code quality is good enough that we would genuinely keep the result.

## Required evidence

Retain and reference evidence showing:

- exact Rack AI SHA;
- selected harness name/version/SHA/configuration;
- target repository/base SHA;
- model identities/endpoints and GPU roles;
- task/campaign definition and explicit authority;
- harness transcript/output evidence;
- worker selection/fallback sequence;
- changed paths and full Git diff evidence;
- deterministic acceptance commands/results;
- independent review request/result;
- recovery/replan/fallback decisions and rationale where exercised;
- final accepted commit(s);
- timeout/cancel/liveness evidence where exercised;
- restart/recovery evidence;
- all operator/external interventions;
- proof of no remote push/merge/default-branch mutation;
- proof of no unauthorized path mutation.

## Qualification report

Commit a final report under `docs/` that contains:

- environment and exact versions/SHAs;
- fixture result;
- substantive target/task description;
- chronological attempt/recovery sequence;
- acceptance and review results;
- interventions, including `none` when there were none;
- final changed paths/commit SHA;
- explicit PASS/FAIL for every required behaviour above;
- residual risks discovered;
- clear statement of which architectural layer caused any failure.

The report must end with exactly:

`PR17_QUALIFICATION = PASS`

or

`PR17_QUALIFICATION = FAIL`

Do not redefine the pass criteria after seeing the result.

## Pass/fail rule

A safe block is operationally preferable to false acceptance, but it is still a qualification FAIL.

PASS requires:

- the real substantive task completes;
- resulting code is code we would genuinely keep;
- no routine development intervention occurred outside the local Rack AI + selected-harness stack;
- deterministic acceptance and independent review both pass;
- authority/path/promotion boundaries remain intact;
- retained evidence is sufficient to reconstruct what happened.

## What PR17 may implement

PR17 may add only the minimum test/fixture/reporting/qualification scaffolding necessary to run and record the proof.

If the qualification exposes a product defect in PR14–PR16 architecture, preserve the failure and identify the responsible layer. Do not quietly turn PR17 into a broad repair PR. Material fixes should normally go into a separate corrective PR, then PR17 should be rerun from a clean qualification state.

## Explicit non-goals

PR17 does not require:

- freeform objective-to-campaign planning;
- adaptive multi-worker concurrency;
- web research;
- automatic cloud/frontier escalation;
- Telegram/web UI;
- automatic remote PR/merge;
- self-modification of the executing Rack AI checkout;
- new coding-agent tools inside Rack AI;
- improvements merely intended to make a failing qualification easier after the fact.

Those capabilities remain future decisions tracked in PR18.

## Implementation-agent handoff

An agent assigned PR17 should:

1. read PR14 selection evidence, PR15 integration docs, PR16 cleanup report and this contract;
2. verify all preconditions before modifying anything;
3. add only the minimum deterministic fixture/qualification/reporting support required;
4. execute the deterministic regression proof;
5. execute the real substantive unattended qualification through the production Rack AI path;
6. do not manually rescue implementation failures;
7. collect durable evidence;
8. commit the qualification report with explicit PASS/FAIL against every requirement.

A Codex/Grok implementation agent preparing PR17 may build the qualification scaffolding, but once the substantive unattended run begins it must not supervise or repair that run.

## Merge gate

Merge PR17 only with a committed qualification report. A passing merge requires `PR17_QUALIFICATION = PASS`. If the run fails, preserve the PR/evidence as diagnostic material and fix the responsible earlier architectural layer before rerunning qualification.