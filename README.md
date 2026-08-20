# Rack AI

This repository is the source-controlled integration root for the GPU rack at `gpurack`.

Current scope:
- capture the live rack architecture and configuration
- pin and document service versions and model roles
- hold the temporary Python control-plane prototype
- define and build the long-term Rust control-plane application
- provide repeatable tests and operational notes

Current engineering stance:
- the existing Python queue/orchestration code is now the reference prototype
- the long-term Rack AI control plane should be a structured Rust application
- JCode remains an execution backend, not the control plane itself

Immediate priorities:
1. freeze Python feature expansion except where needed for migration or live stability
2. install the Rust toolchain on `gpurack`
3. create the Cargo workspace and crate boundaries
4. port the durable execution backbone into Rust
5. reach parity for queue, leases, DAG execution, and status inspection
