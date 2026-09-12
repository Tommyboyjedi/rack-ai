# PR24 v2 qualification — 2026-09-13

The complete dedicated-media service, managed image execution, authenticated remote API, native launcher and Music Director image import are implemented and qualified on isolated gpurack candidates. Neither PR is merged. The companion is [Music Director PR33](https://github.com/Tommyboyjedi/musicvideo-director/pull/33), not Rack AI's heavyweight-inference PR33.

## Qualification state

- CODE_COMPLETE: PASS for PR24 v2 and the specific Music Director PR33 companion.
- FIXTURE_E2E_PASSED: PASS, actual Rust HTTP receiver plus sandboxed browser and disposable Director database/media.
- RACK_LIVE_QUALIFIED: PASS, native browser render from a stopped backend.
- DIRECTOR_LIVE_QUALIFIED: PASS, disposable rack candidate Generate-to-import from a stopped backend, replay, idle release and repeat activation.
- Existing Music Director client deployment: NOT_DEPLOYED. Its installation/database/media were not modified.

Deployed Rack code: `0b50d947909644055bbbae3ec9dbfbe610f17a15`. Deployed Director code: `1e7fa41f0ad1424b10c82a84dec1e7bc5be188ed`. Subsequent qualification-document commits do not change those deployed binaries. ComfyUI revision: `9113c08c2e14f1ca6c0ccab64920777fd01e1bb9`.

## Final live evidence

The final native proof began 2026-09-12T23:10:47Z: authenticated launcher Start reached Ready with a verified systemd invocation, Open loaded the normal ComfyUI frontend, a native Run dispatched prompt `7ba9fafe-2cad-4679-9917-247a4a55b2a3`, and the WebSocket delivered progress and execution_success. The resulting PNG is 512x512, 452658 bytes, SHA-256 `b9e409d6d95eddf465c8257f7312a333e46a5bf626a8d05b4fa3028390adb920`. Finish reached Stopped, MainPID=0, with no media reservation or media GPU allocation.

The final Director proof began 2026-09-12T23:15:51Z: Test connection left ComfyUI stopped; the actual Generate button admitted one durable submission; the browser closed; the separately supervised worker started ComfyUI and imported the exact verified PNG into disposable project/segment 2. Submission `e1f3741e-0d8f-4731-8517-d2835b0dd3bc`, Rack job `8746b630-e1c9-43d4-a7b0-95a7e810d2ac`, Comfy prompt `7c0af031-8da0-49d2-a7b8-ef64dbdd528b`. The imported image is 512x512, 418220 bytes, SHA-256 `8cfa1ae6282f662b9ca0f2d6027b84b0a465d59e5cdc0aeb6c276226edcfe213`. Replaying the original browser form and reconciling again retained one submission, one backend history prompt and one attached asset. A reopened page displayed that image.

Managed idle retention then ended with Stopped and no media lease. A fresh launcher Start reached Ready with a different activation and nonempty systemd invocation; Finish returned to Stopped. The final complete NVIDIA process inventory contained no process on the media GPU.

Protected before/after verification passed: both existing inference container IDs, image IDs, start times, PIDs, physical bindings, compute PIDs and health responses remained unchanged, as did the campaign supervisor invocation/PID, live Rack HEAD and administrator config hash. All preexisting live working-tree changes remained. The only added live status entries were the authorized canonical authority lock and release-receipt directory; no claim of byte-identical live Git status is made.

Evidence is retained on gpurack under `/home/tomp/pr24-pr33-20260912/evidence/`: `deployment.json`, `native-live-proof.json`, `native-proof-history.json`, `director-live-proof.json`, browser screenshots, `protected-before-live.json`, `protected-after-final.json`, and `protected-verification.json`. Initial experiments are retained under `initial-live/`; their readiness/repeat assertions were superseded after final review fixed the launcher's hidden-link CSS and strengthened the browser readiness assertion. Only the final proofs above qualify the deployed revisions.

## Code and fixture results

| Check | Result | Retained evidence |
| --- | --- | --- |
| Full offline Rust workspace suite | 338 passed, zero failed | `final-code-tests.log` |
| Rust formatting and scoped diff whitespace | PASS | final workspace checks |
| Full Rack media HTTP/gate/recovery/browser suite | 44 passed | `media-final-fixtures-all.log` |
| Final launcher readiness correction | 1 passed, 14 deselected | `final-launcher-test.log` |
| Director actual HTTP/browser integration, independent process | 6 passed | `final-director-integration.log` |
| Director system checks and additive migration consistency | PASS | `director-system-check.log`, `director-migrations-final.log` |

The full 44-test fixture run includes the final complete GPU process-inventory implementation. The later launcher-only CSS/assertion correction was verified by its focused browser test and both complete final live journeys. Fake machine commands, temporary resource roots and a sandboxed browser isolate destructive/failure cases. No legacy live lease-deleting smoke was run.

The broader companion suite is not fully green: a preexisting collection error blocks an unfiltered run; excluding that file gives 35 failures also reproduced on its unchanged original branch. A combined run additionally exposed two new-test setup errors after an existing shared database-settings mutation; the independent six-test integration run passes. Exact private repository nodes and baseline comparison are recorded in the companion qualification document and retained local logs. These findings do not qualify unrelated legacy features.

Self-review corrections included native frontend burst admission, durable release/startup reconciliation, queue persistence ordering, complete C/G/C+G GPU process inventory, and launcher readiness visibility. The final tests and live proof follow those corrections.

## Deployed access and scope

Bookmark [Rack AI launcher](https://gpurack.tailc214fc.ts.net:8443/). Connect through the authorized tailnet. As `tomp`, retrieve the operator credential from the mode-0600 file `/home/tomp/pr24-pr33-20260912/install/candidate/secrets/operator` using the authorized SSH session, then paste it into the launcher login. The credential itself must not appear in a URL or Git. Start, wait for Ready, then Open ComfyUI. Finish drains accepted work and releases the media GPU.

The authenticated receiver, disposable Director web and worker units remain enabled/running. ComfyUI is inactive until an admitted session/job starts it. Raw ComfyUI remains loopback-only. The normal native UI is served separately at `https://gpurack.tailc214fc.ts.net:8444/` through the authenticated proxy. The existing CLI/runtime on `/srv/rack-ai` was not replaced.

Code, environments, state and output are on NVMe. The 12 explicitly selected model/dependency files are on `/srv/fast/comfyui-models-pr24`; all destination SHA-256 values matched the USB sources, retained in `model-copy.jsonl`. Ragnarok is the qualified managed image checkpoint. Krea2, LTX2.3 and CyberRealisticPony were copied for compatible native workflows; their generation and video automation are not live-qualified by this proof.

No remaining access, model or permission blocker prevents the qualified candidate image journeys. Existing-client activation remains a separate operator installation step; no production Director database was migrated. No paid inference, additional LLM calls, global driver changes or protected-service restarts were used.

## Stop and rollback

Use Finish and wait for Stopped, or reconcile/cancel managed jobs and wait for idle release. Confirm no media lease or owned GPU process, then disable only the candidate units:

```sh
systemctl --user disable --now music-director-rack-pr33-worker.service music-director-rack-pr33-web.service
systemctl --user disable --now rack-ai-media-pr24.service
sudo tailscale serve --https=8443 off
sudo tailscale serve --https=8444 off
```

Preserve candidate state/database/media, canonical release receipts and evidence. Do not delete a lease or stop an unknown process. On uncertainty follow the identity-based recovery steps in [media operations](pr24-media-operations.md). Code rollback uses normal forward commits or a previously verified isolated release after a clean stop; never reset the live checkout, force-push, reverse additive migrations or run whole-stack Compose.
