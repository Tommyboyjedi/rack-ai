#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$(mktemp)"

python3 "$repo_root/bin/rack-healthcheck" --emit-json > "$out"
grep -q '"worker_id": "local-primary"' "$out"
grep -q '"worker_id": "local-coder"' "$out"
grep -q '"ok": true' "$out"
rm -f "$out"

echo "rack-healthcheck smoke test passed"
