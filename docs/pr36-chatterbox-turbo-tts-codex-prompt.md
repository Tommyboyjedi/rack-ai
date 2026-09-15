# PR36 Codex prompt — Chatterbox Turbo TTS on the RTX 2060

Implement this PR on branch `feat/pr36-chatterbox-turbo-tts`.

This branch is intentionally stacked on PR35 (`design/priority-runtime-reservations`) and must reuse PR35's runtime reservation, priority, preemption, restoration, ownership, recovery, bounded-hosting and authentication machinery. Do not reimplement a parallel scheduler or reservation authority.

## Mandatory first steps

Before editing anything, read and obey:

1. `coding_principles.MD`
2. `AGENTS.md`
3. `agent.MD`
4. `docs/engineering-contract.md`
5. `docs/generic-bounded-workspace-execution.md`
6. `docs/runtime-public-contract.md`
7. `docs/priority-runtime-reservations.md`
8. the current PR description

Inspect the existing PR35 runtime implementation and tests before designing this change, especially `crates/rack_ai_runtime`, `config/runtime`, and `tests/runtime`.

Keep this change narrowly scoped. Do not modify ATHBA, CB, Music Director, or any other repository. Do not merge the PR.

## Goal

Add Chatterbox Turbo as a first-class RackAI managed `audio` runtime profile that can run on the rack's RTX 2060 and temporarily replace `local-coder` under the existing PR35 priority reservation rules.

The intended operational flow is:

```text
normal:
  gpu-2060 -> local-coder

CB requests qualified TTS at Paramount:
  acquire local TTS profile
  -> PR35 reservation authority handles conflict/preemption
  -> local-coder drains/stops through the existing managed-hosting lifecycle
  -> verify gpu-2060 is free and is the exact configured UUID
  -> start Chatterbox Turbo
  -> load model once
  -> reservation becomes Ready

while reservation is held:
  synthesize one or many speech requests without reloading the model

release / expiry / higher-priority transition:
  -> stop Chatterbox cleanly
  -> verify owned process/GPU allocations are gone
  -> release ownership
  -> PR35 restoration machinery restores local-coder when appropriate
  -> local-coder becomes healthy on :8018 again
```

Do not hard-code a special `docker compose stop/start vllm-coder` workflow if PR35's managed-hosting/preemption/restoration path can own this correctly. The runtime authority, not Chatterbox, owns switching.

## Live rack facts and qualification evidence

Physical RTX 2060:

```text
resource: gpu-2060
UUID: GPU-357ef569-8fac-7c7d-ee1c-51677efb174f
VRAM: 6144 MiB reported by nvidia-smi
```

Use the full GPU UUID. Never depend on CUDA/nvidia-smi numeric ordinals; they have already been observed to disagree on this host.

A direct local qualification of `ResembleAI/chatterbox-turbo` on this exact RTX 2060 produced:

```text
model load time:   ~9.04 s
speech duration:   14.92 s
generation time:   5.37 s
realtime speed:    2.78x
RTF:               0.360
peak Torch VRAM:   ~3083 MiB
sample rate:       24000 Hz
```

The operator listened to the generated speech and explicitly accepted Chatterbox Turbo as materially better than the previous CPU TTS. A single-generation configuration with `temperature=1.05` was also listened to and accepted.

Qualified initial synthesis settings are therefore server-owned defaults:

```text
temperature        = 1.05
top_p              = 0.95
top_k              = 1000
repetition_penalty = 1.2
sample_rate        = 24000 Hz
```

Speed is a primary requirement. Generate one take only. Do not add automatic multi-take generation, ranking, or LLM rewriting to this PR.

## Runtime model

Add a logical RackAI tag/profile, preferably `local-tts` unless the existing naming conventions strongly justify another generic name.

Requirements:

- capability: `audio`
- backend: a first-class Chatterbox/TTS backend, not a fake vLLM/chat model
- physical resource: `gpu-2060`
- exact UUID enforced through existing device configuration
- qualified profile can be made available to the existing `cb` source at `paramount`
- ordinary source ceilings and incumbent-wins-ties semantics remain unchanged
- no client chooses a concrete GPU, process, executable path, model file path, or arbitrary voice file path
- the model stays resident for the lifetime of the reservation rather than loading once per utterance

Do not weaken PR35 priority rules. A Paramount TTS request may preempt a lower-priority `local-coder` reservation if the existing authority permits it. It must not steal an equal-priority incumbent; existing incumbent-wins-ties behavior remains authoritative.

## TTS service implementation

Keep the actual Chatterbox inference worker small and isolated. Python is appropriate for the model runtime; do not port Chatterbox inference into Rust.

Use a dedicated pinned Python environment/runtime suitable for Chatterbox Turbo. Prefer Python 3.11 unless current upstream compatibility evidence justifies another supported version. Do not use the experimental `~/chatterbox-test` venv as the production runtime.

Do not commit model weights, Hugging Face cache content, generated audio, real voice references, secrets, or machine-local virtual environments.

The service must:

- bind loopback only
- load `ChatterboxTurboTTS` once at service startup
- expose a minimal health/readiness check
- accept bounded synthesis requests from the RackAI-owned adapter only
- return valid mono 24 kHz WAV audio
- apply the server-owned qualified defaults above
- accept expressive Chatterbox text including supported paralinguistic tags such as `[sigh]`, `[gasp]`, `[chuckle]`, `[laugh]`, `[cough]`, `[groan]`, `[sniff]`, `[shush]`, and `[clear throat]`
- perform only one synthesis per request
- have explicit request, generation and response-size/time bounds

Do not expose raw Chatterbox directly to remote clients. The RackAI authenticated runtime remains the security and reservation boundary.

## Dependency compatibility

The exploratory Python 3.14 environment exposed two upstream/dependency issues:

1. `perth.PerthImplicitWatermarker` resolved to `None`, requiring a temporary `DummyWatermarker` substitution for the experiment.
2. reference-audio loudness normalization produced a Float/Double mismatch, so the experiment used `norm_loudness=False`.

Do not blindly copy these monkeypatches into production.

During implementation/qualification:

- establish a pinned compatible Chatterbox/Perth/NumPy/Torch environment on the rack;
- preserve the normal Chatterbox watermark path if a compatible upstream combination supports it;
- if a compatibility workaround remains genuinely required, isolate it behind a small explicit adapter, document why, and test it;
- `norm_loudness=False` is acceptable as a bounded compatibility setting if still required, but it must be intentional/configured rather than an unexplained patch.

Do not modify the host NVIDIA driver or the existing vLLM environments.

## Voice registry

Voice cloning is required, but remote callers must select a registered voice ID, never supply an arbitrary filesystem path.

Add an administrator-owned voice registry concept such as:

```text
voice id -> approved local reference WAV + integrity metadata
```

The exact storage/config representation should fit the PR35 runtime architecture. Machine-local voice files should live outside the public repository. Checked-in configuration may contain synthetic/example paths and hashes only.

Requirements:

- reject unknown voice IDs
- normalize/validate configured paths using existing RackAI path-safety principles
- do not allow request payloads to contain `audio_prompt_path`
- do not expose local file paths in public responses
- bound reference audio format/size/duration as practical
- verify configured reference identity/integrity before use or at profile validation/startup as architecturally appropriate
- permit more than one registered voice without restarting RackAI

For this PR the client request needs only a voice ID and text. Do not add a general voice-upload product.

## Public contract

Reuse `/runtime/v1` reservation lifecycle for discover/acquire/inspect/control. Do not create a second reservation system.

Add the smallest typed audio/TTS protocol needed for low-latency speech. Do not pretend WAV synthesis is OpenAI Chat Completions.

Preferred shape: when a `local-tts` reservation is Ready, expose an owner/generation-scoped audio gateway capability analogous to PR35's existing scoped gateway. A speech call should conceptually be:

```http
POST <scoped-audio-gateway>/speech
Authorization: existing RackAI source credential / scoped capability as appropriate
Idempotency-Key: caller-owned stable invocation identity when supported by the chosen contract
Content-Type: application/json

{
  "text": "Wait... seriously? [gasp] It actually worked?",
  "voice": "voice-id"
}
```

and return `audio/wav` directly when successful, because first-audio/result latency matters. Do not base64-wrap large WAV data into durable JSON unless the existing architecture clearly makes that safer without materially harming latency.

The exact route can differ if a cleaner fit exists in PR35, but maintain these invariants:

- authenticated owner only
- reservation ID/generation/profile binding
- only while reservation is Ready and owns all required resources
- bounded text bytes/characters
- bounded synthesis duration/response bytes
- one active synthesis per reservation initially unless measured evidence supports safe concurrency
- no arbitrary backend URL or voice path from callers
- deterministic typed validation errors
- transport uncertainty/replay semantics appropriate for pure TTS inference
- no silent duplicate synthesis when an explicit idempotency identity is replayed
- no raw worker port exposed remotely

Because TTS is pure inference with no external side effect, do not overbuild a media-job/artifact system unless necessary. Prefer a small synchronous bounded speech gateway while the reservation is held.

Also extend discovery/profile metadata sufficiently that an authorized client can determine that `local-tts` is available and is an `audio` capability. Voice discovery may be a small authenticated read-only operation/route if it cannot fit cleanly in existing discover metadata.

## Source policy

Update checked-in example/synthetic configuration so:

- `cb` is allowed to discover/acquire `local-tts` at `paramount`
- qualification operator can qualify it
- no unrelated source receives expanded authority accidentally
- ATHBA ceilings remain unchanged

No real credential material may be committed or printed.

## Lifecycle and restoration

This is the most important integration behavior.

Use PR35's existing durable preemption/restoration mechanisms rather than a TTS-specific shadow state machine.

Prove that:

- the TTS reservation owns `gpu-2060` before Chatterbox is started;
- the exact physical UUID is verified;
- lower-priority local-coder can be drained/stopped safely by ordinary priority transition;
- Chatterbox startup failure does not leave false Ready state or a second conflicting process;
- release/expiry/cancel stops only the owned Chatterbox activation;
- ownership is not released until owned process/GPU cleanup is confirmed;
- cleanup uncertainty becomes the existing recovery/fenced state rather than releasing the GPU optimistically;
- local-coder restoration uses existing frozen profile/generation rules and becomes healthy again on `127.0.0.1:8018`;
- receiver restart/reconciliation does not lose an active TTS ownership claim;
- a caller disconnect does not implicitly release the TTS session/reservation.

Do not make one speech request equal one runtime start/stop cycle.

## Performance intent

The measured cold load is ~9 seconds and synthesis is ~2.78x real time. Preserve the architecture needed to exploit that:

- model loads once per reservation activation
- repeated speech calls reuse the resident model
- no automatic four-take generation
- no LLM preprocessing in the hot path
- no unnecessary disk roundtrip before returning WAV unless required for correctness

Live qualification should record cold-start time, warm synthesis time, RTF, peak VRAM and restoration time. Treat the observed `RTF 0.360` as evidence/benchmark, not a brittle unit-test assertion. A live RTF materially worse than 0.5 should be called out for review rather than hidden.

## Bounded request defaults

Choose conservative typed limits and document them. The implementation may refine exact values based on Chatterbox behavior, but initial intent is roughly:

- text: enough for short dialogue/narration, not unbounded prose (for example <= 4096 UTF-8 bytes and preferably a smaller documented character limit for low latency)
- one voice ID
- one synthesis at a time per reservation
- finite generation timeout
- finite maximum WAV bytes/audio duration
- no caller-controlled temperature/top-p/top-k/repetition penalty in v1

The caller should control expressive delivery primarily through text punctuation, wording and Chatterbox's supported tags plus the selected registered reference voice.

## Tests — focused, not sprawling

Add the smallest useful set of tests that prove the new behavior and dangerous failure paths. Reuse PR35 fixtures/fakes where possible.

At minimum cover:

1. config/profile validation for an `audio` Chatterbox profile;
2. source policy allows `cb`/qualification operator but not unrelated authority expansion;
3. unknown/unsafe voice ID fails closed;
4. TTS cannot dispatch without a Ready reservation owning `gpu-2060`;
5. lower-priority local-coder conflict uses ordinary PR35 preemption path;
6. equal-priority incumbent still wins ties;
7. Chatterbox startup/health failure does not release or falsely mark Ready;
8. release/cleanup restores the prior local-coder profile through PR35 restoration;
9. request bounds and one-active-synthesis rule;
10. valid fake TTS response is returned as bounded WAV and replay/idempotency behavior is explicit.

Do not add hundreds of superficial tests. Prefer focused behavioral coverage around ownership, lifecycle, restoration, validation and the public contract.

## Live qualification on gpurack

After deterministic tests pass, perform a bounded live qualification using the existing authorized rack environment. Do not modify the live deployment blindly; inspect current processes/config first and use PR35's documented qualification/cutover discipline.

The live proof must establish, in order:

1. `local-coder` is healthy on `127.0.0.1:8018` and occupies the exact RTX 2060.
2. An authorized `cb`/qualification request acquires `local-tts` at Paramount.
3. RackAI records and executes the ordinary priority transition; local-coder is drained/stopped safely.
4. Chatterbox starts only after ownership and exact UUID are verified.
5. The TTS reservation reaches Ready.
6. A registered reference voice synthesizes one expressive sample using the qualified defaults (`temperature=1.05`, `top_p=0.95`, `top_k=1000`, `repetition_penalty=1.2`).
7. The response is a valid mono 24 kHz WAV.
8. Record model cold-load/start-to-ready time, audio duration, generation time, RTF and peak VRAM. Compare honestly with the exploratory 9.04 s / RTF 0.360 / ~3083 MiB evidence.
9. A second warm synthesis during the same reservation proves the model was not reloaded.
10. Release the TTS reservation.
11. Chatterbox exits and no owned GPU allocation remains.
12. PR35 restoration automatically returns `local-coder` to healthy `:8018` service without an out-of-band manual compose restart.
13. No stale claim/lease/recovery remnant remains.

Use only a voice/reference for which the operator has permission. Do not commit or print the reference audio.

If the live environment cannot safely prove a step, stop and report the precise blocker. Do not weaken the ownership or recovery boundary merely to complete the demo.

## Documentation / handoff

Add a concise PR36 qualification/handoff document recording:

- architecture added
- exact public contract/routes
- server-owned synthesis defaults
- dependency/runtime versions and compatibility decisions
- live RTX 2060 measurements
- voice-registry operator procedure without exposing private voice material
- preemption/restoration proof
- exact test commands/results
- residual risks or deferred work

Update `docs/runtime-public-contract.md` only where the public runtime contract genuinely changes.

Do not add CB integration code in this PR. The finished RackAI contract should be sufficient for a later small CB change to discover/acquire `local-tts`, keep the reservation warm for a conversation/burst, send speech calls, and release it.

## Definition of done

This PR is done when RackAI can truthfully claim:

> An authorized client can acquire a qualified `audio`/Chatterbox Turbo runtime on the RTX 2060 using the existing PR35 priority reservation authority; RackAI can safely preempt a lower-priority local-coder, keep Chatterbox resident for repeated low-latency speech using registered voices, return bounded 24 kHz WAV output, and on release restore local-coder automatically with durable ownership/recovery evidence.

Before declaring completion run the repository-mandated checks, including:

```bash
cargo test --workspace --offline
git status
git diff
git diff --check
```

plus focused runtime/TTS tests and the live qualification described above where safe and authorized.

Do not merge the PR. Commit/push the implementation to `feat/pr36-chatterbox-turbo-tts` and report the resulting commit SHA, tests, live measurements, exact API contract and any remaining blocker.