#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
root="/tmp/rack-ai-pr17-qualification"
rm -rf "$root"
mkdir -p "$root/rack/config" "$root/rack/state/campaigns" "$root/workspaces" "$root/campaigns"
cp "$repo_root/config/workers.json" "$root/rack/config/workers.json"
cp "$repo_root/config/models.json" "$root/rack/config/models.json"
cp "$repo_root/config/operations.json" "$root/rack/config/operations.json"

make_git_repo() {
  local dir="$1"
  git -C "$dir" init -b main >/dev/null
  git -C "$dir" config user.email pr17@example.invalid
  git -C "$dir" config user.name rack-ai-pr17
  git -C "$dir" add .
  git -C "$dir" commit -m "PR17 qualification fixture" >/dev/null
}

# Campaign 1 fixture: Rust ticket tracker.
ticket="$root/tiny-ticket"
mkdir -p "$ticket/src" "$ticket/tests"
cat > "$ticket/Cargo.toml" <<'EOF'
[package]
name = "tiny-ticket"
version = "0.1.0"
edition = "2021"

[lib]
path = "src/lib.rs"

[[bin]]
name = "tiny-ticket"
path = "src/main.rs"
EOF
printf '//! Tiny Ticket qualification fixture.\n' > "$ticket/src/lib.rs"
printf 'fn main() { eprintln!("tiny-ticket is not implemented yet"); }\n' > "$ticket/src/main.rs"
cat > "$ticket/tests/check_domain.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
grep -q 'pub struct Ticket' src/lib.rs
grep -q 'pub enum Status' src/lib.rs
cargo check --offline
EOF
cat > "$ticket/tests/check_store.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test -f src/store.rs
grep -q 'load' src/store.rs
grep -q 'save' src/store.rs
cargo check --offline
EOF
cat > "$ticket/tests/check_cli.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cargo build --offline --quiet
bin="${CARGO_TARGET_DIR:-target}/debug/tiny-ticket"
tmp="$(mktemp)"; rm -f "$tmp"; trap 'rm -f "$tmp"' EXIT
out="$("$bin" create "$tmp" First ticket)"
[[ "$out" == "created 1" ]]
"$bin" create "$tmp" Second ticket >/dev/null
"$bin" close "$tmp" 1 | grep -qx 'closed 1'
"$bin" list "$tmp" | grep -qx '1|closed|First ticket'
"$bin" list "$tmp" | grep -qx '2|open|Second ticket'
EOF
cat > "$ticket/tests/check_final.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
cargo test --offline
bash tests/check_cli.sh
test -s README.md
bin="${CARGO_TARGET_DIR:-target}/debug/tiny-ticket"
if "$bin" create /tmp/pr17-ticket-bad 'bad|title' >/dev/null 2>&1; then exit 1; fi
rm -f /tmp/pr17-ticket-bad
EOF
chmod +x "$ticket/tests/"*.sh
make_git_repo "$ticket"
ticket_sha="$(git -C "$ticket" rev-parse HEAD)"

# Campaign 2 fixture: dependency-free browser game.
game="$root/tiny-dodge"
mkdir -p "$game/tests"
printf '<!doctype html><html><head><meta charset="utf-8"><title>Tiny Dodge</title></head><body><h1>Tiny Dodge</h1></body></html>\n' > "$game/index.html"
: > "$game/game.js"
: > "$game/style.css"
cat > "$game/tests/check_layout.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
grep -q 'id="game"' index.html
grep -q 'id="player"' index.html
grep -q 'id="score"' index.html
grep -Eq 'id="(start|restart)' index.html
grep -q 'game.js' index.html
grep -q 'style.css' index.html
EOF
cat > "$game/tests/check_logic.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
grep -q 'requestAnimationFrame' game.js
grep -q 'keydown' game.js
grep -Ei 'collis' game.js >/dev/null
grep -Ei 'score' game.js >/dev/null
grep -Ei 'restart|startGame' game.js >/dev/null
EOF
cat > "$game/tests/check_style.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
test -s style.css
grep -q '#game' style.css
grep -q '#player' style.css
grep -Ei 'obstacle' style.css >/dev/null
EOF
cat > "$game/tests/check_final.sh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
bash tests/check_layout.sh
bash tests/check_logic.sh
bash tests/check_style.sh
if grep -ERn 'https?://' index.html game.js style.css; then exit 1; fi
grep -Ei 'game over|gameOver' game.js >/dev/null
EOF
chmod +x "$game/tests/"*.sh
make_git_repo "$game"
game_sha="$(git -C "$game" rev-parse HEAD)"

cat > "$root/rack/config/repositories.json" <<EOF
{
  "workspace_root": "$root/workspaces",
  "approved_programs": ["cargo", "rustc", "bash", "true"],
  "executor": {"backend": "podman", "image": "docker.io/library/rust:bookworm", "workspace_path": "/workspace", "memory": "2g", "pids_limit": 256},
  "repositories": [
    {"id": "pr17-tiny-ticket", "root": "$ticket", "default_base_ref": "main", "enabled": true},
    {"id": "pr17-tiny-dodge", "root": "$game", "default_base_ref": "main", "enabled": true}
  ]
}
EOF

cat > "$root/campaigns/tiny-ticket.json" <<EOF
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "pr17-tiny-ticket",
  "repository": {"id": "pr17-tiny-ticket", "base_ref": "main", "base_sha": "$ticket_sha"},
  "branch": "rack/campaign-pr17-tiny-ticket",
  "permitted_paths": ["src/", "README.md"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 7200, "max_steps": 4, "max_total_attempts": 12, "heartbeat_seconds": 10, "network": "disabled"},
  "worker_policy": {"primary": "local-coder", "fallback": "local-primary", "primary_attempts": 1, "repair_attempts": 1, "fallback_attempts": 1},
  "steps": [
    {"id":"domain","kind":"implementation","task":"Implement the Tiny Ticket domain in src/lib.rs. Define public Ticket and Status types. Ticket must have numeric id, title, and open/closed status. Reject empty titles and titles containing |. Keep the implementation dependency-free and compatible with the existing Cargo.toml.","allowed_paths":["src/"],"required_changed_paths":["src/lib.rs"],"acceptance":{"commands":[["bash","tests/check_domain.sh"]],"required_artifacts":["src/lib.rs"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"persistence","kind":"implementation","task":"Add dependency-free file persistence in src/store.rs and wire it through src/lib.rs. Persist one ticket per line using id|status|title, load missing files as an empty store, preserve ids, and provide deterministic next-id behaviour.","allowed_paths":["src/"],"required_changed_paths":["src/store.rs"],"acceptance":{"commands":[["bash","tests/check_store.sh"]],"required_artifacts":["src/store.rs"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"cli","kind":"implementation","task":"Implement src/main.rs as the Tiny Ticket CLI. Exact commands: tiny-ticket create <store> <title...> prints created <id>; tiny-ticket list <store> prints <id>|<open|closed>|<title> in id order; tiny-ticket close <store> <id> prints closed <id>. Invalid input must exit non-zero with a useful error.","allowed_paths":["src/"],"required_changed_paths":["src/main.rs"],"acceptance":{"commands":[["bash","tests/check_cli.sh"]],"required_artifacts":["src/main.rs"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"finish","kind":"implementation","task":"Finish and polish Tiny Ticket without changing its required CLI contract. Add useful Rust unit tests inside src files where appropriate and create a concise README.md with build and usage examples. Fix any remaining robustness issues.","allowed_paths":["src/","README.md"],"required_changed_paths":["README.md"],"acceptance":{"commands":[["bash","tests/check_final.sh"]],"required_artifacts":["README.md","src/lib.rs","src/main.rs","src/store.rs"]},"limits":{"timeout_seconds":600,"network":"disabled"}}
  ]
}
EOF

cat > "$root/campaigns/tiny-dodge.json" <<EOF
{
  "version": "rack-ai/campaign/v1",
  "campaign_id": "pr17-tiny-dodge",
  "repository": {"id": "pr17-tiny-dodge", "base_ref": "main", "base_sha": "$game_sha"},
  "branch": "rack/campaign-pr17-tiny-dodge",
  "permitted_paths": ["index.html", "game.js", "style.css", "README.md"],
  "allow_local_commits": true,
  "limits": {"max_runtime_seconds": 7200, "max_steps": 4, "max_total_attempts": 12, "heartbeat_seconds": 10, "network": "disabled"},
  "worker_policy": {"primary": "local-coder", "fallback": "local-primary", "primary_attempts": 1, "repair_attempts": 1, "fallback_attempts": 1},
  "steps": [
    {"id":"layout","kind":"implementation","task":"Build the Tiny Dodge page structure in index.html. It must reference style.css and game.js and contain elements with ids game, player, score, and start or restart. Keep it dependency-free and suitable for opening directly from disk.","allowed_paths":["index.html"],"required_changed_paths":["index.html"],"acceptance":{"commands":[["bash","tests/check_layout.sh"]],"required_artifacts":["index.html"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"logic","kind":"implementation","task":"Implement Tiny Dodge in game.js. Left/right keyboard input moves the player, obstacles fall over time, requestAnimationFrame drives the loop, collision ends the game, survival increases score, and the start/restart control resets all state. Use plain browser JavaScript only.","allowed_paths":["game.js"],"required_changed_paths":["game.js"],"acceptance":{"commands":[["bash","tests/check_logic.sh"]],"required_artifacts":["game.js"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"style","kind":"implementation","task":"Create a compact attractive game presentation in style.css. Style #game as the bounded playfield, #player clearly, obstacles visibly, and score/control elements legibly. Keep everything local and dependency-free.","allowed_paths":["style.css"],"required_changed_paths":["style.css"],"acceptance":{"commands":[["bash","tests/check_style.sh"]],"required_artifacts":["style.css"]},"limits":{"timeout_seconds":600,"network":"disabled"}},
    {"id":"finish","kind":"implementation","task":"Polish Tiny Dodge as a complete tiny browser game. Ensure game-over feedback is visible, restart works, files remain dependency-free with no external URLs, and add README.md explaining controls and how to open the game.","allowed_paths":["index.html","game.js","style.css","README.md"],"required_changed_paths":["README.md"],"acceptance":{"commands":[["bash","tests/check_final.sh"]],"required_artifacts":["index.html","game.js","style.css","README.md"]},"limits":{"timeout_seconds":600,"network":"disabled"}}
  ]
}
EOF

printf 'Prepared PR17 qualification at %s\n' "$root"
printf 'Ticket campaign: %s\n' "$root/campaigns/tiny-ticket.json"
printf 'Game campaign:   %s\n' "$root/campaigns/tiny-dodge.json"
