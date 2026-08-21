#!/usr/bin/env bash
set -euo pipefail

if ! command -v podman >/dev/null 2>&1; then
  echo "BLOCKED: rootless Podman is not installed on this host." >&2
  exit 2
fi

rootless="$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null || true)"
if [[ "$rootless" != "true" ]]; then
  echo "BLOCKED: podman is present but not rootless." >&2
  exit 2
fi

if ! podman image exists docker.io/library/rust:bookworm; then
  echo "pulling docker.io/library/rust:bookworm on the host (job network stays disabled)" >&2
  podman pull docker.io/library/rust:bookworm
fi

if ! curl -fsS --max-time 2 http://127.0.0.1:8018/v1/models >/dev/null; then
  echo "BLOCKED: local-coder endpoint http://127.0.0.1:8018/v1 is not reachable." >&2
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
edition = "2021"

[lib]
path = "src/lib.rs"
EOF
cat > "$fixture/src/lib.rs" <<'EOF'
pub fn answer() -> i32 {
    0
}

#[cfg(test)]
mod tests {
    use super::answer;

    #[test]
    fn answer_is_42() {
        assert_eq!(answer(), 42);
    }
}
EOF
(cd "$fixture" && cargo generate-lockfile >/dev/null)
git -C "$fixture" init -b main >/dev/null
git -C "$fixture" config user.email "test@example.com"
git -C "$fixture" config user.name "test"
git -C "$fixture" add .
git -C "$fixture" commit -m "init" >/dev/null
base_sha="$(git -C "$fixture" rev-parse HEAD)"

rack="$tmp/rack"
mkdir -p "$rack/config" "$rack/state/changes"
cat > "$rack/config/repositories.json" <<EOF
{
  "workspace_root": "$tmp/workspaces",
  "executor": {"image": "docker.io/library/rust:bookworm"},
  "repositories": [{"id": "fixture", "root": "$fixture"}]
}
EOF

cat > "$tmp/change.json" <<EOF
{
  "change_id": "fixture-implement-001",
  "repository": {
    "id": "fixture",
    "registered_root": "$fixture",
    "base_ref": "main",
    "base_sha": "$base_sha"
  },
  "task": "Using tools, edit src/lib.rs so pub fn answer() returns 42. Do not modify any other files. Do not run cargo. When src/lib.rs is updated, reply exactly COMPLETE.",
  "allowed_paths": ["src/"],
  "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": ["src/lib.rs"]},
  "limits": {"max_implementation_attempts": 1, "timeout_seconds": 180, "network": "disabled"}
}
EOF

output="$(cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- change "$tmp/change.json" --repo-root "$rack" --state-root "$rack")"
echo "$output"

worktree="$tmp/workspaces/fixture-implement-001/repo"
packet="$rack/state/changes/fixture-implement-001/review-packet.json"
test -f "$packet"
grep -q 'status: checks_passed' <<< "$output"
grep -q 'acceptance_verdict: approved' <<< "$output"
grep -q '"status": "checks_passed"' "$packet"
grep -q '"acceptance_verdict": "approved"' "$packet"
test ! -d "$worktree/target"
test ! -d "$worktree/.rack-cargo"
grep -q 'src/lib.rs' "$packet"
grep -q '42' "$worktree/src/lib.rs"
test "$(git -C "$worktree" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"

echo "rack_change_implement_smoke: ok"
