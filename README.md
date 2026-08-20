# Rack AI

This repository is the source-controlled integration root for the GPU rack at `gpurack`.

Current scope:
- capture the live rack architecture and configuration
- pin and document service versions and model roles
- hold the stable Rust control-plane application and its operational wrappers
- provide repeatable tests and operational notes
- eliminate remaining Python utilities as migration cleanup

Current engineering stance:
- the live control-plane path is now Rust-owned
- JCode remains an execution backend, not the control plane itself
- remaining Python files are temporary utilities, plugins, or tests pending migration or deletion

Immediate priorities:
1. remove the remaining Python utilities, tests, and temporary plugin code
2. harden the Rust crate boundaries and naming so the application architecture is explicit
3. keep queue, DAG, lease, and worker behavior stable while deleting old compatibility code
4. continue proving the rack through repeatable smoke coverage on the live hardware
