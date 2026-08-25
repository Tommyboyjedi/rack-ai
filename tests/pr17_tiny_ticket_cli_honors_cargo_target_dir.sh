#!/usr/bin/env bash
# PR17 CLI acceptance must invoke the binary from CARGO_TARGET_DIR, because
# campaign acceptance sandboxes Cargo output outside the protected worktree.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
setup="$root/qualifications/pr17/setup.sh"
grep -q 'bin="${CARGO_TARGET_DIR:-target}/debug/tiny-ticket"' "$setup"
if grep -E '^\./target/debug/tiny-ticket' "$setup"; then
  echo "PR17 setup still hardcodes ./target/debug/tiny-ticket" >&2
  exit 1
fi
# Extract the generated CLI checker and prove it uses CARGO_TARGET_DIR, not ./target.
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
python3 - "$setup" "$tmp/check_cli.sh" <<'PY'
import pathlib, re, sys
text = pathlib.Path(sys.argv[1]).read_text()
match = re.search(r"cat > \"\$ticket/tests/check_cli.sh\" <<'EOF'\n(.*?)EOF", text, re.S)
if not match:
    raise SystemExit("could not extract check_cli.sh from setup.sh")
pathlib.Path(sys.argv[2]).write_text(match.group(1))
PY
grep -q 'bin="${CARGO_TARGET_DIR:-target}/debug/tiny-ticket"' "$tmp/check_cli.sh"
! grep -q './target/debug/tiny-ticket' "$tmp/check_cli.sh"
grep -q 'test -s "$tmp"' "$tmp/check_cli.sh"
echo ok
