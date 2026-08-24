#!/usr/bin/env bash
set -euo pipefail

if [[ "${RACK_AI_LIVE_SMOKE:-0}" != "1" ]]; then
  echo "SKIPPED: set RACK_AI_LIVE_SMOKE=1 to run real local models" >&2
  exit 2
fi
for port in 8017 8018; do
  if ! curl --fail --silent --max-time 3 "http://127.0.0.1:${port}/v1/models" >/dev/null; then
    echo "BLOCKED: local vLLM endpoint on :${port} is unavailable" >&2
    exit 2
  fi
done
if [[ "$(podman info --format '{{.Host.Security.Rootless}}' 2>/dev/null || true)" != "true" ]]; then
  echo "BLOCKED: rootless Podman is required" >&2
  exit 2
fi
if ! podman image exists docker.io/library/rust:bookworm; then
  echo "BLOCKED: podman image docker.io/library/rust:bookworm is missing" >&2
  exit 2
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
evidence_root="${RACK_AI_LIVE_EVIDENCE_DIR:-$(mktemp -d -t rack-ai-live-XXXXXX)}"
fixture="$evidence_root/fixture"
rack="$evidence_root/rack"
mkdir -p "$fixture/src" "$rack/bin" "$rack/config" "$rack/state/campaigns"
git -C "$rack" init -b main >/dev/null
python3 - <<'PY' "$HOME/.jcode/config.toml"
import sys, tomllib
from pathlib import Path
config = tomllib.loads(Path(sys.argv[1]).read_text())
providers = config.get("providers", {})
expected = {
    "local-primary": ("http://127.0.0.1:8017/v1", "local-primary"),
    "local-coder": ("http://127.0.0.1:8018/v1", "local-coder"),
}
for name, (base_url, model_id) in expected.items():
    provider = providers.get(name)
    if provider is None:
        raise SystemExit(f"missing JCode provider profile: {name}")
    if provider.get("base_url") != base_url:
        raise SystemExit(f"provider {name} base_url mismatch: {provider.get('base_url')}")
    if provider.get("default_model") != model_id:
        raise SystemExit(f"provider {name} default_model mismatch: {provider.get('default_model')}")
PY
printf '%s\n' '[package]' 'name = "rack_live_fixture"' 'version = "0.1.0"' 'edition = "2021"' '' '[lib]' 'path = "src/lib.rs"' > "$fixture/Cargo.toml"
printf 'pub fn seed() -> u8 { 1 }\n' > "$fixture/src/lib.rs"
(cd "$fixture" && cargo generate-lockfile >/dev/null)
git -C "$fixture" init -b main >/dev/null
git -C "$fixture" config user.email rack-live@example.invalid
git -C "$fixture" config user.name rack-live
git -C "$fixture" add .
git -C "$fixture" commit -m init >/dev/null
base_sha="$(git -C "$fixture" rev-parse HEAD)"
cp "$repo_root/config/workers.json" "$rack/config/workers.json"
cp "$repo_root/config/models.json" "$rack/config/models.json"
cp "$repo_root/config/operations.json" "$rack/config/operations.json"
cat > "$rack/bin/jcode-log-local-coder" <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "__LOG_PATH__"
exec /home/tomp/.local/bin/jcode "$@"
WRAP
cat > "$rack/bin/jcode-log-local-primary" <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "__LOG_PATH__"
exec /home/tomp/.local/bin/jcode "$@"
WRAP
coder_log="$evidence_root/local-coder-jcode-args.log"
primary_log="$evidence_root/local-primary-jcode-args.log"
sed -i "s|__LOG_PATH__|$coder_log|g" "$rack/bin/jcode-log-local-coder"
sed -i "s|__LOG_PATH__|$primary_log|g" "$rack/bin/jcode-log-local-primary"
chmod +x "$rack/bin/jcode-log-local-coder" "$rack/bin/jcode-log-local-primary"
python3 - <<'PY' "$rack/config/workers.json" "$rack/bin/jcode-log-local-primary" "$rack/bin/jcode-log-local-coder"
import json, sys
from pathlib import Path
path = Path(sys.argv[1])
primary = sys.argv[2]
coder = sys.argv[3]
document = json.loads(path.read_text())
for worker in document["workers"]:
    if worker["id"] == "local-primary":
        worker["entrypoint"] = primary
    elif worker["id"] == "local-coder":
        worker["entrypoint"] = coder
path.write_text(json.dumps(document, indent=2) + "\n")
PY

printf '{\n  "workspace_root": "%s",\n  "approved_programs": ["cargo", "true"],\n  "executor": {"backend":"podman","image":"docker.io/library/rust:bookworm","workspace_path":"/workspace","memory":"2g","pids_limit":256},\n  "repositories": [{"id":"fixture","root":"%s","default_base_ref":"main","enabled":true}]\n}\n' "$evidence_root/workspaces" "$fixture" > "$rack/config/repositories.json"

campaign="$evidence_root/campaign.json"
printf '{
  "version":"rack-ai/campaign/v1",
  "campaign_id":"live-two-step-fallback",
  "repository":{"id":"fixture","base_ref":"main","base_sha":"%s"},
  "branch":"rack/campaign-live-two-step-fallback",
  "permitted_paths":["src/"], "allow_local_commits":true,
  "limits":{"max_runtime_seconds":1200,"max_steps":2,"max_total_attempts":4,"heartbeat_seconds":10,"network":"disabled"},
  "worker_policy":{"primary":"local-coder","fallback":"local-primary","primary_attempts":1,"repair_attempts":0,"fallback_attempts":1},
  "steps":[
    {"id":"alpha","kind":"implementation","task":"Create src/alpha.rs containing a public function alpha returning 1.","allowed_paths":["src/"],"required_changed_paths":["src/alpha.rs"],"acceptance":{"commands":[["cargo","test"]],"required_artifacts":["src/alpha.rs"]},"limits":{"timeout_seconds":300,"network":"disabled"}},
    {"id":"fallback","kind":"implementation","task":"This is a fallback-boundary proof. If your model id is local-coder, make no file changes and reply COMPLETE. If your model id is local-primary, create src/fallback.rs containing a public function fallback returning 2.","allowed_paths":["src/"],"required_changed_paths":["src/fallback.rs"],"acceptance":{"commands":[["cargo","test"]],"required_artifacts":["src/fallback.rs"]},"limits":{"timeout_seconds":300,"network":"disabled"}}
  ]
}\n' "$base_sha" > "$campaign"

cli=(cargo run -q -p rack_ai_cli --manifest-path "$repo_root/Cargo.toml" -- campaign)
"${cli[@]}" start "$campaign" --repo-root "$rack" --state-root "$rack" > "$evidence_root/run.log" 2>&1 &
runner_pid=$!
for _ in $(seq 1 120); do
  [[ -f "$rack/state/campaigns/live-two-step-fallback/state.json" ]] && break
  kill -0 "$runner_pid" 2>/dev/null || break
  sleep 1
done
"${cli[@]}" status live-two-step-fallback --emit-json --repo-root "$rack" --state-root "$rack" > "$evidence_root/status-observed.json" || true
"${cli[@]}" events live-two-step-fallback --emit-json --repo-root "$rack" --state-root "$rack" > "$evidence_root/events-observed.json" || true
"${cli[@]}" inspect live-two-step-fallback --step alpha --repo-root "$rack" --state-root "$rack" > "$evidence_root/inspect-observed.txt" || true
wait "$runner_pid"

worktree="$evidence_root/workspaces/campaign-live-two-step-fallback/repo"
test "$(git -C "$fixture" branch --show-current)" = main
test "$(git -C "$fixture" rev-parse HEAD)" = "$base_sha"
test "$(git -C "$worktree" rev-list --count HEAD ^"$base_sha")" = 2
grep -q '"state": "completed"' "$rack/state/campaigns/live-two-step-fallback/state.json"
grep -q 'campaign_completed' "$rack/state/campaigns/live-two-step-fallback/events.jsonl"
find "$rack/state/campaigns/live-two-step-fallback/steps" -name model-review.json -print -quit | grep -q .
find "$rack/state/campaigns/live-two-step-fallback/steps/alpha" -name worker-transcript.json -exec grep -q '"executor_kind": "jcode-direct"' {} \;
find "$rack/state/campaigns/live-two-step-fallback/steps/fallback" -name worker-transcript.json -exec grep -q '"executor_kind": "jcode-direct"' {} \;
find "$rack/state/campaigns/live-two-step-fallback/steps/alpha" -name git-evidence.json -exec grep -q 'src/alpha.rs' {} \;
find "$rack/state/campaigns/live-two-step-fallback/steps/fallback" -name git-evidence.json -exec grep -q 'src/fallback.rs' {} \;
grep -Fx -- '--provider-profile' "$coder_log"
grep -Fx -- 'local-coder' "$coder_log"
grep -Fx -- '--model' "$coder_log"
grep -Fx -- '--tool-profile' "$coder_log"
grep -Fx -- 'minimal' "$coder_log"
grep -Fx -- '--provider-profile' "$primary_log"
grep -Fx -- 'local-primary' "$primary_log"
grep -Fx -- '--model' "$primary_log"
if grep -Fq -- '--tool-profile' "$primary_log"; then
  echo 'FAIL: local-primary should not use a tool profile override' >&2
  exit 1
fi
echo "rack_campaign_live_model_smoke: ok"
echo "Evidence retained at: $evidence_root"
