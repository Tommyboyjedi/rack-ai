# PR24 v2 media operation and qualification

This candidate implements the complete launcher → native ComfyUI and image job → Music Director import paths. It does not replace the running Rack AI development binary. See `pr24-qualification.md` for measured results and deployed revisions. Music Director's companion is Tommyboyjedi/musicvideo-director PR33.

## Ownership and installation

The machine authority is `RACK_AI_RESOURCE_ROOT`, default `/srv/rack-ai/state/resources` for the Rack CLI. The media receiver's administrator-only config must name the SAME resource root, independently of its job state directory. Inject temporary roots only in tests. No live lease migration occurs. Legacy, corrupt and unknown records block acquisition. Existing development binaries are not rewritten or restarted by this installation; do not claim they were retroactively upgraded. The real media resource is `gpu-4080-super`; the planned 3090 slot is retained as retired data.

Reservations carry an owner and random generation. Acquisition serializes the complete resource set using a bounded machine lock. Release verifies every record before any deletion and writes a durable release intent, so a crash between deletion and state commit can be reconciled without deleting a later owner's reservation. Release receipts remain in the authority's `releases` directory; do not remove active/uncertain records or perform age-based lease cleanup.

Use `config/media/config.example.json` and `config/media/systemd/` as administrator templates. Placeholders deliberately fail validation. The process probe uses the complete NVIDIA XML process inventory, including compute, graphics and mixed contexts; unavailable inventories fail closed. Verify full physical UUIDs with `nvidia-smi` and both protected containers' DeviceRequests with narrowly formatted `docker inspect`; never use an index or reassign a development GPU. Keep config/control credentials mode 0600 in a mode 0700 candidate directory. Each client gets a separate random credential; only its SHA-256 is stored in receiver configuration. Music Director/ATHBA ceilings are medium, operator ceiling paramount. Caller identity fields and proxy identity headers do not grant privileges.

ComfyUI is pinned to `d43a5fa20c8547ff42d13232f589a06536c42b97` (upstream master verified 2026-09-13), with source at `/srv/comfyui/ComfyUI` and environment at `/srv/comfyui/venv`. The original qualified dependency snapshot is `config/media/comfy-runtime.lock.txt`; the promoted environment additionally contains official ComfyUI Manager `4.2.2` from upstream `manager_requirements.txt`. The preserved gate is at `/srv/comfyui/ComfyUI/custom_nodes/rack_ai_gate`. The normal pinned custom-node startup imports the gate before aiohttp freezes the application. Configure authority/control paths via the service environment. Start closed and run with `--disable-api-nodes`, loopback listen and explicit permanent input/output/temp/user directories. The gate covers every unsafe method, including pinned prompt aliases, and serializes closure with in-flight validation/enqueue. The candidate service must have `Restart=no`; only the receiver starts it after reserving.

The approved managed profile uses CheckpointLoaderSimple, CLIPTextEncode, EmptyLatentImage, KSampler, VAEDecode and SaveImage. One image, 64–1024 pixels in multiples of 64, 1–50 steps, signed-64-bit-compatible nonnegative seed, prompts at most 4096 UTF-8 bytes and a 10–900 second admission-to-completion deadline. Workflow, checkpoint hash, prompt UUID and request are retained before dispatch. Native installed workflows may use other local assets; their presence is not qualification of those models or of video automation.

On this rack, permanent ComfyUI storage is on NVMe: `/srv/comfyui/ComfyUI`, `/srv/comfyui/venv`, `/srv/comfyui/user`, `/srv/comfyui/input`, `/srv/comfyui/output`, `/srv/comfyui/temp` and `/srv/comfyui/hf-cache`. Rack AI control state, secrets and its model-path configuration remain under `/home/tomp/pr24-pr33-20260912/install/candidate`, outside ComfyUI user data. The previous qualified ComfyUI revision `9113c08c2e14f1ca6c0ccab64920777fd01e1bb9`, venv and data remain under `/home/tomp/pr24-pr33-20260912/install` for rollback; prior unit/config copies are under `/home/tomp/pr24-housekeeping-20260913/rollback`. The explicitly requested model subset and dependencies were copied from the USB to `/srv/fast/comfyui-models-pr24` on SATA, keeping the USB intact. Each destination was independently SHA-256 verified. The initial managed profile is Juggernaut Ragnarok; Krea2, LTX2.3 and CyberRealisticPony are available to compatible native workflows. LTX GGUF support is pinned to city96/ComfyUI-GGUF `6ea2651e7df66d7585f6ffee804b20e92fb38b8a`. No remote model collection or paid API is enabled.

## Private access and CLI

Raw ComfyUI is `127.0.0.1:8190`. The authenticated receiver and native HTTP/WebSocket proxy listen separately on `127.0.0.1:8191` and `127.0.0.1:8192`. New Tailscale Serve HTTPS ports 8443/8444 route to those adapters only. Preserve all other Serve configuration; do not enable Funnel. The native UI lives at the root of its own origin.

Open the launcher, sign in with the operator credential, then Start → Open ComfyUI. Bookmarking, prefetching, status and profile reads do not start the GPU. Finish closes admission, drains accepted work, verifies process-tree exit and absence of GPU allocations, then releases the reservation. The measured candidate uses a four-hour interactive session expiry that drains normally, and 30 seconds of managed idle retention. Closing the browser does not cancel work or release its session.

The browser holds a random HttpOnly, SameSite=Strict session cookie (Secure over HTTPS), never the bearer/control secret in a URL, JavaScript storage or rendered native content. Cookies authenticate unsafe requests only with an exact allowed Origin. Native HTTP and WebSockets require the requesting operator's active session. Reopening the launcher resumes its durable session.

The `rack_ai_media_client` binary uses the same HTTP use cases:
```sh
export RACK_AI_ENDPOINT=https://gpurack.tailc214fc.ts.net:8443
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

## API and recovery

`config/media/openapi.json` specifies the versioned API; adjacent JSON Schemas and synthetic fixtures are shared with the Director tests. New jobs/sessions return 202 and a status location only after durable storage. Identical replay returns the same object; conflicting identity reuse returns 409. Job, session, cancel and artifact access is principal-scoped. Limits: 16 KiB JSON, 128 active/2048 retained jobs, 2048 sessions, 64 active API operations, 64 active native operations with a 128-request/five-second bounded waiting room, 16 WebSockets, one-hour socket lifetime, 16 MiB artifacts. Full retention returns unavailable; identities are not silently evicted and rendered again. Archive only under an explicit operator retention policy that preserves replay tombstones.

The receiver never retries ComfyUI POST after dispatch intent. A lost acknowledgement is reconciled using the canonical UUID against the same verified activation's queue/history. Missing history yields interrupted/unknown, not success or a new render. A completed artifact requires matching prompt/workflow, successful output node 9, the job namespace, a regular non-symlink bounded PNG, exact dimensions and durable hash/length manifest. Cancellation wins local publication if it was recorded before artifact publication.

Startup reconciles before accepting generation. Same verified activation resumes inspection; an unrelated invocation, ambiguous PID/cgroup, corrupt state or uncertain cleanup yields recovery_required and keeps admission closed. Heartbeats refresh the short-lived authority; receiver death closes new mutations after at most 20 seconds. No health-only adoption, PID-only stop, global interrupt, lease expiry deletion or automatic GPU reassignment is used.

For recovery, first preserve candidate state/config hashes, service show/journal, gate observations and GPU PIDs. Never delete a lease to make a test pass. If the recorded InvocationID exactly matches the owned candidate unit, an operator may stop THAT unit after choosing to abort its remaining work. Verify MainPID=0, inactive/failed status, an empty cgroup and no allocations on the media UUID. Restarting only the receiver then reconciles its durable owner/generation release. If identity differs or the state is corrupt, keep the reservation quarantined and restore a verified state snapshot only after manual reconciliation. No automated script kills an unknown process.

## Validation and rollback

Build/test on gpurack, with fake machine commands and disposable resource/job roots:
```sh
cargo fmt --check
cargo test --workspace --offline
cargo build -p rack_ai_media --offline
python -m pytest -q tests/media
git diff --check
```
Python test requirements are in `tests/media/requirements.txt`. Cross-repository tests additionally set `RACK_AI_TEST_REPO`. Browser tests use sandboxed Playwright; the approved rack installation is `/opt/rack-ai-pr24-browser/chrome-headless-shell` with the exact-path profile in `tests/media/rack-ai-pr24-browser.apparmor`. The global AppArmor user-namespace restriction remains enabled. A private copy of the missing libasound dependency is supplied through the test-only LD_LIBRARY_PATH.

Normal rollback: Finish any interactive session and wait for stopped; allow managed jobs to finish or explicitly cancel them. Confirm no media reservation and no owned process/GPU allocation. Then:
```sh
systemctl --user disable --now rack-ai-media-pr24.service
sudo tailscale serve --https=8443 off
sudo tailscale serve --https=8444 off
```
Stop only the optional Director candidate worker/web units. Preserve candidate database, media, state, release receipts and evidence. Never run whole-stack Compose or stop vLLM/ATHBA. Do not delete a reservation while its backend may remain resident. To roll code back, create ordinary forward commits or select a previously verified isolated release after a clean stop; never force-push, reset the live checkout or reverse additive Director migrations. The browser profile/copy can remain inert; removing it is optional after browser tests finish and is independent of GPU services.
