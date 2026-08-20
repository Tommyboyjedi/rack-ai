# Config

This directory holds version pins, model-role mappings, task templates, and policy/config files for the rack.

`repositories.json` registers external application repositories and the rootless Podman executor used by `bin/rack-change`. The Rack AI repository itself is not a registered target.

Change jobs run with network disabled. The executor image must already be present on the host (`podman pull ...` is a host operation, not a job operation). Cargo writes caches into the worktree at `.rack-cargo/` and `target/` because the container root filesystem is read-only. Projects that need crates.io or other downloads must use a pre-baked image or a vendored tree inside the worktree; the job will not fetch over the network.
