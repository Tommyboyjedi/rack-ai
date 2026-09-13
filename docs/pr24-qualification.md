# PR24 permanent deployment qualification — 2026-09-13

The permanent gpurack deployment passed the complete interactive and managed image journeys. PR24 and companion Music Director PR33 remain unmerged. This record supersedes earlier deployment paths; [the restart qualification](pr24-restart-qualification.md) retains the original acceptance evidence.

The restart implementation accepted at `a0212bca6be8c45e6812a1f55b86754510234074` is unchanged. The deployed receiver binary still comes from `872c3e8624e1387208b0cdae6c2d4991c39bd642` (binary SHA-256 `51beeb92a2644e776971812c45897fad8c78d4db55998f772a247ef2a3df28c4`). This finalization changes deployment assets, documentation and test-browser software rendering only. Final branch/publication SHAs are recorded in the PR and private `publication.json`; they do not imply a different deployed application binary.

## Permanent deployment

| Item | Verified value |
| --- | --- |
| Launcher/bookmark | https://gpurack.tailc214fc.ts.net/ |
| Native UI | https://gpurack.tailc214fc.ts.net:8444/ |
| Media root | `/srv/rack-ai-media` |
| ComfyUI root | `/srv/comfyui` |
| ComfyUI revision | `d43a5fa20c8547ff42d13232f589a06536c42b97` |
| ComfyUI-GGUF revision | `6ea2651e7df66d7585f6ffee804b20e92fb38b8a` |
| Manager | Official `manager_requirements.txt`, installed 4.2.2, classic UI PASS |
| Shared resource authority | `/srv/rack-ai/state/resources`, unchanged |
| Model store | `/srv/fast/comfyui-models-pr24`, unchanged |
| Managed checkpoint | Juggernaut Ragnarok, approved profile/workflow unchanged |
| Disposable Director | `/srv/rack-ai-media/director-test`; app release `1e7fa41f0ad1424b10c82a84dec1e7bc5be188ed` |
| Existing production Director client | NOT_DEPLOYED; its database/media were not modified |

ComfyUI uses `/srv/comfyui/ComfyUI`, `venv`, `user`, `input`, `output`, `temp` and `hf-cache`. The media root contains `config.json`, `state/`, `authority.json`, `secrets/`, `extra-model-paths.yaml`, versioned `releases/` and `current`. Control state and credentials are separate from ComfyUI user data. The root is mode 0700; configuration and secrets are mode 0600. Existing principal credentials/control secret were copied without regeneration.

The receiver is `rack-ai-media-pr24.service`, running `/srv/rack-ai-media/current/rack_ai_media /srv/rack-ai-media/config.json`. ComfyUI remains `rack-ai-comfyui-pr24.service` with `Restart=no` and the same GPU binding, limits and gate environment. Its exact qualified launch is:

~~~sh
/srv/comfyui/venv/bin/python /srv/comfyui/ComfyUI/main.py --listen 127.0.0.1 --port 8190 --disable-auto-launch --enable-manager --enable-manager-legacy-ui --disable-api-nodes  --extra-model-paths-config /srv/rack-ai-media/extra-model-paths.yaml --output-directory /srv/comfyui/output --input-directory /srv/comfyui/input --temp-directory /srv/comfyui/temp --user-directory /srv/comfyui/user
~~~

Gate authority/control environment points to `/srv/rack-ai-media/authority.json` and `/srv/rack-ai-media/secrets/control`. Both Manager flags are required. Manager comes from the official package; no legacy custom-node Manager checkout was added. Tailscale Serve is unchanged: HTTPS 443 → loopback 8191, HTTPS 8444 → loopback 8192; 8443 is disabled. Raw ComfyUI remains loopback 8190.

## Interactive live proof

The proof started `2026-09-13T10:21:39Z` from Stopped, inactive unit, no media lease or 4080 process. Launcher Start reached Ready; Open loaded native ComfyUI. Classic Manager 4.2.2 was visible, and startup logs plus API checks proved core, ComfyUI-GGUF and `rack_ai_gate` loaded. Ragnarok remained visible.

The actual native Run generated a verified 512×512 PNG before restart. Extensions → Restart → Confirm returned HTTP 202. Rack observed `draining → stopping → start_pending → starting → completed`, verified generation 1 → 2, PID 307051 → 307995 and a different systemd invocation. The launcher returned to Ready automatically. No manual service restart, SSH recovery or browser reload was used.

Session `3925ee5a-6778-4fa1-8d98-14bba770ebb6` and activation `b33d5e92-81b9-40e6-9574-ece2fd38d8fa` survived unchanged. The lease owner/generation and exact lease-file bytes remained identical (SHA-256 `cc941ae92522c9d4a16ec4dbacff1834eaadd269b0f9fd25f76a42d52fc7950c`). A second native Run succeeded after restart. Finish returned Stopped, MainPID=0, empty cgroup/job, no NVIDIA process on the media UUID and no media lease.

| Artifact | Prompt | SHA-256 | Bytes |
| --- | --- | --- | --- |
| Before restart | `5e341fd8-1b0a-4c46-8b12-dbcf74101b64` | `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80` | 453003 |
| After restart | `6bf3c0e2-4494-409f-9b38-5f91187aec9e` | `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80` | 453003 |
| Director managed/imported | `25998524-38d7-4bc5-8950-bbe62051dd95` | `b78324f414b4f6cf4800f7b9c54de44f1d1ccb6b133a016d6ecb8f15f3e9a36b` | 406071 |

Native outputs are `/srv/comfyui/output/native-pr24-proof/image_00005_.png` and `/srv/comfyui/output/native-pr24-proof/image_00006_.png`. The identical hashes reflect the unchanged deterministic workflow/seed; distinct prompt histories and output files prove both executions.

## Managed Director live proof

The disposable Director project `pr24-permanent-managed-20260913` began with backend Stopped. Test connection did not start it. The actual Generate button admitted one submission, and the separately supervised worker auto-started ComfyUI. The browser closed while the worker completed and imported the exact manifest-verified PNG into the correct project/segment/image slot.

Rack job `31efe23a-c073-426b-9923-2eb03c2962ab` and Director submission `b58b3978-bfc3-422a-ac43-9301d2eb00af` produced `/srv/rack-ai-media/director-test/media/rack-ai/3/b58b3978-bfc3-422a-ac43-9301d2eb00af/image.png`. Replaying the original browser form before and after completion and reconciling again retained exactly one submission, backend history prompt and attached asset. A reopened Director page displayed the 512×512 image. Managed idle retention returned Stopped with inactive unit, no 4080 process and no lease. No production Director data or application behavior was changed.

## Code and fixture validation

| Check | Result | Evidence |
| --- | --- | --- |
| Full offline Rust workspace | 341 passed, zero failed | `rust-tests.log` |
| Rust formatting / scoped whitespace | PASS | `rust-format.log`, final Git checks |
| Full Rack media fixtures | 61 passed | `media-tests.log` |
| Launcher / supervised restart / managed regressions | PASS within the 61 tests | Same log |
| Director focused real HTTP/browser integration | 6 passed | `director-focused-tests.log` |
| Director system checks / migration consistency | PASS, no changes detected | `director-system-checks.log` |
| Four live user unit validations | PASS | `systemd-verify.log` |
| Config, model-path and unit templates match live | PASS, identity placeholders retained | `asset-validation.json` |

The media suite includes Finish-during-restart, foreign replacement rejection, bounded timeout fail-closed behavior, preserved session/lease/generation verification and managed-mode isolation. These failure cases are fixtures, not destructive live tests. The current live proof exercises one normal restart and the complete image/Finish path.

All build/test/browser/service work ran on gpurack. Browser proofs retained Chromium's sandbox and used software rendering so the test browser did not compete for the media GPU. The broader legacy Director suite was not rerun or repaired; its previously reproduced unrelated failures remain documented in the companion qualification.

## Migration, cleanup and protected services

Pre-cutover inspection recorded references in the receiver, ComfyUI gate/model configuration and disposable Director units/runtime. Only those services were migrated from a stopped backend. State, authority, principal digests, secret bytes, model-path configuration and the disposable database were preserved; canonical resource files were never manually deleted or replaced.

After zero-reference process/unit/config/symlink audits, the old install directory was moved into rollback storage before live qualification. Its original path was absent during both complete live proofs. Once the proofs passed, the obsolete ComfyUI source, old ComfyUI and Director venvs and old browser libraries were removed. Historical source is a non-runnable archive; state, sessions, jobs, credentials, recovery information, outputs and proof records remain in private archives. Final audits find zero live references to dated/retired runtime paths or obsolete media-root names.

`canonical-comfy-processes.json` records the actual command lines of all three qualified ComfyUI generations; every launch used `/srv/comfyui/venv/bin/python /srv/comfyui/ComfyUI/main.py`. `live-references-final.json` and `cleanup.json` establish that the old runtime is gone and /srv/comfyui is the only live ComfyUI installation.

Both protected inference containers retained their IDs, images, start times, PIDs, GPU bindings, compute PIDs and health responses. Protected GPU memory, campaign supervisor invocation/PID, live Rack HEAD/status, administrator configuration hash and Tailscale Serve JSON also matched the baseline. No ATHBA, JCode, campaign or unrelated service was modified.

## Evidence, access and rollback

Current evidence: `/srv/rack-ai-media/evidence/permanent-20260913`. It contains deployment/migration/cleanup manifests, before/after audits, test logs, interactive/managed proof JSON, workflow histories, screenshots, actual process command lines, NVIDIA XML and protected comparison. Historical evidence is `/srv/rack-ai-media/archive/qualification`. Old state/data are preserved under `pr24-pr33-20260912/retired-install/candidate` within that private archive.

Bookmark the launcher on the authorized tailnet. Sign in with the existing operator credential, stored server-side at `/srv/rack-ai-media/secrets/operator`, then Start → Open ComfyUI. Normal Extensions → Restart → Confirm is supported; Finish releases ownership. Do not put credentials into URLs, Git or logs. For the disposable Director UI, use `ssh -L 8193:127.0.0.1:8193 tomp@gpurack` and open `http://127.0.0.1:8193`. Its existing server-side principal and permanent allowlist are already configured.

Rollback configuration/unit copies and the qualified release/binary manifest are at `/srv/rack-ai-media/rollback/permanent-qualified`. First Finish/reconcile managed work and verify Stopped, no lease or media process. Preserve current state/data. Restore only those permanent configuration/units or select a qualified receiver release containing the restart fix; daemon-reload and restart only the media receiver. Do not restore stale state, pre-fix code or dated paths. See [operations and recovery](pr24-media-operations.md#validation-and-rollback).

Acceptance blockers: none. Manager logs report that optional Matrix sharing is unavailable because `matrix-nio` is absent; this does not affect the qualified classic Manager/restart/image paths and no unrelated dependency was installed.
