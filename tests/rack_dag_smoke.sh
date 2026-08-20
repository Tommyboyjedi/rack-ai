#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_DAG_SMOKE.txt"
spec="$(mktemp)"
status_mid="$workdir/rack_dag_status_mid.json"
status_final="$workdir/rack_dag_status_final.json"
task_id="dag-smoke-$(date -u +%Y%m%dT%H%M%SZ)"

rm -f "$target" "$status_mid" "$status_final"
rm -f "$repo_root/state/queue/queued/$task_id.json"
rm -f "$repo_root/state/queue/running/$task_id.json"
rm -f "$repo_root/state/runs/$task_id.json"
rm -f "$repo_root/state/queue/history/$task_id"*.json

cat > "$spec" <<EOF2
{
  "task_id": "$task_id",
  "template": "patch",
  "request": "Run a three-node DAG that plans, implements, and verifies creation of $target.",
  "max_attempts": 1,
  "timeout_seconds": 120,
  "dag": {
    "nodes": [
      {
        "id": "plan",
        "name": "plan",
        "worker": "local-primary",
        "cwd": "$workdir",
        "prompt": "Create a concise execution plan for creating $target with exactly RACK_DAG_OK, then reply with exactly COMPLETE."
      },
      {
        "id": "implement",
        "name": "implement",
        "worker": "local-coder",
        "cwd": "$workdir",
        "prompt": "Create $target containing exactly RACK_DAG_OK. Then read the file and reply with exactly COMPLETE.",
        "depends_on": ["plan"]
      },
      {
        "id": "verify",
        "name": "verify",
        "worker": "local-primary",
        "cwd": "$workdir",
        "prompt": "Verify that $target exists and contains exactly RACK_DAG_OK. Then reply with exactly COMPLETE.",
        "depends_on": ["implement"],
        "artifacts": [
          {
            "path": "$target",
            "exact_text": "RACK_DAG_OK"
          }
        ]
      }
    ]
  }
}
EOF2

"$repo_root/bin/rack-submit" "$spec"
python3 "$repo_root/bin/rack-runner" --once
"$repo_root/bin/rack-status" --emit-json > "$status_mid"
grep -q '"plan"' "$status_mid"
grep -q '"status": "succeeded"' "$status_mid"
python3 "$repo_root/bin/rack-runner" --once
python3 "$repo_root/bin/rack-runner" --once
"$repo_root/bin/rack-status" --emit-json > "$status_final"
grep -q '"status": "succeeded"' "$status_final"
grep -q '"verify"' "$status_final"
grep -qx 'RACK_DAG_OK' "$target"

rm -f "$spec" "$status_mid" "$status_final"
echo "rack-dag smoke test passed"
