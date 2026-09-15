# PR35 isolated runtime verification

These clients exercise the compiled Rust HTTP receiver, canonical authority,
production lifecycle and native media gate. Model endpoints are real disposable CPU
processes; systemd/Docker/NVIDIA commands are controlled fixtures. Nothing here proves
real GPU fit, hosting performance, model quality, or live application cutover.

From the remote RackAI checkout:

```sh
cargo build --workspace --offline
python3 -m venv .venv
.venv/bin/pip install -r tests/media/requirements.txt
RACK_AI_RESOURCE_ROOT="$PWD/evidence/runtime-legacy" \
  timeout 300 .venv/bin/python -m pytest tests/runtime -q
RACK_AI_RESOURCE_ROOT="$PWD/evidence/scenario-legacy" \
  timeout 180 .venv/bin/python tests/runtime/scenario.py evidence/runtime-scenario
RACK_AI_RESOURCE_ROOT="$PWD/evidence/workspace-legacy" \
  cargo test --workspace --offline
cargo clippy -p rack_ai_runtime --all-targets --no-deps --offline -- -D warnings
```

Use a fresh scenario directory for each proof. It retains receiver/authority/media
state and actual process events. Unit-style transport tests use temporary directories
and terminate only their recorded fixture processes. Framework exceptions and uncertain
outcomes fail tests; an HTTP acceptance is never treated as completed inference.

The media regression suite additionally requires the documented sandbox-enabled
Playwright browser and its shared libraries. Use the existing approved browser path;
do not disable Chromium sandboxing or modify production services to obtain a pass.

```sh
RACK_AI_RESOURCE_ROOT="$PWD/evidence/media-legacy" \
  RACK_MEDIA_BROWSER=/opt/rack-ai-pr24-browser/chrome-headless-shell \
  timeout 900 .venv/bin/python -m pytest tests/media -q
```

For the retained PR35 run, the missing ALSA library was extracted into
`evidence/pr35/browser-libs/usr/lib/x86_64-linux-gnu` and supplied through
`LD_LIBRARY_PATH`; there was no system package installation. Live smoke scripts are
separate and were not authorized. See the live qualification/rollback plan before
any service migration or GPU test window.
