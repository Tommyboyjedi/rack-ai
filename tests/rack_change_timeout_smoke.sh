#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
before_sha="$(git -C "$repo_root" rev-parse HEAD)"
tmp_root="$(mktemp -d)"
trap 'rm -rf "$tmp_root"' EXIT

fixture="$tmp_root/app"
rack="$tmp_root/rack"
script="$tmp_root/fake-jcode.sh"
mkdir -p "$fixture" "$rack/config" "$rack/state/changes"

git -C "$fixture" init -b main >/dev/null
git -C "$fixture" config user.email "test@example.com"
git -C "$fixture" config user.name "test"
printf 'seed\n' > "$fixture/README.md"
git -C "$fixture" add .
git -C "$fixture" commit -m "init" >/dev/null
base_sha="$(git -C "$fixture" rev-parse HEAD)"

git -C "$rack" init -b main >/dev/null
cp "$repo_root/config/models.json" "$rack/config/models.json"
cp "$repo_root/config/workers.json" "$rack/config/workers.json"
python3 - <<'PY' "$rack/config/workers.json" "$script"
import json
import sys

path, entrypoint = sys.argv[1:]
with open(path, "r", encoding="utf-8") as handle:
    document = json.load(handle)
for worker in document["workers"]:
    if worker["id"] == "local-coder":
        worker["entrypoint"] = entrypoint
with open(path, "w", encoding="utf-8") as handle:
    json.dump(document, handle, indent=2)
    handle.write("\n")
PY
cat > "$rack/config/repositories.json" <<EOF
{
  "workspace_root": "$tmp_root/workspaces",
  "executor": {"backend": "host"},
  "trusted_dynamic_roots": [{"id": "tmp-root", "root": "$tmp_root", "enabled": true}],
  "repositories": []
}
EOF
cat > "$script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '# timed out\n' > README.md
python3 - <<'PY'
import subprocess
import time

subprocess.Popen(
    [
        "python3",
        "-c",
        "import os,signal,time; signal.signal(signal.SIGTERM, signal.SIG_IGN); os.setsid(); print('descendant open stdout', flush=True); time.sleep(300)",
    ]
)
print("parent running", flush=True)
time.sleep(300)
PY
EOF
chmod +x "$script"
cat > "$tmp_root/change.json" <<EOF
{
  "change_id": "fixture-timeout-001",
  "repository": {
    "id": "fixture",
    "root": "$fixture",
    "base_ref": "main",
    "base_sha": "$base_sha"
  },
  "task": "Edit README.md.",
  "allowed_paths": ["README.md"],
  "acceptance": {"commands": [["/bin/true"]], "required_artifacts": ["README.md"]},
  "limits": {"max_implementation_attempts": 1, "timeout_seconds": 2, "network": "disabled"}
}
EOF

set +e
output="$(cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- change "$tmp_root/change.json" --repo-root "$rack" --state-root "$rack" 2>&1)"
status=$?
set -e
echo "$output"

packet="$rack/state/changes/fixture-timeout-001/review-packet.json"
test -f "$packet"
test "$status" -eq 1
grep -q 'status: failed' <<< "$output"
grep -q 'acceptance_verdict: rejected' <<< "$output"
if grep -q 'accepted_revision:' <<< "$output"; then
  echo "unexpected accepted revision on timed-out implementation" >&2
  exit 1
fi
grep -q '"status": "failed"' "$packet"
grep -q '"acceptance_verdict": "rejected"' "$packet"
grep -q 'worker_provenance: {"worker_id":"local-coder"' <<< "$output"
grep -q '"worker_provenance": {' "$packet"
grep -q '"worker_id": "local-coder"' "$packet"
grep -q '"README.md"' "$packet"
grep -q 'wall-clock timeout exceeded' "$packet"
grep -q 'parent running' "$packet"
grep -q 'descendant open stdout' "$packet"
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$repo_root" rev-parse HEAD)" = "$before_sha"

echo "rack_change_timeout_smoke: ok"
