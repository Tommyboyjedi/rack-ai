#!/usr/bin/env bash
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="/tmp/rack-ai-pr17-qualification"

# Build the disposable repositories and campaign documents.
bash "$here/setup.sh"

# Rack AI's self-modification guard expects its supplied control root to be a
# Git repository, even though this root is only a disposable qualification
# control plane.
if ! git -C "$root/rack" rev-parse --git-dir >/dev/null 2>&1; then
  git -C "$root/rack" init -b main >/dev/null
  git -C "$root/rack" config user.email pr17@example.invalid
  git -C "$root/rack" config user.name rack-ai-pr17
  git -C "$root/rack" add .
  git -C "$root/rack" commit -m "PR17 disposable rack control root" >/dev/null
fi

# The Rust acceptance commands use Cargo. Seed Cargo.lock into the fixture
# before the campaign begins so running cargo check/build cannot create an
# out-of-policy file during an implementation attempt.
cargo generate-lockfile --offline --manifest-path "$root/tiny-ticket/Cargo.toml"
git -C "$root/tiny-ticket" add Cargo.lock
if ! git -C "$root/tiny-ticket" diff --cached --quiet; then
  git -C "$root/tiny-ticket" commit -m "Seed Cargo.lock for qualification" >/dev/null
fi

ticket_sha="$(git -C "$root/tiny-ticket" rev-parse HEAD)"

# These are qualification campaigns, not one-shot model benchmarks. Give the
# controller enough bounded recovery budget to act on its own diagnoses while
# keeping every attempt finite and acceptance unchanged.
python3 - "$root/campaigns/tiny-ticket.json" "$root/campaigns/tiny-dodge.json" "$ticket_sha" <<'PY'
import json
import sys
from pathlib import Path

ticket_path = Path(sys.argv[1])
game_path = Path(sys.argv[2])
ticket_sha = sys.argv[3]

for path in (ticket_path, game_path):
    data = json.loads(path.read_text())
    data["limits"]["max_runtime_seconds"] = 14400
    data["limits"]["max_total_attempts"] = 20
    data["worker_policy"]["primary_attempts"] = 1
    data["worker_policy"]["repair_attempts"] = 2
    data["worker_policy"]["fallback_attempts"] = 2
    if path == ticket_path:
        data["repository"]["base_sha"] = ticket_sha
    path.write_text(json.dumps(data, indent=2) + "\n")
PY

printf 'Prepared corrected PR17 qualification at %s\n' "$root"
printf 'Ticket base SHA: %s\n' "$ticket_sha"
printf 'Ticket campaign: %s\n' "$root/campaigns/tiny-ticket.json"
printf 'Game campaign:   %s\n' "$root/campaigns/tiny-dodge.json"
