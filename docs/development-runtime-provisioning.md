# Development Runtime Provisioning

This tactical Python runtime patch is not Rack AI's final development-environment architecture.

Today Rack AI selects a single pre-baked executor image for isolated repository work. That is sufficient to unblock the ATHBA PR17 Python 3.14 `pytest` proof, but it is not sufficient as a long-term contract for heterogeneous software projects.

For the current tactical Python path, Rack AI records runtime preflight evidence before Python acceptance commands run:

- interpreter path
- Python version
- `pytest` availability and version

That tactical runtime is Rack-AI-owned. It does not rely on the interactive operator shell environment and it does not implicitly trust a target repository virtual environment.

Future runtime provisioning should support explicit, versioned execution profiles such as:

- Python
- Rust
- Node.js / JavaScript / TypeScript
- .NET

Those future profiles will likely need to define:

- toolchain and runtime version
- test/build tooling
- dependency environment and cache policy
- network policy
- reproducibility and provenance requirements
- project declaration or detection rules

This follow-up work is intentionally deferred. The current Rack AI tactical fix only establishes a Rack AI-owned Python 3.14 plus `pytest` executor image and preflight evidence for Python acceptance commands.
