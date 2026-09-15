# External reservation deployment — 2026-09-15

## Historical first deployment: partial

**Superseded by [the managed inference cutover](inference-reservation-cutover.md).**
The account below records the first deployment before the user authorized the idle
maintenance window. Its statements about active legacy services and unqualified
inference describe that earlier state, not the current deployment.

### Original result

The media/runtime receivers and RackAI native admission gate are deployed on gpurack.
The authenticated runtime gateway is available through the tailnet. Managed image and
interactive ComfyUI profiles are enabled. Both production inactivity policies are 1800 seconds.

The complete requested cutover is NOT finished. local-primary and local-coder remain
unqualified/unavailable through the new runtime. Existing inference containers remain
healthy on their legacy local addresses, now private. An ordinary authorized local-primary
acquisition returned a durable denial with reason=unqualified_profile.

Live RackAI configuration and CLI wrappers still select fixed local backend addresses.
They have no deployed adapter that acquires the managed reservation and passes its current
generation-scoped gateway into each existing bounded call. Stopping the resident services
and enabling on-demand profiles would break those callers; retaining the resident GPU
process prevents managed startup (foreign_gpu_process). Reclassifying that process as
owned merely because its port answers would bypass the ownership contract.
A RackAI caller-integration and legacy-ownership migration change, with deterministic and
semantic review, is needed before inference cutover. No source policy or ownership check
was weakened to hide this gap. No companion client integration is claimed.

## Installed state

- User unit rack-ai-runtime.service: enabled and active.
- Runtime config: /srv/rack-ai/deployments/idle-runtime/config.json (0600).
- Runtime release: /srv/rack-ai/deployments/idle-runtime/releases/deploy-idle-20260915T115622Z/rack_ai_runtime.
- Gateway: https://gpurack.tailc214fc.ts.net:8445/runtime/v1, tailnet only,
  proxied to 127.0.0.1:8095; bearer authentication remains mandatory.
- Existing media unit rack-ai-media-pr24.service: active; current release link points to
  /srv/rack-ai-media/releases/deploy-idle-20260915T115622Z.
- RackAI gate.py and entrypoint installed into
  /srv/comfyui/ComfyUI/custom_nodes/rack_ai_gate/.
- Media config retains original origins, devices, runtime, protected containers, existing
  principals and checkpoint. reservation_idle_seconds=1800 is explicit.
- Runtime idle_timeout_seconds=1800; qualified profiles: local-image and comfyui.
- The managed image binding remains Juggernaut Ragnarok, SHA256
  dd08fa32f98d05a2443ca1419e46df1575a0811f6e3b246d9dd47ff20f5eb66a.
  This is not a Krea2 managed-workflow qualification. Manual library selection is unchanged.
- Source cb has Paramount access to logical primary/image tags and a matching media
  principal with operator=false. Its credential remains only on gpurack in
  /srv/rack-ai/deployments/idle-runtime/secrets/cb (0600). No credential was printed or
  copied to a client machine. Permission does not bypass unavailable qualification.
- Existing operator/director media policies are unchanged. No ATHBA policy was raised;
  no new ATHBA credential or client installation was provisioned.
- Temporary qualification principals were removed from both live configurations.
- Canonical authority remains /srv/rack-ai/state/resources. Historical invocation records
  and earlier cancelled demands were compared and preserved; old uncertain outcomes were
  not rewritten as successes. Final claims and legacy leases are empty.
- ComfyUI is stopped after verification and awaits explicit session/acquisition.
  Existing campaign supervision is active.

## Backend isolation

Both inference containers were recreated from their exact inspected configurations with
only HostConfig.PortBindings changed to 127.0.0.1:8017 and 127.0.0.1:8018.
Image IDs, full Config objects (commands/environment), complete mounts and all other
HostConfig fields were compared equal. Original containers remain stopped as
vllm-primary-before-idle-deploy and vllm-coder-before-idle-deploy.

Only those two mappings were changed in administrator-modified /srv/rack-ai/compose.yaml;
comparison against its pre-deployment snapshot verified this. config/repositories.json
was not modified by this deployment.

From the NUC, TCP connections to gpurack ports 8017, 8018 and 8190 failed; 8445 was reachable.
This was a network probe only: no NUC files/code changed. Both model identities and health
were verified through localhost on gpurack. The gateway rejected unauthenticated discovery
with HTTP 401 and accepted authenticated discovery over its Tailscale HTTPS origin.
No firewall changes were made.

The initial coder startup observation reached its 420-second deadline. The same process
finished startup immediately afterward; a fresh health check passed without restarting or
changing model configuration. The failed bounded observation log is retained alongside
subsequent successful identity/health evidence.

## Live verification

1. Release binaries built offline from the tested PR35 worktree.
2. Manual session: explicit Start -> Ready, authenticated native prompt, real 512x512
   image render (31.99 seconds), automatic idle_timeout, closed access, stopped service
   and released legacy lease. Temporary 60-second policy accelerated this proof;
   production was restored to 1800.
3. Canonical managed image reservation: Ready, authenticated bound media job, completed
   real 512x512 image and hashed artifact, automatic idle_timeout, verified process
   removal and empty canonical claims. Temporary runtime policy was 120 seconds;
   production was restored to 1800.
4. Shared interactive reservation: Ready, authenticated native queue access, explicit
   release completed with no process or retained claim.
5. Ordinary cb source over the tailnet gateway: Paramount image acquisition became Ready
   and explicit release completed. Its primary request returned unqualified_profile.
6. Both legacy inference identities, container health, unchanged configurations and new
   private bindings verified. This is not a fresh JCode/workspace execution proof.
7. Both receivers and campaign supervisor active; native service stopped; no test grants
   or legacy leases remain.
8. Formatting and patch whitespace passed. No application code was edited during
   deployment. The earlier 362 Rust / 75 runtime plus 2 subtests / 80 media tests remain
   the deterministic implementation evidence.

Accelerated live checks demonstrate lifecycle effects, not a 30-minute wall-clock soak or
every concurrency interleaving. Deterministic tests cover independent clocks, races and
protection of work that runs beyond the idle threshold.

## Evidence and rollback

Logs, permission-preserving snapshots, artifacts, intents, results, identity comparisons
and SHA256 manifests: /srv/rack-ai/.worktrees/pr35/evidence/deploy-idle-20260915T115622Z

Sensitive snapshots remain in that private directory. Evidence is untracked. No commit,
push or merge occurred. No CB, ATHBA or NUC code/configuration was edited.

For receiver rollback: stop admissions, release active grants through recorded owners,
verify stopped native processes and empty claims, then stop both receivers. Retain current
full authority and media state first. Restore saved media binary link, native gate and
config as a matched set from backup/. Stop/disable the new runtime unit and remove only its
new Tailscale :8445 route if reverting it; retain existing :8444. Do not roll back canonical
state or erase new terminal/evidence records.

Do not start retained old inference containers while replacements run. Their previous
all-interface publications would reopen bypass access. Rollback must retain private
networking or proven equivalent isolation. Replacements already retain the original
image, model and configuration; only networking changed. No model rollback is needed.
