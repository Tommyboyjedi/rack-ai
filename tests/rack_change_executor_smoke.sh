#!/usr/bin/env bash
set -euo pipefail

if ! command -v podman >/dev/null 2>&1; then
  cat >&2 <<'EOF'
BLOCKED: rootless Podman is not installed on this host.

Live isolation tests were not executed. The Podman executor is implemented and
unit-tested to fail closed when `podman` is missing.

Installing the executor on Ubuntu requires privilege this session does not have:
  sudo apt-get install -y podman uidmap
  loginctl enable-linger "$USER"

Do not treat this script as passing live isolation validation.
EOF
  exit 2
fi

echo "podman is present; live isolation coverage is not yet expanded in this smoke."
exit 0
