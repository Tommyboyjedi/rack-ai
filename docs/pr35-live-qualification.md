# RackAI-only live qualification and rollback plan

The general competing-workload window below remains unauthorized. A separate
2026-09-14 single-model GPT-OSS window was authorized but stopped before disruption
because both original vLLM model-cache mounts were deleted; see
[the single-model results](pr35-gpt-oss-120b-results.md). No candidate was loaded.

The following is the earlier general plan. No disruptive competing-priority window has been authorized. This plan does not enable production preemption
or cut over any application. All implementation evidence uses disposable synthetic clients,
processes and machine-command fixtures. The qualification operator must use the same
canonical reservation authority; no unmanaged llama-server or exclusive large-model mode.

## Read-only inventory

Observed on gpurack, 2026-09-13 22:49 UTC (refresh immediately before a window):

| Resource | UUID | Total / used MiB | Observed consumer |
|---|---|---|---|
| 4060 Ti | GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c | 16380 / 14126 | vllm-primary; compute PID 4313 |
| 2060 | GPU-357ef569-8fac-7c7d-ee1c-51677efb174f | 6144 / 5528 | vllm-coder; compute PID 3743 |
| 4080 SUPER | GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b | 16376 / 1 | no compute process in snapshot |

Visible host memory: 61869 MiB total, 50876 MiB available. Swap: 196 MiB used of 8191.
Kernel: 7.0.0-31-generic. The two existing vLLM containers were healthy and up 27 hours,
with `unless-stopped` restart policy. The RackAI campaign supervisor and PR24 media receiver
were active. Other application services were only listed by the system inventory; their
code, configuration, tests and PRs were not accessed or changed.

The root RackAI checkout was `469dc13`, on `pr32-generic-capability-routing`, with existing
administrator changes to `config/repositories.json` and retained state. PR35 implementation
is in the isolated remote worktree `/srv/rack-ai/.worktrees/pr35`, based on `dca54cd`.
The production checkout was not switched or deployed.

Current primary image: `sha256:8bd082c274fae025b7079498fe1da65182ba1d4c2188c0f5a68c1042c38c3695`;
model `cyankiwi/gemma-4-12B-it-AWQ-INT4`, served alias `local-primary`, advertised context 131072.
Current coder image: `sha256:0a51ea5b4ae2dc5d81890e5173f54203d2a3ae0cfffe51b8fd2afd4391bfd967`;
model `NotaMG/eqaq-v2`, alias `local-coder`, advertised context 16368.
These are model-list/inventory observations, not fresh inference qualification.

Candidate `/srv/models/gpt-oss-120b/gpt-oss-120b-MXFP4.gguf` is 63,387,346,208 bytes.
Read-only SHA-256: `582bd40f6886200101f4c4ed9f25f3fe80cc14c86e9e2b37746cd8904a0c622d`.
No llama-server was found on PATH. No runtime was launched and no model-fit or performance
claim follows from this checksum. `config/runtime/config.example.json` prepares the managed
profile with all three UUIDs and host budgets. Its tentative 16 GPU-layer split leaves CPU offload enabled; neither that split nor its memory fit is qualified. Its executable hash must be pinned after an
isolated supported build is installed; it remains unqualified. No production fun-chat model
has been selected or downloaded.

Post-verification read-only check at 2026-09-13 23:58:20 UTC retained the same
GPU memory and compute PIDs (4313 primary, 3743 coder); the 4080 remained at 1 MiB
with no compute process. Both original containers were still running with unchanged
images and `unless-stopped` policy, started on 2026-09-12 at 19:46:00 UTC. Evidence:
`evidence/pr35/live-safety-after.txt`. This is continuity evidence, not inference qualification.

## Proposed bounded window

1. Review the committed code/contracts and isolated evidence first. Refresh the inventory,
   exact deployed service/binary/config SHAs, GPU processes, leases, active invocations,
   disk/host headroom and restart policies. Do not print credentials. Record the approved
   window length, abort thresholds and the exact prior service/container identities.
2. Require the operator to quiesce clients still using raw endpoints. Do not edit their
   installations or pretend that their later integrations have occurred. Stop new RackAI
   work through its existing controls; drain accepted invocations and retain evidence.
3. Back up the canonical authority and media state/config with permissions and hashes while
   quiescent. Preserve all lease records. Live/unknown legacy leases must be released by
   their owning existing path or cause an explicit migration blocker; never reinterpret them
   as Low, delete them, or adopt a process merely because a port answers.
4. Pin the new runtime executable/build and qualified protocol profiles. Validate private
   ingress, source credentials, complete GPU UUID mapping, CPU/RAM/per-device limits,
   ComfyUI unit limits, pinned media-configuration hash and checkpoint bindings, and old/new schema compatibility. The supplied
   example intentionally leaves normal profiles unqualified and credentials unprovisioned.
   Set the canonical root identically for the receiver, legacy CLI and media.
5. With explicit authorization, stop only the named drained old containers/services whose
   resources are being migrated. Verify exact process/container identity, empty cgroups and
   released GPU allocations. Retain old containers/configurations for rollback. Do not run
   whole-stack Compose down/up, alter drivers, delete model libraries, or modify applications.
6. Provision RackAI-owned qualification principals with the same source priority policies
   and explicitly authorized qualification capability. Run A-F and the reverse scenario
   through normal acquisitions. Assert actual model identity, generation, process/container
   start/stop counts, native media renders, coder continuity, complete-set denial and primary
   restoration. A failed drain/cleanup or foreign process is a stop condition, not a retry
   with weaker checks.
7. Boot the candidate only through an authorized qualification acquisition of `big-brain`.
   Record executable/image/artifact/profile hashes, all UUIDs, real CPU/RAM/VRAM, swap,
   context, startup and restoration. Begin with the bounded 4K context profile. Boot success
   is separate from PR33 performance qualification. Keep the profile unavailable to normal
   sources until its recorded qualification is accepted.
8. PR33 performance qualification remains separate: fixed quality/protocol corpus, roughly
   24K populated input within 32K context, prompt processing, TTFT, sustained decode and
   restoration; greater than 10 generated tokens/s on every representative test. Do not
   silently change to a two-GPU profile, shrink the benchmark, or claim fixture throughput.
9. Reconcile every started/uncertain submission without replay. Release the synthetic
   demands, prove process/GPU cleanup, and verify the agreed prior service state. Retain
   all raw evidence and report failures as well as successes.

## Rollback

Close admission on the candidate receiver; cancel only not-started work and drain started
work under its recorded limits. Unknown outcomes stay uncertain. Use the new authority to
release its owned runtimes and verify every GPU/cgroup before restoring old services. If
cleanup is unproven, stop the rollback procedure and retain recovery ownership rather than
starting a competing old model.

Keep the candidate state/evidence intact. Verify its claims are empty and no managed process
survives before re-enabling the prior binaries/configuration and exact old containers. An old
binary must not be run against active new managed claims that it cannot interpret. Restore
only the explicitly backed-up compatible legacy configuration/state under an approved
migration procedure; do not erase new decisions, lease receipts or uncertain invocations.
Recheck model identities, media native admission/artifacts and the prior restart policy.

Production application rollout remains deferred. Later ATHBA, Music Director and CB adapters
must adopt the stable contract and demonstrate cutover before anyone claims their live work
is governed by this authority. No merge is part of this task.
