# Docs

This directory holds operational notes and design docs for the rack.

The first non-swarm orchestration path is the JSON task contract executed by `bin/rack-task`.

`bin/rack-task` supports either:
- a single legacy task shape with `worker`, `cwd`, `prompt`, and `artifacts`
- a pipeline shape with ordered `steps` for planner, worker, and verifier flows

`bin/rack-coordinator` builds those specs from named templates in `config/task_templates.json`.
It can also auto-select a template from the request text and preview the generated spec without running it.

External-repository work is specified by `docs/external-repository-change-workflow.md` and executed by `bin/rack-change`.
The current change command is a Milestone 1–2 pilot: isolated worktree, Podman-backed coder, deterministic Git/path/acceptance gates, and an `acceptance_verdict`. It does not yet run the local-primary planner/implementer/verifier DAG.
The target repository is the workload; Rack AI remains the control plane and does not self-modify as part of a normal change job.
