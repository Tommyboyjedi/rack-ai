# External Repository Change Workflow

## Purpose

Rack AI is the stable local control plane for the GPU rack.  It must be able to
develop a separate application repository without modifying its own source
repository during normal work.

The first target is a reviewable change workflow:

```text
change request
  -> isolated target-repository workspace
  -> planner
  -> coding worker
  -> deterministic checks
  -> verifier
  -> review packet
  -> human approval for commit/merge
```

This is deliberately not a self-modifying Rack AI workflow.  Rack AI remains
the scheduler, executor, and evidence recorder; the target repository is the
only codebase changed by the job.

## Current foundation

The Rust control plane already provides:

- task submission, state, queueing, leases, and DAG dependencies;
- named model workers and per-step working directories;
- bounded execution and artifact validation;
- structured run state and status inspection;
- a direct local-coder worker and a primary planner/verifier path.

The current direct coder can execute a host shell command and write an absolute
path.  Therefore a target repository supplied as a step `cwd` is a useful
operational boundary, but **not yet a security boundary**.  A v1 change
workflow must make the workspace boundary enforceable before it is trusted for
unattended implementation.

## Non-goals for v1

- No changes to Rack AI's own `main` branch from a normal job.
- No automatic merge or push to a target repository.
- No general public API, chat integration, web UI, or autonomous backlog.
- No unrestricted access to host files, credentials, Docker, or unrelated Git
  repositories from a coding job.

## Change request contract

Add a Rust-owned, versioned change-request schema.  The request must specify
the repository identity and expected evidence rather than relying on a freeform
prompt.

```json
{
  "change_id": "adaptos-20260820-001",
  "repository": {
    "id": "adaptos",
    "registered_root": "/srv/projects/adaptos",
    "base_ref": "main",
    "base_sha": "<resolved-at-submission>"
  },
  "task": "Add a bounded feature with tests.",
  "allowed_paths": [
    "src/",
    "tests/",
    "Cargo.toml"
  ],
  "acceptance": {
    "commands": [
      ["cargo", "test", "--workspace"],
      ["cargo", "fmt", "--check"]
    ],
    "required_artifacts": []
  },
  "limits": {
    "max_implementation_attempts": 2,
    "timeout_seconds": 900,
    "network": "disabled"
  }
}
```

At submission time, Rack AI resolves and records the immutable base SHA.  The
request must reject unregistered repositories, empty allowed-path lists, and
commands outside the approved command policy.

## Workspace lifecycle

1. Resolve the registered target repository and base SHA.
2. Create a fresh branch, for example
   `rack/change-<change-id>`.
3. Create a worktree under a configured job root, for example
   `/srv/rack-workspaces/<change-id>/repo`.
4. Run every planning, implementation, test, and verification command against
   that worktree.
5. Capture the final `git status`, base-to-head diff, diff stat, and commit
   identity.
6. Keep the worktree for review or clean it up only after an explicit
   retention decision.

A run must fail closed if the target is not a Git worktree at the recorded base
commit.

## Enforced execution boundary

The coding worker must not run commands directly on the host once it operates
on external repositories.  Introduce a `WorkspaceExecutor` abstraction used
by the direct coder tool loop.

The production implementation should run each job in an isolated execution
environment with:

- the job worktree mounted read/write at one fixed path;
- the Rack AI repository, host home directory, SSH material, Docker socket,
  and unrelated projects unavailable;
- a fixed working directory inside the workspace;
- an explicit environment allowlist;
- network disabled by default;
- resource and time limits;
- no path escape through file tools or shell commands.

The exact isolation backend is an operator decision and must be made
explicitly before implementation.  Acceptable candidates include a
rootless/containerised executor or a Linux namespace sandbox.  The Rust
application must depend on the executor interface, not on a particular
backend.

Until that executor exists, jobs are suitable only for a disposable target
branch under active human supervision.

## DAG shape

The default `change` template should have four nodes:

1. **prepare**: provision and validate the isolated worktree.
2. **plan** (`local-primary`): inspect the approved repository context and
   create a concise plan limited to the allowed paths.
3. **implement** (`local-coder`): make the change, then run the requested
   deterministic checks.
4. **verify** (`local-primary`): inspect the diff and evidence, re-run or
   confirm checks, and issue an approve/reject verdict.

A failed deterministic check may enter one bounded repair cycle.  The repair
prompt receives the exact command output and may not expand the allowed paths
or test list.  After the configured limit, the run stops as failed with a
concise diagnosis.

## Evidence-based completion

A model's `COMPLETE` response is never sufficient.  The verifier and final
manifest must include:

- registered repository ID and recorded base SHA;
- worktree path and generated branch;
- allowed paths and observed changed paths;
- complete diff and diff stat;
- tool command history, exit statuses, stdout/stderr references, and timing;
- deterministic acceptance results;
- planner/implementer/verifier outputs;
- verifier verdict and rationale;
- cleanup/retention status.

Reject a run when:

- a modified path is outside `allowed_paths`;
- the worktree base SHA does not match the submission record;
- a required command fails;
- evidence is missing;
- the verifier rejects the diff;
- execution boundary setup fails.

## Human promotion model

For the first operating phase, Rack AI may create a target-repository branch
but must stop before commit or push.  The user reviews the packet and decides
whether to commit/merge.

After repeated successful runs, enable an explicit opt-in policy that permits
a job to commit to **its own rack branch**.  Push and merge remain separate,
human-authorised actions.  Direct writes to a target `main` branch are never
a normal job capability.

## Implementation sequence

### Milestone 1: request and evidence

- Add the change-request schema and validation.
- Add repository registration configuration.
- Add a `change` CLI command that creates a branch/worktree and emits a run
  manifest.
- Add diff/path/command evidence capture.
- Add smoke tests using a disposable local Git fixture.

### Milestone 2: safe executor

- Introduce `WorkspaceExecutor` and route coder file/shell tools through it.
- Implement the selected isolation backend.
- Add path-escape, credential-access, network, and timeout rejection tests.
- Make the executor mandatory for external-repository changes.

### Milestone 3: bounded autonomous repair

- Add the planner/implementer/verifier `change` DAG template.
- Add deterministic acceptance commands and one or two repair attempts.
- Produce a single review packet for every terminal run.
- Add a persistent runner service and completion/failure notification.

## Acceptance test for Rack AI

A disposable fixture repository is registered under a temporary workspace.
A change request is submitted that asks for a small Rust feature and a unit
test.  The acceptance test proves that Rack AI:

1. creates an isolated worktree from the recorded SHA;
2. changes only approved paths;
3. runs the declared test command;
4. records a complete review packet;
5. returns an approved branch without altering either the target default branch
   or the Rack AI repository.

The first live pilot should use a small external application feature with a
disposable branch.  Do not make Rack AI self-modifying part of the pilot.
