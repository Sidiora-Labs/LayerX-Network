#!/usr/bin/env bash
set -euo pipefail
ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
exec timeout 600 python3 "$ROOT/tests/daemon/finality-authority-chain.py" "${1:-$ROOT/build/tests/lxp_test_daemon_finality_authority}"
