#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_COORDINATOR_SMOKE.txt"

rm -f "$target"

python3 "$repo_root/bin/rack-coordinator"   --template patch   --cwd "$workdir"   --artifact-exact "$target=RACK_COORDINATOR_OK"   --run   "Create $target containing exactly RACK_COORDINATOR_OK. Then read the file and reply with exactly COMPLETE."

grep -qx 'RACK_COORDINATOR_OK' "$target"

echo "rack-coordinator smoke test passed"
