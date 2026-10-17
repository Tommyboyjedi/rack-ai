# Dual-GPU trial inference services

RackAI adds logical services `dual-brain` and `dual-worker` through its existing
authenticated reservation contract. Select one per session/run. Neither replaces
an existing default or consumer route. Source profiles remain `qualified:false`;
private deployment qualification follows managed functional validation.

## Configuration

| Setting | `dual-brain` | `dual-worker` |
| --- | --- | --- |
| Model | Qwen3.8-Flash-Next Unsloth UD-IQ4_XS, three GGUF shards | Nex-N2.5-mini Q5_K_M |
| Backend | `strata` | `llama_cpp` |
| Actual model identifier | `qwen3.8-flash-next-unsloth-ud-iq4_xs` | `nex-n2.5-mini` |
| Runtime | Prepared `/home/tomp/strata-test` server/engine plus RackAI adapter | `/srv/rack-ai/runtimes/llama-cpp/llama-server` |
| Private listener | `127.0.0.1:8098` | `127.0.0.1:8096` |
| Context / input / output ceilings | 131072 / 65536 / 65536 tokens | 131072 / 65536 / 65536 tokens |
| Additional context reserve | 8 tokens, matching installed Strata admission | 0 |
| Parallel slots | 1 | 1 |
| Per-GPU memory allowance | 15800 MiB on each card | 15800 MiB on each card |
| Host-memory allowance | 55000 MiB | 32768 MiB |
| CPU quota / affinity | 3200% / existing runtime placement | 1600% / CPUs 0-15 |
| Startup / inference / drain / stop | 900 / 3600 / 3600 / 60 seconds | 600 / 3600 / 3600 / 60 seconds |
| Asset verification timeout | 900 seconds | 900 seconds |

The 15800-MiB allowances leave room for NVIDIA-reserved memory: free VRAM can
be lower than reported total capacity even while idle. GPU ownership remains
exclusive for the entire pair. These are finite accounting allowances, not measured allocation or
performance promises. Small calls should request small explicit output bounds.
Clients allow for documented queue wait plus inference. The protected response
limit remains 3145728 bytes; request bodies remain bounded to 1048576 bytes.
UTF-8 content, schemas, history and buffered SSE must fit byte bounds too.
Existing profiles and their approved limits remain unchanged.

Both atomically require the complete exclusive resource set in this order:

1. `gpu-4080-super`: `GPU-f9435bc0-a243-ad20-8b8b-166ab076e80b`.
2. `gpu-4060ti`: `GPU-042e18f2-bf9f-c8f6-6975-6f25b15ac71c`.

RackAI supplies physical UUIDs as `CUDA_VISIBLE_DEVICES`; logical CUDA0/CUDA1
identify the 4080 SUPER/4060 Ti. Neither reserves the 2060. Independent GPU work
still needs host-memory admission. These profiles cannot coexist on the shared
cards. Lower/equal priority cannot displace an incumbent. Higher priority uses
normal preempting/drain/verified-cleanup transfer. Preemption is terminal;
clients explicitly reacquire and resubmit wanted work. There is no automatic
restoration or invocation replay.

Systemd owns the real executable, generation, endpoint and complete cgroup,
including Strata's Python parent and engine child. Readiness verifies loaded
model/context and the owned engine, not just HTTP success. Strata restart/reload
within an activation is fenced. Release proves owned processes/GPU allocations
absent before freeing claims. Existing inactivity expiry applies; polling and
counting do not renew workload activity. These systemd backends do not gain
Docker/vLLM warm residency. Activation performs no setup, download or rebuild.

Profiles pin model/executable hashes and launch controls. Strata also pins all
three shards, prepared pack/tokenizer, MTP runtime, expert profile, engine/build,
Python sources, adapter/config and required dependencies/libraries. Nex pins its
model and bundled libraries. Verification runs at activation, not per inference.
Asset changes require a new qualified profile/version. See profile JSON for
exact paths/hashes. Original models, experimental runtimes and consumer code
remain intact.

## Chat, tools, structured output and reasoning

Both publish text reasoning/coding and Chat Completions. They advertise no image,
audio, Responses or autonomous tools. Supported request content is a string per
message, with null/omitted content allowed only for a valid assistant function-tool
turn. Even text-only content-part arrays are rejected by the mandatory counter;
use string content. Use the member's returned `model` in Chat
Completions; the logical service tag is a separate identity. System/user/assistant/
tool messages, tool definitions/choices, IDs, argument strings, results and
reasoning history pass through the scoped gateway. Assistant function-tool turns
may omit `content` or set it null. Tool results carry `role:"tool"`, matching
`tool_call_id` and string content. RackAI never executes model tools or reasoning.

Both retain their working templates and high-reasoning default. For concise
extraction/JSON on **dual-worker**, use the native template variables below and
**omit the top-level `reasoning_effort` field**:

```json
{"chat_template_kwargs":{"enable_thinking":false,"reasoning_effort":"none"}}
```

Nex's embedded template branches on `reasoning_effort`, not `enable_thinking`.
The installed llama.cpp parser handles top-level `reasoning_effort:"none"` by
setting its thinking boolean false but erasing the template's effort variable;
Nex then renders an open thinking prefix. A short answer with no observed reasoning
under that old recipe did not establish thinking was disabled. Nested effort
`"none"` without that top-level override preserves the actual closed prefix.
This is a request-level native control; bodies are forwarded without normalization.

For **dual-brain**, use top-level `"reasoning_effort":"none"` or boolean
`"chat_template_kwargs":{"enable_thinking":false}`. Strata's admitted template
kwargs remain boolean-only. Llama.cpp additionally accepts nested effort
`none`/`low`/`medium`/`high`, alone or alongside the boolean; unknown keys, explicit
nulls and wrong types fail closed. Existing tiny-uncd boolean controls remain
unchanged. Do not assume one template control applies to every model.
Nex's tested defaults are temperature 0.7, top_p 0.95, top_k 40, min_p 0. Explicit
supported sampling/reasoning controls are preserved. `top_k` is a nonnegative
signed 32-bit integer; `min_p` is numeric in [0,1].

Where supplied, reasoning remains separate in `message.reasoning_content`;
preserve it when continuing that backend's history. Reasoning counts within the
output allowance. Check usage and `finish_reason`: a length-limited response can
lack a complete answer, JSON document or tool argument set. No hidden semantic
retry or silent message truncation is introduced.

Nex uses llama.cpp's runtime JSON/schema constraints. Strata uses its installed
**prompt-and-validate** implementation, not constrained decoding: format
instructions contribute prompt tokens; one generation is validated and errors
fail closed. Strata rejects structured `response_format` combined with tools.
Use separate tool and structured-output turns. Clients still validate application
meaning and tool arguments before acting.

RackAI currently buffers backend SSE until its completion marker, then returns
`text/event-stream`. Events, usage and finish metadata are preserved within the
response bound; delivery is **not incremental**. Closing HTTP does not prove
computation stopped. Existing `cancel_work`/`cancel` cancel queued work and record
intent for running work while bounded execution drains and retains late evidence.

## Exact input accounting

POST the complete inference body to `member.input_preflight.path`, normally
`<member.gateway_path>/chat/completions/preflight`, on the same receiver origin.
The owner's bearer credential and current scoped capability are required.
The response is `rack-ai/input-preflight/v1` directly, without a runtime `result`
wrapper. Counting generates no tokens, renews no activity and creates no invocation.

Check `service`, `model`, member `reservation_id`, `generation`, `profile_hash`,
`profile_version`, limits, requested output and `fits`. A member ID may differ
from the public reservation ID. Counts include actual rendered role, tool,
template, special-token framing and thinking defaults. Nex uses hosted
`/apply-template` then `/tokenize`; Strata uses the installed OpenAI parser, schema
preparation and tokenizer, including forced tool prefixes. Do not copy tiny-uncd's
observed zero-schema-overhead result to another template/runtime.

`total_reserved_tokens = input_tokens + requested_output_tokens`. Strata's
`context_reserve_tokens:8` is separate, requiring
`total_reserved_tokens + 8 <= context_tokens`. Both individual ceilings are 65536.
Less output does not raise the input ceiling. Counting binds the frozen profile
and current activation; it grants no authority after release/preemption. Never
substitute serialized-byte estimates or silently trim history to obtain a fit.

Both new profiles set `enforce_input_limits:true` and advertise
`input_preflight.enforcement:"enforced_before_generation"`. RackAI repeats exact
counting of the actual admitted body immediately before generation. Input/context
rejection or a counting failure returns a typed `input_admission_*` error and a
durable Failed invocation; it does not generate or silently truncate. Counting and
network operations have finite deadlines. An idempotent replay returns the
original result/failure without recounting or invoking the model again. Preflight
still lets clients reject an oversized request before creating an invocation.
Other services retain their established admission behavior.

## Public API example

Run on gpurack, or set the configured receiver origin. Supply an ordinary bearer
credential in `RACKAI_TOKEN`. This Bash example needs curl, jq and Python3. Scoped
URLs are secret capabilities; never log them. The private temporary files are
removed only after release is confirmed. `submit_work` remains suitable for plain
bounded prompts; full schema/tool controls use scoped Chat Completions.

```bash
set -euo pipefail
export RACKAI_BASE_URL=http://127.0.0.1:8095
export RACKAI_SERVICE=dual-worker  # or dual-brain
: "${RACKAI_TOKEN:?Set an ordinary RackAI bearer credential}"
umask 077
state_dir=$(mktemp -d)
reservation_id=''
runtime() {
  curl --silent --show-error --fail-with-body --max-time 30 --noproxy '*' \
    -H "Authorization: Bearer $RACKAI_TOKEN" -H 'Content-Type: application/json' \
    --data-binary "$1" "$RACKAI_BASE_URL/runtime/v1"
}
cleanup() {
  if [ -n "$reservation_id" ]; then
    runtime "$(jq -nc --arg id "$reservation_id" \
      '{operation:"release_reservation",reservation_id:$id}')" > "$state_dir/release.json" || true
  fi
}
trap cleanup EXIT
runtime '{"operation":"discover"}' | jq '.result.tags[] | select(.tag=="dual-brain" or .tag=="dual-worker") | {tag,qualified,context_tokens,max_input_tokens,max_output_tokens,input_preflight}'
acquisition_id=$(python3 -c 'import uuid; print(uuid.uuid4().hex)')
runtime "$(jq -nc --arg id "$acquisition_id" --arg service "$RACKAI_SERVICE" \
  '{operation:"reserve",request:{acquisition_id:$id,work_id:$id,services:[$service],priority:"low",ttl_seconds:7200}}')" > "$state_dir/reservation.json"
reservation_id=$(jq -er '.result.id' "$state_dir/reservation.json")
deadline=$((SECONDS + 1200))
while (( SECONDS < deadline )); do
  runtime "$(jq -nc --arg id "$reservation_id" \
    '{operation:"inspect_reservation",reservation_id:$id}')" > "$state_dir/reservation.json"
  state=$(jq -r '.result.state' "$state_dir/reservation.json")
  [ "$state" != preparing ] && break
  sleep 1
done
[ "$state" = ready ] || { printf 'Reservation state: %s\n' "$state"; exit 1; }
jq -e --arg s "$RACKAI_SERVICE" '.result.services[$s]' "$state_dir/reservation.json" > "$state_dir/member.json"
model=$(jq -er '.model' "$state_dir/member.json")
gateway=$(jq -er '.gateway_path' "$state_dir/member.json")
jq -nc --arg model "$model" --arg service "$RACKAI_SERVICE" '{model:$model,messages:[{role:"system",content:"Return only a JSON object."},{role:"user",content:"What is two plus three? Use a numeric answer field."}],max_tokens:128,temperature:0,response_format:{type:"json_object"},chat_template_kwargs:{enable_thinking:false}} + (if $service=="dual-worker" then {chat_template_kwargs:{enable_thinking:false,reasoning_effort:"none"}} else {reasoning_effort:"none"} end)' > "$state_dir/chat.json"
curl --silent --show-error --fail-with-body --max-time 30 --noproxy '*' \
  -H "Authorization: Bearer $RACKAI_TOKEN" -H 'Content-Type: application/json' \
  --data-binary @"$state_dir/chat.json" \
  "$RACKAI_BASE_URL$gateway/chat/completions/preflight" > "$state_dir/preflight.json"
jq -e --arg service "$RACKAI_SERVICE" --slurpfile m "$state_dir/member.json" \
  '.schema=="rack-ai/input-preflight/v1" and .fits==true and .service==$service and .model==$m[0].model and .reservation_id==$m[0].id and .generation==$m[0].generation and .profile_hash==$m[0].profile_hash and .profile_version==$m[0].profile_version and .max_input_tokens==$m[0].max_input_tokens and .max_output_tokens==$m[0].max_output_tokens and .context_tokens==$m[0].context_tokens and (.context_reserve_tokens//0)==($m[0].context_reserve_tokens//0) and .requested_output_tokens==128 and .input_tokens<=.max_input_tokens and .requested_output_tokens<=.max_output_tokens and .total_reserved_tokens==(.input_tokens+.requested_output_tokens) and (.total_reserved_tokens+(.context_reserve_tokens//0))<=.context_tokens' \
  "$state_dir/preflight.json" >/dev/null
curl --silent --show-error --fail-with-body --max-time 3905 --noproxy '*' \
  -H "Authorization: Bearer $RACKAI_TOKEN" -H 'Content-Type: application/json' \
  -H "Idempotency-Key: $acquisition_id-chat" --data-binary @"$state_dir/chat.json" \
  "$RACKAI_BASE_URL$gateway/chat/completions" > "$state_dir/result.json"
jq '{model,choices,usage,timings}' "$state_dir/result.json"
cleanup
release_id=$reservation_id
deadline=$((SECONDS + 180))
while (( SECONDS < deadline )); do
  runtime "$(jq -nc --arg id "$release_id" \
    '{operation:"inspect_reservation",reservation_id:$id}')" > "$state_dir/release.json"
  state=$(jq -r '.result.state' "$state_dir/release.json")
  [ "$state" = released ] && break
  [ "$state" = releasing ] || { printf 'Release state: %s\n' "$state"; exit 1; }
  sleep 1
done
[ "$state" = released ] || { printf 'Cleanup not yet verified\n'; exit 1; }
reservation_id=''
trap - EXIT
rm -f -- "$state_dir/reservation.json" "$state_dir/member.json" "$state_dir/chat.json" \
  "$state_dir/preflight.json" "$state_dir/result.json" "$state_dir/release.json"
rmdir -- "$state_dir"
```

Unavailable means refused now, not queued inference. Follow bounded retry guidance
with a fresh explicit acquisition when desired. An exact acquisition replay returns
its original receipt; inspect current state. If acquisition transport fails,
reconcile that same identity before making a new acquisition. Stable idempotency keys reconcile
calls; distinct deliberate calls require distinct keys.

For a tool round trip, send a small `type:"function"` schema, obtain an assistant
`finish_reason:"tool_calls"`, parse/validate its argument string, and append that
exact assistant message plus a deterministic stub result:

```json
{"role":"tool","tool_call_id":"<returned call id>","content":"{\"value\":5}"}
```

Then send the continued messages for the final assistant answer, with a fresh
idempotency key and exact preflight. Reasoning remains message metadata, never a
tool request. Client conversation state/tool execution stay outside RackAI.
For JSON Schema, replace the example's format with:

```json
{"type":"json_schema","json_schema":{"name":"answer","strict":true,"schema":{"type":"object","properties":{"answer":{"type":"integer"}},"required":["answer"],"additionalProperties":false}}}
```

## Qualification and rollback

Focused fixtures cover launch integrity, truthful readiness, lifecycle,
forwarding, exact token/context admission, atomic contention, stale access and
cleanup. Adapter tests exercise counting and engine fences without model inference.
CI includes formatting, workspace build/tests, existing regressions and
`test_dual_services.py` / `test_strata_adapter.py`. Live qualification uses ordinary
scoped calls with sequential owners, short chat/tool/schema/SSE cases, bounded
speed samples, release, reacquisition and actual process/GPU cleanup. Retain
failures; do not hide repeated model calls.

Operator reference throughput is approximately 45 tokens/s for Strata and 100 for
Nex with short input. These are observations, not SLAs or full-window measurements.
Record backend decode rate separately from client-visible timing. Qualification
covers functional integration and observed workloads; full-window performance,
extended aptitude, quantisation comparisons and MusicDirector suitability remain
unqualified.

The receiver is user `rack-ai-runtime.service`, configured at
`/srv/rack-ai/deployments/idle-runtime/config.json`, with immutable release binaries
and additive drop-in `76-dual-services.conf`. Private nginx ingress is separate.
The deployment manifest is
`/srv/rack-ai/evidence/dual-services-20261017T213615Z/deployment.json`.
Its candidate binary is
`/srv/rack-ai/deployments/idle-runtime/releases/dual-services-e116db02e136e1a2/rack_ai_runtime`
(SHA-256 `e116db02e136e1a290ec808b29b3b8a66e59798be1938da769a6ac4b440a6a60c`).
Live qualification and final publication identities belong in the adjacent evidence
report; the source document alone is not proof of a completed deployment.

For rollback, coordinate a quiet acquisition window, release/drain trial owners
through normal controls and verify all receiver-managed claims/processes are absent.
There is no hot per-profile disable operation; do not restart a busy receiver or
stop unrelated workloads to obtain the window. Then run on gpurack:

```bash
set -euo pipefail
evidence=/srv/rack-ai/evidence/dual-services-20261017T213615Z
live=/srv/rack-ai/deployments/idle-runtime
previous=$live/releases/input-preflight-52e34ade59a2d65e/rack_ai_runtime
dropin=/home/tomp/.config/systemd/user/rack-ai-runtime.service.d/76-dual-services.conf
install -m 0600 "$evidence/config.before.json" "$live/config.rollback.json"
"$previous" validate "$live/config.rollback.json"
test ! -e "$evidence/76-dual-services.conf.disabled"
mv -- "$dropin" "$evidence/76-dual-services.conf.disabled"
mv -- "$live/config.rollback.json" "$live/config.json"
systemctl --user daemon-reload
systemctl --user restart rack-ai-runtime.service
systemctl --user is-active rack-ai-runtime.service
```

This restores the preceding config and the 75-input-preflight drop-in's binary, retaining
authority state, credentials, models and adapter files. Verify authenticated discovery
and preserved existing profiles afterward. Restore no historical authority snapshot.

To disable/remove one service, first coordinate its acquiring clients to pause,
drain/cancel/release owned work and inspect verified terminal closure. Prepare a
validated administrator config with that profile unqualified, then activate it in
an otherwise idle receiver window. It now refuses fresh ordinary reservations.
A later validated config update can remove only that profile and its deployment
adapter config. Keep original runtimes, downloads, receipts/history and other
services. No new generic deletion subsystem is introduced.

A later MusicDirector/other agent selects ONE service per session/run, retaining
conversation/tool state in the client. Before conflicting image/video work,
release/yield LLM ownership, wait for closure, request media, then explicitly
reacquire the chosen LLM when needed. This change implements no consumer workflow.

See [the GPUrack validation report](dual-services-validation.md) for measured performance, ordinary reservation proofs and remaining trial limits.
