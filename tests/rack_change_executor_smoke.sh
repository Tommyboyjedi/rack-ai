#!/usr/bin/env bash
set -euo pipefail

if ! command -v podman >/dev/null 2>&1; then
  cat >&2 <<'EOF'
BLOCKED: rootless Podman is not installed on this host.

Live isolation tests were not executed.
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
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fixture="$tmp/app"
mkdir -p "$fixture/src"
cat > "$fixture/Cargo.toml" <<'EOF'
[package]
name = "fixture"
version = "0.1.0"
edition = "2024"

[lib]
path = "src/lib.rs"
EOF
printf '#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
' > "$fixture/src/lib.rs"
(cd "$fixture" && cargo generate-lockfile >/dev/null)
git -C "$fixture" init -b main >/dev/null
git -C "$fixture" config user.email "test@example.com"
git -C "$fixture" config user.name "test"
git -C "$fixture" add .
git -C "$fixture" commit -m "init" >/dev/null
base_sha="$(git -C "$fixture" rev-parse HEAD)"

rack="$tmp/rack"
mkdir -p "$rack/config" "$rack/state/changes"
git -C "$rack" init -b main >/dev/null
cat > "$rack/config/repositories.json" <<EOF
{
  "workspace_root": "$tmp/workspaces",
  "executor": {"image": "docker.io/library/rust:bookworm"},
  "repositories": [{"id": "fixture", "root": "$fixture"}]
}
EOF

cat > "$tmp/change.json" <<EOF
{
  "change_id": "fixture-executor-001",
  "repository": {
    "id": "fixture",
    "registered_root": "$fixture",
    "base_ref": "main",
    "base_sha": "$base_sha"
  },
  "task": "Run deterministic checks inside the isolated workspace.",
  "allowed_paths": ["src/", "Cargo.toml"],
  "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": []},
  "limits": {"max_implementation_attempts": 1, "timeout_seconds": 120, "network": "disabled"}
}
EOF

output="$(cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- change "$tmp/change.json" --run-checks --repo-root "$rack" --state-root "$rack")"
echo "$output"

packet="$rack/state/changes/fixture-executor-001/review-packet.json"
test -f "$packet"
worktree="$tmp/workspaces/fixture-executor-001/repo"
grep -q 'status: checks_passed' <<< "$output"
grep -q 'acceptance_verdict: approved' <<< "$output"
grep -q 'cargo' "$packet"
grep -q '"status": "checks_passed"' "$packet"
grep -q '"acceptance_verdict": "approved"' "$packet"
grep -q '"exit_code": 0' "$packet"
test ! -d "$worktree/target"
test ! -d "$worktree/.rack-cargo"
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"

echo "rack_change_executor_smoke: ok"
