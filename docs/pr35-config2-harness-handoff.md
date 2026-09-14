# PR35 configuration #2 harness correction

Reviewed baseline: `d35959e0f9354dd62cdffb5b492a85e5ce08302f`.
Work was confined to qualification helpers, tests and documentation in
`/srv/rack-ai/.worktrees/pr35`. No managed runtime policy, placement, llama.cpp
arguments, production code or service configuration was changed.

## Config #2 evidence and scope

The operator's preceding run is retained at
`evidence/gpt-oss-120b-single-model/config2-20260914-190425/`.
The backend journal records a 7171.82 MiB CUDA2 allocation failure, followed by
`cudaMalloc failed: out of memory`, model-load failure and exit status 1.
CUDA2 is the RTX 2060 in this launch. This is placement failure evidence; no
placement correction or configuration #3 experiment was performed here.

The authenticated activation was reservation `fe609dd606a02859b76bf3f9f439601f`,
generation `e34dc76dc68478e759eb46c1b27d618f`, PID 1124135, systemd invocation
`cdfe3713ef1e4fd895e05cf154012f03`. The retained hardware log has 102 samples.
Its last workload sample has zero swap, zero OOM/oom_kill events and
MemoryCurrent 40537485312 below MemoryMax 53687091200. Host/cgroup RAM evidence
must not be confused with the separately reported GPU allocation failure.

The retained release receipt and read-only canonical state both confirm that
exact reservation/generation is cancelled, process=null, released=true and
effect_started=false. The correction did not cancel it again or mutate claims.
Raw config, hardware, container/unit snapshots, release evidence and journals
were hashed before work and preserved.

## Defect 1: lifecycle exit was conflated with integrity failure

`CgroupProbe.read` required the recorded PID to remain MainPID until explicit
retirement. A normal non-zero exit during activation therefore raised
`managed workload process unavailable or changed`, wrapped as `monitor failed`.
It did not consult systemd's exit evidence. After collection the unit's show
properties can be empty/default, including Result=success, even though the
journal retains the exact invocation's failure. Neither a zero MainPID nor
those default properties prove a successful or unexplained outcome.

The new `ExitEvidence` reads bounded structured journal evidence for the exact
unit, invocation, boot and user. It requires systemd's main-process-exit event,
ExecStart, exit code/status and journald-assigned systemd executable/process/user
manager identity. Backend-written message text alone cannot establish this proof.
The authenticated PID must be absent or an exact-start-tick zombie; a live or
reused PID, changed boot, foreign unit/invocation, replaced cgroup, inaccessible
process evidence, missing/duplicate/untrusted journal records or unavailable
monitor transport still fails safely.

Separate one-second bounded visibility windows allow process exit to precede
systemd's state update and the journal record. Observations retain their existing
two-second command bounds; an in-flight command can add its bounded latency.
These are read-only reconciliation waits, never restarts or inference retries.
Persistent ambiguity remains terminal. The single ExecStart, non-restarting
managed transient activation binds the manager's invocation-scoped exit record
to the authenticated main process.

A proven exit is now a terminal `managed backend startup failure: exited status=1`
during loading (or backend execution failure after readiness), not monitor
corruption. Hardware evidence and `backend-exit.json` retain the typed outcome,
full process identity and structured manager record. If authority failure arrives
before the monitoring tick, activation handling obtains the same proof. The
backend journal is still collected from the pinned unit even when the runtime's
current process field has already cleared. Evidence persistence failure remains
explicit. A classified exit never releases claims or authorizes restoration;
the existing managed process/cgroup/GPU cleanup and durable receipt gates remain.
Host reserve, workload swap/OOM limits and inference page-in protection remain.

## Defect 2: mount list order was treated as configuration identity

`Services.restore` used Python equality on the raw Mounts list. Python list
comparison is order-sensitive, although mount record order does not describe
container configuration. The same list content in a different order was refused.
An actually identical ordered list cannot trigger this comparison; a later
matching inspection does not establish the ordering of the failing inspection.

The precise failing inspection pair was not retained in config #2. Its original
snapshot and our first current inspection compare equal. Independent previously
retained read-only inspections of this same coder container demonstrate changed
Mounts order with strict Id/Image/Config/HostConfig equality and identical mount
records. The derived comparison is retained as `mount-order-evidence.json`.
The focused regression reproduces the exact restoration exception by reversing
only the mount list. No additional representation normalization was inferred.

`mount_configuration` now compares a deterministic sorted multiset of complete
mount records. Type, Source, Destination, Mode, RW and Propagation are required;
all additional fields and duplicate counts are retained. Only outer mount order
is ignored. Missing/malformed evidence fails. Id, Image, Config and HostConfig
continue using their original strict comparisons. Genuine changes still prevent
Docker start; the comparison does not recreate or alter containers.

## Regressions and verification

New-run evidence: `evidence/pr35-config2-harness-correction/`.
Before correction, four focused tests produced two expected errors (owned exit
misclassified; reordered mounts refused) and two passes for real identity/config
changes. `before.log` retains the failures.

New coverage includes exact non-zero exit, an actual disposable Python child
exiting 7, same-generation zombie, PID reuse before/during journal lookup,
changed unit/invocation/boot, missing/duplicate/untrusted exit records, journal
permission failure, delayed journal and unit-state visibility, persistent
ambiguity, replaced cgroup, monitor persistence/classification, authority-first
failure and backend-journal preservation after process-record clearing.
Mount regressions cover reorder acceptance, all six required field changes,
additional fields, duplicate counts and strict container/image/config/host-config
identity. Rejected restoration asserts no Docker start was issued. All systemd,
journal and Docker transports in these tests are fixtures; no model was loaded.

| Check | Actual result | Evidence |
|---|---|---|
| Focused new regressions | PASS: 19 tests | `focused-final.log` |
| Full qualification helpers | PASS: 51 tests | `helpers-final.log` |
| Full runtime Python suite | PASS: 72 tests and 2 subtests, 280.39 s | `runtime-full.log` |
| Offline Rust workspace | PASS: 352 tests | `cargo-workspace.log` |
| Full media Python suite | PASS: full media Python suite, 72 tests in 608.02 s | `media-full.log` |
| Rust formatting / strict runtime clippy | PASS | `fmt.log`, `clippy-runtime.log` |
| Python compilation / class and parameter bounds / patch whitespace | PASS | `python-compile.log`, `python-structure-final.json`, final diff check |

Commands: `PYTHONPATH=tools/qualification python3 -m unittest
test_config2_regressions -v`; `python3 -m unittest discover -s tools/qualification
-v`; `.venv/bin/python -m pytest tests/runtime -q`; `.venv/bin/python -m pytest
tests/media -q`; `cargo test --workspace --offline`; `cargo fmt --all -- --check`;
`cargo clippy -p rack_ai_runtime --all-targets --offline --no-deps -- -D warnings`.
No Rust crate changed; the runtime lint was rerun as an additional check.
The previously documented unrelated whole-workspace clippy baseline was not
rerun or repaired in this harness-only correction.

## Continuity and limitations

Read-only before/after checks (`continuity.json`) confirm both resident containers
remain healthy with unchanged IDs, images, Config, HostConfig, semantic mounts,
PIDs and start times. Primary PID 1125002 and coder PID 1126814 remain unchanged
through this correction. RackAI unit states/PIDs, inactive ComfyUI, the canonical
managed-authority digest and all config #2 raw evidence hashes are unchanged.
No containers were stopped, started or recreated, and no live model call was made.
No production checkout, compose file, driver, model library or companion app was
modified. Evidence remains untracked.

Missing trusted exit evidence still produces an evidence/ownership failure rather
than guessing a backend cause. This classifier requires the systemd user-manager
journal schema used by the managed transient profile; it does not qualify another
hosting driver. Exit evidence does not prove child/cgroup/GPU cleanup. An observed
backend exit does not mean RackAI stopped computation. No live qualification is
claimed for these deterministic tests. Configuration #3 placement remains a
separate operator decision.

BACKEND_EXIT_CLASSIFICATION_CORRECTED = YES
DOCKER_MOUNT_RESTORE_COMPARISON_CORRECTED = YES
GPT_OSS_CONFIG_3_RUN = NOT_RUN
LIVE_SERVICES_CHANGED = NO
