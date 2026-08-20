#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
workdir="/tmp/jcode-rack-test"
plan="$workdir/PIPELINE_PLAN.txt"
output="$workdir/PIPELINE_OUTPUT.txt"
verify="$workdir/PIPELINE_VERIFY.txt"
spec="$(mktemp)"

rm -f "$plan" "$output" "$verify"

cat > "$spec" <<EOF
{
  "steps": [
    {
      "name": "plan",
      "worker": "local-primary",
      "cwd": "$workdir",
      "prompt": "Create $plan containing exactly PLAN_READY. Then read the file and reply with exactly COMPLETE.",
      "artifacts": [
        {
          "path": "$plan",
          "exact_text": "PLAN_READY"
        }
      ]
    },
    {
      "name": "implement",
      "worker": "local-coder",
      "cwd": "$workdir",
      "prompt": "Create $output containing exactly PIPELINE_SMOKE_OK. Then read the file and reply with exactly COMPLETE.",
      "artifacts": [
        {
          "path": "$output",
          "exact_text": "PIPELINE_SMOKE_OK"
        }
      ]
    },
    {
      "name": "verify",
      "worker": "local-primary",
      "cwd": "$workdir",
      "prompt": "Read $output. Then create $verify containing exactly VERIFIED_SMOKE_OK. Then read the file and reply with exactly COMPLETE.",
      "artifacts": [
        {
          "path": "$verify",
          "exact_text": "VERIFIED_SMOKE_OK"
        },
        {
          "path": "$output",
          "exact_text": "PIPELINE_SMOKE_OK"
        }
      ]
    }
  ]
}
EOF

"$repo_root/bin/rack-task" --emit-json "$spec"

grep -qx 'PLAN_READY' "$plan"
grep -qx 'PIPELINE_SMOKE_OK' "$output"
grep -qx 'VERIFIED_SMOKE_OK' "$verify"

rm -f "$spec"

echo "rack-pipeline smoke test passed"
