# PR9 Contract — Frozen PR6 Qualification Rerun

## Status

Acceptance/qualification contract only. Do not implement broader autonomy features in this PR.

This PR exists to prove that the capability work in PR7 and PR8 is sufficient to satisfy the ORIGINAL PR6 substantive acceptance target. The acceptance criteria are intentionally frozen; this PR must not move the goalposts.

Before execution, update this branch onto the latest `main` containing merged PR7 and PR8.

## Purpose

PR6 introduced/post-merged reliability improvements and was intended to support a substantive unattended campaign using the real local-model workers. The AdaptOS proving campaign `adaptos-accessibility-findings-v6` exposed a remaining autonomy gap:

- permitted implementation changes broke an existing caller in `src/main.rs`;
- `src/main.rs` was outside `allowed_paths`;
- path protection correctly blocked the caller edit;
- the system failed to infer that the implementation strategy inside permitted files had to change;
- repeated repair/fallback attempts did not resolve the architectural compatibility constraint.

PR7 addresses diagnosis/replanning. PR8 adds read-only semantic repository intelligence. PR9 reruns the original substantive proving target without adding new success criteria.

## Frozen scope

This qualification DOES NOT require:

- objective-to-campaign autonomous planning;
- automatically invented campaign steps;
- adaptive/parallel multi-worker scheduling;
- web research/SearXNG;
- GitHub escalation/PR automation;
- richer post-PR6 operator escalation;
- self-starting goals;
- any cloud model or frontier-model supervisor.

A predeclared campaign is explicitly acceptable for this qualification because that was the PR3/PR6 execution contract.

## Qualification question

Given the same class of bounded, predeclared software-development campaign that PR6 was intended to run, can Rack AI autonomously execute the work with its local models and existing safety constraints, including diagnosing and recovering from the compatibility failure that defeated `adaptos-accessibility-findings-v6`, without routine ChatGPT/Codex/Grok/human intervention?

## Required preconditions

Before starting the substantive rerun:

1. PR7 is merged to `main`.
2. PR8 is merged to `main`.
3. `cargo test --workspace --offline` passes from clean `main`.
4. Existing campaign/change smoke tests pass.
5. The deterministic PR6 compatibility-regression fixture passes from clean `main`.
6. The PR7/PR8 opt-in live-model fixture proof passes.
7. `local-primary` and `local-coder` endpoints are healthy.
8. Rootless Podman campaign execution is healthy.
9. The target AdaptOS baseline is clean and its intended base SHA is recorded.
10. The previous failed PR6 implementation is NOT manually rescued or incorporated into the target baseline merely to make this run easier.

## Test campaign

Re-run an equivalent clean campaign to the original PR6 `accessibility-findings` proving campaign against AdaptOS.

The campaign may be regenerated mechanically to point at the current clean target base SHA, but its substantive task/scope/authority must remain equivalent. Do not add writable paths, broader acceptance, additional human instructions or extra steps merely to route around the original failure.

Specifically preserve the essential constraint that the implementation must maintain the existing caller/CLI contract without granting mutation authority to the out-of-scope caller solely because the previous run failed there.

## No routine supervision

Once started, no frontier model or human should make development decisions for the campaign.

Allowed operator activity:

- observe status/events/evidence;
- respond only to genuine infrastructure/operator conditions that are outside the software-development reasoning being tested (for example machine power, disk failure, endpoint process crash if the normal supervisor cannot recover it);
- stop the test for safety if required.

Not allowed:

- tell the worker how to fix the compiler error;
- tell it which implementation strategy to use;
- manually edit AdaptOS;
- broaden `allowed_paths`;
- rewrite acceptance commands after failure;
- use ChatGPT/Codex/Grok to provide a repair/replan instruction to the active campaign.

If such intervention is needed, qualification has not passed.

## Required pass evidence

The campaign passes this qualification only if the retained evidence proves all of the following:

1. Work executes through the real local-model campaign path.
2. `local-coder` is used as the primary implementation worker according to the configured policy.
3. Deterministic acceptance catches any initial incompatible implementation rather than falsely accepting it.
4. Out-of-policy writes remain blocked.
5. `local-primary` receives/produces a substantive recovery diagnosis when required.
6. The recovery evidence identifies the caller/compatibility constraint and chooses a strategy that stays inside original authority.
7. Any semantic-code intelligence used is read-only and recorded.
8. The revised implementation preserves the existing out-of-scope caller/CLI behaviour without changing that caller.
9. Required tests/acceptance commands pass.
10. Independent semantic review explicitly accepts the final implementation.
11. Only authorized paths are committed.
12. Campaign state/evidence is complete enough to reconstruct primary attempt, failure, diagnosis, replan/repair, subsequent attempt and final acceptance.
13. The campaign reaches its intended successful terminal state without routine human/frontier-model development supervision.

## Failure conditions

Qualification fails if any of the following occur:

- a human/frontier model supplies a software-development decision needed for completion;
- the campaign must be widened beyond its original path/task authority to succeed;
- the system repeatedly tries to edit the forbidden caller instead of changing implementation strategy;
- attempt budgets are exhausted on substantively repeated repair behaviour;
- broken work is committed/accepted;
- path/Podman/network/promotion safety is weakened;
- evidence cannot show why recovery decisions were made;
- a clean equivalent run cannot complete reliably enough to be considered an unattended proving run.

A safe `blocked` outcome is still preferable to false success, but it does not constitute a qualification pass.

## Result record

Commit a concise qualification report under `docs/` after the run containing:

- exact Rack AI `main` SHA;
- exact AdaptOS base SHA;
- campaign ID;
- worker/model identities/endpoints (no secrets);
- start/end timestamps;
- terminal state;
- attempt sequence;
- failure classifications;
- recovery decision(s);
- semantic-tool evidence used;
- accepted commit SHA if successful;
- commands/tests executed;
- paths changed;
- operator interventions, if any;
- explicit PASS/FAIL against each requirement above;
- links/paths to retained campaign evidence.

Do not edit this contract after the run to redefine success.

## What happens after PASS

A PR9 PASS closes the original PR6 qualification gap.

Only after that pass should Rack AI define the NEXT autonomy acceptance target, which may then include capabilities deliberately outside PR6 such as:

- objective-to-campaign planning;
- broader recovery/escalation policy;
- research capability;
- adaptive multi-worker scheduling/parallelism;
- further self-build autonomy.

Those capabilities must get their own acceptance contract rather than being retroactively attached to PR6.