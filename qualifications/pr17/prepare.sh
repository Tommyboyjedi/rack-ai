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
python3 - "$root/campaigns/tiny-ticket.json" "$ticket_sha" <<'PY'
import json
import sys
from pathlib import Path

path = Path(sys.argv[1])
sha = sys.argv[2]
data = json.loads(path.read_text())
data["repository"]["base_sha"] = sha
path.write_text(json.dumps(data, indent=2) + "\n")
PY

printf 'Prepared corrected PR17 qualification at %s\n' "$root"
printf 'Ticket base SHA: %s\n' "$ticket_sha"
printf 'Ticket campaign: %s\n' "$root/campaigns/tiny-ticket.json"
printf 'Game campaign:   %s\n' "$root/campaigns/tiny-dodge.json"
