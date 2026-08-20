#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_TASK_SMOKE.txt"
spec="$(mktemp)"

rm -f "$target"

cat > "$spec" <<EOF
{
  "worker": "local-coder",
  "cwd": "$workdir",
  "prompt": "Create $target containing exactly RACK_TASK_SMOKE_OK. Then read the file and reply with exactly COMPLETE.",
  "artifacts": [
    {
      "path": "$target",
      "exact_text": "RACK_TASK_SMOKE_OK"
    }
  ]
}
EOF

python3 "$repo_root/bin/rack-task" --emit-json "$spec"

test -f "$target"
grep -qx 'RACK_TASK_SMOKE_OK' "$target"

rm -f "$spec"

echo "rack-task smoke test passed"
