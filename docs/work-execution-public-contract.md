# Public Work Execution Results

RackAI exposes RackAI-managed work execution through the existing authenticated
`POST /runtime/v1` interface. The public contract uses generic execution terms;
JCode remains an implementation detail behind bounded workspace execution.

## Evidence Inventory

Recorded and now publicly projected for the authenticated owner:

- work identity, reservation identity, selected service, invocation identity and current state;
- queued/running/terminal timing points when the record was created by a runtime that records them;
- terminal inference result metadata that RackAI already stores, including usage and finish reason when the backend returned them;
- workspace packet status, changed paths, accepted revision when approved, bounded acceptance command outcomes and diagnostic artifact references;
- reservation cleanup/recovery state, including recovery blockers without rewriting historical uncertainty;
- owner-checked artifact text for command stdout/stderr through opaque IDs.

Recorded internally but not exposed through the ordinary public projection:

- private packet filesystem path, worktree path, repository registry roots and state roots;
- scoped access material, tokens, endpoint credentials and worker private JCode configuration;
- raw application packets beyond the sanitized fields and bounded artifacts.

Not currently recorded or unavailable from the backend contract:

- true first-token latency unless a backend later records it explicitly;
- model prefill, backend queue and generation sub-phase timings;
- transport timing split from backend processing;
- generic tool-call streams for the current bounded workspace `ExecuteChange` packet path;
- terminal timestamp for legacy records created before runtime `1.3.0`.

Unavailable values are reported as structured metrics with `availability:
"unavailable"` rather than as zero.

## Operations

`inspect_work_execution` accepts `work_id` and returns `rack-ai/work-execution/v1`.
It resolves active or archived records by owner and work identity. The response
contains authoritative work outcome, execution closure/recovery status, generic
activity counts/timings/events, and any bounded artifact references.

`get_work_artifact` accepts an opaque artifact ID previously returned by
`inspect_work_execution`. The ID is owner-bound and not a filesystem path. RackAI
rechecks the authenticated owner, resolves active or archived result evidence,
and returns at most the public artifact payload size. Missing, expired or
cross-owner artifact reads fail without redispatching work.

## Retention and Safety

Inspection is read-only and never starts, stops, retries or reconciles work.
Verbose diagnostics are projected from retained evidence and must not block
execution when optional telemetry is missing. Public responses do not expose
private server paths, credentials, scoped authorization values or model endpoints.

Archived records remain retrievable during their retention window. After expiry,
old work identities and artifact IDs no longer authorize reads or execution, and
replay semantics are limited to records still retained by RackAI.
