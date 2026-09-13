#!/usr/bin/env bash
# GPU-free ownership regression. This script never removes live resource state or invokes a model.
set -euo pipefail
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
cargo test -p rack_ai_infrastructure --offline resource_reservations::tests
echo "rack_resource_admission_smoke: isolated ownership tests passed"
