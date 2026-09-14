# PR35 correctness and robustness follow-up

Reviewed base: `b84db0f28b9cea10ba517d2907f83f006583ee84` on
`design/priority-runtime-reservations`. All implementation, fixtures, builds and tests
ran over SSH in `/srv/rack-ai/.worktrees/pr35`. No NUC code, companion applications,
other PRs, permanent services, production model endpoints or GPU workloads were changed.
The priority planner is unchanged: Low < Medium < High < Paramount, incumbent wins ties,
and a blocker anywhere in the complete resource set denies the incoming reservation.

## Reproduction and correction

| Finding | Reproduced behavior | Correction and regression result |
|---|---|---|
| 1. Durable cancellation | A real delayed synthetic backend completed after cancellation; the baseline exposed `completed` and ordinary `result`, despite the cancellation error string. | Typed timestamped cancellation intent survives repeated cancellation and receiver restart. A received late valid response is `cancelled`, with `result: null` and `late_result` evidence. Restart/unknown transport remains `uncertain`; no stop or replay is fabricated. `test_started_cancellation_preserves_late_output_without_success` and `test_cancel_restart_retains_intent_and_never_replays_uncertain_dispatch`: **PASS**, exactly one actual backend dispatch each. |
| 2. Waiting versus execution | Under a held primary reservation, the original one-second invocation expired before restoration after a two-second hold. Submission during preemption's Draining phase also returned 409. | Optional bounded `wait_seconds` and durable `waiting_deadline` are separate from `execution_deadline`, set at dispatch from the same start timestamp. Readiness/hold/capacity waiting does not consume execution timeout. Active Draining accepts bounded pending work but cannot dispatch. Held restoration, genuine waiting expiry, pending cancellation, reservation expiry and drain submission tests: **PASS**. Original eligible work dispatches once; expired/cancelled pending work never dispatches. |
| 3. Compatibility identity | Identical payload after generation restoration conflicted with old owner-scoped identity. Two distinct real workspace transactions with identical model payloads reused one backend invocation (baseline count 1, expected 2). | Lookup scope is owner + reservation + explicit logical ID; same-key retries reconcile, changed requests conflict, and valid restored-generation retries reconcile original evidence. Headerless fallback includes activation; JCode private configuration adds stable workspace/task call scope, and direct review sends a stable campaign/step/evidence key. Explicit distinct IDs/replay/conflict, restoration, release/reacquisition, stale capability, uncertain response/restart, distinct identical workspaces and actual review HTTP header tests: **PASS**. No fresh random identity is minted per transport attempt. |
| 4. Queue, workers, storage | The baseline accepted saturated pending work, ran two backends concurrently under the requested one-worker scenario, and rewrote the authority inode on status reads. A seeded 32 MiB-minus-64 KiB baseline document produced a generic authority-lock failure before a new admission could complete. | Validated global/per-reservation pending limits, dispatch/lifecycle file permits before spawn, dispatch eligibility and one candidate per reservation, bounded async gateway waiters, and separate bounded admission/control HTTP slots. Read-only snapshots do not write; stable supervisor inspection does not acquire the mutation lock, and real mutations avoid redundant serialization. Unchanged mutations skip writes. Admission reserves worst-case encoded results and cleanup space below a default 30 MiB ceiling. Queue/worker/read-only/gateway-pressure tests, 512 KiB near-admission-ceiling integration, 30 MiB-minus-64 KiB retained-history idle-lock/control regression, actual filesystem write failure, configuration bounds and the 32 MiB hard-bound authority test: **PASS**. New work receives precise HTTP 429 capacity codes; previously accepted work completes and all owned claims release in the near-ceiling integration. |
| 5. Positive managed workspace proof | This was a coverage gap, not proof that all baseline managed workspace requests failed. A baseline first valid request succeeded, but held work became an ordinary failure and a second identical transaction reused old model output. | Three RackAI-only integration tests run the actual `work-unit` executable, selection/resolver, private JCode configuration, bubblewrap/Unix-TCP bridge, managed gateway, controllable synthetic backend and Podman acceptance executor. **PASS**: valid request, held primary, independent coder, restoration once, distinct identical transactions, stale access rejection, selection/execution/returned provenance agreement, original base revision, committed accepted revision, path isolation, rejected acceptance and bounded timeout. Existing raw JCode/review/recovery bypass tests still pass. |

The seeded near-32 MiB baseline experiment did **not** reproduce a late completion failing
at the storage ceiling: it stopped earlier at authority-lock starvation and is reported
as such. The unchanged baseline `ManagedAuthority::update` also shows that reads and
completion both serialize/write the entire document and reject any encoding above
32 MiB, without reserving output space at admission. The correction's near-ceiling test
uses real accepted/returned backend results and proves completion and release after
precise refusal. The added large-history regression reproduced 20/20 blocked lock probes before the idle-path correction and 0/20 afterward, with read-only status and precise new-admission refusal. A separate hard-bound test retains the original large evidence string
and permits a bounded cleanup addition after refusing an oversized write.

Cancellation with an unknown result remains uncertain even when cancellation was
requested. The system does not claim whether a killed receiver's backend actually
received or finished a call. Synthetic backend event counts provide the actual invocation
counts in these tests. A filesystem failure cannot be turned into durable success by
logical headroom; recovery remains fenced.

## Verification actually run

Evidence is retained, unstaged, under
`/srv/rack-ai/.worktrees/pr35/evidence/pr35-corrections/`.

| Check | Actual result | Evidence |
|---|---|---|
| Initial baseline regressions | Four expected failures: cancellation, held deadline, restored fallback identity, read rewrite | `reproduced.log` |
| Additional unmodified baseline binaries with identical disposable CPU fixtures | Three expected failures: queue admission, distinct identical workspace calls, held workspace failure | `baseline-extra.log`, `baseline-build.log`, `reproduce-baseline.py` |
| Baseline actual dispatch concurrency | Expected failure: peak 2 instead of 1 | `baseline-workers.log` |
| Large retained-history idle-lock regression | Expected failure: 20/20 probes blocked; correction: 0/20 blocked and precise capacity refusal | `idle-lock-red.log`, `idle-lock-green.log` |
| Draining submission regression before its correction | Expected 409 `reservation_not_dispatchable` | `draining-red.log` |
| Seeded near-hard-limit baseline experiment | Blocked earlier by generic authority-lock refusal; no successful completion claim | `baseline-retention.log`, `reproduce-retention-baseline.py` |
| Final full Rust workspace tests | **352 passed**, zero failures | `cargo-test-final.log` |
| Final full runtime Python suite | **49 passed**, zero failures, including all **19 new Python regressions** | `runtime-complete.log` |
| Focused review/workspace/protocol/schema checks | **21 passed** before the added large-history case; final workspace provenance assertions **3 passed** and large-history case **1 passed**, all also covered by the full suite | `focused-final.log`, `workspace-final.log`, `idle-lock-green.log` |
| Existing media suite | **72 passed**, zero failures, 609.83 seconds | `media.log` |
| Retained A–F and reverse three-resource scenario | **PASS**; three native synthetic media renders | `scenario-final.log`, `scenario-final/` |
| Workspace prepare, Podman executor, path-policy and timeout shell smokes | **PASS** | `change-smoke.log`, `change-executor_smoke.log`, `change-path_policy_smoke.log`, `change-timeout_smoke-final.log` |
| Rust format, patch whitespace and runtime clippy with warnings denied | **PASS** | `fmt.log`, `clippy-runtime.log`, final patch check |
| Strict whole-workspace clippy | **FAIL: unchanged baseline application warnings**, 37 library / 39 with tests | `clippy-workspace.log` |

The original 349/30/72 results in the earlier handoff describe the reviewed base and
are not used as evidence for these additional cases. The current Rust count includes
actual review-client HTTP identity and authority hard-bound tests. Full runtime and
focused counts overlap and must not be added together.

For semantic baseline comparisons, the unmodified baseline binaries used the same
synthetic backend/harness fixtures, with unsupported new `limits` configuration removed.
The baseline had no such bounds. Intermediate setup failures were corrected in fixtures:
the positive workspace registry needed its own Git root and the test used the actual
`packet_path`/`selection_decision` response fields. The preexisting timeout smoke copied
today's generic worker roles but expected a legacy default implementer; its disposable
worker now explicitly has `implementer-tester` role. Production selection was not weakened.

A–F/reverse retains primary/coder/fun-chat/big-brain dispatch counts 3/3/1/1. Coder stays
running through A–F; its one stop occurs only for the reverse all-device takeover.
ComfyUI's synthetic lifecycle records two starts, one stop and three native renders.
These are CPU fixture processes exercising production logic, not live GPU qualification.

## Operational boundary and handoff

The [public contract](runtime-public-contract.md#configuration-validation-and-retention)
publishes exact limits, identity/replay rules, cancellation outcomes and the retention
procedure. Retention admission is deliberately conservative and can refuse before the
nominal byte ceiling. Results have a frozen default 256 KiB bound (administrator maximum
4 MiB). Existing on-disk deadlines and response bounds remain bounded and readable.

There is no automatic evidence deletion or archive compaction. At capacity, stop new
submissions, finish/cancel accepted work, reconcile uncertainty, prove owned cleanup,
and retain the complete canonical identity history. After quiescence take a verified
immutable copy. Do not restart an empty authority under the same credentials; resumable
archive migration remains a separately reviewed operational change. Older authorities
without reserved headroom require a quiescent capacity review before upgrade.

| Marker | Status |
|---|---|
| `RACKAI_PR35_CORRECTIONS_VERIFIED` | PASS |
| `PRIORITY_SCENARIO_PASSED` | PASS, isolated synthetic processes |
| `MANAGED_WORKSPACE_PROOF` | PASS, actual bounded entrypoint and harness |
| `RACKAI_LIVE_QUALIFIED` | NOT_RUN |
| `BIG_BRAIN_BOOTED` | NOT_RUN |
| `BIG_BRAIN_QUALIFIED` | NOT_RUN |
| `PRODUCTION_ROLLOUT_STATUS` | NOT_RUN |

No merge, production cutover, new production service or live GPU/model qualification
was performed. Work stops at the PR35 correction and verification handoff.
