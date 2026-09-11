#!/bin/sh
set -eu
umask 077

usage() {
    cat <<'USAGE'
usage: live-check.sh

Probes the hosted agentd Service of a disposable beta cluster through probe.sh:
the mutually authenticated health request must report ready, the same request
without the agent program bearer must be refused, and a request without a
client certificate must never reach HTTP.

The Service endpoint, the internal CA and the client certificate the boundary
admits are read from the cluster environment file that
platform/hosted/tests/beta-cluster.sh up writes, PLATFORM_BETA_CLUSTER_ENV
(default build/beta-cluster/env), or from the environment when that file is not
readable:

  LAYERX_AGENTD_URL               agentd Service endpoint of the cluster
  LAYERX_TEST_CA_FILE             internal CA that issued the Service certificate
  LAYERX_AGENTD_CLIENT_CERT_FILE  client certificate the boundary admits
  LAYERX_AGENTD_CLIENT_KEY_FILE   key of that client certificate
  LAYERX_AGENTD_BEARER_FILE       file holding the agent program bearer; when it
                                  is unset and the cluster environment file was
                                  read, it is secrets/human/agent/program-token
                                  in the work directory holding that file, where
                                  the bring-up writes it

The persistent host chain on port 18545 is refused before anything else, so
this gate only ever runs against a disposable cluster.
USAGE
}

case "${1:-}" in
    -h|--help) usage; exit 0 ;;
    "") ;;
    *) printf 'live-check.sh: unknown argument %s\n' "$1" >&2; usage >&2; exit 2 ;;
esac

root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
cd "$root"

env_file=${PLATFORM_BETA_CLUSTER_ENV:-build/beta-cluster/env}
env_read=0
if [ -r "$env_file" ]; then
    # shellcheck disable=SC1090
    . "$env_file"
    env_read=1
fi

case "${LAYERX_AGENTD_URL:-}" in
    *://*:18545 | *://*:18545/*)
        printf 'live-check.sh: LAYERX_AGENTD_URL names the persistent host chain port 18545: persistent-host-chain-forbidden\n' >&2
        exit 2
        ;;
esac

if [ -z "${LAYERX_AGENTD_BEARER_FILE:-}" ] && [ "$env_read" = 1 ]; then
    LAYERX_AGENTD_BEARER_FILE=$(dirname -- "$env_file")/secrets/human/agent/program-token
fi

missing=0
require() {
    eval "value=\${$1:-}"
    if [ -z "$value" ]; then
        printf 'live-check.sh: missing input %s: %s\n' "$1" "$2" >&2
        missing=$((missing + 1))
    fi
}
require LAYERX_AGENTD_URL "the agentd Service endpoint of the beta cluster"
require LAYERX_TEST_CA_FILE "the internal CA file of the beta cluster"
require LAYERX_AGENTD_CLIENT_CERT_FILE "the client certificate the agentd boundary admits"
require LAYERX_AGENTD_CLIENT_KEY_FILE "the key of that client certificate"
require LAYERX_AGENTD_BEARER_FILE "the file holding the agent program bearer"
if [ "$missing" -gt 0 ]; then
    printf 'live-check.sh: %d missing input(s); supply them in the environment or in %s, the cluster environment file the beta cluster bring-up writes\n' \
        "$missing" "$env_file" >&2
    exit 2
fi

for file in "$LAYERX_TEST_CA_FILE" "$LAYERX_AGENTD_CLIENT_CERT_FILE" "$LAYERX_AGENTD_CLIENT_KEY_FILE" "$LAYERX_AGENTD_BEARER_FILE"; do
    [ -f "$file" ] && [ -r "$file" ] && [ -s "$file" ] || {
        printf 'live-check.sh: %s is not a readable non-empty file\n' "$file" >&2
        exit 2
    }
done
command -v curl >/dev/null || { printf 'live-check.sh: curl is required\n' >&2; exit 2; }

sh platform/hosted/agentd/probe.sh \
    --url "$LAYERX_AGENTD_URL" \
    --ca "$LAYERX_TEST_CA_FILE" \
    --client-cert "$LAYERX_AGENTD_CLIENT_CERT_FILE" \
    --client-key "$LAYERX_AGENTD_CLIENT_KEY_FILE" \
    --bearer-file "$LAYERX_AGENTD_BEARER_FILE"

printf 'agentd-live-check: the deployed agentd Service at %s answered as a ready, mutually authenticated owner daemon\n' "$LAYERX_AGENTD_URL"
