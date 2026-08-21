# Autonomous Campaign Runner Contract

## Status

**Implementation contract for the next Rack AI feature.** This document is intentionally precise because the resulting runner will be trusted to make bounded, unattended progress in a separate product repository for up to 48 hours.

Current implementation checkpoint as of August 21, 2026:

- Rust campaign schema, validation, durable state, event log, attempt review packet, worker transcript packet, and command-evidence packet are implemented.
- `rack-campaign validate`, `start`, `runner`, `status`, `events`, and `inspect` are wired into the Rust CLI and exposed through `bin/rack-campaign`.
- Campaign worktrees now use the configured writable workspace root and the shared Git worktree adapter accepts both `rack/change-*` and `rack/campaign-*` branch families.
- Live rack proof completed on August 21, 2026 with a verification-only AdaptOS campaign against branch `rack/change-adaptos-20260821-005-cli`, producing `completed` state, persisted events, and per-attempt evidence under `state/campaigns/adaptos-verify-20260821/`.
- Full contract coverage is not complete yet: pause/resume/cancel/revise controls, detached runner lifecycle, bounded lease recovery, and the full implementation-step repair/fallback loop still need to be finished and hardened.

It does **not** describe a generic autonomous-agent platform. It describes one controlled capability built on the existing external-repository change workflow.

## Decision

Add a Rust-native autonomous campaign runner to Rack AI.

A campaign is a predeclared sequence of small external-repository changes. Rack AI executes the sequence on one local rack branch, verifies each change, makes a **local** commit only after policy gates pass, persists all evidence and state, and stops safely when it cannot prove progress.

The campaign runner replaces manual coordination duties that previously required an external agent:

- detect that an implementer claimed completion without changing files;
- reject apparently passing checks when the requested implementation did not occur;
- independently review each implementation attempt against its requested outcome and evidence;
- reject inadequate work with a recorded, bounded repair instruction;
- choose the configured fallback worker after a bounded failure;
- carry accepted work forward to the next step;
- keep running after a process restart;
- expose progress and accept bounded operator interventions.

The campaign runner must use only the rack's existing local model endpoints and local host resources. It must not require subscription tokens, cloud API keys, OpenCodex, a SaaS queue, a database server, or network access from a job container.

## Goal and measurable outcome

Given a valid campaign and healthy local workers, Rack AI can produce a reviewable candidate branch containing a sequence of accepted commits without an external coordinator remaining connected.

A successful 48-hour run produces all of the following:

1. A single local branch named `rack/campaign-<campaign-id>`.
2. Zero or more accepted local commits, one per accepted implementation step.
3. A complete immutable state record and evidence packet for every attempt.
4. A human-readable status command and JSON status/event stream.
5. No push, merge, target-default-branch update, or Rack AI source modification.
6. A terminal campaign state of `completed`, `blocked`, `failed`, `cancelled`, or `expired`.

A campaign may produce fewer commits than requested, or no commits, if the evidence gates cannot establish a safe change. That is correct behaviour. It must never convert uncertainty into a false success.

## Scope

This feature includes:

- campaign schema, validation, durable state, and CLI;
- a persistent/restartable runner process;
- local branch and worktree continuity;
- model selection, bounded fallback, and repair prompts;
- strict no-change and tool-protocol failure detection;
- Podman-backed execution for **all** external-repository reads, writes, shell commands, and acceptance commands;
- local commits after accepted steps only;
- operator status, pause, resume, cancel, and revision controls;
- event/evidence/health telemetry;
- fixture and live smoke coverage.

This feature does not include:

- automatic remote push, pull request creation, merge, or branch deletion;
- direct writes to a target repository's default branch;
- arbitrary interactive shell access for the campaign;
- a web UI, chat bot, public API, database, or distributed scheduler;
- arbitrary task generation from a vague product brief;
- unrestricted planning or self-expanding backlogs;
- new cloud-model/API dependencies;
- OpenCodex or another coding-agent harness.

## Non-negotiable safety invariants

These are acceptance conditions, not aspirations.

1. Rack AI never modifies its own checkout during a normal campaign.
2. Only a repository registered in `config/repositories.json` may be targeted.
3. The campaign records an immutable initial base SHA before execution.
4. Each campaign owns exactly one `rack/campaign-<campaign-id>` branch and one isolated worktree.
5. The target repository's default branch is never checked out or mutated by a campaign.
6. A model can only interact with the target worktree through `WorkspaceExecutor` and rootless Podman.
7. `local-primary` fallback must use the same Podman executor as `local-coder`. Calling `bin/rack-task`, JCode, or any host-shell worker directly against an external-repository worktree is forbidden.
8. Job containers have no network, host home, SSH material, Docker/Podman socket, Rack AI checkout, or unrelated repository mount.
9. No campaign code path can invoke Git remote operations. The Git adapter used by campaigns may only inspect, create the campaign worktree/branch, create a local commit, and inspect history.
10. A local commit is allowed only when the campaign's explicit `allow_local_commits` option is true and every acceptance gate for the step passes.
11. Every implementation step with an empty final source diff fails as `no_change`, even if its pre-existing tests pass.
12. A verification-only step may have an empty diff, but must declare `kind: "verification"`; it may never create a commit.
13. No automatic retry may broaden `allowed_paths`, acceptance commands, resource limits, or the overall campaign duration.
14. A campaign that reaches its duration, retry, safety, health, or evidence limit stops and retains its worktree and records.
15. An implementation attempt is accepted only after an independent coordinator review records an explicit `accepted` disposition. Model self-reports and a passing test command are never a review disposition.
16. A retryable rejected attempt receives a persisted, scope-bounded repair instruction derived from the coordinator review. The runner never silently advances past a rejected attempt.

## Campaign schema

Add `config/schemas/campaign.json` and validate it before any worktree is created.

The first supported version is `rack-ai/campaign/v1`.

```json
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "adaptos-foundation-20260821",
  "repository": {
    "id": "adaptos",
    "base_ref": "main",
    "base_sha": "0123456789abcdef0123456789abcdef01234567"
  },
  "branch": "rack/campaign-adaptos-foundation-20260821",
  "permitted_paths": ["src/domain/", "tests/domain/", "Cargo.toml"],
  "allow_local_commits": true,
  "limits": {
    "max_runtime_seconds": 172800,
    "max_steps": 16,
    "max_total_attempts": 32,
    "heartbeat_seconds": 30,
    "network": "disabled"
  },
  "worker_policy": {
    "primary": "local-coder",
    "fallback": "local-primary",
    "primary_attempts": 1,
    "repair_attempts": 1,
    "fallback_attempts": 1
  },
  "steps": [
    {
      "id": "domain-identifiers",
      "kind": "implementation",
      "task": "Add validated domain identifiers and tests.",
      "allowed_paths": ["src/domain/", "tests/domain/"],
      "required_changed_paths": ["src/domain/"],
      "acceptance": {
        "commands": [["cargo", "test", "--workspace"]],
        "required_artifacts": ["src/domain/mod.rs"]
      },
      "limits": {
        "timeout_seconds": 900,
        "network": "disabled"
      }
    }
  ]
}
```

### Schema requirements

- `campaign_id` is unique, non-empty, and safe for a path and Git branch suffix.
- `repository.id` must identify an enabled registered repository.
- `repository.base_sha` is mandatory for `v1`; the runner rejects a mismatch instead of resolving a later moving ref.
- `branch` must exactly equal `rack/campaign-<campaign_id>` in `v1`.
- `permitted_paths` is a non-empty, immutable campaign-wide path-prefix allowlist. Every step's `allowed_paths`, `required_changed_paths`, and every later revision path must be a subset of it.
- `allow_local_commits` is mandatory and must be `true` for an implementation campaign. It is the only campaign-specific promotion privilege.
- `max_runtime_seconds` is between 60 and 172800 inclusive.
- `max_steps` is between 1 and 16 inclusive.
- `max_total_attempts` is between 1 and 32 inclusive.
- `heartbeat_seconds` is between 10 and 60 inclusive.
- `network` is always `"disabled"`.
- Each implementation step has non-empty `allowed_paths`, non-empty `required_changed_paths`, at least one acceptance command, and a timeout between 1 and 900 seconds.
- `required_changed_paths` uses the same prefix semantics as `allowed_paths` and is evaluated against the final Git evidence.
- `kind` is either `implementation` or `verification`.
- A verification step declares no `required_changed_paths`, has no implementation worker attempt, and cannot commit.
- A campaign is predeclared: the runner does not invent steps. Operator revisions append immutable, separately validated steps as described below.

## Required CLI and operator controls

Provide a thin `bin/rack-campaign` wrapper for the Rust CLI. Commands must work from SSH and produce both concise human output and `--emit-json` output.

```text
rack-campaign validate <campaign.json>
rack-campaign start <campaign.json> [--detach]
rack-campaign runner <campaign-id>
rack-campaign status <campaign-id> [--emit-json]
rack-campaign events <campaign-id> [--follow] [--emit-json]
rack-campaign pause <campaign-id>
rack-campaign resume <campaign-id>
rack-campaign cancel <campaign-id> [--reason <text>]
rack-campaign revise <campaign-id> <revision.json>
rack-campaign inspect <campaign-id> [--step <step-id>]
```

### Operator semantics

- `validate` is read-only. It validates schema, registered repository identity, base SHA, worker availability, Podman rootless availability, required local image, and acceptance command policy.
- `start` creates state, branch, and worktree only after a successful preflight. Without `--detach`, it runs `runner <campaign-id>` in the foreground. With `--detach`, it must start `runner <campaign-id>` through `systemd-run --user --unit rack-ai-campaign-<campaign-id> --collect`. If user-level systemd is unavailable, it fails with setup instructions; it must not fall back to `nohup`, an orphan process, or a shell background job. The deployment guide must document the required user-linger setup for a 48-hour run.
- `status` reports campaign/step/attempt state, current worker, current action, last progress time, last error, worktree, branch, HEAD SHA, elapsed and remaining budget, and packet paths.
- `events --follow` streams append-only JSON Lines events. It is the primary way to watch a live unattended run.
- `pause` sets `pause_requested`. The runner does not begin another model call, tool call, test command, or commit after the current bounded action completes. It becomes `paused` at the next safe checkpoint.
- `cancel` immediately prevents a commit and prevents further actions. It may terminate an active Podman command using the existing container cleanup path. It retains dirty worktree state and evidence for inspection.
- `resume` is valid only from `paused`, `blocked`, or a recoverable interrupted state. It re-runs preflight and resumes from the durable checkpoint. It does not silently discard a dirty worktree.
- `revise` is the controlled answer to “revisit this work.” It is valid only while paused or blocked. The revision file contains one or more new bounded campaign steps and a human instruction. It cannot rewrite accepted history, modify the original campaign document, or use any path outside the immutable campaign-wide `permitted_paths`. It appends a revision record and new step IDs to campaign state. Each revision remains subject to the same limits and total-duration budget.
- `inspect` prints the final diff, changed paths, tool transcript summary, command evidence, model/worker identity, and the exact reason for the last disposition.

No command may accept an unbounded free-form instruction that runs immediately against a worktree. Human instructions always become a persisted, validated revision step.

### User linger for detached 48-hour runs

`--detach` uses `systemd-run --user --collect`. A 48-hour unattended campaign requires the operator session to outlive SSH:

```text
loginctl enable-linger "$USER"
systemctl --user is-system-running
```

If user-level systemd is unavailable, `rack-campaign start --detach` fails with these instructions. It must not fall back to `nohup`, an orphan process, or a shell background job.

## Durable state and recovery

Store state under:

```text
state/campaigns/<campaign-id>/
  campaign.json
  state.json
  events.jsonl
  steps/<step-id>/attempt-<n>/review-packet.json
  steps/<step-id>/attempt-<n>/worker-transcript.json
  steps/<step-id>/attempt-<n>/command-evidence.json
```

`state.json` must include:

- schema version and campaign ID;
- immutable campaign digest;
- repository ID, initial base SHA, branch, worktree, and current HEAD SHA;
- overall state, current step, current attempt, pause/cancel flags;
- every step's disposition, coordinator-review disposition and rationale, and accepted commit SHA where applicable;
- selected worker, fallback reason, repair instruction, and repair attempt linkage for each attempt;
- timestamps, elapsed duration, remaining duration, and last heartbeat;
- active lease/container identifiers where applicable;
- final error and blocked reason.

The runner must persist state before and after every state transition. On restart it must:

1. acquire the campaign lease;
2. load and validate state against the campaign digest;
3. verify the branch, worktree, expected HEAD SHA, and clean/known Git state;
4. detect an expired or stale active attempt;
5. recover only from a safe checkpoint;
6. emit a recovery event;
7. refuse to continue if it cannot prove continuity.

A stale runner lease must not permit two processes to advance the same campaign.

## Execution lifecycle

### Preflight

Before the first step, require:

- registered target repository and matching immutable base SHA;
- rootless Podman;
- configured executor image already present locally;
- healthy local-primary and local-coder endpoints;
- adequate workspace root and disk availability;
- no active campaign lease for the same target repository;
- campaign branch absent, or an exactly matching resumable branch/state pair;
- approved acceptance programs;
- no externally configured network or remote-promotion capability.

Preflight failure is `blocked`, not `completed`.

### Step execution

For each step:

1. Emit `step_started`.
2. For an implementation step, invoke the primary worker through a model-backed change implementer.
3. The model request is made from Rack AI to the local inference endpoint. Every model tool operation—read, write, shell, and test—uses `WorkspaceExecutor` and rootless Podman.
4. Capture the model response, parsed tool calls, tool results, model/worker identity, timings, and failure classification.
5. Inspect Git and enforce `allowed_paths` and `required_changed_paths`.
6. If the final source diff is empty, classify the attempt as `no_change`. Do not run or accept a misleading implementation success.
7. Run declared deterministic acceptance commands in Podman.
8. Re-inspect Git after all checks and artifact reads. Enforce path policy again.
9. Run the independent coordinator review described below. It must emit an explicit disposition and rationale.
10. Only if every deterministic gate and the coordinator review pass, create one local commit using the campaign branch only. Record its immutable SHA and emit `step_accepted`.
11. The next step begins from that accepted commit.

A local commit message must be deterministic and attributable:

```text
rack(<campaign-id>): <step-id>
```

The commit author must be explicitly configured as a non-user automation identity. It must not reuse personal credentials.

### Independent coordinator review and repair loop

The runner, not the operator, is responsible for the coordinating review loop. This is a separate decision from the implementer's claim of completion and from the raw acceptance-command exit status.

For every implementation attempt that reaches evidence collection, the runner must create a coordinator-review record before it can accept, repair, fall back, or block. The record must examine:

- the step's requested outcome, allowed paths, required changed paths, and declared artifacts;
- the final diff and post-check Git status;
- parsed tool transcript and any tool-protocol failure;
- deterministic acceptance-command and artifact evidence;
- relevant prior attempt and repair context.

The record must contain one of `accepted`, `rejected_retryable`, or `rejected_terminal`, a machine-readable failure classification, a concise human-readable rationale, and references to the evidence it considered. It is persisted in the attempt packet and surfaced by `status`, `events`, and `inspect`.

An `accepted` disposition is required before a local commit. A test suite passing alone is insufficient: the review must reject a missing or inadequate requested implementation even when old tests pass.

For `rejected_retryable`, the runner must construct and persist one bounded repair instruction for the configured worker. The instruction contains the original step, the exact rejection reason, relevant diff summary, required changed paths, and deterministic failing evidence. It may not introduce new tasks, broaden paths, acceptance commands, duration, resource limits, or promotion authority. The repair result receives a new full review.

For `rejected_terminal`, or when the configured repair/fallback budget is exhausted, the runner records the reason and blocks the step/campaign. It must not turn a rejection into success because a worker says `COMPLETE` or exits successfully.

A model-backed coordinator review may be added as supplementary evidence only if it uses the same local, Podman-isolated workspace boundary. It may not replace the deterministic review gates above, and it is not required for the first implementation.

### Failure and fallback policy

Classify failures at minimum as:

- `tool_protocol_violation`: malformed or absent tool call when a tool action is required, including markdown pretending to be a tool invocation;
- `no_change`: final source diff lacks required changed paths;
- `path_policy_failed`;
- `acceptance_failed`;
- `artifact_missing`;
- `worker_timeout`;
- `executor_unavailable`;
- `model_unavailable`;
- `campaign_expired`;
- `operator_paused`;
- `operator_cancelled`;
- `continuity_failed`.

Default worker policy:

1. One primary `local-coder` attempt.
2. If it has a retryable failure, one repair attempt with the exact classification, final diff summary, and deterministic test output. The repair prompt cannot broaden scope.
3. If the primary sequence still fails, one `local-primary` fallback attempt through the same Podman tool runner.
4. No retry after a path-policy, continuity, executor-isolation, or campaign-expiry failure.
5. Stop the step and campaign as `blocked` after its bounded attempts are exhausted.

The runner must never mark a step accepted merely because a model said `COMPLETE`, an old test suite passed, or the worker process exited zero.

## Worker implementation requirement

Current Rack AI has a direct local-coder worker and a separate host-oriented primary/JCode path. This feature must introduce a common model-backed change implementer abstraction that supports both configured local endpoints without bypassing the executor.

Required behaviour:

- `local-coder` and `local-primary` are selected by configured worker ID, endpoint, model alias, and tool-call parser.
- The implementation path accepts an injected `CoderToolRunner`; it may not write files or execute shell commands itself.
- Both workers run model-driven file tools through `WorkspaceCoderToolRunner -> WorkspaceExecutor -> PodmanWorkspaceExecutor`.
- Existing host-oriented worker entrypoints remain available for Rack AI's internal/legacy task workflows only. They are rejected for an external-repository campaign.
- Tool-call parser failures, text-only fake tool calls, and empty completion output are observable, structured worker failures.
- The fallback worker does not receive host credentials, a Rack AI source mount, or a host `cwd` escape route.

The implementation may reuse the existing direct HTTP client and workspace tool runner, but must not add an independent agent framework.

## Git and promotion boundary

Campaigns may create local commits because cross-step continuity requires an immutable accepted base. This is an explicit, narrow exception to the ordinary change-job pilot.

Allowed campaign Git operations:

- inspect status/diff/HEAD;
- create or validate the campaign worktree and `rack/campaign-<id>` branch;
- create a local commit after all gates pass;
- read commit history needed for recovery.

Forbidden campaign Git operations:

- `push`, `fetch`, `pull`, `remote`, `merge`, `rebase`, `checkout` of a target default branch, destructive reset, tag creation, branch deletion, or configuration mutation;
- any operation on the Rack AI repository;
- any remote credential lookup or use.

The documented human promotion process remains separate:

1. inspect campaign evidence and local branch;
2. explicitly choose to push from a human-controlled shell;
3. open/review a pull request;
4. merge only with human approval.

## Monitoring and health

The runner must make unattended work inspectable without an external coordinator.

Emit an event at least on:

- campaign created, started, resumed, paused, cancelled, blocked, completed, expired;
- step/attempt started, completed, failed, accepted;
- coordinator review started, accepted, rejected, and repair instruction recorded;
- worker selected, repair selected, fallback selected;
- model request started/completed;
- tool invocation started/completed;
- acceptance command started/completed;
- Git inspection/commit;
- heartbeat;
- health or lease transition;
- recovery after restart.

Each event carries a timestamp, campaign ID, step ID where relevant, attempt number, worker ID, action, state, and compact detail. Large stdout/stderr/model outputs are stored by path and digest rather than duplicated in every event.

The runner writes a heartbeat every configured interval during a live model or Podman action. If no heartbeat arrives within two intervals plus the action timeout, the attempt becomes `worker_timeout` or `executor_unavailable` after cleanup. It must never silently continue.

`status --emit-json` must expose enough information for a lightweight SSH or dashboard poller. No web UI is required for this PR.

## Tests and definition of done

The implementation is complete only when all unit, integration, and live tests below pass.

### Deterministic fixture tests

1. A two-step campaign creates two accepted local commits on one campaign branch; step two starts from step one's accepted SHA.
2. An implementation worker returning `COMPLETE` with no source diff is rejected as `no_change`, even if the old acceptance command passes.
3. A worker that emits markdown/JSON text instead of a valid tool call is rejected as `tool_protocol_violation`.
4. A post-check out-of-policy write is rejected and no commit is made.
5. A required changed path missing from an otherwise allowed diff is rejected.
6. A failed primary worker performs only the configured repair/fallback attempts and then stops.
7. The local-primary fallback is exercised through a fake/instrumented `WorkspaceExecutor`; no host-shell/JCode executor may be used.
8. A passing pre-existing test suite with an inadequate requested change receives a persisted `rejected_retryable` coordinator-review disposition, no commit, and a repair instruction that remains inside the original step scope.
9. State recovery after a simulated process restart resumes at a safe checkpoint and never repeats an accepted commit.
10. A pause at a checkpoint prevents the next action; resume continues correctly.
11. A revision appends immutable new steps and does not alter an accepted step record.
12. A cancellation prevents commit/promotion and retains evidence.
13. Expiry, stale lease, base-SHA mismatch, dirty/unprovable worktree, missing Podman, and unavailable worker endpoint all fail closed.
14. No production campaign path exposes a Git remote operation.

### Rack live smoke

Add `tests/rack_campaign_smoke.sh`.

It must use a disposable registered Git fixture and rootless Podman, then prove:

1. a two-step Rust fixture campaign completes;
2. each accepted step produces a local commit on `rack/campaign-...`;
3. target base/default branch remains unchanged;
4. no network is available inside jobs;
5. no target `target/` or dependency cache is written into the worktree;
6. no-op and path-policy rejection cases do not create commits;
7. `status --emit-json` and `events --follow` expose expected progress;
8. restarting the runner after a checkpoint resumes without duplicate work.

The live test may use deterministic fake model-backed implementers where model inference would make the test flaky. A separate opt-in live-model smoke may verify the local-primary/local-coder endpoint integration.

### Manual readiness gate

Before a real two-day unattended campaign:

1. run the full Rust test suite;
2. run all existing rootless Podman change smoke tests;
3. run `tests/rack_campaign_smoke.sh`;
4. run one small real two-step campaign in a disposable external repository;
5. inspect the campaign state/events from a second SSH session;
6. exercise pause, resume, revision, and cancellation;
7. verify no remote operation occurred;
8. only then launch a predeclared AdaptOS campaign.

## Explicit launch limitations

The first unattended campaign must be a predeclared backlog of small, independently testable steps. It should not ask the rack to “build AdaptOS” from an open prompt.

For the first 48-hour AdaptOS run:

- use at most 16 steps;
- set each implementation timeout to 15 minutes or less;
- require a deterministic test command on every implementation step;
- use narrow allowed paths;
- provide the desired outcome and required changed paths for every step;
- treat every completed local commit as a candidate for later human review, not production-ready software;
- expect that some steps may be inaccurate or blocked; preserve evidence rather than forcing progress.

## Out of scope follow-up

After this campaign runner is proven, future work may add richer planning, model evaluation, a project-specific Python/Django executor image, a dashboard, notifications, or human-approved remote promotion. None are required for this implementation.
