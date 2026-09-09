# Config

This directory holds version pins, model-role mappings, task templates, and policy/config files for the rack.

`repositories.json` registers external application repositories, administrator-approved trusted dynamic repository roots, and the rootless Podman executor used by `bin/rack-change`. The Rack AI repository itself is not a registered target.

Rack AI no longer maintains a language- or framework-specific acceptance executable allow-list. If `approved_programs` is still present in `repositories.json`, it is treated as a legacy no-op field kept only for backward-compatible config parsing.

Trusted dynamic roots are static administrator policy. A caller may point Rack AI at a concrete Git repository beneath one of those roots without adding a per-project config entry, but Rack AI still canonicalizes the path, rejects traversal/symlink escape, requires the requested path to be the Git top-level, and refuses the live Rack AI repository.

Trusted environment roots are a separate administrator policy. A caller may request an environment resource path beneath one of those roots, and Rack AI will expose that path inside the isolated executor as a read-only bind mount at the same absolute container path. Rack AI does not interpret whether that path contains a Python venv, Node toolchain, Rust toolchain, or anything else; it only authorizes and mounts an administrator-approved host path. Traversal, symlink escape, and live Rack AI self-path exposure are rejected.

Change jobs run with network disabled. The executor image must already be present on the host (`podman pull ...` is a host operation, not a job operation). Cargo home and target directories are tmpfs mounts at `/rack-build` inside the container, not the Git worktree. Projects that need crates.io or other downloads must use a pre-baked image or a vendored tree; the job will not fetch over the network.

Acceptance commands are trusted through the approved workspace and executor boundary, not by matching the executable name against a Rack AI-maintained tool list. Rack AI still enforces direct `argv` execution, worktree isolation, path-policy review, timeout/resource bounds, and network policy.

`bin/rack-change` records a deterministic `acceptance_verdict` from Git/path/acceptance gates. It does not yet run the local-primary planner/verifier DAG.


`operations.json` defines unattended supervision and retention policy for `bin/rack-campaign supervise`. It is versioned, validated at load time, and controls restart scan interval, stale-container cleanup command, terminal campaign retention bounds, and auxiliary artifact retention bounds.
