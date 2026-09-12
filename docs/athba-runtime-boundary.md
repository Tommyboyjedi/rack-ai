# ATHBA / Rack AI Development Runtime Boundary

## Architectural invariant

Rack AI is **language-, framework-, and client-workflow agnostic**.

Rack AI does not own the development environment or software-engineering semantics of software being built. ATHBA owns those concerns.

Rack AI owns the generic bounded workspace transaction used to execute an already-ready task safely on the rack.

The general infrastructure contract and its rationale are defined in:

`docs/generic-bounded-workspace-execution.md`

This boundary is hard and should be preserved even when a live proof exposes a missing tool, runtime, package manager, test runner, generated-file convention, model limitation, or scheduling inconvenience.

## Why this boundary exists

ATHBA understands requirements, Behavior Contracts, strict TDD scenarios and frontiers, Tester/Developer meaning, model-attempt accounting, semantic repair, trusted project revisions, and Gatekeeper reconciliation. Rack AI does not need those concepts to place and execute a bounded task.

Rack AI understands registered model profiles, worker runtimes, GPUs and other resources, trusted worktrees, path/network/process/time policy, deterministic command execution, candidate revisions, and terminal evidence. ATHBA should not duplicate this privileged infrastructure.

The boundary avoids two opposite failures:

- teaching Rack AI ATHBA's development state machine would make Rack AI client-specific;
- reducing Rack AI to a raw prompt proxy would force ATHBA and every other client to duplicate resource, worktree, isolation, timeout, and evidence logic.

The correct relationship is therefore:

```text
ATHBA owns what the work means and when it is ready.
Rack AI owns how an already-ready generic task is safely executed.
```

The prompt is advisory. The typed execution envelope is authoritative.

## Rack AI responsibilities

Rack AI is the physical/trust execution authority for the rack. It may:

- admit a generic request under source policy;
- evaluate broad model capability, complexity, and context requirements;
- queue already-ready work;
- select and address workers, models, GPUs, and other rack resources;
- start, stop, or route to registered model services;
- resolve registered repositories and trusted revisions;
- create isolated workspaces/worktrees;
- invoke a registered harness and model runtime;
- execute caller-supplied deterministic commands under an accepted contract;
- enforce generic path, network, process, timeout, resource, and command policies;
- capture stdout, stderr, exit status, revisions, selection evidence, and execution provenance;
- materialize a candidate revision when the generic transaction permits it;
- fail closed when the execution contract cannot be satisfied.

Rack AI may understand generic execution concepts such as source identity, opaque work/submission identity, model capabilities, complexity, context requirement, priority, worker/runtime/resource eligibility, execution backend, workspace, command, allowed path set, declared resources, resource limits, network policy, idempotency, cancellation, and terminal status.

## Rack AI must not own

Rack AI must not contain ATHBA or language/framework development semantics such as:

- Tester, Developer, Senior Reviewer, or Gatekeeper roles;
- scenario authoring or repair;
- RED, GREEN, or strict TDD frontier meaning;
- ATHBA dependencies or readiness state;
- whether a result consumes an ATHBA attempt;
- Python/pytest knowledge;
- Node/npm/pnpm knowledge;
- Rust/cargo project semantics;
- .NET SDK/test semantics;
- application dependency installation policy;
- project-specific virtual environments;
- framework-specific generated-file conventions;
- application build/test strategy;
- requirement interpretation or semantic acceptance.

Examples of logic that should **not** be hard-coded into Rack AI:

- "this is a scenario-authoring job";
- "this is attempt three of a Tester repair";
- "Python projects require pytest";
- "run `python3 --version` before Python work";
- "`__pycache__` is always safe";
- "Node projects use `node_modules`";
- "Rust projects produce `target/`";
- "this project requires .NET 10".

The task objective may contain domain language because the selected model needs to perform the task. Rack AI treats that text as opaque payload, not as scheduling or state-machine data.

## Generic routing input from ATHBA

ATHBA may request only broad model and scheduling requirements such as:

```text
capabilities:
  reasoning
  coding
  visual
  audio

complexity:
  small
  medium
  large

requires_large_context:
  true | false

priority from ATHBA:
  low | medium
```

A request may require more than one broad capability, for example `[reasoning, coding]`.

ATHBA does not send scenario, frontier, review, repair, or escalation-stage names. It does not choose a concrete worker, model, GPU, endpoint, or JCode profile.

## Rack AI internal model eligibility profiles

Rack AI owns internal metadata describing which registered model profiles and worker runtimes can satisfy generic requests.

That internal metadata may include:

- broad model capabilities;
- qualified complexity envelope;
- large-context eligibility;
- context/runtime constraints;
- qualification status and evidence;
- profile version;
- worker/harness status;
- throughput, warm state, and resource placement;
- concurrency and lease state.

ATHBA does not transmit or author these profiles. It sends only the requirements of the current request. Rack AI may return generic selection evidence and execution provenance so ATHBA can verify what happened.

A model profile, worker runtime, and physical GPU are distinct Rack AI concepts. Two workers running the same model profile on a 4060 Ti and 4080 Super expose the same broad intelligence capabilities while differing in throughput and availability.

## ATHBA priority ceiling

Rack AI's global priority vocabulary may include:

```text
low
medium
high
paramount
```

ATHBA is a continuous slow-burn source and may submit only `low` or `medium`.

Rack AI admission policy should record a source-specific ceiling:

```text
source_system = athba
max_priority = medium
```

The ATHBA connector should reject an outbound priority above medium, and Rack AI must independently reject a buggy or forged ATHBA request above that ceiling.

High and paramount remain available to other separately authorized rack workloads or operator/system policy. Rack AI may queue ATHBA behind higher-priority work or remove a GPU from ATHBA capacity; it must not promote ATHBA's own priority.

## ATHBA-owned development environments

ATHBA is responsible for defining and managing the development environment for each application it builds.

That may include:

- runtime/toolchain and version;
- project dependencies;
- package manager;
- test runner and test commands;
- build commands;
- project-specific environment variables;
- generated/ignored paths;
- persistent or semi-persistent development environments;
- Docker/Podman/devcontainer/venv/Nix or other implementation choices.

Rack AI should not need to know whether an environment contains Python, Rust, Node, .NET, or another toolchain. It executes safely in the environment described or selected by ATHBA.

Rack AI supports multiple generic executor backends. Trusted host execution is appropriate when ATHBA owns a host-resident environment such as `/srv/ATHBA/.venv`; Podman remains appropriate when container isolation is the correct boundary. This is an execution-backend choice, not a Python feature.

## Bounded workspace transaction

The desired relationship is:

```text
ATHBA
  -> determines work is semantically ready
  -> maps its internal stage to broad generic requirements
  -> supplies exact repository/base, paths, task, limits, and acceptance

Rack AI
  -> validates source admission and the generic contract
  -> selects an eligible worker/model/resource
  -> creates/uses an isolated workspace
  -> executes the bounded request
  -> enforces generic policy
  -> returns selection, execution, Git, command, and terminal evidence

ATHBA
  -> interprets the result in software-development terms
  -> repairs, progresses, escalates, or blocks
```

Rack AI may report that supplied commands passed or failed. It does not decide whether the result is valid RED, correct GREEN, semantic completion, or a reason to alter ATHBA's plan.

## Queue and dependency boundary

ATHBA owns one authoritative semantic work ledger and submits only work it has already determined is ready and dispatchable.

Rack AI may queue accepted ready jobs while resources are busy. It owns generic queue order, source priority, worker/resource eligibility, leases, execution, and terminal evidence.

Rack AI must not receive or infer ATHBA's semantic dependency graph. An opaque work ID or sequence number is not a dependency instruction.

Temporary absence of capacity is a queued condition. Absence of any qualified worker for the requested generic capabilities is a capability-unavailable blocker. These are different outcomes.

## Submission and attempt boundary

A stable opaque `work_id` may link several ATHBA-authorized submissions for the same logical work.

A unique `submission_id` identifies one requested backend model invocation. Rack AI must not silently hide several semantic model submissions behind one ATHBA submission ID.

Rack AI may retry genuinely low-level infrastructure operations that do not invoke the model again, but infrastructure recovery and another model invocation must remain distinguishable in evidence. ATHBA owns the meaning and limit of its model attempts.

## Generated files and path policy

Rack AI should not maintain a global list of Python-, Rust-, Node-, or .NET-specific generated paths.

Instead, ATHBA/project configuration declares generated or gate-neutral paths appropriate to the project. Rack AI enforces that declaration generically.

For example, ATHBA might declare `__pycache__/` for a Python project or `target/` for a Rust project, but Rack AI treats both simply as declared paths under the execution contract.

## Tool and harness policy

Rack AI chooses the qualified worker runtime, harness, and tool profile internally. ATHBA must not request a concrete JCode profile to make one model output pass.

A model attempting an unavailable tool is not, by itself, evidence that Rack AI should grant that tool. Tool and harness changes require independent generic qualification and safety evidence.

## Failure handoff

If ATHBA work fails because a requested generic execution capability is unavailable, ATHBA persists its state and uses Rack AI evidence to classify the external blocker or authorize a different generic capability request.

If Rack AI work fails because the project environment, test command, dependency set, or software semantics are wrong, Rack AI returns evidence and ATHBA owns the correction.

Neither side should cross the repository or semantic boundary to unblock itself.

## Current contract and migration

The current `rack-ai/work-unit/v1` contract is an MVP and still contains `application-development` and singular `implementation` vocabulary. Those fields remain backward-compatibility facts, not the target semantic boundary.

The immediate generalization should be limited to:

- broad capability sets;
- existing small/medium/large complexity;
- existing large-context flag;
- global priority with source-specific ceilings;
- internal model eligibility profiles;
- generic selection evidence linked to execution provenance;
- opaque work/submission identity;
- preservation of the existing bounded workspace transaction.

A universal inference/media execution framework, ComfyUI arbitration, three-GPU scheduling, preemption, and idle-worker optimization are separate Rack AI design work.

## Change-control rule

Any further harness or boundary change must demonstrate that it restores or extends a documented generic contract rather than merely allowing one client proof to move one step farther.

Consequences:

- do not grant a tool merely because a model attempted to call it;
- do not add an ATHBA workflow field because it simplifies one connector case;
- do not infer client dependencies or semantic states;
- do not encode access, timeout, or acceptance only in prompt prose;
- do not convert ordinary model failure into hidden semantic retries;
- do not move language/framework or software-engineering knowledge into Rack AI.

This document is the authoritative ATHBA-specific application of `docs/generic-bounded-workspace-execution.md`.
