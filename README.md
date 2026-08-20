# Rack AI

This repository is the source-controlled integration root for the GPU rack at `gpurack`.

Current scope:
- capture the live rack architecture and configuration
- pin and document service versions and model roles
- hold the stable Rust control-plane application and its operational wrappers
- keep Python only where vLLM structurally requires it
- provide repeatable tests and operational notes

Current engineering stance:
- the live control-plane path is Rust-owned
- JCode remains an execution backend, not the control plane itself
- Python is retained only for the temporary vLLM tool parser plugin that must integrate through vLLM's Python plugin surface

Immediate priorities:
1. keep deleting compatibility code that is not required by the rack's live execution path
2. harden the Rust crate boundaries and naming so the application architecture is explicit
3. keep queue, DAG, lease, and worker behavior stable while simplifying the repository
4. continue proving the rack through repeatable smoke coverage on the live hardware
