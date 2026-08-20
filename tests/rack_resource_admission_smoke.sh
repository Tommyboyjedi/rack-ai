#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
target="$workdir/RACK_RESOURCE_ADMISSION.txt"
spec="$(mktemp)"
status_before="$workdir/rack_resource_status_before.json"
status_after="$workdir/rack_resource_status_after.json"
task_id="resource-admission-smoke-$(date -u +%Y%m%dT%H%M%SZ)"
lease_path="$repo_root/state/resources/leases/gpu-2060.json"

rm -f "$target" "$status_before" "$status_after"
rm -f "$repo_root/state/queue/queued/$task_id.json"
rm -f "$repo_root/state/queue/running/$task_id.json"
rm -f "$repo_root/state/runs/$task_id.json"
rm -f "$repo_root/state/queue/history/$task_id"*.json
rm -f "$lease_path"

cat > "$spec" <<EOF2
{
  "task_id": "$task_id",
  "template": "patch",
  "request": "Create $target containing exactly RACK_RESOURCE_OK. Then read the file and reply with exactly COMPLETE.",
  "max_attempts": 1,
  "timeout_seconds": 120,
  "steps": [
    {
      "name": "implement",
      "worker": "local-coder",
      "cwd": "$workdir",
      "prompt": "Create $target containing exactly RACK_RESOURCE_OK. Then read the file and reply with exactly COMPLETE.",
      "artifacts": [
        {
          "path": "$target",
          "exact_text": "RACK_RESOURCE_OK"
        }
      ]
    }
  ]
}
EOF2

"$repo_root/bin/rack-submit" "$spec"
cat > "$lease_path" <<EOF2
{
  "task_id": "external-holder",
  "resource_id": "gpu-2060",
  "worker_ids": ["external-worker"],
  "model_ids": ["external-model"],
  "acquired_at": "2026-08-20T00:00:00Z"
}
EOF2
python3 "$repo_root/bin/rack-runner" --once
"$repo_root/bin/rack-status" --emit-json > "$status_before"
grep -q '"admission_state": "waiting_for_resources"' "$status_before"
grep -q '"gpu-2060"' "$status_before"
if [[ -e "$target" ]]; then
  echo "target should not exist while resource is leased" >&2
  exit 1
fi

rm -f "$lease_path"
python3 "$repo_root/bin/rack-runner" --once
"$repo_root/bin/rack-status" --emit-json > "$status_after"
grep -q '"status": "succeeded"' "$status_after"
grep -qx 'RACK_RESOURCE_OK' "$target"

rm -f "$spec" "$status_before" "$status_after" "$lease_path"
echo "rack-resource-admission smoke test passed"
