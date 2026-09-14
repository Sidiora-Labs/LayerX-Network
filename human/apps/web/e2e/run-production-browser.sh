#!/usr/bin/env bash
set -euo pipefail
umask 077
case "${1:-}" in
    test:perf|test:journey|test:settings|test:explorer) suite=$1 ;;
    *) printf 'production browser requires a supported test suite\n' >&2; exit 2 ;;
esac
test "$#" -eq 1 || { printf 'production browser accepts one test suite\n' >&2; exit 2; }
root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)
web="$root/human/apps/web"
for tool in authbind certutil openssl python3 node npm; do
    command -v "$tool" >/dev/null || { printf 'production browser prerequisite missing: %s\n' "$tool" >&2; exit 1; }
done
test -x /etc/authbind/byport/443 || { printf 'production browser requires explicit authbind permission for port 443\n' >&2; exit 1; }
mkdir -p "$root/qual-logs"
output=$(mktemp -d "$root/qual-logs/human-browser-XXXXXX")
python3 "$web/e2e/prepare-production.py" "$root" "$output" > "$output/environment"
mapfile -t configuration < "$output/environment"
test "${#configuration[@]}" -eq 5
export HUMAN_E2E_REAL_STACK=1 HUMAN_E2E_LOCAL_PRODUCTION=1
export HUMAN_E2E_BASE_URL="${configuration[0]}" LAYERX_HUMAN_WEB_ORIGIN="${configuration[0]}"
export LAYERX_HUMAN_SERVICE_URL="${configuration[1]}" NODE_EXTRA_CA_CERTS="${configuration[2]}"
export HUMAN_E2E_BROWSER_HOME="${configuration[3]}" HUMAN_E2E_TLS_CONFIG="${configuration[4]}"
export LAYERX_RUM_STORAGE_DIRECTORY="$output/rum-data"
npm --prefix "$web" run build
npm --prefix "$web" run "$suite"
