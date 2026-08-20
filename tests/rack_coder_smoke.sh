#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_CODER_SMOKE.txt"

rm -f "$target"

"$repo_root/bin/rack-coder" --cwd "$workdir" -- "Create $target containing exactly RACK_CODER_SMOKE_OK. Then read the file and reply with exactly COMPLETE."

test -f "$target"
grep -qx 'RACK_CODER_SMOKE_OK' "$target"

echo "rack-coder smoke test passed"
