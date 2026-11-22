#!/usr/bin/env bash
# Install one Shared-Bridge V2 release and restart its user service.
#
# Usage: deploy.sh /path/to/rack-ai-checkout
#
# The operator-owned configuration lives outside Git at
# /srv/rack-ai/deployments/shared-bridge/config.json (mode 0600) and is never
# overwritten by this script. The RackAI reservation engine is not touched.
set -euo pipefail

REPO=${1:?usage: deploy.sh REPO_ROOT}
DEST=/srv/rack-ai/deployments/shared-bridge
SHA=$(git -C "$REPO" rev-parse --short=12 HEAD)
RELEASE="$DEST/releases/$SHA"

install -d -m 700 "$DEST" "$DEST/releases" "$DEST/state"
rm -rf "$RELEASE"
install -d -m 700 "$RELEASE"
cp -a "$REPO/shared_bridge" "$RELEASE/shared_bridge"
find "$RELEASE" -name '__pycache__' -type d -prune -exec rm -rf {} +
chmod -R go-rwx "$RELEASE"
ln -sfn "$RELEASE" "$DEST/current"

if [ ! -f "$DEST/config.json" ]; then
  echo "missing $DEST/config.json (install config.example.json first, mode 0600)" >&2
  exit 1
fi
chmod 600 "$DEST/config.json"

PYTHONPATH="$DEST/current" /usr/bin/python3 -m shared_bridge.app \
  --config "$DEST/config.json" --check

systemctl --user restart rack-ai-shared-bridge.service
systemctl --user is-active rack-ai-shared-bridge.service
echo "deployed $SHA"
