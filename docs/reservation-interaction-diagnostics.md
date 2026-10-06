# Reservation interaction diagnostics

Opt in only when creating a public reservation:

~~~json
{"operation":"reserve","request":{"acquisition_id":"dev-observation","work_id":"generic-work",
"services":["local-primary"],"priority":"low","ttl_seconds":1800,
"diagnostics":{"retained_model_interactions":true}}}
~~~

The typed Reserve.diagnostics field participates in frozen acquisition identity
and exact replay comparison. Omitted/false retains the previous serialized request
shape. Changed same-identity opt-in conflicts; refresh/work cannot enable it
retrospectively. Legacy acquire stays unchanged. There is no implicit client,
executor, model, GPU, priority, timeout or application enablement.

## Architecture and effective capture

Insertion points are the existing scoped gateway, dispatcher/backend adapter,
public execution projection, artifact retrieval and lifecycle maintenance.
Only scoped child calls with an existing parent work association are eligible.
The gateway inserts the reserved profile's output bound; capture records the
effective backend JSON immediately before dispatch. Headers, transport URLs
and runner configuration are excluded.

The rack-ai/model-interaction/v1 record retains reservation/work/parent/child
identity, durable queue sequence, service/model/profile version/hash, protocol,
accepted/start times, effective request, ordinary output, backend error,
terminal state, dispatch/completion timestamps and duration. Request JSON contains
messages, instructions, history, tools/tool choice, generation settings and output
bounds where provided. JSON output stays structured; SSE output preserves the
original event sequence as a string, including content/tool calls/usage/finish
reason where the backend reports them. No hidden reasoning is requested or invented.

Emitted tool-call IDs correlate with subsequent tool messages/results. Existing
execution inspection retains its bounded/redacted tool transcript. Missing IDs
or tool timing cannot establish a causal association. Prefill, first-token and
backend queue times remain explicitly null with an unavailable reason; receiver
queue timing remains in compact execution inspection. Request-only records report
diagnostic_complete:false and unavailable confirmed dispatch/completion metrics.
Read failures may retain bounded partial output beside the original backend error.

## Private bounded storage

Outside managed.json and the 14-day history archive, mode-0700 reservation
directories under model-interactions contain one mode-0600 lease and separate
append-only atomic request/completion documents. Owner-bound SHA-256 identifiers
are opaque. The existing private durable-write and bounded-lock mechanisms are
reused. Diagnostics do not grant execution authority.

Administrator interaction_diagnostics configuration defaults:

| Limit | Default |
| --- | --- |
| max_request_bytes | 65536 |
| max_response_bytes | 65536 |
| max_interactions_per_work | 64 |
| max_interactions_per_reservation | 128 |
| max_reservation_bytes | 4194304 |

Limits are snapshotted into the private lease on first capture and remain fixed
across restart. Content limits include escaped serialized prefixes. A fixed
8192-byte allocation per interaction and one lease allocation reserve metadata;
response content capacity is committed before dispatch, preventing concurrent
overspend. Oversized content has a JSON-text prefix and truncated:true. Exhausted
content space retains metadata with an unavailable reason. Exhausted interaction
limits increment a bounded saturating reservation omission count; compact ordinary
invocation history still provides identity/timing. No per-call omission list grows.
Invalid configuration fails validation.

Optional I/O failure/contention leaves partial/unavailable records and never fails
inference, workspace acceptance, resource release or archive maintenance. Disabled
reservations create no diagnostic documents/directories and do not duplicate
backend responses.

Redaction extends public assignment/path rules while preserving semantic whitespace,
source text and ordinary long identifiers. Structured credential keys, Bearer values,
known scoped capabilities, environment secrets, private paths/endpoints are removed
before persistence. An exact effective-body representation remains subject to these
documented secret removals and finite bounds.

## Public retrieval and compatibility

Discovery advertises support/schema/enablement/retrieval and active-reservation-only
retention, with no owner's individual setting. Public contract remains 1.4.0:
optional definitions/discovery are additive, existing operations and compact default
work/artifact responses remain compatible.

Owner-authenticated inspect_work_execution adds model_interactions only for a live
enabled reservation, containing ordered IDs, limits and omission count. Full
prompt/response bodies are retrieved separately:

~~~json
{"operation":"get_work_artifact","artifact_id":"mi1.<opaque-reservation>.<opaque-call>"}
~~~

Cross-owner, invalid, unknown and closed IDs return not_found. Compact parent work
may already be archived while its reservation remains live; diagnostic access is
always gated by current canonical authority, never the archive or an old receipt.

## Closure, retry and restart

Explicit release/cancel revokes visibility at the persisted logical closure fence,
including while resources drain. Targeted deletion runs outside authority mutation.
Terminal expiry, safe preemption, warm transfer and recovery use existing member
states. A group retains diagnostics while another member remains live.

Failed deletion leaves a bounded private lease directory as the durable cleanup
obligation; cleanup_pending is recorded where I/O permits. Authority denies access
even if marker writes fail. Startup and existing maintenance remove closed/orphan
directories in sweeps of at most 32 reservations using a durable cursor and bounded
memory. Diagnostic failure cannot block resource cleanup or archival.

Capture/cleanup share a diagnostic lock and recheck authority, so a late completion
cannot recreate a closed lease. Active records survive restart; the existing durable
invocation sequence avoids collisions. No model calls are replayed. Compact history
keeps its existing 14-day retention.

## Verification and qualification

Rust regressions cover default-off, frozen opt-in, ordering, inputs/outputs/tools,
usage, unavailable timing, redaction, bounds/escaping/omissions, terminal states,
active restart, deletion failure/restart retry, partial errors and late completion.
Compiled HTTP fixtures exercise real effective requests, bounded workspace execution,
five correlated calls, idempotency, owner denial, active restart retrieval, release
denial/deletion, ordinary work and successful execution with diagnostic I/O failure.

Live qualification uses public APIs and a disposable generic registered workspace.
Prompt/response exports are temporary while active: after closure retain only compact
IDs/counts/digests and verification outcomes in evidence.
