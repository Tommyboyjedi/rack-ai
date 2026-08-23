# PR17 Contract — Harness-Routed Unattended Qualification

## Status

Post-PR16 qualification contract. Do not run until the qualified Rust harnesses are integrated, worker/model routing is active, and the superseded native coding-agent layer has been removed or simplified.

## Goal

Prove the new Rack AI architecture can complete substantive software development unattended using local models while Rack AI chooses the appropriate qualified coding harness for each worker/model profile and independently supervises/verifies the result.

This replaces the old PR9 qualification path.

## Stack under test

```text
Rack AI control plane
    -> worker/model selection
    -> qualified harness routing
        -> JCode or Abacus
            -> local vLLM model
                -> isolated target worktree
    -> Rack AI deterministic acceptance
    -> independent review
    -> bounded recovery/replan/fallback
    -> controlled local commit
```

The harness may claim success. Only Rack AI may accept the attempt.

## Preconditions

Before qualification:

- PR14 has committed harness capability classifications and initial routing policy;
- PR15 has integrated the required harness adapters into the real production worker path;
- PR16 has removed/simplified superseded native coding-agent duplication;
- current safety/campaign/recovery/routing tests are green;
- local endpoints are healthy;
- the target starts from a clean known SHA;
- production routing configuration is frozen for the qualification run.

## What PR17 is proving

PR17 proves the **composed control plane**, not merely that JCode or Abacus can code.

Evidence must show that Rack AI correctly:

1. selects a worker/model role;
2. selects that role's configured qualified harness;
3. launches the harness against the correct endpoint/worktree;
4. rejects invalid/no-change work independently;
5. runs deterministic acceptance independently;
6. obtains fresh independent review;
7. invokes bounded recovery/replan/fallback without widening authority;
8. records worker/model/harness identity and evidence durably;
9. commits only after all Rack AI gates pass.

## Qualification layers

### 1. Deterministic regression fixtures

Provide repeatable fixtures proving:

- correct worker/model -> harness routing;
- explicit endpoint binding;
- substantive change;
- compatibility preservation;
- deterministic acceptance;
- independent review;
- rejected/no-change path;
- bounded recovery/replan/fallback;
- no unauthorized mutation;
- safe timeout/cancel/process cleanup.

Where current routing uses different harnesses for `local-coder` and `local-primary`, fixtures should exercise both routes.

### 2. Real substantive target

Run at least one real external-repository task we would genuinely keep.

The task must require:

- repository inspection;
- meaningful implementation;
- compiler/test feedback;
- preservation of an existing behaviour/API/compatibility constraint;
- deterministic acceptance;
- fresh independent review.

A toy edit does not qualify.

The task may naturally use one worker/harness route or may exercise fallback to another. Do not force an artificial cross-harness transition solely for demonstration.

## No-routine-supervision rule

Once the substantive unattended run begins, no human, ChatGPT, Codex, Grok or other frontier model may provide:

- implementation strategy;
- code changes;
- repair instructions;
- harness-switch instructions not already encoded in policy;
- path widening;
- acceptance weakening;
- manual review acceptance.

Observation is allowed.

Genuine infrastructure intervention is allowed only when it does not make a development decision and is fully recorded.

If manual development guidance is required, the qualification is a **FAIL** even if the code is later repaired.

## Harness-routing expectations

The qualification uses the deterministic routing policy established by PR14/PR15.

It does not require dynamic self-learning harness selection.

If routing is approximately:

```text
local-coder   -> Abacus
local-primary -> JCode
```

then the evidence should demonstrate that Rack AI actually used those routes where the corresponding workers were selected.

If PR14 established different routing, use that instead.

A route may fall back to another harness only where that fallback was explicitly qualified and configured before the run.

## Required evidence

Retain evidence showing:

- exact Rack AI SHA;
- routing configuration/version;
- JCode version/configuration if used;
- Abacus version/configuration if used;
- target repository/base SHA;
- model identities/endpoints/GPU roles;
- task/campaign definition and authority;
- selected worker and harness for every attempt;
- harness transcript/output evidence;
- changed paths and Git diff evidence;
- deterministic acceptance commands/results;
- independent review request/result;
- recovery/replan/fallback sequence where exercised;
- any harness switch and the pre-existing policy reason;
- final accepted commit(s);
- timeout/cancel/liveness evidence where exercised;
- no remote push/merge/default-branch mutation;
- no unauthorized path mutation.

## Required behaviours

1. A qualified external harness produces substantive implementation work.
2. Rack AI selects the harness according to configured worker/model policy.
3. Rack AI rejects no-change/invalid work even if the harness reports completion.
4. Rack AI runs deterministic acceptance independently of harness-local tests.
5. A fresh reviewer independently evaluates accepted-looking work.
6. Rejected implementations can enter bounded PR7 recovery/replan/fallback without authority expansion.
7. If fallback changes worker or harness, it does so only through pre-qualified policy.
8. Accepted commits are created only after every Rack AI gate passes.
9. Process restart/state recovery remains safe enough for unattended operation.
10. The run completes or safely blocks without routine external supervision.

## Pass/fail rule

A safe block is preferable to false acceptance but is still a qualification FAIL.

PASS requires:

- substantive code we would genuinely keep;
- correct harness routing;
- no unauthorized mutation;
- independent deterministic acceptance/review;
- no routine external development intervention;
- trustworthy retained evidence.

## Required report

Commit a qualification report containing:

- exact versions/SHAs;
- frozen routing configuration;
- task and authority;
- chronological attempts;
- worker/harness route selected for each attempt;
- acceptance and review evidence;
- recovery/fallback sequence;
- interventions;
- final diff/commit;
- residual risks;
- explicit PASS/FAIL against every requirement.

It must end with exactly:

`PR17_QUALIFICATION = PASS`

or

`PR17_QUALIFICATION = FAIL`

Do not move the goalposts after seeing the result.

## What an implementation agent may do

A Codex/Grok agent assigned PR17 may:

- prepare the minimum deterministic fixtures;
- prepare qualification scripts/reporting support;
- verify preconditions;
- launch the qualification.

Once the real substantive unattended run begins, that agent must stop supervising or repairing development and allow the local Rack AI + routed-harness stack to succeed or fail on its own.

If the qualification exposes a product defect, preserve evidence and identify the responsible layer (`rack_control_plane`, `routing`, `jcode_adapter`, `abacus_adapter`, `model`, `runtime`, etc.). Do not quietly turn PR17 into a broad repair PR. Normally repair the earlier layer separately and rerun from a clean state.

## Non-goals

PR17 does not require:

- objective-to-campaign planning;
- dynamic/learned harness selection;
- adaptive multi-worker concurrency;
- web research;
- automatic cloud/frontier escalation;
- Telegram/web UI;
- automatic remote PR/merge;
- self-modification of the executing Rack AI checkout;
- new Rack AI coding-agent tools.

Those remain future decisions tracked in PR18.

## Merge gate

Merge only with a committed report stating `PR17_QUALIFICATION = PASS` and evidence supporting that result.
