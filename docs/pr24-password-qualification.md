# PR24 browser password qualification

Qualified on gpurack on **2026-09-13**. The deployed implementation is **`6c5e97fecf8f225635e82853490f1db79f1814fb`**. The following documentation commit records these results without changing executable code. Both PR24 and companion Music Director PR33 remain unmerged.

Human entry point: **https://gpurack.duckdns.org/**. Native ComfyUI: **https://gpurack.tailc214fc.ts.net:8444/**. Personal browser passwords and 30-day cookies are implemented and deployed; existing machine/API credentials remain unchanged.

**The live human password is deliberately unset.** A fresh browser successfully used the existing operator credential, displayed Set password with Current/New/Confirm fields, retained login after reopening, and logged out with the prior cookie rejected. No personal password was selected, entered or submitted by Codex. The operator completes first-use setup interactively through **Change password**. Password mutation and local recovery below were exercised only with synthetic disposable fixtures.

## Deterministic qualification

| Check | Result |
| --- | --- |
| Full Rust workspace, offline | **346 passed**, no failures |
| Full media fixtures, including launcher, restart, ownership and managed-mode regressions | **72 passed** in 606.12 seconds |
| Focused companion Director HTTP/browser fixtures | **6 passed** in 29.41 seconds |
| Formatting, scoped diff, debug and release builds | **PASS** |
| Independent read-only local-primary semantic review | **Accepted**, no unresolved blocking findings |

Password fixtures cover initial bootstrap; persisted random-salt Argon2id; rejection of plaintext persistence; dedicated-password login; rejection of the machine token as browser password after setup; continued API authorization; current-password verification; confirmation and length checks; salt rotation; concurrent changes; other-session revocation; restart persistence; exact/missing Origin rejection; bounded backoff and recovery; logout; and local administrative reset without machine credential or media-state mutation. Native browser sockets close following logout, password change or reset. A corrupt human record fails closed for browser authentication while API bearer authentication remains available, including after receiver restart.

The KDF uses **Argon2id v19, 64 MiB, three iterations, one lane, a fresh 16-byte random salt and 32-byte output**. Atomic storage uses mode 0600 inside a mode-0700 directory. Focused Rust tests cover private writes, bounded KDF parameters, independent salts, cookie attributes and the fixed-size throttle.

The full media run includes the previously accepted restart lifecycle and managed isolation fixtures. No lifecycle/generation/restart implementation, model or qualified workflow definition changed. Director remains at `bb5cd3262b82f784ed0c300ececc9f9615a09e41`; only the focused integration tests ran, without repairing unrelated legacy failures.

Two low-priority automated review observations were checked against source: the existing double-UUID session identifier already uses OS randomness, and logout already explicitly emits SameSite=Strict. Both were resolved without weakening tests or changing qualified behavior. Private review request/response and the written disposition are retained with the evidence.

## Live browser and interactive qualification

The receiver was deployed only after verifying a stopped backend with no media process or lease. The configuration change adds only `browser_auth_file`. Receiver and ComfyUI units are byte-identical to their pre-deployment versions.

| Check | Live result |
| --- | --- |
| Fresh bootstrap login at the memorable HTTPS hostname | **PASS** |
| Account page, all three password fields, no password submission | **PASS** |
| 30-day server expiry aligned with the cookie | **PASS** |
| Secure, HttpOnly, SameSite=Strict, Path=/ | **PASS** |
| Reopened browser retains login | **PASS** |
| Launcher Start, Ready, Open native UI | **PASS** |
| Classic Manager 4.2.2, ComfyUI-GGUF, rack_ai_gate and Ragnarok visibility | **PASS** |
| Known-good 512x512 PNG before restart | **PASS** |
| Actual Extensions → Restart → Confirm, HTTP 202 | **PASS** |
| Supervised draining/stopping/start_pending/starting/completed phases | **PASS** |
| Same interactive session and exact GPU lease bytes | **PASS** |
| Verified backend generation 1 → 2, returns to Ready | **PASS** |
| Known-good 512x512 PNG after restart | **PASS** |
| Finish, inactive unit, MainPID=0, empty cgroup, no media GPU process or lease | **PASS** |
| Log out returns to login; previous browser cookie rejected | **PASS** |

**NORMAL_COMFYUI_RESTART_REQUIRES_SSH=NO.** There were no SSH repair commands and no browser page reload during normal restart. The receiver supervised the restart through its existing lifecycle.

Session: `06d5db37-1ce7-4e63-ab09-640494b81883`. Activation: `f1880d25-0776-4b52-a7f4-401ec7c39142`. Lease generation: `9619a20965b3e73ef7c3116a57f794c7`. ComfyUI PID changed from 407787 to 408535 while the session and lease remained the same.

| Artifact | Backend prompt | SHA-256 |
| --- | --- | --- |
| `native-pr24-proof/image_00009_.png`, before restart | `0ed75ac7-662f-405c-84df-e03fbfb9011b` | `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80` |
| `native-pr24-proof/image_00010_.png`, after restart | `02fea426-e8eb-4453-a382-1134815f1684` | `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80` |

Both files are separate successful backend executions, 453003 bytes each. Identical hashes are expected for the same deterministic qualified workflow. History, dispatched workflow, PNG copies and browser screenshots are retained.

## Live managed Director qualification

A disposable Director project started with the backend stopped. The actual browser **Generate** action authenticated with the unchanged Director API credential, automatically started ComfyUI, generated one image, and imported exactly that artifact through independent worker reconciliation. Replaying the same request retained one submission, backend job, prompt and imported asset. The browser reopened the imported 512x512 image. Managed idle shutdown returned to Stopped and released the media GPU.

- Project: `pr24-password-managed-20260913`.
- Submission: `0b7c7faa-6009-4aa5-aa29-68d930f22381`.
- Rack job: `c8e3b9ef-5821-4d2b-96c4-da8fa12a173c`.
- Backend prompt: `d24a4a6b-406c-4fc3-855f-7392ffc52e35`.
- Imported asset: 5, 424368 bytes.
- Exact artifact SHA-256: `d0be65b9e7b8dbbe4f12db01589a586305b8d3a98c7cd36a623677651fe62b2e`.
- **LIVE_MANAGED_JOB_QUALIFICATION=PASS; MANAGED_JOB_AUTH_UNCHANGED=PASS.**

Production Director data was not modified. No paid inference or additional model qualification ran.

## Permanent deployment and final audit

- Media root: `/srv/rack-ai-media`.
- Human authentication record: `/srv/rack-ai-media/secrets/browser-auth.json`; absent after qualification because setup is left for the operator. The private parent is mode 0700.
- Current receiver release: `/srv/rack-ai-media/releases/rack-6c5e97fecf8f225635e82853490f1db79f1814fb` via `/srv/rack-ai-media/current`.
- Receiver binary SHA-256: `abaad2dcfdfaf693b3008f9ed95eae9eb4f9430a06c69356c9f54f666262d778`.
- ComfyUI root: `/srv/comfyui`; core `d43a5fa20c8547ff42d13232f589a06536c42b97`, unchanged.
- Official Manager dependencies remain in `/srv/comfyui/venv`; the unchanged launch retains both `--enable-manager` and `--enable-manager-legacy-ui`.
- Evidence: `/srv/rack-ai-media/evidence/password-20260913`.
- Verification scripts: `/srv/rack-ai-media/verification/scripts`.

The final audit verified exact byte preservation of operator, Director, control, disposable Django and DuckDNS credentials without reporting their values. API principal configuration is unchanged; both operator and Director bearer status requests succeed. The private HTTPS service and both DNS/certificate renewal timers remain enabled and active; trusted TLS and Tailscale Serve configuration are preserved. Certificate issuance/renewal was not repeated in this authentication task.

The protected 2060/4060 Ti GPU identities, process IDs and memory usage, inference containers and start identities, endpoint health, campaign process and live Rack checkout/configuration all match the pre-task snapshot. No protected service was stopped or restarted. The final backend state is **Stopped**, with no ComfyUI process or 4080 lease. **PROTECTED_SERVICES_UNCHANGED=PASS.**

## Recovery and rollback

[Browser password operations](pr24-browser-passwords.md) documents first use, change/logout, 30-day persistence and the shell-only `rack_ai_media_admin` reset. The reset revokes browser cookies and returns only human authentication to bootstrap, without changing machine credentials or media/resource state.

The previous release is retained at `/srv/rack-ai-media/releases/rack-872c3e8624e1387208b0cdae6c2d4991c39bd642`. The pre-change compatible config, units and release manifest are retained at `/srv/rack-ai-media/rollback/password-bootstrap-650a67a`.

While deliberately still in bootstrap mode, an operator can Finish admitted work, verify Stopped with no media process/lease, stop only the media receiver, atomically restore the saved compatible config and prior `current` release symlink, then start only the media receiver and verify authenticated status. Do not restore media state or manually remove leases. **After a personal password has been configured, use only an authentication-aware release for rollback**; the previous pre-password receiver cannot enforce the new revocation generations.

No rollback was required. There are no unresolved acceptance blockers. Password-setting security claims are fixture-qualified; live proof intentionally leaves the personal password unset.
