#!/usr/bin/env bash
set -euo pipefail

if ! command -v podman >/dev/null 2>&1; then
  cat >&2 <<'EOF'
BLOCKED: rootless Podman is not installed on this host.

Live path-policy isolation tests were not executed.
EOF
  exit 2
fi

rootless="$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null || true)"
if [[ "$rootless" != "true" ]]; then
  cat >&2 <<EOF
BLOCKED: podman is present but not rootless (Host.Security.Rootless=${rootless:-unknown}).
EOF
  exit 2
fi

if ! podman image exists docker.io/library/rust:bookworm; then
  cat >&2 <<'EOF'
BLOCKED: required executor image is not present locally.

Run:
  podman pull docker.io/library/rust:bookworm
EOF
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
before_sha="$(git -C "$repo_root" rev-parse HEAD)"

export RACK_AI_LIVE_PATH_POLICY_SMOKE=1
output="$(cargo test -p rack_ai_infrastructure --lib \
  live_path_policy::live_podman_bash_forbidden_write_rejected_by_path_gate \
  --manifest-path "$repo_root/Cargo.toml" \
  -- --exact --nocapture 2>&1)"
echo "$output"
echo "$output" | grep -q "running 1 test"
echo "$output" | grep -q "test live_path_policy::live_podman_bash_forbidden_write_rejected_by_path_gate ... ok"
echo "$output" | grep -q "1 passed"

test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"
echo "rack_change_path_policy_smoke: ok"
