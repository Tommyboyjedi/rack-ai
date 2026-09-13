# Normal ComfyUI Restart qualification — 2026-09-13

The acceptance defect is implemented and deployed in isolated Rack receiver release `872c3e8624e1387208b0cdae6c2d4991c39bd642`. The subsequent qualification commit changes documentation only. PR24 remains open; Music Director, the live Rack checkout, model contents, workflows, ComfyUI source/unit and receiver configuration were not changed.

The normal Manager action is mediated as one bounded Rack-controlled stop/start, preserving the activation/session/lease. See [operations and recovery](pr24-media-operations.md#normal-comfyui-restart) for the typed phase model, process verification, Finish cancellation and strict managed-mode boundary.

## Code and fixture qualification

- `cargo fmt --check`, `git diff --check` and committed release build passed.
- `cargo test --workspace --offline`: **341 passed**.
- Full `tests/media` run: **60 passed, 1 failed**. The failed new CSRF test omitted the Origin required by existing login authentication. Only that test setup was corrected; the focused rerun passed.
- Complete final `tests/media/test_restart.py` rerun: **17 passed in 265.03 seconds**. The 44 existing media fixtures passed in the full run above.
- The restart cases cover changed real disposable fixture PIDs/InvocationIDs, repeated restart, unchanged lease bytes/session, duplicate intent, temporary native 503, foreign GPU/runtime/unit/gate rejection, bounded timeout without resubmission, Finish at four phases, cancellation of a pending systemd job, receiver recovery of durable intent, managed-mode rejection, unannounced replacement and authentication/CSRF.

These fixtures use real HTTP and the production gate with isolated fake machine commands; they do not simulate a live model qualification by claiming health-only success. Finish-during-restart, timeout, foreign replacement and managed regression acceptance markers are fixture results. No destructive foreign-process or timeout injection was performed on the live GPU.

## Live browser acceptance

The final sandboxed Playwright run began `2026-09-13T03:00:02Z` on gpurack and completed through the private launcher. The test browser used software rendering (`--disable-gpu --use-angle=swiftshader`). The actual sequence was Start → Ready → Open ComfyUI → Extensions → Manager Restart → Confirm → automatic Ready → native Ragnarok Run → a second Manager Restart/Confirm → automatic Ready → Finish → Stopped. There were no backend/receiver repair commands during that sequence and no page reload was needed.

- Same session: `7fe0389e-46ab-449c-b3f1-5b3676e868da`.
- Same activation: `3961dd54-996c-4c1e-b16c-c853914fb0c5`.
- Same lease owner: `media:3961dd54-996c-4c1e-b16c-c853914fb0c5`.
- Same lease generation: `42a37090047580544afe7aab2c32ebfa`.
- Lease-file SHA-256 remained `e6804a2bcc6293b0e9c42a268cda6a7f4240d1c33f3f2e1c778403625d530abd` throughout sampled transitions.
- Both normal Restart POSTs returned HTTP 202; both transitions observed draining/stopping/start_pending/starting before Ready.
- All three generations used the expected unit cgroup `/user.slice/user-1000.slice/user@1000.service/app.slice/rack-ai-comfyui-pr24.service`.
- Native WebSockets reconnected, including render completion traffic after Restart 1 and a connected socket after Restart 2.

| Stage | Generation | PID | InvocationID |
| --- | --- | --- | --- |
| Initial | 1 | 209245 | `5915f191feb44318bcc95e739b6a5989` |
| Restart 1 | 2 | 209763 | `227ab8f1c3234c09ac928be776afb40c` |
| Restart 2 | 3 | 210675 | `4ecd229ea1cd4d47a4ae43aa0ea6c70a` |

Ragnarok prompt `83e7b253-0303-4fc1-bd13-112f073d73ac` completed successfully at output node 9. The preserved qualified workflow file was byte-identical to `config/media/examples/native-ragnarok.json`. Result: one valid PNG, 512×512, 453003 bytes, at `/srv/comfyui/output/native-pr24-proof/image_00004_.png`; SHA-256 `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80`. This is image execution evidence, not qualification of other model workflows.

Finish proved `ActiveState=inactive`, `MainPID=0`, empty InvocationID/Job/ControlGroup, no process of any reported NVIDIA type on the physical 4080 UUID, and no `gpu-4080-super` lease file. Protected 2060/4060 Ti container identities, start times, GPU bindings, compute PIDs, memory usage and health were unchanged. Campaign supervisor, live Rack HEAD/status and administrator configuration hash were unchanged. The media card's memory fell from 258 MiB at the initial pre-task snapshot to 1 MiB after Finish.

ComfyUI remains at `d43a5fa20c8547ff42d13232f589a06536c42b97` in `/srv/comfyui`, with Manager 4.2.2 and the existing legacy UI flag, GGUF and Rack gate. The receiver config and ComfyUI user unit match their pre-task copies byte-for-byte.

## Acceptance markers

| Marker | Result | Evidence class |
| --- | --- | --- |
| RESTART_LIFECYCLE_IMPLEMENTED | YES | deployed code |
| RESTART_STATE_MODEL | Restarting + durable phase intent + verified backend generations | code/fixture/live |
| SAME_SESSION_PRESERVED | PASS | fixture/live |
| SAME_GPU_LEASE_PRESERVED | PASS | fixture/live |
| NORMAL_RESTART_1 | PASS | live |
| NORMAL_RESTART_2 | PASS | live |
| FINISH_DURING_RESTART | PASS | fixture |
| FOREIGN_REPLACEMENT_REJECTED | PASS | fixture |
| RESTART_TIMEOUT_FAILS_CLOSED | PASS | fixture |
| MANAGED_MODE_REGRESSION | PASS | fixture |
| LIVE_IMAGE_AFTER_RESTART | PASS | live |
| 4080_RELEASE_AFTER_FINISH | PASS | live |
| PROTECTED_SERVICES_UNCHANGED | PASS | live comparison |
| NORMAL_COMFYUI_RESTART_REQUIRES_SSH | NO | live |

## Retained evidence and problems encountered

Private evidence is at `/home/tomp/pr24-restart-20260913/evidence`: deployment manifest/binary hashes; Rust, full-media and focused/final restart logs; `live-restart-proof.json`; workflow/history; Manager confirmation/Ready/image/Finish screenshots; final NVIDIA XML; and protected before/after/comparison JSON. The browser harness is `/home/tomp/pr24-restart-20260913/live-restart-proof.py`. Secrets and control state remain outside ComfyUI user data.

Initial browser attempts are retained separately. They required correcting the current Extensions/Confirm selectors. One setup attempt quarantined on a transient foreign GPU process; its process identity was not captured, and subsequent inventory contained only owned ComfyUI. Switching the test browser to software rendering avoided recurrence. The ordinary launcher Finish cleared that quarantined session after ownership verified. A later harness attempt stalled collecting Manager's unused response body after Rack had already reached Ready; only that test process was terminated. The final harness records the actual HTTP 202 and durable phase observations, and completed both restarts and image/Finish without service intervention. None of these preliminary attempts is presented as the final acceptance run.

The pre-existing stuck session also had a durable pending Finish. On initial receiver deployment, the new exact-identity recovery path honored that request and reached Stopped; no lease/state deletion or manual ComfyUI stop was used. No remaining blocker is known.

## Use and rollback

Bookmark [Rack launcher](https://gpurack.tailc214fc.ts.net:8443/) on the authorized tailnet. Sign in using the existing operator credential. Start → Open ComfyUI; use Extensions → Restart → Confirm normally. The launcher shows Restarting and returns to Ready automatically. Finish remains the cleanup action.

The previous isolated receiver release is retained at `/home/tomp/pr24-pr33-20260912/install/candidate/releases/rack-0b50d947909644055bbbae3ec9dbfbe610f17a15`. Unit/config copies and previous-release pointer are under `/home/tomp/pr24-restart-20260913/rollback`. To roll back, first Finish and prove Stopped with no lease, unit/cgroup process or 4080 allocation. Only then atomically repoint the candidate `current` symlink to that retained release and restart `rack-ai-media-pr24.service`. The old release does not support normal Restart, and must not be selected while state is Restarting. Preserve state, sessions, artifacts and release receipts. No ComfyUI source/unit rollback, Tailscale change or other service restart is needed.
