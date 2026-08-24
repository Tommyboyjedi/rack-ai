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
python3 - <<'PY' "__ROLE__" "__PROBE_PATH__" "$HOME" "$@"
import json
import socket
import sys
import tomllib
import urllib.request
from pathlib import Path

role, probe_path, home, *argv = sys.argv[1:]
config_path = Path(home) / ".jcode" / "config.toml"
config = tomllib.loads(config_path.read_text())
provider_index = argv.index("--provider-profile") + 1
model_index = argv.index("--model") + 1
provider_name = argv[provider_index]
model_id = argv[model_index]
tool_profile = None
if "--tool-profile" in argv:
    tool_profile = argv[argv.index("--tool-profile") + 1]
provider = config["providers"][provider_name]
context_window = None
for model in provider.get("models", []):
    if model.get("id") == model_id:
        context_window = model.get("context_window")
        break
expected = {
    "local-coder": {
        "base_url": "http://127.0.0.1:8018/v1",
        "model": "local-coder",
        "context_window": 16368,
        "tool_profile": "minimal",
    },
    "local-primary": {
        "base_url": "http://127.0.0.1:8017/v1",
        "model": "local-primary",
        "context_window": None,
        "tool_profile": None,
    },
}[role]
if provider_name != role:
    raise SystemExit(f"provider profile mismatch: {provider_name}")
if provider.get("base_url") != expected["base_url"]:
    raise SystemExit(f"base_url mismatch: {provider.get('base_url')}")
if model_id != expected["model"]:
    raise SystemExit(f"model mismatch: {model_id}")
if context_window != expected["context_window"]:
    raise SystemExit(f"context_window mismatch: {context_window}")
if tool_profile != expected["tool_profile"]:
    raise SystemExit(f"tool profile mismatch: {tool_profile}")
loopback_ok = False
loopback_error = None
try:
    with urllib.request.urlopen(f"{provider['base_url']}/models", timeout=3) as response:
        response.read(1)
    loopback_ok = True
except Exception as error:
    loopback_error = str(error)
if not loopback_ok:
    raise SystemExit(f"loopback endpoint unavailable: {loopback_error}")
external_blocked = False
external_error = None
external_errno = None
try:
    socket.create_connection(("8.8.8.8", 53), timeout=3).close()
except OSError as error:
    external_blocked = True
    external_errno = error.errno
    external_error = str(error)
if not external_blocked:
    raise SystemExit("external network unexpectedly succeeded")
probe = {
    "role": role,
    "home": home,
    "config_path": str(config_path),
    "provider_profile": provider_name,
    "base_url": provider.get("base_url"),
    "model": model_id,
    "context_window": context_window,
    "tool_profile": tool_profile,
    "loopback_ok": loopback_ok,
    "external_blocked": external_blocked,
    "external_errno": external_errno,
    "external_error": external_error,
}
Path(probe_path).write_text(json.dumps(probe, indent=2) + "\n")
PY
exec /home/tomp/.local/bin/jcode "$@"
WRAP
cat > "$rack/bin/jcode-log-local-primary" <<'WRAP'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$@" > "__LOG_PATH__"
python3 - <<'PY' "__ROLE__" "__PROBE_PATH__" "$HOME" "$@"
import json
import socket
import sys
import tomllib
import urllib.request
from pathlib import Path

role, probe_path, home, *argv = sys.argv[1:]
config_path = Path(home) / ".jcode" / "config.toml"
config = tomllib.loads(config_path.read_text())
provider_index = argv.index("--provider-profile") + 1
model_index = argv.index("--model") + 1
provider_name = argv[provider_index]
model_id = argv[model_index]
tool_profile = None
if "--tool-profile" in argv:
    tool_profile = argv[argv.index("--tool-profile") + 1]
provider = config["providers"][provider_name]
context_window = None
for model in provider.get("models", []):
    if model.get("id") == model_id:
        context_window = model.get("context_window")
        break
expected = {
    "local-coder": {
        "base_url": "http://127.0.0.1:8018/v1",
        "model": "local-coder",
        "context_window": 16368,
        "tool_profile": "minimal",
    },
    "local-primary": {
        "base_url": "http://127.0.0.1:8017/v1",
        "model": "local-primary",
        "context_window": None,
        "tool_profile": None,
    },
}[role]
if provider_name != role:
    raise SystemExit(f"provider profile mismatch: {provider_name}")
if provider.get("base_url") != expected["base_url"]:
    raise SystemExit(f"base_url mismatch: {provider.get('base_url')}")
if model_id != expected["model"]:
    raise SystemExit(f"model mismatch: {model_id}")
if context_window != expected["context_window"]:
    raise SystemExit(f"context_window mismatch: {context_window}")
if tool_profile != expected["tool_profile"]:
    raise SystemExit(f"tool profile mismatch: {tool_profile}")
loopback_ok = False
loopback_error = None
try:
    with urllib.request.urlopen(f"{provider['base_url']}/models", timeout=3) as response:
        response.read(1)
    loopback_ok = True
except Exception as error:
    loopback_error = str(error)
if not loopback_ok:
    raise SystemExit(f"loopback endpoint unavailable: {loopback_error}")
external_blocked = False
external_error = None
external_errno = None
try:
    socket.create_connection(("8.8.8.8", 53), timeout=3).close()
except OSError as error:
    external_blocked = True
    external_errno = error.errno
    external_error = str(error)
if not external_blocked:
    raise SystemExit("external network unexpectedly succeeded")
probe = {
    "role": role,
    "home": home,
    "config_path": str(config_path),
    "provider_profile": provider_name,
    "base_url": provider.get("base_url"),
    "model": model_id,
    "context_window": context_window,
    "tool_profile": tool_profile,
    "loopback_ok": loopback_ok,
    "external_blocked": external_blocked,
    "external_errno": external_errno,
    "external_error": external_error,
}
Path(probe_path).write_text(json.dumps(probe, indent=2) + "\n")
PY
exec /home/tomp/.local/bin/jcode "$@"
WRAP
coder_log="$evidence_root/local-coder-jcode-args.log"
primary_log="$evidence_root/local-primary-jcode-args.log"
coder_probe="$evidence_root/local-coder-probe.json"
primary_probe="$evidence_root/local-primary-probe.json"
sed -i "s|__LOG_PATH__|$coder_log|g; s|__PROBE_PATH__|$coder_probe|g; s|__ROLE__|local-coder|g" "$rack/bin/jcode-log-local-coder"
sed -i "s|__LOG_PATH__|$primary_log|g; s|__PROBE_PATH__|$primary_probe|g; s|__ROLE__|local-primary|g" "$rack/bin/jcode-log-local-primary"
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
python3 - <<'PY' "$coder_probe" "$primary_probe"
import json
import sys
from pathlib import Path

coder = json.loads(Path(sys.argv[1]).read_text())
primary = json.loads(Path(sys.argv[2]).read_text())
for probe, role, base_url, model, context, tool_profile in [
    (coder, "local-coder", "http://127.0.0.1:8018/v1", "local-coder", 16368, "minimal"),
    (primary, "local-primary", "http://127.0.0.1:8017/v1", "local-primary", None, None),
]:
    if probe["role"] != role:
        raise SystemExit(f"role mismatch: {probe}")
    if "rack-ai-jcode-run-" not in probe["home"]:
        raise SystemExit(f"unexpected HOME: {probe['home']}")
    if not probe["config_path"].endswith("/.jcode/config.toml"):
        raise SystemExit(f"unexpected config path: {probe['config_path']}")
    if probe["provider_profile"] != role:
        raise SystemExit(f"provider mismatch: {probe}")
    if probe["base_url"] != base_url:
        raise SystemExit(f"base_url mismatch: {probe}")
    if probe["model"] != model:
        raise SystemExit(f"model mismatch: {probe}")
    if probe["context_window"] != context:
        raise SystemExit(f"context mismatch: {probe}")
    if probe["tool_profile"] != tool_profile:
        raise SystemExit(f"tool profile mismatch: {probe}")
    if probe["loopback_ok"] is not True:
        raise SystemExit(f"loopback not reachable: {probe}")
    if probe["external_blocked"] is not True:
        raise SystemExit(f"external network was not blocked: {probe}")
PY

echo "rack_campaign_live_model_smoke: ok"
echo "Evidence retained at: $evidence_root"
