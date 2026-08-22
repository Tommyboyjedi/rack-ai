# Config

This directory holds version pins, model-role mappings, task templates, and policy/config files for the rack.

`repositories.json` registers external application repositories and the rootless Podman executor used by `bin/rack-change`. The Rack AI repository itself is not a registered target.

Change jobs run with network disabled. The executor image must already be present on the host (`podman pull ...` is a host operation, not a job operation). Cargo home and target directories are tmpfs mounts at `/rack-build` inside the container, not the Git worktree. Projects that need crates.io or other downloads must use a pre-baked image or a vendored tree; the job will not fetch over the network.

`bin/rack-change` records a deterministic `acceptance_verdict` from Git/path/acceptance gates. It does not yet run the local-primary planner/verifier DAG.


`operations.json` defines unattended supervision and retention policy for `bin/rack-campaign supervise`. It is versioned, validated at load time, and controls restart scan interval, stale-container cleanup command, terminal campaign retention bounds, and auxiliary artifact retention bounds.
