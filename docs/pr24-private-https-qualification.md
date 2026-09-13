# PR24 private HTTPS qualification — 2026-09-13

Current browser authentication is described in [human password operations](pr24-browser-passwords.md). That change supersedes older credential-login details retained in this historical proof.

**Launcher: https://gpurack.duckdns.org/**. Native UI remains **https://gpurack.tailc214fc.ts.net:8444/**.

All acceptance checks below passed against the permanent deployment on gpurack. PR24 remains unmerged. This task adds private DNS/TLS operations and changes the launcher origin; Rack application source, authentication, resource ownership, workflow definitions, ComfyUI and Music Director source remain unchanged. The accepted restart intent at `a0212bca6be8c45e6812a1f55b86754510234074` is preserved.

## Versions and live layout

| Item | Qualified value |
| --- | --- |
| Starting PR24 head | `b9238c682e1e4f3353c29bafa9309ef5fe41424d` |
| Final publication | Exact SHA in PR24 and private `publication.json`; this document is part of that commit |
| Deployed Rack application | `872c3e8624e1387208b0cdae6c2d4991c39bd642` |
| Deployed receiver SHA-256 | `51beeb92a2644e776971812c45897fad8c78d4db55998f772a247ef2a3df28c4` |
| Media / HTTPS roots | `/srv/rack-ai-media` / `/srv/rack-ai-media/https` |
| ComfyUI / venv | `/srv/comfyui/ComfyUI` / `/srv/comfyui/venv` |
| ComfyUI core revision | `d43a5fa20c8547ff42d13232f589a06536c42b97` |
| Manager | Official `manager_requirements.txt`, 4.2.2; `--enable-manager --enable-manager-legacy-ui` retained |
| Model / resource roots | `/srv/fast/comfyui-models-pr24` / `/srv/rack-ai/state/resources` |
| nginx / Certbot | Ubuntu packages 1.28.3-2ubuntu1.10 / 4.0.0; no package upgrades |
| Companion PR33 head | `bb5cd3262b82f784ed0c300ececc9f9615a09e41`, unchanged |
| Disposable Director application | `1e7fa41f0ad1424b10c82a84dec1e7bc5be188ed`, unchanged |

The ComfyUI systemd unit is byte-identical to the permanent qualified snapshot, including both Manager flags and `/srv/comfyui/ComfyUI/main.py`. Only `public_origin` differs in the media configuration. Credentials, state roots, native origin, model paths and gate settings match the pre-migration snapshot.

## DNS, privacy and TLS

- Installed `tailscale ip -4` returned **100.116.176.86**. Independent Cloudflare and Google queries both returned exactly that A record and no AAAA. The updater never uses WAN autodetection.
- nginx listens only on **100.116.176.86:443**. There is no new LAN/wildcard/public listener or port 80/8443 listener. Systemd permits only loopback and Tailscale IPv4 traffic for this service. No router forwarding or Funnel was created; nftables rules are byte-identical to the initial snapshot.
- The address is in the non-publicly-routable shared-address range. Privacy qualification is based on address, listener, service filter and firewall inspection; it does not claim an external WAN penetration scan.
- A Windows tailnet client established TCP from **100.75.57.95** to **100.116.176.86:443**. All rack builds/tests/browser work ran on gpurack.
- Default-trust TLS verification and a fresh sandboxed Chromium context passed with no certificate bypass. The served certificate has exactly SAN `gpurack.duckdns.org`, issuer Let's Encrypt YE1, expiry **2026-12-12 13:28:43 UTC**, TLS 1.3. The private key remains 0600.
- One production issuance and one full Let's Encrypt **staging renewal dry run** with deploy hooks passed. No repeated production issuance was forced. The deploy hook reloaded nginx's worker, preserving nginx's master and the Rack receiver/ComfyUI identities.
- Dedicated DNS and renewal timers are enabled/active. DNS runs after boot and every six hours; renewal checks twice daily with random delay and persistent missed-run handling. Both oneshot services last succeeded. Default nginx and Certbot service/timer units remain masked.
- The DuckDNS token remains only in its protected 0700 directory / 0600 file. The audit found no DuckDNS token in repository changes, operation files, logs, argv or environment. Existing Django secret use in the two disposable Director service environments and protected rollback file was explicitly distinguished from disclosure.

Certbot's supported automatic DNS hooks were selected because the reviewed stock acme.sh DuckDNS provider persists a token copy and uses a token-bearing HTTP command URL. The [operations runbook](pr24-private-https.md) records the source review and exact renewal configuration.

## Code and fixture validation

| Check | Result |
| --- | --- |
| Full Rust workspace | 341 passed |
| Rust formatting | PASS |
| Rack media fixtures: launcher, lifecycle/restart, ownership, managed jobs | 61 passed |
| Private HTTPS operations fixtures | 32 passed |
| Director focused real HTTP/browser integration | 6 passed |
| Systemd templates/configuration | PASS; only existing unrelated xfs CPUAccounting deprecation warnings |
| Live templates/helpers equal checked-in assets | PASS |
| Protected inference and campaign services | PASS, unchanged |

The new fixtures cover refusal of non-tailnet addresses, invalid secret ownership/permissions, redirect/transport redaction, explicit A/AAAA behavior, DNS deadlines, unrelated ACME domains, exact private binding, failed-rebind retry, inactive renewal, foreign nginx/Serve identities, resumable cutover and service-user preflight before mutations. No unrelated legacy Director tests were repaired or claimed green.

## Fresh browser interactive proof

The new domain loaded the login page, accepted the existing operator credential and reached Start → Ready → Open. Classic Manager 4.2.2, GGUF, Ragnarok model visibility and `rack_ai_gate` were observed. The known-good Ragnarok workflow produced a PNG before and after a normal **Extensions → Restart → Confirm** action.

The native domain retains separate host-only/Secure/HttpOnly/SameSite=Strict cookies. A fresh browser signs in again with the same operator credential when Open reaches native 8444. This extra domain-specific login is expected and was exercised; cross-domain single sign-on is not claimed. Keep the native tab open during a session.

- Interactive session: `a9954efd-3a7b-48ca-8028-7b5c2b414805`.
- Activation: `055a76c3-7b18-453b-b561-de779f777d33`.
- Backend PID **372197 → 372878**, verified generation **1 → 2**.
- Observed `Restarting`: draining → stopping → start_pending → starting → completed.
- Session and lease remained identical. Lease generation `a083481e9daea0a18a36e3ede54ca9d5`; byte hash `379361faee15490044169c39ab3dfdb748a3e4d1e74d5195840b03da4893dde1`.
- No SSH/manual recovery or page reload was used for the Manager restart.
- Before prompt `8f00ad39-c502-4ff1-9261-c6807cc071f5`; after prompt `548d0e58-3e38-45d2-8365-45802486d152`.
- Before output `/srv/comfyui/output/native-pr24-proof/image_00007_.png`; after output `/srv/comfyui/output/native-pr24-proof/image_00008_.png`.
- Both 512×512 PNGs, 453003 bytes, SHA-256 `c1a39353669d35624dc68fe24144baae59d3fab4a808417b00f31e502f513d80`. The matching bytes are expected from the fixed seed; prompts/files are distinct.
- Finish reached Stopped, inactive unit/MainPID 0/empty cgroup, no 4080 process in complete NVIDIA XML inventory and no media lease.

**NORMAL_COMFYUI_RESTART_REQUIRES_SSH=NO.** Negative foreign replacement, timeout, Finish-during-restart and managed isolation remain fixture-qualified; no destructive live tests were added.

## Managed Director proof

The disposable project `pr24-duckdns-managed-20260913` began with backend Stopped. Settings/Test connection succeeded against DuckDNS without starting ComfyUI. The actual Generate action submitted through the authenticated new origin, Rack auto-started ComfyUI, and the independent Director worker completed import after the submitting browser closed.

- Submission `dc5175ae-2411-4acd-8940-348af398f7e3`; Rack job `f688125c-c62f-45ef-9abc-a8cf2c8004c8`; backend prompt `c84f29fd-08a4-412f-8e42-fdc7a752fab1`.
- Imported asset `4` belongs to the expected project and segment; its bytes/hash exactly match the authenticated Rack artifact.
- PNG: 512×512, `405837` bytes, SHA-256 `ba4459fac49fc8f3e19d1064c2215c032daf48359749820856c03a0e70403835`.
- Browser replay/reconcile/retry retained one submission, one backend history prompt and one image asset. A reopened browser displayed the imported PNG.
- Managed idle release returned Stopped, inactive ComfyUI/MainPID 0, no media lease and no 4080 process.
- Production Director data was not modified.

## Protected state and rollback

Protected 2060/4060 Ti container IDs, images, start timestamps, PIDs, GPU bindings, memory and health responses match the before snapshot. Campaign supervisor PID/invocation is unchanged. Live `/srv/rack-ai` HEAD/WIP and administrator configuration hash are unchanged. No drivers, model contents, workflow definitions, ATHBA/JCode code or campaigns were modified.

Tailscale Serve's native 8444 TCP/Web entries are identical to the preserved snapshot. The old launcher 443 entry alone is disabled. The rollback helper was rehearsed through authenticated old-host Stopped, then returned successfully to authenticated DuckDNS Stopped. Old .ts.net 443 is **restore-only**, not simultaneously served; 8443 remains disabled because a third origin is not authorized by the existing application allowlist.

After Finish/reconcile and verified Stopped, run:

~~~sh
sudo /usr/bin/python3 /srv/rack-ai-media/https/ops/switch_origin.py tsnet
~~~

This disables the dedicated proxy, restores the old .ts.net 443 launcher, restores the receiver/disposable Director origins, and preserves native 8444, current state and credentials. Run the same helper with `duckdns` to return. Rollback copies are at `/srv/rack-ai-media/https/rollback`. Never restore stale state or delete resource/lease files.

## Issues found and resolved

Initial nginx validation exposed packaged temporary-directory defaults and root-owned PID/temp files. All paths are now inside the permanent HTTPS root, owned by tomp; preflight uses the service user and only the binding capability. The operator helper also now handles an already-disabled owned Serve 443 during resumption and rejects foreign routes. These corrections passed regression fixtures and live rollback/cutover rehearsal.

A Python child initially lacked its caller's sudo timestamp; the scoped operator helper now runs explicitly with sudo and drops identity for user services. A local Windows HTTP wrapper/Schannel check failed in its execution context; TCP tailnet connectivity and default-trust rack/browser TLS independently passed. No TLS bypass, firewall relaxation or Rack lifecycle repair was used.

Raw evidence is retained privately at `/srv/rack-ai-media/evidence/duckdns-20260913`: interactive/managed proof JSON and screenshots, artifact copies/histories, DNS/TLS/listener/secret audit, before/after protected/firewall snapshots, staging renewal identity proof, rollback/cutover checks and test logs. Verification scripts are under `/srv/rack-ai-media/verification/scripts`.

**Blockers: none.** ComfyUI is left stopped; receiver, private HTTPS and disposable Director web/worker are active. PR24 stays Ready for Review and both PRs remain unmerged.
