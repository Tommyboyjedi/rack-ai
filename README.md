# Rack AI

Rack AI is the stable control plane for a heterogeneous local AI rack.

It is not another coding agent and it is not a replacement for JCode, vLLM, or future model-serving tools. Its job is to know what the rack contains, what each backend is good at, what resources are currently available, and how to run bounded work safely against those backends.

On `gpurack`, that currently means:
- coordinating a `local-primary` reasoning endpoint on the RTX 4060 Ti
- coordinating a `local-coder` coding endpoint on the RTX 2060
- running bounded local jobs through Rust-owned orchestration
- executing external-repository work in isolated Git worktrees with rootless Podman
- recording deterministic evidence, review packets, and run state

## What Rack AI Is

Rack AI is the orchestration layer above model servers and execution tools.

It is responsible for:
- repository and workload registration
- worker and model role mapping
- task and pipeline submission
- durable queue, DAG, and lease behavior
- isolated change execution against external repositories
- bounded autonomous campaigns with pause, resume, cancel, revise, and recovery
- operator-facing evidence and review artifacts

It treats JCode as one execution backend, vLLM as one inference backend, and future services such as ComfyUI, vision, speech, or audio tools as additional backends.

## What Rack AI Is Not

Rack AI is not:
- the product repository being changed
- a normal target of its own external-repository workflow
- a free-running self-modifying agent
- dependent on JCode swarm for its core control-plane behavior

The control plane owns orchestration. Model-serving and coding tools are plugged into that control plane rather than allowed to define it.

## Current Architecture

The repository is a Rust workspace:
- `crates/rack_ai_domain`: small domain types and invariants
- `crates/rack_ai_application`: orchestration use cases, campaign logic, change workflow, reviews, and state transitions
- `crates/rack_ai_infrastructure`: Git, filesystem, Podman, registry, path-policy, and worker integrations
- `crates/rack_ai_cli`: operator CLI commands

Supporting areas:
- `bin/`: operational entry points used on the rack
- `config/`: worker, model, resource, repository, and template configuration
- `docs/`: architecture and workflow contracts
- `tests/`: smoke and boundary coverage for live rack behavior
- `plugins/`: Python only where an external integration structurally requires it

## Execution Model

The current live execution model has three layers.

First, there are direct rack task flows:
- `bin/rack-primary`
- `bin/rack-coder`
- `bin/rack-coordinator`
- `bin/rack-task`

These provide the current non-swarm path for single-worker and pipeline execution.

Second, there is the external-repository change workflow:
- `bin/rack-change`

This is the bounded implementation path for making a change in a registered external repository. It prepares an isolated worktree, runs a coder through rootless Podman, enforces allowed paths, runs deterministic acceptance commands, and produces a final `acceptance_verdict` with evidence.

Third, there is the autonomous campaign runner:
- `bin/rack-campaign`

This is the control-plane layer for multi-step unattended repository work. A campaign is a bounded, restartable sequence of steps with:
- persistent state
- heartbeats
- leases
- operator controls
- independent coordinator review per attempt
- scoped repair and fallback behavior
- fail-closed continuity checks

## Safety Model

Rack AI is intentionally conservative.

Key rules in the current design:
- external repository work runs in isolated Git worktrees
- change jobs run with network disabled
- rootless Podman is required for live executor-backed change work
- changed files must stay inside declared allowed paths
- required artifacts must exist before a step can pass
- acceptance commands are explicit and deterministic
- campaigns can be paused, resumed, revised, or cancelled by the operator
- continuity failures fail closed rather than silently resetting state

The target repository is the workload. Rack AI remains the controller.

## Current Backend Stance

The current rack setup uses two local OpenAI-compatible endpoints:
- `local-primary` at `http://127.0.0.1:8017/v1`
- `local-coder` at `http://127.0.0.1:8018/v1`

JCode remains part of the system, but not as the control plane.

Because JCode swarm has been unreliable in this rack configuration for cross-provider delegation, the current working pattern is:
- direct JCode coordinator usage where appropriate
- a repo-local direct coder path for the 2060 worker
- Rust-owned orchestration above both

That is deliberate temporary glue. The long-term direction is to delete glue as upstream tooling becomes reliable enough to sit cleanly behind Rack AI.

## Python Policy

The live control-plane path is Rust-owned.

Python is retained only where a dependency structurally requires it, most notably the temporary vLLM plugin surface. Python is not the primary orchestration language for this repository.

## Operator Entry Points

Main operator commands:
- `bin/rack-healthcheck`: endpoint and registry health
- `bin/rack-submit`: queue submission
- `bin/rack-runner`: queue and DAG runner
- `bin/rack-status`: run-state inspection
- `bin/rack-task`: explicit task and pipeline execution
- `bin/rack-coordinator`: template-driven task generation
- `bin/rack-change`: bounded external-repository change execution
- `bin/rack-campaign`: bounded autonomous campaign execution

## Tests

The repository includes smoke coverage for:
- direct coder execution
- task and pipeline orchestration
- queue and DAG behavior
- resource admission and health checks
- external-repository change preparation
- live Podman-backed implementation
- path-policy rejection
- autonomous campaign execution and restart behavior

Some live tests require:
- rootless Podman
- a prepared executor image
- the local model endpoints to be healthy

See [tests/README.md](/C:/CodexProjects/GpuRackAgent/tests/README.md) for the current smoke inventory.

## Current Status After PR #3

The merged `main` branch now includes:
- the external-repository change workflow
- the autonomous campaign runner
- coordinator review per implementation attempt
- bounded repair and fallback handling
- lease, heartbeat, pause, resume, cancel, revise, and recovery behavior
- operator CLI separation from test-only seams

That means Rack AI is now in a position to build bounded software slices in registered target repositories under operator control.

It does not mean the rack should be turned loose to build arbitrary software without defined repository scope, allowed paths, acceptance commands, and review checkpoints.

## Recommended Use

Use Rack AI to run narrowly scoped, reviewable jobs against a specific repository.

Good use:
- build a small backend slice in a registered Rust repository
- run a sequenced campaign for a defined feature set
- validate that the rack can repeatedly make safe, reviewable changes

Bad use:
- unrestricted autonomous coding without repository policy
- letting the control plane modify itself as part of normal workload execution
- relying on implicit model behavior without acceptance gates

## Key Documents

- [docs/external-repository-change-workflow.md](/C:/CodexProjects/GpuRackAgent/docs/external-repository-change-workflow.md)
- [docs/autonomous-campaign-runner-contract.md](/C:/CodexProjects/GpuRackAgent/docs/autonomous-campaign-runner-contract.md)
- [docs/rust-application-architecture.md](/C:/CodexProjects/GpuRackAgent/docs/rust-application-architecture.md)
- [config/README.md](/C:/CodexProjects/GpuRackAgent/config/README.md)
- [tests/README.md](/C:/CodexProjects/GpuRackAgent/tests/README.md)
