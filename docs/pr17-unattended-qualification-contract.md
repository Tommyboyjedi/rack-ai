# PR17 Contract — Harness-Backed Unattended Qualification

## Status

Post-PR16 qualification contract. Do not run until the selected Rust harness is integrated and legacy native coding-agent duplication has been removed/simplified.

## Goal

Prove the new architecture can complete substantive software development unattended using local models while Rack AI independently supervises and verifies the result.

This is the replacement for the old PR9 qualification path.

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

## Qualification scenario

Use at least one clean substantive external-repository task large enough to require repository inspection, implementation, tests and at least one meaningful compatibility constraint. Prefer a real target repository/task we would genuinely keep rather than a toy fixture, while retaining a smaller deterministic regression fixture for repeatability.

The qualification must not require routine frontier-model, Codex, Grok or human development decisions after the run begins.

## Required evidence

Retain evidence showing:

- exact Rack AI SHA;
- selected harness/version/configuration;
- target repository/base SHA;
- model identities/endpoints and GPU roles;
- task/campaign definition and authority;
- harness transcript/output evidence;
- changed paths and Git diff evidence;
- deterministic acceptance commands/results;
- independent review request/result;
- any recovery/replan/fallback sequence;
- final accepted commit(s);
- timeout/cancel/liveness evidence where exercised;
- no remote push/merge/default-branch mutation;
- no unauthorized path mutation.

## Required behaviours

1. The implementation harness produces substantive work against the bounded target.
2. Rack AI rejects no-change/invalid work even if the harness says it is complete.
3. Rack AI runs deterministic acceptance independently of the harness.
4. A fresh reviewer independently evaluates accepted-looking work.
5. A rejected implementation can enter the existing bounded recovery/replan/fallback path without broadening authority.
6. Accepted commits are created only after all Rack AI gates pass.
7. Process restart/state recovery remains safe enough for unattended operation.
8. The run completes or safely blocks without routine external supervision.

## Pass/fail rule

A safe block is operationally preferable to false acceptance, but it is still a qualification FAIL.

PASS means the substantive task completes with code we would genuinely keep and with no routine development intervention outside the local Rack AI + selected-harness stack.

## Explicit non-goals

PR17 does not require:

- freeform objective-to-campaign planning;
- adaptive multi-worker concurrency;
- web research;
- automatic cloud/frontier escalation;
- Telegram/web UI;
- automatic remote PR/merge;
- self-modification of the executing Rack AI checkout.

Those capabilities remain future decisions tracked in PR18.

## Merge gate

Merge only with a committed qualification report stating explicit PASS/FAIL against every requirement. If FAIL, preserve evidence and identify the architectural layer responsible before adding new functionality.