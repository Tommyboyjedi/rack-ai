#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_QUEUE_SMOKE.txt"
spec="$(mktemp)"
task_id="queue-smoke-$(date -u +%Y%m%dT%H%M%SZ)"

rm -f "$target"
rm -f "$repo_root/state/queue/queued/$task_id.json"
rm -f "$repo_root/state/queue/running/$task_id.json"
rm -f "$repo_root/state/runs/$task_id.json"
rm -f "$repo_root/state/queue/history/$task_id"*.json

cat > "$spec" <<EOF
{
  "task_id": "$task_id",
  "template": "patch",
  "request": "Create $target containing exactly RACK_QUEUE_OK. Then read the file and reply with exactly COMPLETE.",
  "max_attempts": 1,
  "timeout_seconds": 120,
  "steps": [
    {
      "name": "implement",
      "worker": "local-coder",
      "cwd": "$workdir",
      "prompt": "Create $target containing exactly RACK_QUEUE_OK. Then read the file and reply with exactly COMPLETE.",
      "artifacts": [
        {
          "path": "$target",
          "exact_text": "RACK_QUEUE_OK"
        }
      ]
    }
  ]
}
EOF

"$repo_root/bin/rack-submit" "$spec"
"$repo_root/bin/rack-status" --emit-json > "$workdir/rack_queue_status_before.json"
grep -q "$task_id.json" "$workdir/rack_queue_status_before.json"
"$repo_root/bin/rack-runner" --once
"$repo_root/bin/rack-status" --emit-json > "$workdir/rack_queue_status_after.json"
grep -q '"status": "succeeded"' "$workdir/rack_queue_status_after.json"
grep -qx 'RACK_QUEUE_OK' "$target"
rm -f "$spec" "$workdir/rack_queue_status_before.json" "$workdir/rack_queue_status_after.json"

echo "rack-queue smoke test passed"
