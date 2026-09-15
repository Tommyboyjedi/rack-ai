# PR35 memory monitoring and transient cleanup correction

Correction baseline: `b7cdaed6de0cc296904316b5e2b7dc015625eb97`, remote
`/srv/rack-ai/.worktrees/pr35`. This is a deterministic correction and test pass,
not another GPU qualification window. Raw launch evidence is unchanged.

## What the first managed launch established

The earlier cache-preflight blocker in `pr35-gpt-oss-120b-results.md` preceded
the operator's repair of the resident models. The later actual managed attempt
is retained at `evidence/gpt-oss-120b-single-model/live-20260914-140111/`.
Its 99 hardware samples record global swap rising from 391 to 680 MiB (+289),
while minimum host MemAvailable was 58166 MiB. The retained `ABORT` is exactly
`swap growth exceeds 256 MiB`. All samples are in the loading phase; successful
inference or model performance is not established by this trace.

`initial/runtime-limits.txt` records MemoryCurrent=46246285312,
MemoryMax=47244640256 and MemorySwapMax=0. Global swap movement therefore did
not identify swapping by the managed workload or exhaustion of the host reserve.
Unrelated cold host pages are a plausible explanation, not a measured attribution:
the old monitor did not retain per-cgroup swap/events counters. We cannot
retrospectively prove the absence of every workload memory event.

The acquisition ID is `6ee1f6c169eb0406c304dcb91662f5ce`, generation
`cbb41acf450446b65dbd138dbe601b45`. The operator reported the first cancel reaching
`recovery_required` with `cgroup_still_populated`, then a successful repeated
cancel after physical teardown. Read-only inspection during this correction
confirmed that exact retained generation is cancelled, process=null,
released=true, effect_started=false, with no canonical claims. No cancel or
claim mutation was performed in this correction task.

## Memory signal correction

The old rule reproduced: a healthy host and zero-swap workload were aborted
solely because global swap increased. The old monitor also ignored synthetic
workload swap/OOM evidence. Opening its evidence log outside its exception
handler could silently kill the monitor thread.

The monitor now retains global swap totals/deltas as observations. Host available
memory below the configured reserve (default 6144 MiB) still aborts. Temperature
checks and the ten-successive-sample inference page-in guard are unchanged.
There is no higher global swap allowance and no model-specific exception.

`WorkloadWatch` binds observations to the authenticated reservation/generation
at acquisition, before Ready. After effect_started, missing process registration
has a bounded 10-second allowance; it is not an unlimited unobserved load.
Once a process is recorded, `CgroupProbe` verifies its boot/start ticks,
activation environment, cgroup membership, systemd unit, InvocationID and MainPID.
The cgroup must be an absolute non-root path under cgroup v2. Its device/inode
identity is checked during and across samples; unrelated/replaced groups are
not rebound silently.

Required observations are memory.current, memory.max, memory.swap.current,
memory.swap.max, memory.events and memory.swap.events. The memory ceiling must
match the approved profile and swap ceiling must remain zero. Workload swap,
OOM/oom_kill/oom_group_kill, swap allocation failure or an exceeded memory bound
aborts. Reclaim high/max event counters are retained without treating reclaim
alone as OOM. PSI is retained when available; its absence on kernels without PSI
does not substitute for the required counters.

Missing/malformed counters, permission/transport failures, changed ownership and
unexpected disappearance fail the monitor. Explicit managed cancellation marks
retirement: a verified ending unit can disappear; a still-readable retiring
cgroup continues reporting memory events. Ambiguous teardown observations still
fail. The watch clears only after the matching durable terminal cleanup receipt.
A read-only probe never releases claims or authorizes service restoration.
Monitor failure propagates through inference polling and the final window check,
even if the ABORT file itself cannot be written. Safe cleanup remains mandatory.
MemorySwapMax=0 and all profile/host limits remain enforced.

## Cleanup correction

The old cleanup loop waited for PID exit, then made one systemd/cgroup check.
A deterministic 0.8-second delayed teardown reproduced premature recovery.
A persistent teardown also failed after about 0.36 seconds instead of waiting
the fixture's 2-second cleanup budget. A changed invocation after process exit
was not checked by the old final cleanup gate.

`Teardown` now polls both process and transient unit/cgroup state within the
profile's stop_seconds budget. It does not reissue termination in a retry loop.
An inactive unit with a still-existing reported cgroup remains unproven, even
if that cgroup is empty. Pending jobs/deactivation also retain claims. Unit
identity, invocation and recorded PID are checked on every observation; process
start-tick changes, unknown invocation and observation errors fail immediately.
The original activation/executable checks remain before signaling; unit name,
nonempty invocation and MainPID checks also protect that boundary.

Only proven teardown followed by the existing GPU/host cleanup checks can release
claims. Deadline exhaustion remains `stop_deadline_cleanup_unproven` and enters
recovery_required. Command observations retain their own existing bounded
transport timeout, so an in-flight probe can add its bounded command latency to
the polling budget. Repeated valid cancellation after physical cleanup remains
safe; ownership or persistence uncertainty never becomes permission to restore.
Docker and ComfyUI lifecycle implementations are unchanged.

## Regression and verification evidence

Evidence for this correction is under `evidence/pr35-memory-cleanup-correction/`.
The valid before-fix monitor run had five failures and one error across seven
tests: global swap false abort, four ignored workload-failure subcases, and log
creation failure escaping the thread handler. The valid before-fix teardown run
had four failures and one pass. An earlier fixture setup error (missing pinned
artifact) is retained separately and is not counted as reproduction.

New coverage includes:

- Healthy reserve plus large global swap movement and zero managed swap; host
  reserve violation; owned swap/OOM/changed-limit failures; reclaim-only events;
  retained loading/inference page-in distinction.
- Real synthetic kernel-file reads, optional PSI, required-counter loss/corruption,
  permission/transport failures, process/unit/cgroup identity changes, replaced
  cgroup inode, bounded registration before Ready, exact cleanup receipts,
  retirement memory events and unwritable monitor evidence propagation.
- Process-first teardown with a briefly retained reported cgroup; persistent
  deactivation and inactive-but-present cgroup; changed/unknown InvocationID;
  observation failure; changed recorded process generation without a stop signal;
  foreign GPU evidence; repeated valid cancel. All backend processes are disposable
  RackAI fixtures, and systemd/NVIDIA transports are synthetic.
- Existing real authority-directory permission failure: physical cleanup cannot
  release claims durably until storage recovers; uncertain output is not replayed.

| Check | Actual result | Retained log |
|---|---|---|
| Focused monitor and kernel-file regressions | PASS: 22 tests | `monitor-focused.log` |
| Full qualification helper suite | PASS: 32 tests | `helpers-verified.log` |
| Focused runtime cleanup/hosting/qualification cleanup | PASS: 19 tests; final cgroup fixture also covered by full runtime rerun below | `cleanup-focused-final.log` |
| Full runtime Python suite | PASS: full runtime Python suite, 72 tests and 2 subtests in 277.47 s | `runtime-verified.log` |
| `cargo test --workspace --offline` | PASS: 352 tests, zero failures/ignored | `cargo-workspace.log` |
| Full media Python suite | PASS: full media Python suite, 72 tests in 609.51 s | `media-full.log` |
| Strict affected-crate clippy, all targets, no dependencies | PASS, warnings denied | `clippy-runtime.log` |
| Rust formatting, Python compilation, patch whitespace | PASS | `fmt.log`, `python-compile.log`, final `git diff --check` |
| Strict whole-workspace clippy | FAIL: unchanged application baseline, 37 library / 39 including tests | `clippy-workspace.log`, `lint-baseline-check.json` |

Commands used: `PYTHONPATH=tools/qualification python3 -m unittest test_monitor
test_workload_probe -v`; `python3 -m unittest discover -s tools/qualification -v`;
`.venv/bin/python -m pytest tests/runtime -q`; `.venv/bin/python -m pytest
tests/media -q`; `cargo test --workspace --offline`; `cargo fmt --all -- --check`;
`cargo clippy -p rack_ai_runtime --all-targets --offline --no-deps -- -D warnings`.
The focused runtime command selected `test_teardown.py`, `test_hosting.py` and
`test_qualification_cleanup.py`. Full runtime coverage retains the A-F and reverse
scenarios, durable cancellation/replay/capacity and actual workspace timeout and
held-work restoration regressions. Counts above are from actual new runs.

All 15 source files cited by the strict workspace lint failure were checked
unchanged from the reviewed baseline. No warning suppression was added. Tests
used the existing remote venv, NVMe temporary directory, and existing isolated
browser/library setup for media. No packages or system services were installed.

Read-only continuity checks (`continuity.json`) confirmed both resident containers
remain healthy with unchanged IDs, images, configuration, mounts, PIDs and start
times: primary PID 1024306, coder PID 1024524. Campaign supervisor PID 1027484
and media receiver PID 1027491 remain running; ComfyUI remains inactive.
The canonical managed authority digest and all retained launch evidence hashes
are unchanged. Only PR35 code/tests/docs and its disposable test/evidence files
were written; evidence remains untracked.

## Next qualification boundary

The retained journal reports:
`failed to fit params to free device memory: n_gpu_layers already set by user to -2, abort`
with `--gpu-layers all --n-cpu-moe 24 --split-mode layer --fit on`.
This message is followed by model loading, so it alone is not evidence that the
backend exited. Explicit placement with `--fit off` is the proposed configuration
#2 decision for a later, separately authorized window. No fit/placement option,
runtime binary, model artifact or private launch configuration was changed here.
The reported CPU-MoE/fit compatibility concern must be considered in that review.
Full artifact hashing now reportedly takes about 188 seconds; the next approved
configuration must also budget verification from current measurements.

The corrected code is ready for review and manual authorization of the next
bounded qualification attempt after its placement, limits and restoration
preflight are approved. It does not prove GPT-OSS fits, starts, performs adequately,
or restores on real hardware. The monitor requires readable owned systemd/cgroup
v2 evidence; other hosting drivers are not qualified by these tests. Polling does
not capture every intermediate event, though OOM/event counters are cumulative
while the pinned cgroup exists. The retained host-wide inference page-in heuristic
can still include unrelated I/O. Missing/ambiguous evidence remains an abort.

QUALIFICATION_MEMORY_SIGNAL_CORRECTED = YES
TRANSIENT_CGROUP_CLEANUP_CORRECTED = YES
GPT_OSS_CONFIG_2_RUN = NOT_RUN
ORIGINAL_SERVICES_CHANGED = NO
