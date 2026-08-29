# Work-Unit Contract MVP

This document describes the PR22 MVP boundary between an external client such as ATHBA and Rack AI.

It is intentionally small. ATHBA tells Rack AI that a wider workload exists and submits one bounded work unit that is ready to execute. Rack AI then chooses the execution worker/resource internally and runs the unit through the existing qualified bounded change path.

## Ownership boundary

ATHBA owns:

- application specification and architecture
- decomposition into small work units
- dependency/readiness ordering
- development-domain acceptance requirements
- project progress and next-ticket selection

Rack AI owns:

- resource and worker selection
- JCode-backed execution harness selection
- isolated worktree preparation
- allowed-path enforcement
- timeouts and implementation budgets
- independent acceptance execution
- evidence capture and fail-closed behaviour

The external request must not name a GPU, model id, or worker id.

## Version

Current version:

```text
rack-ai/work-unit/v1
```

Unknown fields are rejected. This is deliberate: external callers should not be able to smuggle worker/model selection into the contract.

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

## Fields

### `workload`

- `id`: stable workload/project identity across many work units
- `kind`: current MVP supports `application-development`

This is the coarse signal that the work unit belongs to a wider continuing build rather than a one-off coding request.

### `repository`

- `id`: Rack AI repository identity
- `base_ref`: expected base branch/ref
- `base_sha`: optional exact expected starting revision
- `registered_root`: optional exact-match field for a statically registered repository
- `root`: optional concrete repository root for a dynamically created Git repository beneath an administrator-approved trusted dynamic root

Rack AI still resolves the target repository, enforces self-target protection, requires an exact Git top-level for dynamic roots, and preserves worktree isolation.

### `work_unit`

- `id`: stable work-unit identity within the workload
- `objective`: exact bounded implementation objective
- `allowed_paths`: the only writable paths the implementation is allowed to change
- `acceptance.commands`: deterministic acceptance commands that Rack AI runs itself
- `acceptance.required_artifacts`: files that must exist for the unit to pass
- `readiness.ready`: must be `true` for execution in the MVP
- `readiness.depends_on`: optional prerequisite work-unit ids
- `requirements.capability`: current MVP supports `implementation`
- `requirements.complexity`: `small`, `medium`, or `large`
- `requirements.requires_large_context`: hint that this unit should prefer the stronger worker
- `limits.max_implementation_attempts`: bounded implementation budget
- `limits.timeout_seconds`: bounded execution timeout
- `limits.network`: current MVP default is `disabled`

## Rack AI internal selection in PR22

The external caller does not select a worker.

Current MVP policy is intentionally simple and replaceable:

- small bounded implementation work prefers the minimal implementer worker
- medium/large or `requires_large_context=true` work prefers the stronger non-minimal worker

Today, on `gpurack`, that means Rack AI will usually map:

- small bounded implementation -> `local-coder`
- larger-context implementation -> `local-primary`

That is internal Rack AI policy, not part of the external contract.

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
- `selected_worker_id`
- `placement`
- `status`
- `acceptance_verdict`
- `branch`
- `worktree_path`
- `packet_path`

This is enough for ATHBA to determine whether the unit was accepted or rejected and where to inspect the evidence.

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