# PR35 RackAI implementation and verification handoff

The reviewed-base results below are historical. The [correctness follow-up](pr35-correctness-handoff.md) records the new cancellation, deadline, identity, capacity and positive workspace corrections and their separate verification.

Scope: RackAI only, branch `design/priority-runtime-reservations`. No companion
application code/configuration/tests/PRs were changed. No merge, deployment, model
inference against production, or disruptive service window was performed.

## Implemented responsibilities

- `rack_ai_runtime` publishes authenticated `rack-ai/runtime/v1` discovery,
  acquisition/inspection/renewal/release, bounded inference, results/reconciliation
  and cancellation. Profiles freeze model/version, launch arguments, artifact and
  executable hashes, capabilities/context, backend, complete GPU requirements,
  host memory/CPU limits, protocols and lifecycle deadlines.
- Managed claims extend the existing resource authority under its `authority.lock`.
  Whole-set admission uses canonical Low < Medium < High < Paramount, incumbent
  wins ties. Fences and durable transitions precede effects; process loads/stops
  run outside the global lock. Legacy ownership needs explicit migration.
- Independent primary/coder reservations support bounded drain, proven owned
  unload, held demand, fresh-generation restoration, and unchanged unrelated work.
  A multi-GPU activation stops as a whole. Unknown ownership/cleanup remains
  recovery-required and retains claims. Expired/cancelled demand cannot resurrect.
- Acquisition/submission identities preserve payloads and durable decisions.
  Started intent precedes transport; receiver interruption becomes uncertain and
  never causes automatic inference/tool replay. Reconcile reports durable evidence.
- vLLM Docker and systemd hosting coexist with managed llama_cpp. ComfyUI reuses
  the existing lifecycle, native gate, private access, restart, jobs/artifacts and
  permanent model library. Shared activation replaces dedicated placement only
  with verified UUID/grant checks. Its configuration hash and unit limits are pinned.
- Raw JCode/review/recovery routes are fenced before dispatch. Generation-scoped
  capability URLs preserve qualified Chat/Responses payloads and optional buffered
  SSE. Old capabilities cannot reach replacement models on reused endpoints.
  Repository changes remain inside the existing bounded workspace executor.
- Administrator examples configure ATHBA Low/Medium (big-brain Medium), interactive
  operator/CB Paramount, and new Director managed images Paramount. Existing
  accepted media requests retain their recorded priority. Examples remain
  unqualified and unprovisioned; no production CB model was selected/downloaded.

## Stable interface and repeatable clients

- [Public contract and later client guide](runtime-public-contract.md)
- [Request/response/error schemas and policy/profile examples](../config/runtime/)
- [Paramount managed image fixture](../config/media/fixtures/job-request-paramount.json)
- [Isolated commands and dependencies](../tests/runtime/README.md)
- [A-F plus reverse scenario runner](../tests/runtime/scenario.py)
- [Live inventory, proposed qualification window and rollback](pr35-live-qualification.md)

Clients persist acquisition/submission identities, acquire independent demands,
inspect readiness, use the current generation/profile binding, reconcile uncertain
transport and explicitly renew/release. RackAI does not own client dependencies or
software-development semantics. ATHBA, Music Director and CB integrations/cutover
are deferred until separate review and acceptance.

## Verification

Evidence root: `/srv/rack-ai/.worktrees/pr35/evidence/pr35` (retained, untracked).
Synthetic hosting fixtures use real CPU processes and HTTP; machine-command fixtures
stand in for GPU/systemd/Docker operations. These results are not live GPU qualification.

| Check | Result | Retained evidence |
|---|---|---|
| Workspace tests, including JCode/workspace safety | 349 passed | `workspace-release.log` |
| Runtime HTTP, policy, hosting, race/recovery, schema tests | 30 passed | `runtime-verified.log` |
| A-F and reverse, including production media/native gate | PASS | `scenario-verified.log`, `scenario-verified/` |
| Full media/browser/lifecycle/model-library regression | 72 passed (607.03s) | `media-confirmed.log` (earlier full run: 72 passed in `media-final.log`) |
| Bounded workspace prepare smoke | PASS | `change-smoke.log` |
| Rust format, patch whitespace, runtime lint with warnings denied | PASS | `runtime-lint-verified.log`; final command checks |
| Strict whole-workspace lint | BLOCKED by unchanged baseline warnings | `clippy.log`, `clippy-all.jsonl` |
| Application-owned type-size review | No affected production type over 100 counted executable/non-comment lines | `audit-size.py`, `size-review.txt` |

The mandatory scenario records three completed native media renders. Final model
start/stop/dispatch counts are primary 3/2/3, coder 2/1/3, fun-chat 1/1/1 and
big-brain 1/1/1. Coder has zero stops throughout A-F; its one stop occurs only in
the reverse three-GPU takeover. ComfyUI records two starts and one stop. Whole-set
High denial and Paramount tie denial leave incumbents unchanged. Both original
Low demands restore after the reverse scenario frees their GPUs.

Robustness includes eight simultaneous equal requests/one winner, opposite resource
orders/aliases, source spoofing/ceilings, denial and payload replay, tag/policy changes,
hold/start/drain cancellation and expiry, receiver restart/simulated reboot, stale
capabilities, uncertain once-only inference, failed startup with restoration, failed
persistence, foreign GPU ownership, insufficient host/device memory, uncertain release,
already-exited container cleanup, shared image ownership/replay, normal media restart and late readiness callbacks racing release.
Three Rust child-process tests additionally prove no raw backend connection/JCode start
occurs when the shared authority owns an endpoint.

Intermediate failures remain retained: missing browser ALSA/websocket dependencies
were resolved inside the isolated test environment without a system install or sandbox
relaxation; an incorrect new test field was corrected; a media fixture startup timeout
occurred before reservation logic and lacked sufficient retained diagnostics to establish
its cause. The fixture now holds all three port selections together and reports startup
process/log failures. Its final full rerun is recorded above. No production validation
was weakened and no test success is inferred from an HTTP acceptance.

Strict workspace lint fails first on 37 application-library warnings (39 with test
warnings); those application files are unchanged from `dca54cd`. Ordinary whole-workspace
clippy completes with warnings. New runtime lint passes with warnings denied. Existing
JCode `OpenOptions` lint predates this change. Unrelated lint cleanup was not folded in.

## Safety, limitations and qualification status

All code work occurred over SSH in the remote RackAI worktree. The original checkout
remains on `469dc13`; administrator `config/repositories.json`, historical state and
lease receipts remain unstaged. The initial regression created four inert endpoint
lock files under the existing authority root before subsequent tests were redirected
to disposable roots. No lease records or production services were replaced or removed.

The final 23:58 UTC read-only check retained the original GPU PIDs/container start times and idle 4080 (`live-safety-after.txt`). The GGUF checksum and rack inventory are real read-only observations, separately
recorded in the live plan. The proposed 16 GPU-layer/three-device CPU-offload profile,
4K context, memory budgets, runtime build and model quality remain unqualified.
No llama-server was booted. PR33 performance qualification remains a separate gate.

Legacy raw callers and their existing reservations require an approved quiescent
migration. No foreign process is adopted by port alone. Source credentials, private
ingress, pinned binaries/media config and actual hardware fit must be provisioned and
qualified before use. Streaming is buffered, not first-token delivery. The managed
evidence document retains its 32 MiB hard bound. The correctness follow-up adds conservative admission headroom for accepted results and cleanup, with precise capacity refusal. Sustained use follows the published quiescent retention procedure; there is no silent deletion or empty-authority reset.
Unknown in-flight outcomes require evidence/operator reconciliation, not an automatic retry.

| Marker | Status |
|---|---|
| `RACKAI_CODE_COMPLETE` | PASS |
| `PRIORITY_SCENARIO_PASSED` | PASS (isolated production logic and synthetic processes) |
| `RECOVERY_AND_RACE_TESTS_PASSED` | PASS (30 runtime tests plus workspace/media coverage above) |
| `RACKAI_LIVE_QUALIFIED` | NOT_RUN: disruptive qualification window not authorized |
| `BIG_BRAIN_BOOTED` | NOT_RUN: managed runtime not installed/booted in an authorized window |
| `BIG_BRAIN_QUALIFIED` | NOT_RUN: no real fit, quality or throughput measurements; PR33 separate |
| `PRODUCTION_ROLLOUT_STATUS` | NOT_RUN: no deployment; companion integrations and cutover deferred |

## Retained evidence digests

| File | SHA-256 |
|---|---|
| `workspace-release.log` | `7b9e17ed1c55c8b5c0ba1e7f1b81d3d03b0ff3db44e56ce59041c496f2065f24` |
| `runtime-verified.log` | `117d9c3455e90307bf4cb70be238fa6060f0dc054a1c25d88406efd9cb56b4bc` |
| `scenario-verified.log` | `c0fdc87a799fdbbd09a8e31e34fb459edf8dc5aa016b6aa1d152892fe78bdf7f` |
| `media-confirmed.log` | `f119e8e6d12e6f4b145265c09f76620761d298eda0a136b7a3f5b737f5fd044a` |
| `runtime-lint-verified.log` | `d2ad8abfe2660ad9d188d98939b50ecb3d20773d1290ace299ab9f5748cce6e8` |
