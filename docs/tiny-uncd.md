# tiny-uncd

`config/runtime/profiles/tiny-uncd.json` is the additive installation candidate
for Qwen3.5-4B Heretic Q6_K. The example configuration registers the exact logical
service/model alias `tiny-uncd`. This candidate is intentionally unqualified until
managed live qualification passes. No private configuration or deployment is
changed by adding these source files. `local-uncd` remains a separate profile.

## Configuration

- Model: `/srv/models/qwen3.5-4b-heretic/model-Q6_K.gguf`.
- SHA-256: `5693ee63d29a204b61b44e72ce0c34fde5762a9180bd87f694cdd1e5554c3215`.
- Executable: `/srv/rack-ai/runtimes/llama-cpp/llama-server`.
- SHA-256: `5cb679dce989d69935aca461984b1423f65e7d6ee80f5066a00c87f94b961e06`.
- Backend/driver: existing `llama_cpp` / `systemd`, loopback port 8023.
- Resource: existing `gpu-2060`, resolved through `config.devices` to its UUID;
  no CUDA ordinal is stored in this profile.
- Admission budget: 5700 MiB on the 6144 MiB RTX 2060; this is a reservation
  allowance, not a measured model allocation. Host allowance: 8192 MiB, 400% CPU.
- Context: 8192; published input ceiling: 7168; output ceiling: 1024 tokens.
- Exact input preflight: opt-in `input_token_preflight=true`, qualified for this
  frozen llama.cpp/model/template profile.
- One parallel slot, all GPU layers, temperature default 0, Jinja enabled.
- Thinking default: `--chat-template-kwargs '{"enable_thinking":false}'`.
- Startup: 300 s; inference/drain: 120 s; stop: 30 s; artifact verification: 900 s.

Context and generation limits use ordinary profile fields. Keep `--ctx-size`
and `context_tokens` aligned when tuning; the combined input/output ceilings
must fit the configured context. Existing protected limits are unchanged.

## Managed lifecycle and public API

The service uses ordinary RackAI discovery, qualification, atomic reservation,
priority, ownership, scoped access, FIFO invocation dispatch, draining,
preemption, failure recovery and release. It conflicts with `local-coder` on
`gpu-2060`; equal/lower priority cannot displace its incumbent. Higher priority
uses normal managed drain and cleanup before ownership transfer. Workloads on
other GPUs retain independent ownership. Foreign GPU processes are a preflight
failure, never targets for termination.

The managed lifecycle verifies the model alias and endpoint/process ownership.
The installed gpurack runtime additionally includes pre-existing ownership checks
for exact launch arguments, configured CUDA UUID and owned GPU allocation; this
change reuses the existing managed lifecycle. Cleanup must prove the generation's
systemd/process/GPU effects absent before releasing claims. No authority JSON
edits or alternative GPU lock mechanism are needed.

The existing default inactivity window is 1800 seconds, including shared public
reservation activity protection. Inspection and renewal do not count as model
activity. Existing warm residency is for Docker/vLLM only: this llama.cpp service
stops and releases its GPU normally rather than gaining a separate warm cache.
Lifecycle state/reasons, resource claims and invocation receipts use existing
RackAI reporting. Interaction diagnostics remain opt-in; `--log-disable` avoids
backend payload logging. No consumer-specific routing or API is introduced.

After qualification/deployment, ordinary authenticated clients discover and
reserve `tiny-uncd` through `POST /runtime/v1`, then send Chat Completions through
the returned scoped gateway. The `submit_work` inference payload accepts a plain
prompt and bounds; explicit JSON Schema and template controls use scoped Chat
Completions.
`response_format` is forwarded unchanged, including `json_object` and
`json_schema`. Schema enforcement is delegated to llama.cpp. The managed
adversarial schema test below exercises that constraint. RackAI does not
substitute prompt-only validation for that runtime constraint.

Chat Completions on llama.cpp additionally accepts:

```json
{
  "chat_template_kwargs": {"enable_thinking": false},
  "response_format": {"type": "json_object"}
}
```

Only the boolean `enable_thinking` option is admitted; unknown template keys or
malformed values fail closed. This extension is rejected on other backends and
protocols. The request body is forwarded intact, preserving explicit structured
output constraints and explicit thinking overrides. The profile supplies the
thinking/temperature defaults when those fields are omitted. Do not set a global
reasoning budget of zero, which would override an explicit thinking request.

## Verification and review boundary

Automated tests use disposable authority roots and controlled CPU/systemd/NVIDIA
fixtures. They prove registration, qualification gating, reservation lifecycle,
UUID selection, exact request forwarding and contention policy. They do not prove
real RTX 2060 allocation, schema generation, thinking suppression or throughput.

The installed runtime's `--help` confirms the configured all-layer, one-slot,
temperature and template-default flags. Model and executable hashes were checked
on gpurack. The installation template remains unqualified by convention; the
deployed profile is qualified with the live evidence below.

For another installation, add this profile to the private administrator configuration
without replacing any existing profile/device mapping, validate it, and use the
existing qualification principal/flow. Protect active reservations and defer any
conflicting test; do not stop an unrelated workload to qualify this service.
Record real RTX 2060 VRAM, default and explicit non-thinking JSON, an adversarial
JSON Schema constraint, chronological scene state, release and actual cleanup.

`tests/runtime/tiny_uncd_live.py` is an opt-in ordinary public-service smoke for
an already qualified deployment. It is not a deployment or qualification bypass.
It acquires only Low priority and releases only its own reservation. Synthetic
priority tests cover contention without touching active rack workloads. Real
priority/drain/recovery/idle tests require an approved otherwise-idle window and
must remain unclaimed until run.

Compare backend decode tokens/second and end-to-end tokens/second separately
against the owner's approximate 65 tokens/second standalone observation. Retain
actual token counts and times; never present configured VRAM or the standalone
reference as a managed measurement. Keep synthetic payloads and credentials in
private RAM and retain only sanitized metrics.

## Historical 4K qualification, 2026-10-17

The measurements in this section describe the earlier 4096-token profile;
the current 8192-token qualification is recorded below.

The deployed profile is qualified and uses only RTX 2060 UUID
`GPU-357ef569-8fac-7c7d-ee1c-51677efb174f`. Ordinary authenticated discovery,
public reservation, five-second stable ownership, inference, strict lower/equal
priority refusal, higher-priority preemption, release and owned process/GPU/claim
cleanup were verified. The unrelated manual model process on the 4060 Ti remained
unchanged. The temporary qualification credential was revoked after testing.

Measured model allocation: 3646 MiB (3.56 GiB), with about 3652 MiB total GPU use
after inference, within the RTX 2060's 6144 MiB. A bounded 512-token decode sample
measured 62.97 output tokens/second during decoding (3.13 percent below the owner's
approximate 65 reference), and 34.13 tokens/second end to end over 15.0012 seconds.
The benchmark intentionally ends at its token bound; complete JSON/schema tests
are separate and ended normally.

The live default temperature was 0. The default applied chat template exactly
matched explicit `enable_thinking=false` and differed from explicit true.
Default JSON and adversarial JSON Schema requests completed without visible
reasoning. This runtime does not report a separate reasoning-token usage counter;
that metric remains unavailable rather than being invented as zero.

Chronological extraction passed with explicit field definitions and separate
future activity/condition fields: removed coat, retained shirt/trousers/boots,
current kitchen/tea/waiting for rain, and future garden visit after the rain.
Two earlier probes failed strict assertions: vague circumstances became location,
then a free-text future intention omitted its condition. Both failures remain
retained; this bounded result does not establish broad extraction reliability.

Sanitized evidence: `/srv/rack-ai/evidence/tiny-uncd-20261017T143355Z/`.
`live-qualification-attempt3.json`, `template-defaults-proof.json` and
`public-priority-verified.json` record the proof. The original private priority
harness exited 1 because it tried to release two synthesized `unavailable-*`
decision identities; independent contract/state reconciliation proved that no
reservations existed for those refusals and both actual owners were released.
The original result is retained as `public-priority.json`, not rewritten.

Automated checks: clean publication source passed all 461 Rust tests serially,
plus four tiny lifecycle fixtures. Another 25 relevant runtime regressions passed
on the deployed-source baseline. Parallel workspace tests expose one unchanged
campaign shared-state failure that passes alone and in the full serial suite.
Strict Clippy retains 14 baseline lint failures; its baseline-category check,
formatting, build, config validation and diff checks pass. Full real 1800-second
inactivity expiry, receiver-crash recovery and sustained/running-call preemption
were not retested live; existing synthetic checks cover those lifecycle policies.

The immutable deployed binary includes the rack's pre-existing uncommitted
local-uncd readiness/residency work unchanged. This feature's publication contains
only tiny-uncd files and the generic template-control extension; it does not
publish that separate work or alter local-uncd's model/profile.


## Current 8K input preflight qualification, 2026-10-17

The current profile is `qwen3.5-4b-heretic-q6k-llama-5cb679dce989-8k-preflight-v2`:
8192 context, 7168 input and 1024 output tokens, with the same model, executable,
RTX 2060 UUID, one parallel slot and non-thinking/temperature defaults.
The increase follows measured token counts rather than serialized byte lengths.

Four supplied neutral scene fixtures counted 1901, 3712, 2540 and 480 input
tokens. The 3712-token fixture with 700 requested output tokens exceeds both the
old 3072 input ceiling and old 4096 combined context (4412 total); it fits the
new limits. All four runtime preflight counts exactly matched actual Chat
Completions `usage.prompt_tokens`, and all four schema-constrained generations
completed with `stop` and no visible reasoning. Messages, histories, trackers,
memories, schemas and requested output bounds remained intact.

Model allocation on the RTX 2060 measured 3784 MiB. A bounded 512-token decode
sample measured 63.2269 decode tokens/second and 52.8925 tokens/second end to end
(about 9.680 seconds). Stable ownership was held for five seconds and then
released with verified cleanup. Separate bounded conformance cases also matched
counts exactly: plain text 15, Unicode/roles 41, strict schema 18 and tools 268.
Evidence is retained under `/srv/rack-ai/evidence/input-preflight-20261017T160544Z/`.


The exact count uses the installed service's complete `/apply-template` parse,
then `/tokenize` with `add_special=true`, `parse_special=true` and
`with_pieces=false`. It counts the final template tokens, including applicable
role/tool/special-token overhead and the service thinking default. For this
qualified Qwen template, tested JSON Schema constraints add zero prompt tokens;
they constrain decoding instead. That result is not a rule for other templates.
The embedded template showed no detected clock/random/environment identifiers.

The matching original source checkout is unavailable. Qualification therefore
binds the installed model/runtime and observed count-to-usage equality, rather
than asserting that an arbitrary upstream checkout matches this binary. Requalify
on model, server implementation, template, default or supported-content changes.
Other services default to preflight disabled; `local-uncd` remains unchanged.

See [reservation-work.md](reservation-work.md#exact-input-preflight) for the
scoped bearer-authenticated preflight. It reports fit without inference, activity
renewal or an invocation ledger entry. Callers must check `fits`, preserve the
same payload and compare the returned member generation/profile binding before
submission. Normal inference admission is unchanged; preflight is not a token
reservation or a durable authorization to submit later.
