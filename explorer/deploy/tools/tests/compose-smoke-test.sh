#!/usr/bin/env bash
# Proves the committed deployment definitions without a node and without a
# deployment.
#
# Brings the database, the recorded JSON-RPC server (tools/rpc-fixture-server.py
# serving tools/fixtures) and the published backend image up from
# docker-compose.local.yml under its `fixture` profile, waits for the liveness
# path, asserts that the Paxeer X capability endpoint answers with the body the
# recorded chain owes, and tears the stack down on every exit path. Any step
# that does not hold returns non-zero.
#
# The stack runs under its own compose project and its own published ports, so
# it touches neither a production project nor anything else on the machine.
#
# Overridable: EXPLORER_IMAGE_TAG (default latest) and the three published
# ports below.

set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
tools_dir="$(dirname -- "${script_dir}")"
deploy_dir="$(dirname -- "${tools_dir}")"
compose_file="${deploy_dir}/docker-compose.local.yml"
fixture_server="${tools_dir}/rpc-fixture-server.py"
fixture_dir="${tools_dir}/fixtures"

project_name="paxeer-x-explorer-smoke"

export EXPLORER_IMAGE_TAG="${EXPLORER_IMAGE_TAG:-latest}"
export POSTGRES_PUBLISHED_PORT="${POSTGRES_PUBLISHED_PORT:-57432}"
export BACKEND_PUBLISHED_PORT="${BACKEND_PUBLISHED_PORT:-54000}"
export RPC_FIXTURE_PUBLISHED_PORT="${RPC_FIXTURE_PUBLISHED_PORT:-58545}"

# The recorded chain: tools/fixtures answers eth_chainId with 0x7d.
export CHAIN_ID=125
export CHAIN_TYPE=paxeer_x

# The node is the recorded server on the compose network.
export RPC_HTTP_URL="http://rpc-fixture:8545/"
export RPC_WS_URL="ws://rpc-fixture:8545/"

# The two microservices are not part of this stack, so the backend must not be
# told to call them.
export MICROSERVICE_SC_VERIFIER_ENABLED=false
export MICROSERVICE_SIG_PROVIDER_ENABLED=false

# What is under test is the deployment definition, the images and the Paxeer X
# capability surface, not indexing: the recorded server answers one pinned
# height and nothing here imports a chain.
export DISABLE_INDEXER=true

# Off by default in env/backend.example.env; this run is a deployment that
# turns it on, which is what makes the capability answer a probe and not the
# never-probed default.
export PAXEER_X_CAPABILITIES_ENABLED=true

liveness_url="http://127.0.0.1:${BACKEND_PUBLISHED_PORT}/api/health/liveness"
capabilities_url="http://127.0.0.1:${BACKEND_PUBLISHED_PORT}/api/v2/paxeer-x/capabilities"
journal_url="http://127.0.0.1:${RPC_FIXTURE_PUBLISHED_PORT}/__journal"

# Every LayerX surface reads as absent on the chain as recorded: the five
# surface precompiles carry no code and getUnifiedAccount at the addr
# precompile reverts.
expected_capabilities='{"addr": false, "anchor": false, "bridge": false, "custody": false, "exchange": false, "launchpad": false}'

liveness_timeout_seconds="${LIVENESS_TIMEOUT_SECONDS:-900}"
probe_timeout_seconds="${PROBE_TIMEOUT_SECONDS:-120}"

failed=1

compose() {
  docker compose --file "${compose_file}" --project-name "${project_name}" --profile fixture "$@"
}

say() {
  printf '[compose-smoke] %s\n' "$*"
}

die() {
  printf '[compose-smoke] FAIL: %s\n' "$*" >&2
  exit 1
}

teardown() {
  local status=$?

  if [ "${failed}" -ne 0 ]; then
    say "stack logs"
    compose logs --no-color --timestamps || true
  fi

  say "tearing the stack down"
  compose down --volumes --remove-orphans --timeout 20 || true

  exit "${status}"
}

require_command() {
  command -v "$1" > /dev/null 2>&1 || die "$1 is required and is not on PATH"
}

require_command docker
require_command curl
require_command python3

[ -f "${compose_file}" ] || die "no compose file at ${compose_file}"
[ -f "${fixture_server}" ] || die "no fixture server at ${fixture_server}"
[ -d "${fixture_dir}" ] || die "no fixture directory at ${fixture_dir}"

say "compose file ${compose_file}"
say "image tag ${EXPLORER_IMAGE_TAG}"

# A stale stack from an interrupted run would answer in place of this one.
compose down --volumes --remove-orphans --timeout 20 > /dev/null 2>&1 || true

trap teardown EXIT

say "checking the compose definition"
compose config --quiet || die "the compose definition is not valid"

say "pulling the published images"
compose pull --quiet db rpc-fixture backend || die "could not pull the images under test"

say "starting the database, the recorded JSON-RPC server and the backend"
compose up --detach --no-build db rpc-fixture backend || die "the stack did not start"

say "waiting for the recorded JSON-RPC server"
deadline=$((SECONDS + 60))
until curl -fsS --max-time 5 "http://127.0.0.1:${RPC_FIXTURE_PUBLISHED_PORT}/__health" > /dev/null 2>&1; do
  [ "${SECONDS}" -lt "${deadline}" ] || die "the recorded JSON-RPC server never answered /__health"
  sleep 2
done

say "waiting for ${liveness_url}"
deadline=$((SECONDS + liveness_timeout_seconds))
until curl -fsS --max-time 10 "${liveness_url}" > /dev/null 2>&1; do
  if [ "$(compose ps --status running --services | grep -c '^backend$' || true)" -eq 0 ]; then
    die "the backend container stopped before it answered the liveness path"
  fi

  [ "${SECONDS}" -lt "${deadline}" ] ||
    die "the backend did not answer the liveness path within ${liveness_timeout_seconds}s"

  sleep 5
done

say "the backend answers the liveness path"

say "waiting for the capability probe to reach the recorded JSON-RPC server"
deadline=$((SECONDS + probe_timeout_seconds))
until journal="$(curl -fsS --max-time 10 "${journal_url}")" &&
  printf '%s' "${journal}" | python3 -c '
import json
import sys

surfaces = {
    "custody": "0x0000000000000000000000000000000000001013",
    "anchor": "0x0000000000000000000000000000000000001014",
    "exchange": "0x0000000000000000000000000000000000001015",
    "bridge": "0x0000000000000000000000000000000000001016",
    "launchpad": "0x0000000000000000000000000000000000001017",
}
addr_precompile = "0x0000000000000000000000000000000000001004"

calls = json.load(sys.stdin)["calls"]
answered = [call for call in calls if call["matched"]]

probed = {
    call["params"][0].lower()
    for call in answered
    if call["method"] == "eth_getCode" and call["params"]
}
called_addr = any(
    call["method"] == "eth_call"
    and call["params"]
    and isinstance(call["params"][0], dict)
    and call["params"][0].get("to", "").lower() == addr_precompile
    for call in answered
)

missing = sorted(name for name, address in surfaces.items() if address not in probed)

if missing or not called_addr:
    sys.exit(1)
'; do
  [ "${SECONDS}" -lt "${deadline}" ] ||
    die "the capability probe did not read every surface precompile within ${probe_timeout_seconds}s"

  sleep 3
done

say "the capability probe read every surface precompile from the recorded responses"

say "asserting ${capabilities_url}"
capabilities="$(curl -fsS --max-time 10 "${capabilities_url}")" ||
  die "the capability endpoint did not answer"

printf '%s' "${capabilities}" | python3 -c '
import json
import sys

expected = json.loads(sys.argv[1])
actual = json.load(sys.stdin)

if actual != expected:
    sys.stderr.write(
        "capabilities body is %s, expected %s\n"
        % (json.dumps(actual, sort_keys=True), json.dumps(expected, sort_keys=True))
    )
    sys.exit(1)
' "${expected_capabilities}" || die "the capability endpoint answered the wrong body"

say "the capability endpoint answers the documented body"

failed=0
say "PASS"
