# Native ComfyUI session authority

Native `comfyui` reserves the GPU-backed service. It does not select an image model.
The current deployment maps this capability to gpu-4080-super; applications request
this logical capability, never a physical device or a raw backend address.

An authenticated CB source may acquire `comfyui` at Paramount. Once Ready, its matching
media principal uses the existing authenticated native ComfyUI origin. The native proxy
requires the current owned interactive session and verified canonical reservation. CB
retains operator=false: browser account administration and operator restart privileges
are not granted. Its native workflow payload is forwarded to ComfyUI unchanged.

## Separate optional image service

`local-image` remains the structured RackAI image-job API. It constructs its existing
fixed workflow and uses the configured checkpoint and SHA256. That checkpoint is checked
before each job dispatch, and a missing/mismatched file fails the job before submission.
The check no longer runs when the shared media receiver starts. Thus a broken optional
checkpoint does not prevent manual or application native sessions from starting.

Native runtime profile `model` is omitted. Native reservation responses omit model;
manual native lease metadata contains an empty model_ids list. The native media adapter
pins configuration after excluding the optional image profile. Executable, endpoint,
unit, credentials, resource root, device and lifecycle/security settings remain pinned.
The managed image path retains the complete configuration hash and checkpoint identity
binding. Process, boot/start identity, invocation, generation, canonical lease, GPU UUID,
private endpoint and gate checks are unchanged. Historical records are not rewritten.

Both capabilities contend for the same canonical resource. An incumbent Paramount
reservation denies an equal-priority conflicting request. Owning a native session does
not authorise the fixed-job API, and owning a fixed-job grant does not open native access.

Manual Start/Open/Finish and authenticated native HTTP/WebSocket access remain intact.
Admitted GPU work refreshes activity; inspection, polling and renewal do not. Independent
1800-second idle expiry, active-work protection and verified cleanup are unchanged.

## Correction paths

- crates/rack_ai_runtime/src/config.rs: optional serialized model; native-mode predicate.
- crates/rack_ai_runtime/src/validation.rs: native profiles need no model identity.
- crates/rack_ai_runtime/src/media.rs: checkpoint identity binding only for managed mode.
- crates/rack_ai_runtime/src/media_limits.rs: native config digest excludes image recipe;
  regression proves service configuration remains bound.
- crates/rack_ai_runtime/src/api.rs: omit native model identity from public records.
- crates/rack_ai_media/src/main.rs: remove global checkpoint startup prerequisite.
- crates/rack_ai_media/src/dispatch.rs: verify configured checkpoint before fixed-job execution.
- crates/rack_ai_media/src/profile.rs: checkpoint verification always verifies the file;
  disabling new image admission cannot bypass validation for an already accepted job.
- crates/rack_ai_media/src/activation.rs: omit model identity from native lease metadata.
- config/runtime/config.example.json: CB comfyui permission and native model omission.
- config/runtime/response.schema.json: model is optional for native responses.
- tests/runtime/scenario.py and test_shared_media.py: pin native service configuration.
- tests/runtime/test_native_boundary.py: CB policy, metadata, missing checkpoint,
  shared resource contention, recipe changes and explicit release.
- tests/media/test_native_boundary.py: missing/changed checkpoint does not block native
  startup/work; fixed jobs reject it before backend submission; manual release.

No Krea2 profile is created. No CB/ATHBA/NUC changes or client workflow installation.
No model, node, driver or inference backend was installed or replaced for this correction.

## Deployment and rollback

Evidence and before-images are retained under evidence/native-session-boundary-current.
Release binaries and exact deployment paths are recorded in deployed.json. Before-images
include both receiver configurations, runtime unit and previous media release link.
For rollback, stop admission, release active grants and verify clean owned shutdown and
empty claims. Retain current authority and invocation history. Restore matching previous
binaries/configurations/unit; never replace canonical state with an old snapshot. The
older implementation restores the unwanted checkpoint dependency, so rollback withdraws
this correction. Do not expose raw ports or enable legacy resident containers.

## Deterministic validation

- Five focused boundary/shared-media tests passed; the final checkpoint-verifier adjustment
  was followed by both crate test suites, rebuilt binaries and both damaged-file tests.
- 365 Rust workspace tests passed using an isolated test authority. The initial run saw
  the live review reservation through the default resource root, correctly triggering the
  raw-endpoint fence in three harness tests. No production fence or test was weakened.
- Runtime suite: 74 passed plus 2subtests; two short-lived fixture cases failed during
  concurrent suites (3-second idle before Ready; endpoint listener ambiguity). Both passed
  on the isolated targeted rerun, giving 76 passing runtime cases plus 2subtests.
- Media suite: 80 passed; two browser launches initially lacked libasound.so.2. With the
  existing /opt/rack-ai-pr24-browser browser and retained browser library directory, both
  passed. Total 82 media/browser cases passed, without installing dependencies or disabling
  the browser sandbox.
- Runtime Clippy with --no-deps and warnings denied passed. Dependency-inclusive Clippy
  reports pre-existing application-crate findings; unrelated lint code was not changed.
- Formatting and patch whitespace passed. Independent read-only semantic review accepted
  the exact correction diff after deterministic checks; its GPU reservation was released.

This records the initial failures as well as the passing targeted reruns; it does not claim
that the first full-suite invocation was entirely green.


## Production result

Deployed and complete. Manual Start/Open/Finish and CB Paramount native sessions both
ran real 512x512 images with cyberrealisticPony_v170.safetensors while the optional
local-image checkpoint path pointed to a nonexistent file. Native PNG dimensions and
SHA256 evidence are retained. Native reservation metadata omitted model identity.
A competing Paramount local-image acquisition was denied while CB owned the GPU.
CB native expiry completed with idle_timeout under the temporary 60-second observation
policy; production was restored to 1800 seconds. With the original media configuration
restored, local-image completed its separate Ragnarok job with the expected workflow
and checkpoint hash, then explicitly released. Claims and manual leases are empty.
Both receivers are active; native ComfyUI is stopped until its next authorised session.
The raw ComfyUI listener was verified loopback-only during the live run. No other
networking, idle, source ceiling, client repository or model-library changes were made.

No further tests or implementation are planned. No commit or push was performed.
