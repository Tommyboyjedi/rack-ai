#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

fixture="$tmp/app"
mkdir -p "$fixture/src"
cat > "$fixture/Cargo.toml" <<'CARGO'
[package]
name = "fixture"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"
CARGO
printf 'pub fn value() -> u8 { 1 }\n' > "$fixture/src/lib.rs"
(cd "$fixture" && cargo generate-lockfile >/dev/null)
git -C "$fixture" init -b main >/dev/null
git -C "$fixture" config user.email "test@example.com"
git -C "$fixture" config user.name "test"
git -C "$fixture" add .
git -C "$fixture" commit -m "init" >/dev/null
base_sha="$(git -C "$fixture" rev-parse HEAD)"

rack="$tmp/rack"
mkdir -p "$rack/config" "$rack/state/campaigns"
cp "$repo_root/config/workers.json" "$rack/config/workers.json"
cp "$repo_root/config/models.json" "$rack/config/models.json"
cp "$repo_root/config/operations.json" "$rack/config/operations.json"
cat > "$rack/config/repositories.json" <<JSON
{
  "workspace_root": "$tmp/workspaces",
  "approved_programs": ["cargo", "rustc", "true"],
  "executor": {"backend": "podman", "image": "docker.io/library/rust:bookworm", "workspace_path": "/workspace", "memory": "2g", "pids_limit": 256},
  "repositories": [{"id": "fixture", "root": "$fixture", "default_base_ref": "main", "enabled": true}]
}
JSON

cli() {
  cargo run -q -p rack_ai_cli --features campaign-test-seams --manifest-path "$repo_root/Cargo.toml" -- campaign "$@" --repo-root "$rack" --state-root "$rack"
}

cat > "$tmp/campaign.json" <<JSON
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "fixture-supervision",
  "repository": {"id": "fixture", "base_ref": "main", "base_sha": "$base_sha"},
  "branch": "rack/campaign-fixture-supervision",
  "permitted_paths": ["src/"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 600, "max_steps": 1, "max_total_attempts": 1, "heartbeat_seconds": 10, "network": "disabled"},
  "worker_policy": {"primary": "local-coder", "fallback": "local-primary", "primary_attempts": 1, "repair_attempts": 0, "fallback_attempts": 0},
  "steps": [
    {
      "id": "add-alpha",
      "kind": "implementation",
      "task": "Add src/alpha.rs.",
      "allowed_paths": ["src/"],
      "required_changed_paths": ["src/alpha.rs"],
      "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": ["src/alpha.rs"]},
      "limits": {"timeout_seconds": 120, "network": "disabled"}
    }
  ]
}
JSON

cat > "$tmp/script.json" <<'JSON'
{
  "attempts": [
    {"writes": [{"path": "src/alpha.rs", "content": "pub fn alpha() -> u8 { 1 }\n"}], "output": "COMPLETE"}
  ]
}
JSON

cli validate "$tmp/campaign.json" --skip-live-health --fixture-implementer "$tmp/script.json" >/dev/null
cli start "$tmp/campaign.json" --skip-live-health --fixture-implementer "$tmp/script.json" >/dev/null
worktree="$tmp/workspaces/campaign-fixture-supervision/repo"
commit_count_before="$(git -C "$worktree" rev-list --count HEAD)"
python3 - <<PY2
import json
from pathlib import Path
state_path = Path("$rack/state/campaigns/fixture-supervision/state.json")
state = json.loads(state_path.read_text())
state["state"] = "running"
state["end_time"] = None
state["current_action"] = "recovering"
state_path.write_text(json.dumps(state, indent=2) + "\n")
PY2
status_before="$(cli status fixture-supervision --emit-json)"
echo "$status_before" | grep -q '"state": "running"'
report="$(cli supervise --emit-json --skip-live-health --fixture-implementer "$tmp/script.json")"
echo "$report" | grep -q '"resumed_campaigns": 1'
echo "$report" | grep -q '"action": "resume"'
status_after="$(cli status fixture-supervision --emit-json)"
echo "$status_after" | grep -q '"state": "completed"'
test "$(git -C "$worktree" rev-list --count HEAD)" = "$commit_count_before"

echo "rack_campaign_supervision_smoke: ok"
