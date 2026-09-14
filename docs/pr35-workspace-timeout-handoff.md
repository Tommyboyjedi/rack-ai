# PR35 authoritative workspace timeout follow-up

Reviewed head: `fcbb64f7918e6763a15fca9e026758bcee0ab5fc`, branch
`design/priority-runtime-reservations`. All changes and disposable tests ran over SSH
in `/srv/rack-ai/.worktrees/pr35`. Scope is RackAI only.

## Reproduction

The finding was reproduced through the actual `work-unit` CLI, registered synthetic
JCode harness, real bubblewrap/Unix-TCP bridge and managed gateway. Low primary was
preempted by Paramount fun-chat. The test waited for its exact, sole invocation to
be durably Accepted with `started: null`, then allowed the workspace's three-second
deadline to expire. It did not cancel the invocation or release primary. With both
the runtime waiting deadline and primary TTL still valid, releasing fun-chat caused
one actual backend dispatch and a Completed invocation. The expected-zero dispatch
assertion failed. Evidence: `evidence/pr35-workspace-timeout/reproduced.log` (one
expected failure, 8.95 seconds). The previous test's manual cancellation supplied
cleanup that the production timeout path lacked.

## Correction

The trusted runner durably registers the exact workspace/task scope and its deadline
before JCode starts, then closes it on timeout or other exit. Registration, closure
and inference admission are serialized through the existing authority. Closed/expired
scope records cannot be reopened, extended, or crossed by a delayed new submission.
Scope eligibility is rechecked at dispatch and completion. The deadline is captured
before registration/setup from the same bounded workspace budget as the process
watchdog, so a control delay cannot extend permission beyond the workspace timeout.

Pending calls become durably Cancelled; Started calls keep truthful bounded draining
and late-output/uncertainty evidence. No whole reservation or unrelated invocation
is cancelled. A rotated original capability can only close its own registered scope;
stale inference access remains rejected. HTTP disconnect alone still leaves durable
work intact. Failed registration prevents harness startup. Failed cancellation
persistence remains explicit in the workspace terminal packet, while the already
persisted deadline fences dispatch even before cancellation can be saved.

The change preserves priority planning, waiting/execution budgets, invocation replay,
queue/worker limits, read-only inspection, retained evidence and workspace protections.
Scope admission reserves bounded closure growth and uses existing capacity refusals.
See [the public contract](runtime-public-contract.md#bounded-workspace-call-lifetime).

## Regression coverage and results

The three new workspace regressions use the actual bounded entrypoint and harness;
the seven scope regressions exercise its production control/gateway boundary directly.
No standalone runtime infer call substitutes for the workspace timeout proof.

| Test | Actual result |
|---|---|
| `test_workspace_timeout_cancels_exact_pending_call_before_restoration` | PASS: exact Accepted invocation observed before timeout; Cancelled afterward, `started: null`, zero primary dispatches after restoration; primary remains reserved and independent coder workspace succeeds. |
| `test_workspace_timeout_during_actual_dispatch_retains_late_evidence` | PASS: actual workspace deadline races an actual delayed backend; one dispatch, typed cancellation, Cancelled with `late_result`, no ordinary result, reservation intact. |
| `test_workspace_reports_cancellation_persistence_failure_without_late_dispatch` | PASS: real authority-directory write denial at workspace timeout; terminal packet explicitly reports unconfirmed cancellation persistence. After storage recovery/restoration the original invocation is Cancelled with zero dispatches, without manual cancellation. |
| `test_delayed_http_submission_cannot_cross_closed_or_expired_scope` | PASS, two subtests: HTTP headers and partial body arrive before closure/expiry; the remaining body arrives afterward. Both are rejected, with zero admitted invocations and zero backend calls. |
| `test_close_replays_and_preserves_unrelated_shared_reservation_work` | PASS: repeated close preserves identical cancellation evidence; registration replay cannot reopen; unrelated pending primary work completes once. Old capability can close only its scope after restoration, while stale inference fails. |
| `test_close_during_started_call_retains_one_late_success` | PASS: repeated closure during a real delayed backend call; exactly one dispatch, retained late output, no ordinary success. |
| `test_close_during_readiness_probe_prevents_dispatch` | PASS: closure races a delayed readiness probe; final dispatch transaction leaves `started: null`, zero actual dispatches. |
| `test_temporary_disconnect_reconciles_and_legitimate_work_restores` | PASS: caller HTTP timeout leaves Accepted work uncancelled; restoration completes it once and a retry reconciles the same result. |
| `test_close_storage_failure_is_explicit_and_deadline_survives_restart` | PASS: actual write failure is explicit; the precommitted scope deadline survives receiver restart and the pending invocation terminalizes without dispatch. A lost old HTTP response is reconciled, not treated as success. |
| `test_scope_deadline_does_not_replace_per_call_waiting_limits` | PASS: a 900-second workspace scope is accepted independently of shorter per-call limits; closure still prevents new submission. |

The existing successful held-primary/coder/restoration/provenance proof, distinct
identical workspaces, stale-generation rejection and revision/path/acceptance tests
remain. The old timeout test's manual runtime cancellation was removed; it now releases
only the blocker and asserts no extra backend dispatch.

Evidence is retained, unstaged, under `evidence/pr35-workspace-timeout/`.

| Verification | Actual result | Evidence |
|---|---|---|
| Initial focused workspace/scope suite | 12 passed, two subtests passed (74.00 seconds), before adding the separate 900-second-scope regression | `focused-second.log` |
| Full runtime suite before final local process-boundary extraction | 59 passed, two subtests passed (224.41 seconds) | `runtime-final.log` |
| Final full runtime suite | **59 passed**, two subtests passed, 226.56 seconds | `runtime-verified.log` |
| Final Rust workspace suite | **352 passed**, zero failures | `cargo-test-verified.log` |
| A–F and reverse scenarios | PASS; three synthetic native media renders; primary/coder/fun-chat/big-brain dispatch counts 3/3/1/1 | `scenario.log`, `scenario/` |
| Workspace prepare, Podman executor, path-policy and timeout smokes | PASS, all four exit zero | `change-verified-*.log` |
| Runtime clippy with warnings denied, Rust formatting and patch whitespace | PASS | `clippy-runtime-verified.log`, `fmt-verified.log`, final diff check |
| Infrastructure clippy | PASS with baseline warnings: 26 messages / 17 distinct primary spans; every warning's source fragment was verified present at the reviewed head; none in new scope code | `clippy-infrastructure-verified.jsonl`, `lint-baseline-comparison-verified.json` |
| Strict whole-workspace clippy | FAIL on unchanged baseline application warnings: 37 library / 39 including tests | `clippy-workspace.log` |

The full runtime total includes all 10 new Python tests, the existing 49 runtime tests,
and all six actual workspace cases. Focused and full counts overlap. Earlier 352/49/72
counts are historical evidence, not proof of this correction. The full 72-test media
Python suite was not rerun for this targeted workspace change; the current Rust media
tests and retained synthetic A–F/reverse media scenario did run.

Initial focused test failures were test-observation mistakes: the timeout diagnostic
lives in the durable packet rather than the concise CLI response, and a killed receiver
can close an old HTTP connection without a response. Assertions now inspect the actual
packet and reconcile durable state across restart. Neither was fixed by manual cancellation.

The small process-boundary extraction keeps scope lifetime control separate from
process launch/watch/drain; no unrelated harness policy was changed.

`WORKSPACE_TIMEOUT_PROPAGATION_VERIFIED`: **PASS**. Production functions and class-equivalent units remain within the repository size rule; the two local process functions are 69 and 46 nonblank/noncomment lines.

## Limits

Synthetic CPU backends prove the production control and workspace paths; they do not
qualify a GPU model. Storage failure cannot be reported as durable cancellation until
persistence recovers. Abrupt runner loss has the registered deadline as its bounded
fallback; a temporary HTTP disconnect intentionally does not cancel a live workspace.
Pre-upgrade unscoped calls cannot be retrospectively attributed to an execution scope.
There is no evidence deletion, production service change, companion application change,
merge or cutover. `RACKAI_LIVE_QUALIFIED`, `BIG_BRAIN_BOOTED`, `BIG_BRAIN_QUALIFIED`
and `PRODUCTION_ROLLOUT_STATUS` remain **NOT_RUN**.
