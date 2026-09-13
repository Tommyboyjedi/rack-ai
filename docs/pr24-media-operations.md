# PR24 v2 media operation and qualification

The permanent deployment provides launcher → native ComfyUI and image job → Music Director import paths. It does not replace the running Rack AI development binary. See `pr24-qualification.md` for current measured results and deployed revisions. Music Director PR33 is tested in a separate disposable integration instance; existing client data is not modified.

## Ownership and installation

The machine authority is `RACK_AI_RESOURCE_ROOT`, default `/srv/rack-ai/state/resources` for the Rack CLI. The media receiver's administrator-only config must name the SAME resource root, independently of its job state directory. Inject temporary roots only in tests. No live lease migration occurs. Legacy, corrupt and unknown records block acquisition. Existing development binaries are not rewritten or restarted by this installation; do not claim they were retroactively upgraded. The real media resource is `gpu-4080-super`; the planned 3090 slot is retained as retired data.

Reservations carry an owner and random generation. Acquisition serializes the complete resource set using a bounded machine lock. Release verifies every record before any deletion and writes a durable release intent, so a crash between deletion and state commit can be reconciled without deleting a later owner's reservation. Release receipts remain in the authority's `releases` directory; do not remove active/uncertain records or perform age-based lease cleanup.

Use `config/media/config.example.json` and `config/media/systemd/` as administrator templates. Placeholders deliberately fail validation. The process probe uses the complete NVIDIA XML process inventory, including compute, graphics and mixed contexts; unavailable inventories fail closed. Verify full physical UUIDs with `nvidia-smi` and both protected containers' DeviceRequests with narrowly formatted `docker inspect`; never use an index or reassign a development GPU. Keep configuration and credentials mode 0600 in a mode 0700 media runtime directory. Each client gets a separate random credential; only its SHA-256 is stored in receiver configuration. Music Director/ATHBA ceilings are medium, operator ceiling paramount. Caller identity fields and proxy identity headers do not grant privileges.

ComfyUI is pinned to `d43a5fa20c8547ff42d13232f589a06536c42b97` (upstream master verified 2026-09-13), with source at `/srv/comfyui/ComfyUI` and environment at `/srv/comfyui/venv`. The installed dependency snapshot is `config/media/comfy-runtime.lock.txt`, including official ComfyUI Manager `4.2.2` from upstream `manager_requirements.txt`. The preserved gate is at `/srv/comfyui/ComfyUI/custom_nodes/rack_ai_gate`. The normal pinned custom-node startup imports the gate before aiohttp freezes the application. Configure authority/control paths via the service environment. Install Manager with the permanent venv using the official ComfyUI `manager_requirements.txt`; keep both `--enable-manager` and `--enable-manager-legacy-ui` for the classic UI. Start closed and run with `--disable-api-nodes`, loopback listen and explicit permanent input/output/temp/user directories. The gate covers every unsafe method, including pinned prompt aliases, and serializes closure with in-flight validation/enqueue. The ComfyUI service must have `Restart=no`; only the receiver starts it after reserving.

The approved managed profile uses CheckpointLoaderSimple, CLIPTextEncode, EmptyLatentImage, KSampler, VAEDecode and SaveImage. One image, 64–1024 pixels in multiples of 64, 1–50 steps, signed-64-bit-compatible nonnegative seed, prompts at most 4096 UTF-8 bytes and a 10–900 second admission-to-completion deadline. Workflow, checkpoint hash, prompt UUID and request are retained before dispatch. Native installed workflows may use other local assets; their presence is not qualification of those models or of video automation.

On this rack, permanent ComfyUI storage is on NVMe: `/srv/comfyui/ComfyUI`, `/srv/comfyui/venv`, `/srv/comfyui/user`, `/srv/comfyui/input`, `/srv/comfyui/output`, `/srv/comfyui/temp` and `/srv/comfyui/hf-cache`. Rack AI configuration, authority and credentials live separately under `/srv/rack-ai-media`. ComfyUI must never use that control directory as its user/input/output/temp directory.

The media root contains `config.json`, `state/`, `authority.json`, `secrets/`, `extra-model-paths.yaml`, versioned `releases/` and the `current` receiver symlink. Resource leases remain in the canonical `/srv/rack-ai/state/resources` authority; migration does not copy, delete or replace them. `director-test/` contains only the disposable Director environment/database/media. `verification/` contains the test environment support and known workflow; `evidence/` and `archive/qualification/` retain current and historical proofs. Keep the root mode 0700 and credentials/configuration mode 0600.

The canonical model library is now `/srv/fast/comfyui-models` on SATA, following the operator's complete USB recopy. Configure it through `/srv/rack-ai-media/extra-model-paths.yaml`, the file named by the ComfyUI unit; `/srv/comfyui/extra_model_paths.yaml` is not used by this deployment. The checked-in template maps the standard installed categories plus IPAdapter, including both `diffusion_models` and legacy `unet`, and both `text_encoders` and legacy `clip`. The qualified managed profile remains Juggernaut Ragnarok with its existing checkpoint digest. Model discovery does not qualify every model or install additional custom nodes. `checkpoints/miracleinNSFWGeneration_30Bf16Fp8.safetensors` was intentionally excluded for SSD capacity and is not required. No model copy, deletion, download or paid inference is part of this path update. See [model-library qualification](comfyui-model-library-qualification.md).

## Private access and CLI

Human password setup, change/logout, 30-day browser persistence and shell recovery are described in [browser password operations](pr24-browser-passwords.md).

Current HTTPS routing, automatic renewal and hostname rollback are described in [private HTTPS operations](pr24-private-https.md).

Raw ComfyUI is `127.0.0.1:8190`. The authenticated receiver and native HTTP/WebSocket proxy listen separately on `127.0.0.1:8191` and `127.0.0.1:8192`. Dedicated nginx on the Tailscale IPv4:443 routes to the receiver, and Tailscale Serve 8444 routes to the native adapter. The launcher is https://gpurack.duckdns.org/ and native UI remains https://gpurack.tailc214fc.ts.net:8444/. Port 8443 is disabled. The old .ts.net launcher on 443 is a tested restore-only rollback route. Preserve all other Serve configuration; do not enable Funnel. The native UI lives at the root of its own origin.

Open the launcher, sign in with your personal password, then Start → Open ComfyUI. Before a personal password exists, use the existing operator credential once and choose Change password. The retained native domain has its own host-only cookie and may require the same personal password there; see the browser password runbook. Bookmarking, prefetching, status and profile reads do not start the GPU. Finish closes admission, drains accepted work, verifies process-tree exit and absence of GPU allocations, then releases the reservation. The deployment uses a four-hour interactive session expiry that drains normally, and 30 seconds of managed idle retention. Closing the browser does not cancel work or release its session.

The browser holds a random HttpOnly, SameSite=Strict session cookie (Secure over HTTPS), never the bearer/control secret in a URL, JavaScript storage or rendered native content. Cookies authenticate unsafe requests only with an exact allowed Origin. Native HTTP and WebSockets require the requesting operator's active session. Reopening the launcher resumes its durable session.

The `rack_ai_media_client` binary uses the same HTTP use cases:
```sh
export RACK_AI_ENDPOINT=https://gpurack.duckdns.org
export RACK_AI_CREDENTIAL_FILE=/path/to/mode-0600-client-credential
rack_ai_media_client status
rack_ai_media_client profiles
rack_ai_media_client start UNIQUE_SESSION_KEY
rack_ai_media_client session SESSION_UUID
rack_ai_media_client finish SESSION_UUID
rack_ai_media_client submit exact-saved-request.json
rack_ai_media_client job JOB_UUID
rack_ai_media_client cancel JOB_UUID
```
The CLI cannot select a state root or issue arbitrary commands. Retrying a submission means reusing its exact saved body. A deliberate new image uses a new submission/idempotency identity.

## Normal ComfyUI Restart

The normal authenticated Manager Restart action is supported without SSH. The native proxy mediates the existing POST `/v2/manager/reboot` (including its `/api` alias) into one supervised stop/start cycle instead of forwarding Manager's untracked process exec. No alternate UI or API action is required. The same activation, interactive session and exact resource reservation owner/generation remain in force.

The durable service state is `restarting`. Its typed intent advances through `draining`, `stopping`, `start_pending` and `starting` to `completed`. Admission stays closed while accepted work drains, the verified old unit exits and its cgroup/GPU allocations disappear. Before starting, Rack verifies the loaded unit, permanent runtime, gate file bindings and physical GPU placement. It admits the new generation only after matching its new InvocationID, PID, kernel process start time, expected cgroup, `/srv/comfyui` runtime, GPU and activation-bound gate. Only then does it return to Ready and reopen native access.

The launcher displays Restarting and polls automatically; transient native requests receive 503 with a bounded Retry-After. Duplicate Restart requests retain the current intent. The service remains `Restart=no`: each intent submits at most one start, uses the configured drain/stop/start deadlines and never loops after failure. A receiver restart resumes inspection of the durable phase, retaining the lease; it does not resubmit an uncertain start.

Finish remains available during Restarting. It cancels a start that has not been admitted or a pending systemd job, or stops the verified new generation even before its gate HTTP endpoint is ready. `finishing` records that cleanup phase. Release occurs only after the unit, cgroup and physical GPU inventory prove it gone. Finish and lifecycle effects are serialized, so a start cannot be submitted after Finish has been accepted.

Unannounced process replacement, a changed unit/runtime/GPU binding, an invalid gate or a deadline failure still closes admission and quarantines ownership. Managed jobs cannot invoke this transition: their existing generation, dispatch and reconciliation rules remain strict, with no restart or redispatch allowance. Runtime paths are explicit in `config/media/config.example.json`; configs without explicit runtime paths default to the permanent `/srv/comfyui` installation.

## API and recovery

`config/media/openapi.json` specifies the versioned API; adjacent JSON Schemas and synthetic fixtures are shared with the Director tests. New jobs/sessions return 202 and a status location only after durable storage. Identical replay returns the same object; conflicting identity reuse returns 409. Job, session, cancel and artifact access is principal-scoped. Limits: 16 KiB JSON, 128 active/2048 retained jobs, 2048 sessions, 64 active API operations, 64 active native operations with a 128-request/five-second bounded waiting room, 16 WebSockets, one-hour socket lifetime, 16 MiB artifacts. Full retention returns unavailable; identities are not silently evicted and rendered again. Archive only under an explicit operator retention policy that preserves replay tombstones.

The receiver never retries ComfyUI POST after dispatch intent. A lost acknowledgement is reconciled using the canonical UUID against the same verified activation's queue/history. Missing history yields interrupted/unknown, not success or a new render. A completed artifact requires matching prompt/workflow, successful output node 9, the job namespace, a regular non-symlink bounded PNG, exact dimensions and durable hash/length manifest. Cancellation wins local publication if it was recorded before artifact publication.

Startup reconciles before accepting generation. Same verified activation resumes inspection; a persisted interactive restart intent permits only its verified expected generation transition. An unannounced invocation, ambiguous PID/cgroup, corrupt state or uncertain cleanup yields recovery_required and keeps admission closed. Heartbeats refresh the short-lived authority; receiver death closes new mutations after at most 20 seconds. No health-only adoption, PID-only stop, global interrupt, lease expiry deletion or automatic GPU reassignment is used.

An already-requested interactive Finish can drain a quarantined backend only while its recorded invocation, generation (when present), gate and physical ownership still verify exactly. It never adopts a replacement or reopens admission.

For recovery, first preserve media state/config hashes, service show/journal, gate observations and GPU PIDs. Never delete a lease to make a test pass. If the recorded InvocationID exactly matches the owned media unit, an operator may stop THAT unit after choosing to abort its remaining work. Verify MainPID=0, inactive/failed status, an empty cgroup and no allocations on the media UUID. Restarting only the receiver then reconciles its durable owner/generation release. If identity differs or the state is corrupt, keep the reservation quarantined and restore a verified state snapshot only after manual reconciliation. No automated script kills an unknown process.

## Validation and rollback

Build/test on gpurack, with fake machine commands and disposable resource/job roots:
```sh
cargo fmt --check
cargo test --workspace --offline
cargo build -p rack_ai_media --offline
python -m pytest -q tests/media
git diff --check
```
Python test requirements are in `tests/media/requirements.txt`. Cross-repository tests additionally set `RACK_AI_TEST_REPO`. Browser tests use sandboxed Playwright; the approved rack installation is `/opt/rack-ai-pr24-browser/chrome-headless-shell` with the exact-path profile in `tests/media/rack-ai-pr24-browser.apparmor`. The global AppArmor user-namespace restriction remains enabled. Use software rendering (`--disable-gpu --use-angle=swiftshader`) in rack-side browser proofs so the browser does not compete for the media GPU. The test-only `LD_LIBRARY_PATH=/srv/rack-ai-media/verification/browser-libs/root/usr/lib/x86_64-linux-gnu` supplies libasound.

Normal operation uses the existing user units `rack-ai-media-pr24.service` and `rack-ai-comfyui-pr24.service`. Do not enable ComfyUI independently: the receiver starts it only after reserving. After an approved configuration/unit edit, use `systemctl --user daemon-reload` and restart only the receiver from a clean stopped backend. Keep the classic Manager flags, physical GPU binding, gate environment, limits and `Restart=no` intact.

For rollback, Finish interactive work and let managed work finish or cancel it explicitly. Verify Stopped, no media lease, inactive unit/empty cgroup and no 4080 allocations. Preserve all current state, sessions, jobs, artifacts and receipts. Restore only the permanent configuration/unit copies in `/srv/rack-ai-media/rollback/permanent-qualified`, or select a previously qualified receiver release that includes the supervised restart fix. Reload user systemd and restart only `rack-ai-media-pr24.service`. Never select a pre-restart-fix binary during Restarting, restore a stale state snapshot over newer work, or delete leases to obtain a clean result.

Historical pre-migration files are retained as private archives for audit, not live deployment instructions. Do not re-enable dated runtime paths, change Tailscale routing during binary rollback, restart vLLM/JCode/ATHBA/campaigns, reset the live checkout or reverse Director migrations.
