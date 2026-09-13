# Permanent ComfyUI model library — 2026-09-13

The canonical library is **`/srv/fast/comfyui-models`** following the operator's USB recopy. The former PR24 subset directory is absent and is no longer needed by the live deployment.

This focused change starts from merged `main`, commit `5b7b5068e681651c39d44bf6f621320ee510532f`. It updates model-path configuration and documentation only. The merged PR24 branch and live `/srv/rack-ai` checkout are untouched.

## Deployment

The ComfyUI user unit already loads **`/srv/rack-ai-media/extra-model-paths.yaml`** through `--extra-model-paths-config`. Neither `/srv/comfyui/extra_model_paths.yaml` nor a source-root override is active. The checked-in `config/media/extra-model-paths.yaml` is byte-identical to the deployed file.

Both `diffusion_models` and `unet` are searched for diffusion weights; both `text_encoders` and `clip` are searched for text encoders. Standard categories present in the copied library and IPAdapter are explicitly mapped. The other live edit changes only `profile.checkpoint` in `/srv/rack-ai-media/config.json` to `/srv/fast/comfyui-models/checkpoints/Juggernaut_Ragnarok.safetensors`. Its existing SHA-256 remains `dd08fa32f98d05a2443ca1419e46df1575a0811f6e3b246d9dd47ff20f5eb66a`.

No models were copied, deleted or downloaded. The 228-file library inventory, sizes, modification times and symlinks match before/after; the known checkpoint was separately hash-verified. `checkpoints/miracleinNSFWGeneration_30Bf16Fp8.safetensors` remains intentionally absent for SSD capacity and is not required.

ComfyUI stays at `/srv/comfyui`, core `d43a5fa20c8547ff42d13232f589a06536c42b97`. The existing receiver release, ComfyUI venv, custom nodes, both Manager flags, systemd units, resource authority, personal password and machine credentials remain unchanged. No receiver rebuild was needed.

## Qualification

**346 Rust workspace tests passed**, plus formatting, scoped diff checking and systemd unit validation. The deployed ComfyUI folder-path loader parsed the new YAML and resolved existing models; the known native Ragnarok workflow and managed profile still resolve the same checkpoint. Independent read-only local-primary review accepted the configuration after deterministic checks. The full media/Director fixture suites were not repeated for this configuration-only change.

Live authenticated ComfyUI discovery returned these non-placeholder model filenames before and after normal Manager restart:

| Category | Count |
| --- | ---: |
| `checkpoints` | 6 |
| `diffusion_models` | 1 |
| `text_encoders` | 12 |
| `vae` | 17 |
| `loras` | 98 |
| `controlnet` | 4 |
| `clip_vision` | 2 |
| `ipadapter` | 3 |
| `upscale_models` | 4 |
| `unet_gguf` | 2 |

GGUF's dedicated loader correctly lists both existing LTX and Flux GGUF files under `unet_gguf`; core `diffusion_models` lists its supported core formats. Discovery is not execution qualification of every listed model and does not install missing workflow nodes. The single live generation used the existing Ragnarok workflow unchanged.

The rack-side browser used the existing operator API credential in private request headers, with outgoing browser requests restricted to the two existing HTTPS origins. Personal password setup and browser-authentication behavior were not changed.

| Live check | Result |
| --- | --- |
| Launcher Start → Ready → Open ComfyUI | **PASS** |
| All nine requested model categories populated | **PASS** |
| Classic Manager, GGUF and rack_ai_gate load | **PASS** |
| Known-good 512x512 image generation | **PASS** |
| Normal Extensions → Restart → Confirm | **PASS**, HTTP 202 |
| Supervised Restarting phases, same session and exact lease bytes | **PASS** |
| Verified backend generation 1 → 2; returns to Ready | **PASS** |
| Model discovery after restart | **PASS** |
| Finish → Stopped; MainPID=0, empty cgroup, no 4080 process or lease | **PASS** |
| Protected 4060 Ti/2060 processes, containers, endpoint health and campaign service | **UNCHANGED** |

**NORMAL_COMFYUI_RESTART_REQUIRES_SSH=NO.** No manual recovery or page reload occurred during the qualified normal Manager restart.

The one generated image is `/srv/comfyui/output/native-pr24-proof/image_00011_.png`, 453003 bytes, 512x512. Prompt `a07c5788-e9bb-427d-9156-9b3798ee4647`; PNG SHA-256 `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80`, matching the earlier deterministic qualified output.

Session `f34d13a0-cb84-4b3e-ae29-28036db261af` and lease generation `e0baaea1ab8408d703fc02181dd82fdc` survived restart. ComfyUI PID changed from 467898 to 468558, with a verified new systemd invocation.

## Inspection findings and retained evidence

At task entry, an earlier session was already `recovery_required` with “backend invocation changed,” although its backend was inactive and the media GPU had no process. Rack AI accepted Finish. Receiver startup initially failed with “approved checkpoint missing” because the retired library was absent. After applying the verified new paths while the backend was stopped, receiver startup reconciliation verified/released the recorded reservation and returned to Stopped. No session or lease files were manually edited.

An initial discovery probe incorrectly expected GGUF filenames in the core diffusion listing. Inspection confirmed their presence in the dedicated GGUF loader. That probe was corrected without any product change; its session was Finished normally before the successful proof, and it dispatched no image.

Final scans found **zero retired-root references in current tracked files, active deployment configuration, service/script definitions, ComfyUI source or running process arguments**. The actual retired directory remains absent throughout the successful live proof. Historical references remain in preserved private evidence, rollback snapshots, Manager's completed batch history and the untouched prior PR worktree. These are inactive records, not canonical configuration or required runtime paths.

Private evidence is retained under `/srv/rack-ai-media/evidence/model-library-20260913`, including before/after inventories, protected-service snapshots, configuration backups, recovery/probe records, review, workflow history, browser screenshots and final audit. No secret values or private model filename inventory are published.

The saved former configuration is evidence, not a runnable rollback while its old library is absent. Any configuration rollback must preserve the current canonical root and verified checkpoint path: Finish through Rack AI, verify no backend process/lease, update only the intended model mappings, then reload only the receiver configuration. Start ComfyUI through Rack AI again. Never restore private state or remove resource records manually.

The deployment is left **Stopped**, ready for the next admitted session. There are no unresolved blockers; the new PR must remain unmerged.
