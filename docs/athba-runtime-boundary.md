# ATHBA / Rack AI Development Runtime Boundary

## Architectural invariant

Rack AI is **language- and framework-agnostic**.

Rack AI does not own the development environment of software being built. ATHBA owns that environment.

This boundary is hard and should be preserved even when a live proof exposes a missing tool, runtime, package manager, test runner, or generated-file convention.

## Rack AI responsibilities

Rack AI is the physical/trust execution authority for the rack. It may:

- select and address workers, models, GPUs, and other rack resources;
- start/stop or route to model services;
- resolve registered repositories and trusted revisions;
- create isolated workspaces/worktrees;
- execute commands supplied under an accepted contract;
- enforce generic path, network, timeout, resource, and command policies;
- capture stdout, stderr, exit status, revisions, and evidence;
- determine deterministic acceptance/rejection;
- fail closed when the execution contract cannot be satisfied.

Rack AI may understand generic execution concepts such as an environment identifier, workspace, command, allowed path set, declared generated/ignored paths, resource limits, and network policy.

## Rack AI must not own

Rack AI must not contain language- or framework-specific development semantics such as:

- Python/pytest knowledge;
- Node/npm/pnpm knowledge;
- Rust/cargo project semantics;
- .NET SDK/test semantics;
- application dependency installation policy;
- project-specific virtual environments;
- framework-specific generated-file conventions;
- application build/test strategy;
- TDD, specification, architecture, or requirement interpretation.

Examples of logic that should **not** be hard-coded into Rack AI:

- "Python projects require pytest";
- "run `python3 --version` before Python work";
- "`__pycache__` is always safe";
- "Node projects use `node_modules`";
- "Rust projects produce `target/`";
- "this project requires .NET 10".

If such knowledge is required for a software-development project, it belongs to ATHBA or to an ATHBA-owned project environment description.

## ATHBA-owned development environments

ATHBA is responsible for defining and managing the development environment for each application it builds.

That may include, for example:

- runtime/toolchain and version;
- project dependencies;
- package manager;
- test runner and test commands;
- build commands;
- project-specific environment variables;
- generated/ignored paths;
- persistent or semi-persistent development environments;
- Docker/Podman/devcontainer/venv/Nix or other implementation choices.

Rack AI should not need to know whether an environment contains Python, Rust, Node, .NET, or another toolchain. It should execute safely in the environment described or selected by ATHBA.

## Contract shape

The desired relationship is conceptually:

```text
ATHBA
  -> defines project development environment and software-development semantics
  -> requests a bounded execution using that environment and policy

Rack AI
  -> allocates rack resources
  -> creates/uses the isolated workspace
  -> executes the bounded command/work request
  -> enforces generic policy
  -> returns trusted evidence and revision
```

A future generic Rack AI execution contract may accept things such as:

- environment/profile identifier;
- command argv;
- working directory;
- allowed paths;
- declared generated/ignored paths;
- network/resource limits.

Those fields must remain generic. Their language-specific values are supplied from ATHBA.

## Generated files and path policy

Rack AI should not maintain a global list of Python-, Rust-, Node-, or .NET-specific generated paths.

Instead, ATHBA/project configuration should declare the generated or gate-neutral paths appropriate to the project. Rack AI should enforce that declaration generically.

For example, ATHBA might declare `__pycache__/` for a Python project or `target/` for a Rust project, but Rack AI treats both simply as declared paths under the execution contract.

## Failure handoff

If ATHBA work fails because the requested generic execution capability is unavailable, ATHBA should stop and hand the evidence to the Rack AI owner.

If Rack AI work fails because the project environment, test command, dependency set, or software semantics are wrong, Rack AI should stop and hand the evidence back to ATHBA.

Neither side should cross the repository/ownership boundary to unblock itself.

## Consequence for runtime provisioning

A generalized language-specific runtime provisioning subsystem does **not** belong in Rack AI.

Rack AI may eventually provide generic mechanisms for mounting, selecting, or executing within an environment, but ATHBA owns the meaning and lifecycle of software-development environments.

This invariant supersedes any tactical proof implementation that placed Python-, pytest-, Node-, Rust-, .NET-, or framework-specific development knowledge into Rack AI.