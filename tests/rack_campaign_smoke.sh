#!/usr/bin/env bash
set -euo pipefail

if ! command -v podman >/dev/null 2>&1; then
  cat >&2 <<'EOF'
BLOCKED: rootless Podman is not installed on this host.

Live campaign tests were not executed.
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
edition = "2021"

[lib]
path = "src/lib.rs"
EOF
printf 'pub fn value() -> u8 { 1 }\n\n#[cfg(test)]\nmod tests {\n    #[test]\n    fn it_works() {\n        assert_eq!(super::value(), 1);\n    }\n}\n' > "$fixture/src/lib.rs"
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
cat > "$rack/config/repositories.json" <<EOF
{
  "workspace_root": "$tmp/workspaces",
  "approved_programs": ["cargo", "rustc", "true"],
  "executor": {"backend": "podman", "image": "docker.io/library/rust:bookworm", "workspace_path": "/workspace", "memory": "2g", "pids_limit": 256},
  "repositories": [{"id": "fixture", "root": "$fixture", "default_base_ref": "main", "enabled": true}]
}
EOF

cli() {
  cargo run -q -p rack_ai_cli --features campaign-test-seams --manifest-path "$repo_root/Cargo.toml" -- campaign "$@" --repo-root "$rack" --state-root "$rack"
}

operator_cli() {
  cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- campaign "$@" --repo-root "$rack" --state-root "$rack"
}

cat > "$tmp/two-step.json" <<EOF
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "fixture-two-step",
  "repository": {"id": "fixture", "base_ref": "main", "base_sha": "$base_sha"},
  "branch": "rack/campaign-fixture-two-step",
  "permitted_paths": ["src/", "Cargo.toml"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 600, "max_steps": 2, "max_total_attempts": 4, "heartbeat_seconds": 10, "network": "disabled"},
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
    },
    {
      "id": "add-beta",
      "kind": "implementation",
      "task": "Add src/beta.rs.",
      "allowed_paths": ["src/"],
      "required_changed_paths": ["src/beta.rs"],
      "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": ["src/beta.rs"]},
      "limits": {"timeout_seconds": 120, "network": "disabled"}
    }
  ]
}
EOF

cat > "$tmp/two-step-script.json" <<'EOF'
{
  "attempts": [
    {"writes": [{"path": "src/alpha.rs", "content": "pub fn alpha() -> u8 { 1 }\n"}], "output": "COMPLETE"},
    {"writes": [{"path": "src/beta.rs", "content": "pub fn beta() -> u8 { 2 }\n"}], "output": "COMPLETE"}
  ]
}
EOF

set +e
operator_out="$(operator_cli start "$tmp/two-step.json" --skip-live-health --fixture-implementer "$tmp/two-step-script.json" 2>&1)"
operator_rc=$?
set -e
test "$operator_rc" -ne 0
echo "$operator_out" | grep -q "unsupported campaign flag"

cli start "$tmp/two-step.json" --skip-live-health --fixture-implementer "$tmp/two-step-script.json"
worktree="$tmp/workspaces/campaign-fixture-two-step/repo"
test "$(git -C "$worktree" branch --show-current)" = "rack/campaign-fixture-two-step"
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$fixture" branch --show-current)" = "main"
test "$(git -C "$worktree" rev-list --count HEAD)" = "3"
second="$(git -C "$worktree" rev-parse HEAD)"
first="$(git -C "$worktree" rev-parse HEAD^)"
test "$first" != "$base_sha"
test "$(git -C "$worktree" log -1 --format=%s)" = "rack(fixture-two-step): add-beta"
test "$(git -C "$worktree" log -1 --format=%s "$first")" = "rack(fixture-two-step): add-alpha"
test ! -d "$worktree/target"
status_json="$(cli status fixture-two-step --emit-json)"
echo "$status_json" | grep -q '"state": "completed"'
echo "$status_json" | grep -q "$second"
cli events fixture-two-step --emit-json | grep -q campaign_completed
cli inspect fixture-two-step --step add-alpha | grep -q accepted

cli runner fixture-two-step --skip-live-health --fixture-implementer "$tmp/two-step-script.json"
test "$(git -C "$worktree" rev-parse HEAD)" = "$second"

cat > "$tmp/noop.json" <<EOF
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "fixture-noop",
  "repository": {"id": "fixture", "base_ref": "main", "base_sha": "$base_sha"},
  "branch": "rack/campaign-fixture-noop",
  "permitted_paths": ["src/"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 600, "max_steps": 1, "max_total_attempts": 1, "heartbeat_seconds": 10, "network": "disabled"},
  "worker_policy": {"primary": "local-coder", "fallback": "local-primary", "primary_attempts": 1, "repair_attempts": 0, "fallback_attempts": 0},
  "steps": [
    {
      "id": "noop",
      "kind": "implementation",
      "task": "Add src/alpha.rs.",
      "allowed_paths": ["src/"],
      "required_changed_paths": ["src/alpha.rs"],
      "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": ["src/alpha.rs"]},
      "limits": {"timeout_seconds": 120, "network": "disabled"}
    }
  ]
}
EOF
cat > "$tmp/noop-script.json" <<'EOF'
{"attempts": [{"writes": [], "output": "COMPLETE"}]}
EOF
set +e
cli start "$tmp/noop.json" --skip-live-health --fixture-implementer "$tmp/noop-script.json"
noop_rc=$?
set -e
test "$noop_rc" -ne 0
cli status fixture-noop --emit-json | grep -q no_change
test "$(git -C "$tmp/workspaces/campaign-fixture-noop/repo" rev-parse HEAD)" = "$base_sha"

cat > "$tmp/policy.json" <<EOF
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "fixture-policy",
  "repository": {"id": "fixture", "base_ref": "main", "base_sha": "$base_sha"},
  "branch": "rack/campaign-fixture-policy",
  "permitted_paths": ["src/", "README.md"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 600, "max_steps": 1, "max_total_attempts": 1, "heartbeat_seconds": 10, "network": "disabled"},
  "worker_policy": {"primary": "local-coder", "fallback": "local-primary", "primary_attempts": 1, "repair_attempts": 0, "fallback_attempts": 0},
  "steps": [
    {
      "id": "escape",
      "kind": "implementation",
      "task": "Add src/alpha.rs only.",
      "allowed_paths": ["src/"],
      "required_changed_paths": ["src/alpha.rs"],
      "acceptance": {"commands": [["cargo", "test"]], "required_artifacts": ["src/alpha.rs"]},
      "limits": {"timeout_seconds": 120, "network": "disabled"}
    }
  ]
}
EOF
cat > "$tmp/policy-script.json" <<'EOF'
{"attempts": [{"writes": [{"path": "README.md", "content": "pwned\n"}], "output": "COMPLETE"}]}
EOF
set +e
cli start "$tmp/policy.json" --skip-live-health --fixture-implementer "$tmp/policy-script.json"
policy_rc=$?
set -e
test "$policy_rc" -ne 0
cli status fixture-policy --emit-json | grep -q path_policy_failed
test "$(git -C "$tmp/workspaces/campaign-fixture-policy/repo" rev-parse HEAD)" = "$base_sha"

test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"
echo "rack_campaign_smoke: ok"
