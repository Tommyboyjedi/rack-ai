# GPT-OSS-120B qualification history

## Current handoff: configuration #2 harness corrections

The operator completed configuration #2. CUDA2 / RTX 2060 attempted a 7171.82
MiB allocation and failed with CUDA out of memory; the exact managed process
exited with status 1 during startup. That placement decision remains separate.
See [the config #2 harness handoff](pr35-config2-harness-handoff.md) for corrected
owned-exit classification, deterministic Docker mount restoration comparison,
regressions and verification. No configuration #3 launch or live service change
occurred in this correction task.

## Configuration #1: first managed launch and safety corrections

After the earlier preflight recorded below, the operator repaired the resident
models onto `/srv/models/huggingface` and performed the first managed launch.
That launch aborted on +289 MiB global swap movement despite 58166 MiB minimum
MemAvailable and MemorySwapMax=0. Its initial cleanup encountered transient
cgroup teardown; a later valid cancel completed cleanup without deleting state.

See [the memory and cleanup correction handoff](pr35-memory-cleanup-handoff.md)
for exact evidence, deterministic reproductions, fixes, verification and limits.
No additional GPT-OSS launch or service change occurred in this correction task.
Configuration #2 remains NOT_RUN; model qualification remains unestablished.

## Historical preflight report, before the operator's repair

The following records the earlier blocked attempt and its then-current state.
It does not describe the later repaired services or the later actual launch.

On 2026-09-14, the user authorized a bounded single-model experiment from PR35
head `1f4c1bd3963049f8998903b1781c0cc88d9c3507`, including temporary model-service
stops and exact restoration. No live competing-priority scenario was authorized.

**The experiment stopped before disruption because the original models cannot
currently be proven restartable. GPT-OSS received zero acquisitions, launches or
inference calls. Its performance and usefulness remain unmeasured.**

## Concrete restoration blocker

Both running, healthy original containers have this configured bind mount:

```text
/srv/fast/huggingface -> /root/.cache/huggingface
```

The host source does not exist. Read-only inspection of each live container's
mount table shows `/huggingface//deleted` on `/dev/sda1`. Inside both containers,
`/root/.cache/huggingface/hub` also does not exist. This is a deleted cache mount,
not just an unverified directory name. Healthy resident processes do not establish
that their model artifacts remain available for another process start.

Stopping these containers could leave the rack unable to restore its original
models. Recreating an empty bind directory could also cause model downloads onto
the reserved filesystem. This task neither authorizes restoring that deleted
library nor permits writing new experiment files to `/srv/fast`. No workaround,
cache movement, model download, lease deletion or container recreation was tried.
Before another experiment, the operator must establish an authorized persistent
artifact/mount arrangement for the existing models and prove restartability.

Exact original identities remained unchanged:

| Service | Container ID | Image digest | Original process |
|---|---|---|---|
| primary | `bc08fff979a20cba5cc358161b4ca362215595426bdb9d079bfc352d1c3ff5a3` | `sha256:8bd082c274fae025b7079498fe1da65182ba1d4c2188c0f5a68c1042c38c3695` | init 2980; engine 4313 |
| coder | `ce5f8f57d793f010afc5d2f78db96d1b07d28b73b7ba651ebf0a97ed8ea49251` | `sha256:0a51ea5b4ae2dc5d81890e5173f54203d2a3ae0cfffe51b8fd2afd4391bfd967` | init 2981; engine 3743 |

Both retain their September 12 19:46 UTC start timestamps, configurations, mounts,
images and `unless-stopped` policy. Primary remains `local-primary` / Gemma 4 12B
AWQ; coder remains `local-coder` / NotaMG eqaq-v2. Endpoint request counters also remained unchanged: primary two prior completed
requests, coder zero, with no running/waiting requests. No fresh inference or
restart qualification is claimed for these unchanged processes.

The RackAI campaign supervisor (PID 2705) and media receiver (PID 466343) remained
active. ComfyUI remained inactive, with media admission closed and every retained
session stopped. All five media jobs were terminal. The 305-entry ComfyUI library
metadata manifest (names, modes, sizes, mtimes and symlinks) remained identical;
model contents were not rehashed. The deployed media configuration also matches its protected preflight backup.
No companion code, service or configuration was changed. The production RackAI checkout and administrator configuration were not
switched or edited.

## Verified artifact, hardware and preparation

Existing artifact: `/srv/models/gpt-oss-120b/gpt-oss-120b-MXFP4.gguf`, exactly
63,387,346,208 bytes. Full SHA-256 matched:

```text
582bd40f6886200101f4c4ed9f25f3fe80cc14c86e9e2b37746cd8904a0c622d
```

Full hashing took 55.953 s. The filename, size, LFS digest and retained download
revision match the official [ggml-org artifact revision](https://huggingface.co/ggml-org/gpt-oss-120b-GGUF/tree/238abdd290bb874b90a5da1b4549881b7d05c091).
GGUF metadata identifies GPT-OSS, 36 layers, 128 experts / 4 used, and MXFP4
file type. No second artifact was downloaded.

The candidate, build, Docker storage, test temporaries and evidence were verified
on root NVMe `/dev/nvme0n1p2`; initial available space was approximately 516 GiB.
`/srv/fast` is `/dev/sda1`, had approximately 3 GiB available, and was treated as
unavailable. No new files were placed there.

CPU: Threadripper 1950X, 16 cores / 32 threads. RAM: 61869 MiB total, approximately
50663 MiB available initially; swap 194 MiB used. NVIDIA driver: 595.91.07.

| GPU | UUID | Total / used MiB at preflight and continuity check |
|---|---|---|
| 4080 SUPER | `GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b` | 16376 / 1 |
| 2060 | `GPU-357ef569-8fac-7c7d-ee1c-51677efb174f` | 6144 / 5528 |
| 4060 Ti | `GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c` | 16380 / 14126 |

Topology: 4080-to-other links NODE, 2060-to-4060 link PHB; no NVLink. Both model
endpoints reported zero running and waiting requests. Canonical authority is
`/srv/rack-ai/state/resources`, also used by deployed RackAI media. Its lease
directory contained only `.gitkeep`; `managed.json` was absent before and after.
No qualification receiver or independent physical authority was started.

Source pinned for the isolated build: llama.cpp tag `b10952`, commit
`661643e43079a4ee6faab4c1895291767b67ea8d`. Official source archive SHA-256:
`3eee3bbd2a0910493e306cc582c1755a455d91384c1464099dc7802b37bb173d`.
Build image: CUDA 12.8.1 devel Ubuntu 24.04,
`sha256:520292dbb4f755fd360766059e62956e9379485d9e073bbd2f6e3c20c270ed66`.
The build used CUDA architectures 75 and 89, Release, Ninja, GGML_CUDA enabled,
tests/examples disabled and origin-relative build RPATH. Dependencies were
installed only inside the isolated build container; no driver/CUDA/vLLM/ComfyUI
host dependency was changed.

Compilation was stopped when the restoration blocker was confirmed. The build
container is exited, PID 0, not OOM-killed; exit 137 records the deliberate bounded
Docker stop, not a failed model load. Source and partial build remain at
`/home/tomp/rackai-runtimes/gpt-oss-120b-b10952`. There is **no completed pinned
llama-server executable or validated live profile** from this attempt.

Compilation reached actual CPU Tdie 68 C. The sensors responded to a pause by
falling to 61.375 C within 16 seconds. Build CPU quota was reduced from 8 to 4,
then 2 CPUs. Tctl has an offset and was not treated as actual die temperature.
The proposed live guard uses [AMD's 68 C Tjmax](https://www.amd.com/en/support/downloads/drivers.html/processors/ryzen-threadripper/ryzen-threadripper-1000-series/amd-ryzen-threadripper-1950x.html).
Thermal headroom would need fresh verification before a future window.

## RackAI preparation and scoped correction

Reusable tools in `tools/qualification` contain fixed expected answers, explicit
fresh identities, backend timing assessment, a reload check, protected evidence,
monitoring and managed cancellation/restoration. Their README records the proposed
4K / three-GPU / CPU-expert-offload configuration and all resource/time/abort
limits. **That configuration was never launched; zero of the three permitted
launch configurations were consumed.** The tools are synthetically tested, not
live-qualified.

A preparation regression exposed a real runtime cleanup gap: after an invocation
became Uncertain, explicit release reached `drain_deadline_invocation_uncertain`
and kept the backend and claims indefinitely. The smallest retirement correction
now permits verified owned shutdown after bounded draining. Started callbacks
must still persist their late output or uncertainty before claims clear. Uncertain
answers and identities remain retained; no replay or successful answer is invented.
Foreign GPU evidence, unproven ownership and failed persistence still fence claims.
Competing-priority victim drain and the priority policy were not changed.

Regressions cover uncertain release, receiver restart, bounded cancellation of an
actual delayed Started call, failed cleanup persistence, and a foreign GPU at
uncertain shutdown. The original uncertain-release regression failed before the
fix and passed afterward. A separate tool regression refuses a missing model-cache
bind source before any service command.

## Verification

- Rust: **352 tests passed**, `cargo test --workspace --offline`.
- Runtime: **63 tests plus two subtests passed** in 244.22 s on the final Rust
  correction. The subsequently added storage-cleanup test passed in a focused
  run of all **four cleanup tests** (14.36 s): 64 distinct runtime tests verified
  across those runs, without claiming a 64-test full-suite run.
- Hosting/cleanup focus: **10 passed** before adding the storage-cleanup case,
  including foreign-GPU retention and delayed Started cancellation.
- Benchmark/preflight helpers: **12 tests passed**. They cover expected answers,
  missing timings, short/cache-only generations, truncation, the 10 tok/s boundary,
  exact acquisition reconciliation, unproven cleanup, restoration fencing,
  missing bind sources and actual-die versus control temperature.
- `cargo fmt --all -- --check`, strict runtime clippy with `-D warnings`, Python
  compilation and `git diff --check` passed. Whole-workspace strict clippy was not
  rerun; the previously recorded unrelated warnings are not new qualification
  failures.
- Media: full suite **70 passed, two browser environment failures** in 591.68 s.
  Both browser cases then **passed** with the existing pinned browser/library
  environment (22.70 s). Thus all **72 cases passed across those runs**; a single
  clean 72-case run is not claimed.

Initial verification mistakes were corrected in the test environment: a temporary
root nested under the Rust workspace broke four fixture Cargo manifests; system
Python lacked pytest for two shared-media tests. The existing `.venv` and an
external NVMe temporary root resolved both. Browser tests also required the
already-installed pinned browser and its existing isolated audio library; no
system or application dependencies were installed to make them pass. All failed
logs remain retained. No safety test was removed or weakened.

## Model decision and markers

| Marker | Result |
|---|---|
| `BIG_BRAIN_BOOTED` | NOT_RUN - preflight blocked |
| `BIG_BRAIN_BENCHMARK_COMPLETED` | NOT_RUN |
| `PRACTICAL_SPEED_GATE` | NOT_RUN - no measured decode |
| `BIG_BRAIN_QUALIFIED` | NOT_RUN - remains unavailable to normal clients |
| `ORIGINAL_SERVICES_RESTORED` | NOT_REQUIRED - original operating state preserved without stopping; restartability remains BLOCKED |

Largest populated candidate context tested: **none**. Startup, warm throughput,
TTFT, final-answer latency, quality, model RAM/VRAM and reload reliability are
unavailable. No model-performance conclusion follows from artifact verification,
compilation or synthetic test counts.

Live competing-priority qualification, application integration and production
rollout remain **NOT_RUN**. No merge or cutover occurred.

Raw protected evidence remains outside Git at
`/srv/rack-ai/.worktrees/pr35/evidence/gpt-oss-120b-single-model/`, notably
`artifact-verification.json`, `restoration-blocker.json`, `final-continuity.json`,
`build.log`, and verification logs. Secret-bearing snapshots are under `private/`.

**Does GPT-OSS earn a place on this rack? Undetermined.** Its model test is
blocked by the existing services' deleted cache mount, so there are no speed or
quality measurements on which to accept or reject it. Preserve the running models
until their restart path is repaired and proven under separate authorization.
