# PR36 Chatterbox Turbo handoff

## Status

Code implemented; **LIVE_QUALIFIED = false**. The checked-in profile remains unqualified.
No production service, container, authority record, CB code, or other repository was changed.

Live preflight on 2026-09-15 found:
- deployed receiver `native-session-boundary-current` has no `local-tts` profile;
- canonical `/srv/rack-ai/state/resources` contains an existing CB/ComfyUI
  `recovery_required` demand, ID `46032ea334e144037adfdd517b1af9f7`;
- its `gpu-4080-super` claim remains fenced; the qualification window is not quiescent;
- local-coder was idle/unloaded, so no healthy initial :8018 baseline was established.

Installing a replacement receiver while this unrelated recovery is unresolved is outside
a safe bounded 2060 proof. The retained preflight is `evidence/pr36/live-preflight.json`.
No fresh cold-load, warm RTF, GPU VRAM, or automatic live restoration result is claimed.
The earlier operator figures (9.04 s load, RTF 0.360, 3083 MiB) remain historical evidence.

## Architecture

`Backend::Chatterbox` and `Protocol::Speech` reuse PR35's existing authority, immutable
profile, systemd hosting, UUID checks, priority transition, Started persistence,
one-active dispatch, uncertainty, retirement and automatic restoration. No second scheduler.

The pinned zipapp loads one Chatterbox model at activation. Up to eight bounded worker HTTP
threads keep health responsive; a separate nonblocking lock allows exactly one generation.
Only loopback is accepted, and internal calls require the activation bearer. This is within
the existing trusted administrator-account boundary, not protection against arbitrary same-user code.
The worker refuses any CUDA mapping except the full qualified RTX 2060 UUID.

## Exact client contract

Use authenticated `POST /runtime/v1` discover/acquire/inspect/control unchanged.
Acquire tag `local-tts`, capabilities `["audio"]`, context_tokens `8192`, and authorized
priority `paramount`. Keep the reservation warm and renew explicitly. Ordinary CB acquisitions
require an administrator-qualified profile; the qualification operator can opt into an unqualified profile.
ATHBA policy is unchanged.

Once Ready, use the returned `gateway_path` on the receiver origin:

```http
POST <gateway_path>/speech
Authorization: Bearer <existing source credential>
Idempotency-Key: <stable caller identity>
Content-Type: application/json

{"text":"Wait... seriously? [gasp] It actually worked?","voice":"approved-example"}
```

Success: bounded binary `audio/wav`, PCM16 mono 24000 Hz, with `Cache-Control: no-store`.
No client temperature, paths, URLs, model, GPU, upload, or automatic retry fields.
`POST <gateway_path>/voices` with the same source authorization and `{}` returns
`{"voices":["id",...]}`. It is available only on a Ready, owned TTS reservation.
The capability, owner, profile, generation and all claims are verified; raw `infer` speech
requests are refused. Speech is not Chat Completions.

Bounds: 4096 UTF-8 bytes and 1000 Unicode characters, nonblank/no NUL; one valid registered
ID; 60-second inference limit; one active/accepted/uncertain speech per reservation;
60 seconds of output and 2,880,044 WAV bytes. Worker request body is at most 8192 bytes.
Busy/capacity errors are HTTP 429; invalid capability/state/voice/request/replay conflicts
are typed JSON errors with HTTP 409 on this scoped route. No backend diagnostic exposes a voice path.

An explicit key is mandatory. Equal identity and payload reconcile the original invocation;
changed payload conflicts. A disconnect does not release, cancel, or repeat generation.
A Started operation with unknown outcome stays Uncertain. Replaying it never synthesizes
again. Generation rotates on restoration; obtain the current capability before reconciling.
Result/reconcile return durable metadata; WAV retrieval uses the same speech request while
the reservation is Ready. After release there is no public standalone audio-download route.

## Binary retention

WAVs are stored once under the private canonical authority's `speech/<invocation-id>.wav`,
mode 0600 in a mode-0700 directory. Completion records only SHA-256/length and content type.
The bytes and parent directory are synced before durable completion; partial/failed writes
leave an uncertain invocation, never an automatic retry. Replay validates the full checksum
and canonical WAV header. Audio is not base64-encoded into authority JSON.

Admission reserves a conservative 256 MiB total across retained speech identities,
including uncertain ones: at most 93 lifetime calls per retained authority history.
This intentionally finite v1 allowance requires operator retention planning. No records or
audio are automatically deleted. Archive the entire quiescent authority and audio together
under PR35's retention procedure; do not erase idempotency history or rotate behind the same
identity scope. Configurable archival/retention is deferred.

## Worker environment and compatibility

Clean machine-local environment: `/home/tomp/rack-ai-runtime-pr36/worker`.
Python 3.11.16; chatterbox-tts 0.1.7; Torch 2.6.0+cu124; torchaudio 2.6.0;
NumPy 1.26.4; resemble-perth 1.0.1; setuptools 80.9.0.
The complete resolved environment is pinned in `runtimes/chatterbox/requirements.lock`.
The experimental Python 3.14 environment, driver, and vLLM environments were unchanged.

Perth's hidden ImportError was `ModuleNotFoundError: pkg_resources`. Pinning setuptools
80.9.0 restores the real `PerthImplicitWatermarker`, which successfully loaded its checkpoint.
No DummyWatermarker replacement, monkeypatch, or watermark bypass exists.
The worker fails startup if the ordinary watermarker is unavailable.

A CPU-only compatibility probe through installed upstream `norm_loudness` verified
float32 -> float32 with NumPy 1.26.4. Normal upstream reference normalization is enabled.
The probe is compatibility evidence, not GPU synthesis qualification.

Settings are server-owned: temperature 1.05, top_p 0.95, top_k 1000,
repetition_penalty 1.2. Exactly one `generate` call, no LLM rewriting/ranking/multiple takes.
Reference conditioning is reused while the selected voice content hash stays unchanged.

Upstream references:
- https://github.com/resemble-ai/chatterbox/blob/master/pyproject.toml
- https://github.com/resemble-ai/chatterbox/blob/master/src/chatterbox/tts_turbo.py

## Operator installation and voice procedure

1. Create a dedicated Python 3.11 environment and install `requirements.lock`; do not reuse
   the experiment or modify vLLM. This lock is Linux/CUDA-specific.
2. Place approved model files outside Git. Build a fresh zipapp with
   `python runtimes/chatterbox/package_worker.py --model ABS_MODEL_DIR --output ABS_WORKER.pyz`.
   The archive includes a manifest of the complete model file identities. Startup verifies
   it before loading CUDA. Pin the resulting archive SHA-256 as profile artifact identity;
   pin the Python executable too.
3. Configure the example `local-tts` profile with those paths/hashes, model directory,
   loopback port and full UUID. Keep unqualified until live acceptance. PR35 preflight hashes
   the worker archive before stopping a victim; model integrity failure at startup follows
   PR35 cleanup/restoration.
4. Store authorized PCM16 mono reference WAVs outside the repository. Supported reference rates
   are 16/22.05/24/44.1/48 kHz; duration must exceed 5 and be at most 30 seconds; max 6 MiB.
5. Write a mode-0600 registry, using canonical absolute root and relative files:
   `{"root":"/approved/voices","voices":{"approved":{"file":"reference.wav","sha256":"..."}}}`.
   Only administrator-controlled regular paths are accepted; traversal, symlink escape,
   malformed IDs, content hash mismatch, truncation and invalid audio fail closed.
6. Atomically replace the registry to add/change voices without restarting RackAI or reloading
   the model. Integrity-checked bytes, rather than a reopened caller path, are used for conditioning.
   Neither paths nor reference audio are returned by discovery.
7. Resolve the existing recovery through its owning operational procedure first. Do not clear
   claims or manually restart Docker Compose to make qualification succeed. Plan the receiver
   upgrade with the active deployment owner. Old receivers do not understand Speech records;
   retaining a PR36-compatible receiver/state reader is required after accepting speech.
8. On an installed PR36 receiver, `tools/qualification/tts.py` runs the bounded operator proof
   using a credential file, approved voice ID and report path. Its preflight must pass before
   effects. It acquires a Low coder baseline, preempts via Paramount TTS, issues two calls,
   captures worker timing/VRAM logs, releases TTS and checks automatic coder restoration.
   The restored coder reservation remains available for normal use until release/expiry.

## Verification

- `RACK_AI_RESOURCE_ROOT=$PWD/evidence/pr36/test-resources cargo test --workspace --offline`:
  **366 passed**, including doc-test suites.
- `cargo clippy -p rack_ai_runtime --all-targets --no-deps --offline -- -D warnings`: passed.
- Focused `tests/runtime/test_speech.py`: **3 passed**; ownership/authentication, unknown/unsafe
  voice, request bounds, one active call, replay/conflict, malformed WAV uncertainty,
  priority preemption/ties, one activation for repeated calls, failed startup cleanup,
  and automatic coder restoration are covered through real HTTP fixture processes.
- Worker/registry tests: **3 passed**, including health responsiveness during active synthesis.
- Runtime transport regression: 71 tests completed without failure before the documented
  300-second bound; the remaining teardown test passed in 2.23 s and all seven workspace-scope
  tests passed (plus two subtests) in 19.55 s. All 79 collected tests therefore passed across
  bounded runs; no single uninterrupted full-suite exit-zero is claimed.
- Final focused speech repeat: 3 passed in 17.006 s. Worker tests: 3 passed in 0.529 s.
- Independent semantic review: **ACCEPT** after correcting Speech response schema.
- `git diff --check`: passed.

An initial workspace invocation without the documented isolated resource root encountered
the live managed endpoint fence in three existing JCode fixtures; the corrected command
passed without production changes. An initial system-Python unittest run lacked pytest for
nine imported media fixtures; the documented existing test environment is used for the final
transport regression.

## Residual risks

Real GPU startup, normal watermarking during synthesis, selected-voice quality under pinned
dependencies, RTF and live automatic restoration remain unqualified. No production cutover
or multi-GPU claim is made. Independent code acceptance and fake-service restoration are
not substitutes for the required RTX 2060 proof.
