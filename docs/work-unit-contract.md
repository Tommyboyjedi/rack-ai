# Work-Unit Contract MVP

This document describes the PR22 MVP boundary between an external client such as ATHBA and Rack AI.

It is intentionally small. ATHBA tells Rack AI that a wider workload exists and submits one bounded work unit that is ready to execute. Rack AI then chooses the execution worker/resource internally and runs the unit through the existing qualified bounded change path.

The broader architectural rationale is authoritative in:

- `docs/generic-bounded-workspace-execution.md`
- `docs/athba-runtime-boundary.md`

## Current contract status

`rack-ai/work-unit/v1` is a deliberate MVP. It still contains the client-shaped values `application-development` and `implementation`. Those values describe the current compatibility surface, not the target long-term semantic boundary.

The stable generic operation underneath them is a bounded workspace transaction:

```text
exact repository/base
+ bounded objective
+ allowed paths/resources
+ timeout/network/process policy
+ deterministic acceptance
→ candidate revision and evidence, or durable terminal failure
```

Rack AI owns this operation because worker/resource selection, trusted worktrees, process isolation, policy enforcement, Git evidence, and terminalization should be centralized once for the rack rather than reimplemented by every client.

Rack AI does not own why the work exists. Client terms such as Tester, Developer, scenario, RED, GREEN, frontier, repair, review, or dependency state must not become Rack AI routing fields.

> The prompt is advisory. The typed execution envelope is authoritative.

## Ownership boundary

ATHBA owns:

- application specification and architecture
- decomposition into small work units
- dependency/readiness ordering
- development-domain acceptance requirements
- project progress and next-ticket selection
- model-attempt and escalation meaning
- semantic interpretation of the returned result

Rack AI owns:

- source admission
- generic model capability eligibility
- resource and worker selection
- JCode-backed execution harness selection
- isolated worktree preparation
- allowed-path enforcement
- timeouts and physical execution budgets
- deterministic command execution
- evidence capture and fail-closed behaviour
- candidate revision materialization

The external request must not name a GPU, model id, worker id, endpoint, or JCode profile.

## Version

Current version:

```text
rack-ai/work-unit/v1
```

Unknown fields are rejected. This is deliberate: external callers should not be able to smuggle worker/model selection or client workflow semantics into the contract.

## Request shape

```json
{
  "version": "rack-ai/work-unit/v1",
  "workload": {
    "id": "adaptos",
    "kind": "application-development"
  },
  "repository": {
    "id": "adaptos",
    "base_ref": "main",
    "base_sha": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
  },
  "work_unit": {
    "id": "adaptos-001",
    "objective": "Implement TicketStore::save(path) for one open ticket.",
    "allowed_paths": ["src/lib.rs"],
    "acceptance": {
      "commands": [["cargo", "test", "save_single_open_ticket"]],
      "required_artifacts": ["src/lib.rs"]
    },
    "environment_resources": ["/srv/ATHBA/.venv"],
    "readiness": {
      "ready": true,
      "depends_on": []
    },
    "requirements": {
      "capability": "implementation",
      "complexity": "small",
      "requires_large_context": false
    },
    "limits": {
      "max_implementation_attempts": 2,
      "timeout_seconds": 900,
      "network": "disabled"
    }
  }
}
```

## Three contract layers

Even in the MVP, fields fall into three responsibilities.

### Generic routing data

Current v1 provides:

```text
capability = implementation
complexity = small | medium | large
requires_large_context = true | false
```

The target generic extension replaces singular `implementation` with broad model capability sets such as `reasoning`, `coding`, `visual`, and `audio`, while retaining complexity and the context flag.

### Machine-enforced execution envelope

```text
repository/base SHA
allowed paths
authorized resources
acceptance commands and artifacts
timeout and network policy
opaque identities
```

These values are enforced independently of the model's prompt compliance.

### Model-facing task payload

```text
objective
relevant immutable context
```

The objective tells the model what to do. It is not the only place where permissions, routing, timeout, or acceptance may be defined.

## Fields

### `workload`

- `id`: stable workload/project identity across many work units
- `kind`: current MVP supports `application-development`

This is a coarse compatibility signal that the work unit belongs to a wider continuing build rather than a one-off coding request. A future generic boundary should not infer client workflow semantics from it.

### `repository`

- `id`: Rack AI repository identity
- `base_ref`: expected base branch/ref
- `base_sha`: optional exact expected starting revision
- `registered_root`: optional exact-match field for a statically registered repository
- `root`: optional concrete repository root for a dynamically created Git repository beneath an administrator-approved trusted dynamic root

Rack AI resolves the target repository, enforces self-target protection, requires an exact Git top-level for dynamic roots, and preserves worktree isolation.

### `work_unit`

- `id`: stable work-unit identity within the workload
- `objective`: exact bounded model-facing objective
- `allowed_paths`: the only writable paths the implementation is allowed to change
- `acceptance.commands`: deterministic acceptance commands that Rack AI runs itself
- `acceptance.required_artifacts`: files that must exist for the unit to pass
- `environment_resources`: administrator-authorized host paths Rack AI should mount read-only into the isolated executor at the same absolute path
- `readiness.ready`: must be `true` for execution in the MVP
- `readiness.depends_on`: optional prerequisite work-unit ids retained by the compatibility contract
- `requirements.capability`: current MVP supports `implementation`
- `requirements.complexity`: `small`, `medium`, or `large`
- `requirements.requires_large_context`: hint that this unit should prefer the stronger worker
- `limits.max_implementation_attempts`: bounded internal implementation budget in the current MVP
- `limits.timeout_seconds`: bounded execution timeout
- `limits.network`: current MVP default is `disabled`

The target connector submits only already-ready work. Rack AI must not become authoritative for ATHBA dependency ordering, and a future sequence number must remain audit data rather than a dependency mechanism.

## Rack AI internal selection in PR22

The external caller does not select a worker.

Current MVP policy is intentionally simple and replaceable:

- small bounded implementation work prefers the minimal implementer worker
- medium/large or `requires_large_context=true` work prefers the stronger non-minimal worker

Today, on `gpurack`, that means Rack AI will usually map:

- small bounded implementation -> `local-coder`
- larger-context implementation -> `local-primary`

That is internal Rack AI policy, not part of the external contract.

The target generic selector will use broad requested capabilities, complexity, context, source priority, qualification evidence, and current resource state. Internal model eligibility metadata remains owned by Rack AI and is never sent by ATHBA.

## Priority direction

The target global Rack AI priority vocabulary is:

```text
low
medium
high
paramount
```

Source systems have configured admission ceilings. ATHBA may emit only `low` or `medium`. Rack AI must reject ATHBA-originated work above medium and must not promote it above that ceiling.

High and paramount remain available for other separately authorized rack workloads and operator/system policy.

## CLI entry point

The MVP Rack AI entry point is:

```bash
cargo run -q -p rack_ai_cli -- \
  work-unit /path/to/work-unit.json \
  --repo-root /srv/rack-ai \
  --state-root /srv/rack-ai
```

- `--repo-root` points at the Rack AI control-plane repository/config root
- `--state-root` points at the root used for review packets and other persisted evidence
- the target application repository may be statically registered, or supplied as `repository.root` beneath a configured trusted dynamic root

## Result shape

Rack AI returns a structured result including:

- `workload_id`
- `work_unit_id`
- `change_id`
- selected worker information
- placement
- status
- acceptance verdict
- accepted revision
- branch
- worktree path
- packet path
- worker execution provenance where available

Future capability routing should also return a generic selection decision explaining why the worker was eligible and selected. Selection evidence explains **why**; execution provenance proves **what actually ran**. A mismatch fails closed.

## Submission identity

A stable opaque work ID may link several client-authorized submissions for the same logical work. A unique submission ID should identify one requested model invocation.

Rack AI may retry genuinely low-level infrastructure actions when that does not invoke the model again, but it must not silently hide several semantic model submissions behind one client submission ID. The evidence must allow a client to account for attempts truthfully.

## Reused safety boundary

PR22 does not introduce a new unsafe implementation path.

The work-unit request is translated into the existing bounded change request and then reuses:

- registered repository resolution
- self-target protection
- isolated Git worktree creation
- qualified JCode execution
- allowed-path enforcement via post-run Git inspection
- deterministic acceptance run by Rack AI
- independent review/evidence packet persistence
- fail-closed status handling

## Deliberate non-goals in PR22

PR22 does not implement:

- ATHBA itself
- multi-day scheduling policy
- fairness/preemption/capacity optimisation
- generic future-proof schema expansion
- application semantics inside Rack AI
- direct GPU/model selection by the client
- any new model-facing agent loop

Those remain for later PRs.

## Immediate target generalization

The next bounded evolution should add only what is required for generic model selection around the existing workspace transaction:

- capability sets: `reasoning`, `coding`, `visual`, `audio`;
- existing complexity and large-context inputs;
- global priority with source-specific ceilings;
- internal model eligibility/qualification profiles;
- generic selection evidence linked to execution provenance;
- opaque work/submission identity;
- backward compatibility for v1 requests and packets.

It should not add client workflow stages, dependency semantics, a universal media/inference framework, ComfyUI arbitration, preemption, or three-GPU optimization.


## PR32 wire contract

`rack-ai/work-unit/v2` is the published generic routing schema. `work_unit.routing` is required only for v2 and has this exact shape:

```json
{"source_system":"athba","work_id":"opaque-work","submission_id":"opaque-submission","idempotency_key":"opaque-key","required_capabilities":["reasoning","coding"],"priority":"medium"}
```

`required_capabilities` is a non-empty, duplicate-free set from `reasoning`, `coding`, `visual`, and `audio`. Rack AI canonicalizes its persisted ordering. Complexity remains exactly `small`, `medium`, or `large`; `requires_large_context` remains a boolean eligibility constraint. Priority is exactly `low`, `medium`, `high`, or `paramount` and governs admission/scheduling only.

Source admission is typed Rack AI configuration. `athba` is capped at `medium`; high and paramount requests are rejected before worker selection or execution. The configuration includes an explicit wildcard default ceiling of `paramount`; without either a matching source policy or the wildcard default, admission fails closed.

Profiles are internal. The minimal `local-coder` profile is coding-only, small-only, non-large-context, and retains its minimal JCode constraint. `local-primary` is qualified for reasoning and coding at small, medium, and large complexity according to its configured profile. For a small coding request, least-scarce-sufficient selection chooses `local-coder`; reasoning plus coding at medium selects `local-primary`.

A selected v2 request persists a generic selection decision and must have execution provenance for the same worker. A mismatch fails closed. Selection evidence is retained for terminal accepted, rejected, timeout, protocol, and post-selection executor failures; failures before selection do not fabricate a decision. Resource-capable but busy workers produce the typed temporary-unavailable outcome, distinct from no capable worker.

`work_id`, `submission_id`, and `idempotency_key` are opaque. A submission-specific safe internal transaction identifier preserves distinct submissions for the same work ID. A repeat with the same persisted source/work/submission/idempotency identity is rejected before another execution.

V1 remains readable with its historical singular `implementation` routing semantics. Rack AI never interprets a v1 packet as v2. This compatibility window remains until an explicitly versioned deprecation change. PR32 does not add client dependency scheduling, universal inference/media execution forms, ComfyUI arbitration, preemption, idle-worker overflow, or three-GPU scheduling.
