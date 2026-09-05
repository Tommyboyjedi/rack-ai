# PR29 ATHBA Change Hang Diagnosis

Date: `2026-08-31`

## Live evidence inspected

Change id:

`pr17-reservation-book-final-20260831T071500Z--test_add_resource_unique_and_duplicate--red`

Confirmed managed worktree:

`/srv/rack-ai/state/workspaces/pr17-reservation-book-final-20260831T071500Z--test_add_resource_unique_and_duplicate--red/repo`

Observed state:

- branch existed: `rack/change-pr17-reservation-book-final-20260831T071500Z--test_add_resource_unique_and_duplicate--red`
- worktree `HEAD`: `b414fd1474da7f87e099d1c599624f3952af5a0b`
- `git status --porcelain=v1 -uall` reported:
  `?? tests/test_reservation_book.py`
- candidate file existed at:
  `tests/test_reservation_book.py`
- no terminal review packet existed at:
  `/srv/rack-ai/state/changes/pr17-reservation-book-final-20260831T071500Z--test_add_resource_unique_and_duplicate--red/review-packet.json`

The preserved candidate content skipped on `ImportError`, but that was not treated as root cause for Rack AI reliability. It was only evidence that the implementation worker had already mutated the worktree.

## Furthest confirmed lifecycle point

Confirmed from preserved evidence:

1. request parsed: not directly preserved
2. repository resolved: implied by managed worktree creation
3. worktree prepared: yes
4. initial inspection: not durably preserved
5. JCode implementer started: implied by candidate write
6. JCode wrote candidate: yes

Not confirmed from preserved evidence:

7. JCode process returned
8. post-implement inspection
9. deterministic acceptance started
10. deterministic acceptance finished
11. final inspection
12. accepted revision materialization
13. review packet persistence
14. CLI summary output

## Root cause

The live preserved worktree proved that Rack AI had crossed the post-prepare boundary and allowed the JCode implementation harness to mutate the isolated worktree, but it did not preserve a terminal packet.

Focused reproduction on the same `change` CLI path exposed the generic defect:

- `JCodeProcessRunner` already returned a structured failure on bounded worker timeout.
- `JCodeChangeImplementer` converted that failure into an `ImplementChangeResult` carrying `worker_error`.
- `ExecuteChange::implement()` ignored `worker_error` and `protocol_error`, treated the result as nominal implementer output, and continued into later stages.
- additional post-prepare `?` paths in `ExecuteChange` could still escape after worktree creation without converting the failure into a terminal persisted packet.

This meant Rack AI did not have a trustworthy architectural rule that:

`post-prepare implementation failure -> terminal failed packet -> CLI exit`

In the worst case, a timed-out implementer could still flow forward and be approved if deterministic acceptance happened to pass.

## Why the live ATHBA call appeared hung

`rack_ai_cli change` prints its summary only after `ExecuteChange::execute()` returns.

The preserved ATHBA worktree shows the call had already reached implementation and workspace mutation, but no packet had been written yet. That left ATHBA blocked inside the CLI transport waiting for a terminal result.

The exact last blocking operation of that interrupted live run could not be reconstructed from durable evidence because no packet or phase log had yet been written. However, the deterministic reproduction proved the more serious generic defect in the same post-prepare path: implementation timeout/failure was not being promoted to terminal failure.

## Fix

`ExecuteChange` now owns a single post-prepare execution wrapper that converts late failures into terminal persisted packets.

Changes:

- after worktree creation, all later execution now flows through `execute_prepared()`
- implementer `worker_error` / `protocol_error` now become `ChangeStatus::Failed`
- post-prepare hard errors from the implementer no longer escape unpersisted
- accepted-revision materialization failures no longer escape unpersisted
- post-prepare `run_checks` construction failures no longer escape unpersisted
- failed implementation still reuses the existing post-implement Git inspection path, so changed-path evidence is retained where safely inspectable

## Deterministic regression evidence

Application tests added:

- `worker_timeout_becomes_terminal_failed_packet_without_checks`
- `post_prepare_implementer_error_persists_terminal_packet`
- `accepted_revision_materialization_failure_persists_failed_packet`

CLI smoke added:

- `tests/rack_change_timeout_smoke.sh`

That smoke proves:

- a fake timed-out JCode implementer mutates the allowed file
- Rack AI returns from `cargo run -q -p rack_ai_cli -- change ...`
- no `accepted_revision` is emitted
- a terminal `review-packet.json` is persisted
- the packet records timeout failure and retained candidate output

## Live-model evidence

Bounded live local-coder reproduction on `2026-08-31` through the PR29 host-executor path completed normally and returned a terminal approved packet. That confirmed the broad host-executor route was sound and the defect was narrower than "all JCode changes hang".

## Residual risk

The exact final blocking syscall of the interrupted ATHBA run was not directly reconstructable because no packet or phase file existed before the operator interruption. The repaired invariant is stronger than the specific missing trace:

once Rack AI has prepared a managed external-repository worktree, later implementation failures now resolve into a terminal persisted packet instead of being silently downgraded or escaping via late `?` propagation.
