#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
before_sha="$(git -C "$repo_root" rev-parse HEAD)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fixture="$tmp/app"
mkdir -p "$fixture/src"
printf 'pub fn value() -> u8 { 1 }\n' > "$fixture/src/lib.rs"
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
  "change_id": "fixture-001",
  "repository": {
    "id": "fixture",
    "registered_root": "$fixture",
    "base_ref": "main",
    "base_sha": "$base_sha"
  },
  "task": "Add a bounded feature with tests.",
  "allowed_paths": ["src/", "Cargo.toml"],
  "acceptance": {"commands": [["cargo", "test"]]},
  "limits": {"max_implementation_attempts": 2, "timeout_seconds": 90, "network": "disabled"}
}
EOF

output="$(cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- change "$tmp/change.json" --prepare-only --repo-root "$rack" --state-root "$rack")"
echo "$output"

worktree="$tmp/workspaces/fixture-001/repo"
packet="$rack/state/changes/fixture-001/review-packet.json"
test -d "$worktree"
test -f "$packet"
test "$(git -C "$worktree" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$worktree" branch --show-current)" = "rack/change-fixture-001"
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$fixture" branch --show-current)" = "main"
grep -q "$base_sha" "$packet"
grep -q "rack/change-fixture-001" "$packet"
grep -q '"status": "prepared"' "$packet"

test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"

echo "rack_change_smoke: ok"
