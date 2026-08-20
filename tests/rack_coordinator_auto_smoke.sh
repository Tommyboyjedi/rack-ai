#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_COORDINATOR_AUTO.txt"
preview="$(mktemp)"

rm -f "$target"

python3 "$repo_root/bin/rack-coordinator"   --auto-template   --cwd "$workdir"   --artifact-exact "$target=RACK_COORDINATOR_AUTO_OK"   --preview   "Create $target containing exactly RACK_COORDINATOR_AUTO_OK. Then read the file and reply with exactly COMPLETE." > "$preview"

grep -q '"template": "patch"' "$preview"

python3 "$repo_root/bin/rack-coordinator"   --auto-template   --cwd "$workdir"   --artifact-exact "$target=RACK_COORDINATOR_AUTO_OK"   --run   "Create $target containing exactly RACK_COORDINATOR_AUTO_OK. Then read the file and reply with exactly COMPLETE."

grep -qx 'RACK_COORDINATOR_AUTO_OK' "$target"
rm -f "$preview"

echo "rack-coordinator auto smoke test passed"
