# PR28 — Trusted Dynamic Workspaces

## Goal

Provide the smallest generic Rack AI trust mechanism that allows bounded agent execution against dynamically created project repositories without requiring a per-project Rack AI config edit.

## Architecture boundary

Rack AI remains language- and framework-agnostic. It owns:

- worker/model/resource selection;
- JCode-backed bounded implementation execution;
- worktree isolation;
- process execution;
- timeouts and network policy;
- allowed-path enforcement;
- stdout/stderr and Git evidence;
- candidate/result revisions;
- fail-closed trust enforcement.

ATHBA owns software-development meaning: project creation, environment definition, dependencies, tests/build commands, development orchestration, and deciding what work happens next.

## Existing trust model

Before PR28, Rack AI required every target repository to appear explicitly in `config/repositories.json`.

That static registry provided three concrete security properties:

1. Rack AI resolved one administrator-approved repository root for a given repository id.
2. Rack AI rejected unknown repository ids before worktree creation or execution.
3. Rack AI could refuse the live Rack AI repository itself by resolving the target Git top-level and comparing it to the running Rack AI Git top-level.

That registry was therefore not just convenience metadata. It was the authorization boundary that prevented a caller from naming an arbitrary filesystem path and obtaining bounded agent execution there.

## Problem

ATHBA can create project repositories dynamically. Requiring a new static repository entry for every such repository is operationally wrong, but simply accepting an arbitrary caller-supplied path would destroy the existing trust model.

## Options considered

### 1. Explicit runtime repository registration

Rejected for PR28.

This would still require a mutating registration step per repository and would add a second lifecycle to manage and secure. It solves convenience, not the core authorization shape.

### 2. Generic workspace authorization without static administrator roots

Rejected.

This is too broad. It risks turning Rack AI into a generic path executor where trust is supplied by the caller.

### 3. Administrator-approved trusted dynamic roots

Chosen.

Rack AI configuration remains the authorization boundary, but at the level of approved parent roots rather than one entry per child repository.

An administrator authorizes a stable parent such as:

`/srv/ATHBA/state/projects/`

A caller may then submit a bounded request with a concrete repository root such as:

`/srv/ATHBA/state/projects/<project-id>`

Rack AI canonically verifies that the requested repository is genuinely beneath an approved trusted dynamic root and then reuses the existing change/work-unit/worktree/execution path.

This is the smallest generic extension because it preserves the existing control-plane shape:

- administrator policy remains in Rack AI config;
- caller trust does not bypass config;
- worktree/executor/JCode/acceptance logic remains unchanged.

## Implemented mechanism

`config/repositories.json` now supports:

```json
{
  "trusted_dynamic_roots": [
    {
      "id": "athba-projects",
      "root": "/srv/ATHBA/state/projects",
      "enabled": true
    }
  ]
}
```

Requests may continue to use static repositories exactly as before.

For dynamically created repositories, a change/work-unit request may now include:

```json
{
  "repository": {
    "id": "project-123",
    "root": "/srv/ATHBA/state/projects/project-123",
    "base_ref": "main"
  }
}
```

Resolution rules:

- if `repository.id` matches a static repository entry, Rack AI uses the static entry;
- if no static entry exists and `repository.root` is present, Rack AI attempts trusted-dynamic authorization;
- if neither path succeeds, resolution fails closed.

## Security properties

The new mechanism preserves and extends the trust boundary.

Dynamic authorization requires all of the following:

- `repository.root` must be an absolute path;
- `repository.root` must not contain `.` or `..` traversal components;
- `repository.root` must canonicalize successfully;
- `repository.root` must itself be the Git repository top-level, not merely a child directory inside a repository;
- the canonical requested repository root must be strictly beneath an enabled trusted dynamic root;
- symlink aliases that escape the trusted root are rejected after canonicalization;
- malformed or non-Git targets are rejected;
- the live Rack AI repository is still rejected;
- static repository behaviour remains intact.

Rack AI therefore still does **not** execute against arbitrary filesystem paths merely because a caller supplied one.

## CLI / contract behaviour

No new CLI was added.

PR28 extends the existing generic `change` and `work-unit` contracts:

- static repositories: `repository.id` plus optional legacy `registered_root` exact-match field;
- dynamic repositories: `repository.id` plus `repository.root` beneath an approved trusted dynamic root.

The existing execution path remains unchanged after repository resolution:

`request -> repository resolution -> isolated worktree -> JCode worker -> deterministic acceptance -> evidence packet`

## Live proof

Date: `2026-08-29`

Disposable proof area:

- authorized root: `/tmp/rack-ai-pr28-live-proof/authorized-projects`
- authorized repo: `/tmp/rack-ai-pr28-live-proof/authorized-projects/proof-app`
- rejected control repo: `/tmp/rack-ai-pr28-live-proof/outside-projects/rejected-app`
- temporary Rack AI config root: `/tmp/rack-ai-pr28-live-proof/rack`

Important proof detail: the temporary `repositories.json` contained:

- one `trusted_dynamic_roots` entry for the authorized parent root;
- zero static repository entries.

Observed authorized result through the real `change` command:

- `status: checks_passed`
- `acceptance_verdict: approved`
- `branch: rack/change-dynamic-proof-001`
- `worktree: /tmp/rack-ai-pr28-live-proof/workspaces/dynamic-proof-001/repo`
- packet: `/tmp/rack-ai-pr28-live-proof/rack/state/changes/dynamic-proof-001/review-packet.json`
- changed path: `src/lib.rs`

Observed rejected control result:

- no static repo entry was added;
- request failed before execution with:
  `repository rejected-app root /tmp/rack-ai-pr28-live-proof/outside-projects/rejected-app is outside trusted dynamic roots`
- no review packet was created for the rejected control request.

This proves:

1. no per-project Rack AI config mutation was required;
2. the authorized dynamic repo entered the normal bounded execution path;
3. the normal isolated worktree/evidence path was used;
4. an equivalent repo outside the trusted root was rejected before execution.

## ATHBA / Rack AI boundary

PR28 does not add Python, pytest, ReservationBook, Node, cargo semantics, Gatekeeper logic, or ATHBA-specific development behaviour to Rack AI.

PR28 also keeps the ATHBA runtime boundary intact: Rack AI trusts the approved
workspace and executor boundary, not a Rack AI-maintained allow-list of
language- or framework-specific executable names.

ATHBA continues to decide **what** software-development work means.

Rack AI continues to decide **how** to execute bounded agent work safely on available rack resources.
