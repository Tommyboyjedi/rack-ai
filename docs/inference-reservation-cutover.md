# Managed inference cutover — 2026-09-15

## Deployment record

The user authorized a maintenance window with no current RackAI users. The former
fixed-address inference containers were stopped and retained for rollback; their
restart policies are now `no`, and the live compose services require the explicit
`legacy-rollback` profile. The legacy campaign supervisor is stopped and disabled.
The managed runtime starts a pinned backend only after canonical admission.

This report supersedes the earlier partial-deployment report. The managed API deployment is complete. Final operating state and live results follow.

## Two deployment corrections

1. The managed coder exhausted the existing 256-process limit during startup because
   native support libraries created excessive CPU threads. Derived images set only
   OMP_NUM_THREADS, OPENBLAS_NUM_THREADS, MKL_NUM_THREADS and NUMEXPR_NUM_THREADS to 4.
   Image root filesystems and every other image-config field were verified identical.
   Models, libraries, GPU mapping and PID limits are unchanged; no CPU inference or
   model offload was introduced. The build uses deploy/runtime/Dockerfile.support-threads.
2. An unrelated process disappearing during a Docker `find /proc/.../fd` scan caused
   a false ownership failure. The old scan failed 6 of 9 live checks; the bounded
   replacement passed all 9 with an exact positive listening-socket match. Docker
   identity, activation, PID/start/boot, unique listener and model identity checks
   remain enforced. Missing or mismatched socket evidence still rejects ownership.
   Regression tests cover directory disappearance and absent/incorrect socket proof.

Primary image: sha256:53fb2135569432595a0b307e96e3e84c3b889c2e56ffbed2d6c373126f22ebf3.
Coder image: sha256:3a25e20575abf8e8858a0413d4164192570c1075caf5dd95c935a6425728c0b4.
Runtime binary SHA256: 0f8f8793b0269d5931d5e8893d2d612cb1ae4e6f3e4d3d27bbf6d76249785e59.
Release: /srv/rack-ai/deployments/idle-runtime/releases/inference-cutover-20260915T123837Z-socket-probe.

## Validation and evidence

- 364 Rust workspace tests passed.
- 75 runtime HTTP tests plus 2 subtests passed (298.46 seconds).
- Strict runtime Clippy, formatting and patch whitespace checks passed.
- Existing media/browser evidence: 80 tests passed before this inference-only correction.
- Independent read-only semantic reviews accepted both deployment corrections.
- Both managed models completed Chat Completions and Responses requests through RackAI.
- NUC TCP probes while both models were Ready: 8017/8018/8190 unreachable; 8445 reachable.

Evidence: /srv/rack-ai/.worktrees/pr35/evidence/inference-cutover-20260915T123837Z.
Attempt 3 contains final protocol requests/results and socket-fix review. Earlier failed
attempts are retained. Two oversized review requests were rejected before execution by
inference_limits; the accepted compact review fits the reservation's conservative byte
budget. A proof script inspected immediately after a receiver restart before its listener
opened; the read-only observation resumed without replaying any GPU operation.

## Scope and compatibility limits

Only RackAI code and its gpurack deployment were changed. No CB, ATHBA or NUC code or
client credentials were installed. Server-side source provisioning does not install a
client adapter. External applications must acquire a logical resource and use its current
scoped gateway. Existing fixed-address RackAI/JCode launchers are not qualified for this
new contract; the campaign supervisor remains disabled. No end-to-end ATHBA/JCode
qualification is claimed. Their server policy ceiling remains Low/Medium.

The image profile remains the previously verified Juggernaut Ragnarok checkpoint.
This deployment does not claim a Krea2 managed workflow. Existing manual library selection
is preserved. First startup of the pinned inference profiles can take several minutes;
requests report Preparing while ownership and readiness are established.

## Rollback

Stop new admissions, explicitly release active grants, and prove owned processes stopped
and claims empty. Preserve the complete current authority and evidence; never restore an
old state snapshot over new invocation outcomes. Stop the runtime before restoring a
matched saved binary/config/unit. Both legacy loopback containers remain available, but
must never run concurrently with managed replacements. Restoring legacy operation requires
keeping private networking and deliberately restoring its caller/supervisor mode; it would
withdraw managed inference availability. Do not start the retained all-interface originals.
The media receiver/gate rollback is documented in the earlier deployment report.

## Live idle and normal-admission results

With a temporary 120-second policy, primary inference completed at Unix time 1789478271
and a separate real 512x512 image job completed at 1789478324. Primary expired with
idle_timeout while image remained Ready; image later expired independently with the
same terminal reason. Cleanup left canonical claims empty. Runtime policy was restored
to 1800 before normal acquisitions. This accelerates the lifecycle proof; it is not a
30-minute wall-clock soak. Deterministic tests cover active work and execution/reaper races.

Ordinary Paramount primary/image requests were accepted through the tailnet HTTPS API.
A conflicting Medium request was denied, and a High request from the source capped at
Medium received source_policy_denied. Both normal grants reached Ready. Primary used a
fresh reservation/generation and served inference. Exact idempotent replay returned the
same response without refreshing last_activity_at.

## Final operating state — 13:26:49 UTC

- Runtime and media receivers are active; all four logical profiles are qualified.
- Runtime and manual idle policies are 1800 seconds.
- Normal explicit releases completed; manual Start/Open/Finish passed afterward.
- All GPU processes stopped and canonical claims/legacy leases are empty.
- Temporary qualification source removed; operator/director/CB policies preserved.
- ATHBA server credential is staged mode 0600, with Low/Medium access only. No client
  repository or client credential store was changed.
- Gateway rejects unauthenticated requests with 401. Final principal removal/restart
  preserved all invocation outcomes.
- Legacy campaign supervisor is disabled; fixed-address launchers require a managed
  gateway adapter before use. This compatibility work and Krea2 workflow qualification
  are not claimed by this deployment.
- No commit, push or merge. No CB, ATHBA or NUC code/config edits.

Exact changed source paths and hashes are in source-manifest.json alongside the evidence.
