# PR29 — Trusted Host Execution for ATHBA Environments

Date: 2026-08-30

## Goal

Add a trusted host `WorkspaceExecutor` backend alongside the existing Podman executor so Rack AI can execute bounded work inside an ATHBA-owned host development environment without trying to transplant that environment into an unrelated container image.

## Architecture

Rack AI keeps one generic execution abstraction:

`WorkspaceExecutor`

Concrete backends are now:

- `host`
- `podman`

The application layer continues to depend only on `WorkspaceExecutor`.
Infrastructure selects the configured backend and preserves the same:

- trusted repository resolution
- isolated Rack AI worktree
- command policy
- timeout handling
- stdout/stderr evidence
- post-run Git inspection
- allowed-path enforcement
- accepted-revision materialization

## When to use each backend

### Trusted host execution

Use when the caller owns a host-resident development environment that already exists on the rack and has been administrator-authorized through generic environment-resource trust, for example:

`/srv/ATHBA/.venv`

Host execution is not a Python feature. Rack AI remains language/framework agnostic; ATHBA supplies the runtime/toolchain meaning.

### Podman execution

Use when sealed container isolation is the correct execution boundary.

Podman remains supported and unchanged for those workloads.

## Configuration

`config/repositories.json` executor configuration is now generic:

```json
{
  "executor": {
    "backend": "host"
  }
}
```

or:

```json
{
  "executor": {
    "backend": "podman",
    "image": "docker.io/library/rust:bookworm"
  }
}
```

## Security boundary

Trusted host execution does not bypass Rack AI controls.

The host executor still:

- operates only inside the Rack AI-managed worktree
- executes argv directly without shell interpolation
- enforces wall-clock timeout
- captures stdout/stderr/exit status
- relies on the same repository trust and environment-resource trust
- remains subject to post-run Git/path-policy review before acceptance

## Qualification target

The PR29 live proof demonstrates:

- host backend selected through config
- `/srv/ATHBA/.venv/bin/python --version`
- `/srv/ATHBA/.venv/bin/python -m pytest --version`
- bounded pytest execution inside the isolated worktree
- normal accepted revision materialization
- unauthorized repository rejection
- unauthorized environment-resource rejection
- Podman regression still green
