# Docs

This directory holds operational notes and design docs for the rack.

The first non-swarm orchestration path is the JSON task contract executed by `bin/rack-task`.

`bin/rack-task` now supports either:
- a single legacy task shape with `worker`, `cwd`, `prompt`, and `artifacts`
- a pipeline shape with ordered `steps` for planner, worker, and verifier flows
