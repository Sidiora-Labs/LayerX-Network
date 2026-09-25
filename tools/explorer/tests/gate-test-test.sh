#!/bin/sh
#
# gate-test-test.sh
#
# The test of the explorer test gate. It builds a throwaway copy of the gate in
# a temporary directory beside a backend tree of Paxeer X suites spread over
# four umbrella applications, a recording stand-in for the containerised mix
# runner and a recording stand-in for yarn, then asserts that the gate runs one
# mix invocation per application carrying only that application's suites, in
# path order, with a log per application, and that a failing application stops
# the gate on its own exit code before the next application or the frontend.
#
# Run from anywhere:
#
#   tools/explorer/tests/gate-test-test.sh
#
# Exit codes: 0 when every case holds, 1 when one does not.

set -eu

script_dir=$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd -P)
gate_script=$script_dir/../gate-test.sh

[ -x "$gate_script" ] || {
    printf 'gate-test-test: %s is not an executable script\n' "$gate_script" >&2
    exit 1
}

work_dir=$(mktemp -d)
trap 'rm -rf "$work_dir"' EXIT HUP INT TERM

output_file=$work_dir/output
record_file=$work_dir/record
expected_file=$work_dir/expected
failures=0

pass() {
    printf 'ok   %s\n' "$*"
}

fail() {
    printf 'FAIL %s\n' "$*"
    failures=$((failures + 1))
}

assert_status() {
    if [ "$2" -eq "$1" ]; then
        pass "$3 exits $1"
    else
        fail "$3: expected exit $1, got $2"
        sed -e 's/^/     /' "$output_file"
    fi
}

assert_says() {
    if grep -qF -- "$1" "$output_file"; then
        pass "the output says: $1"
    else
        fail "the output does not say: $1"
        sed -e 's/^/     /' "$output_file"
    fi
}

assert_record() {
    if diff -u "$expected_file" "$record_file" >"$work_dir/diff" 2>&1; then
        pass "$1"
    else
        fail "$1"
        sed -e 's/^/     /' "$work_dir/diff"
    fi
}

assert_file() {
    if [ -f "$1" ]; then
        pass "the gate wrote a $2"
    else
        fail "the gate wrote no $2"
    fi
}

refute_file() {
    if [ -f "$1" ]; then
        fail "the gate wrote a $2 after the leg that failed"
    else
        pass "the gate wrote no $2 after the leg that failed"
    fi
}

repository=$work_dir/repository
log_dir=$work_dir/logs
stub_bin=$work_dir/bin
mkdir -p "$repository/tools/explorer" "$repository/explorer/deploy/tools" \
    "$repository/explorer/frontend" "$stub_bin"

cp "$gate_script" "$repository/tools/explorer/gate-test.sh"
chmod +x "$repository/tools/explorer/gate-test.sh"

# The suites of four umbrella applications, one of them carrying two, plus a
# suite the gate must leave alone because its path does not carry the fork's
# chain type.
for suite in \
    apps/block_scout_web/test/block_scout_web/paxeer_x_view_test.exs \
    apps/ethereum_jsonrpc/test/ethereum_jsonrpc/paxeer_x_variant_test.exs \
    apps/explorer/test/explorer/chain/paxeer_x/receipt_test.exs \
    apps/explorer/test/explorer/chain/paxeer_x/status_test.exs \
    apps/indexer/test/indexer/transform/paxeer_x_logs_test.exs \
    apps/explorer/test/explorer/chain/block_test.exs; do
    mkdir -p "$repository/explorer/backend/$(dirname "$suite")"
    printf 'defmodule Throwaway do\nend\n' >"$repository/explorer/backend/$suite"
done

cat >"$repository/explorer/deploy/tools/mix-in-builder.sh" <<'RUNNER'
#!/bin/sh
set -eu
printf 'mix %s\n' "$*" >>"$GATE_TEST_RECORD"
if [ -n "${GATE_TEST_FAIL_ON:-}" ]; then
    case "$*" in
        *"$GATE_TEST_FAIL_ON"*) exit "$GATE_TEST_FAIL_STATUS" ;;
    esac
fi
RUNNER
chmod +x "$repository/explorer/deploy/tools/mix-in-builder.sh"

cat >"$stub_bin/yarn" <<'YARN'
#!/bin/sh
set -eu
printf 'yarn %s\n' "$*" >>"$GATE_TEST_RECORD"
YARN
chmod +x "$stub_bin/yarn"

printf '{\n  "scripts": {\n    "lint:tsc": "tsc",\n    "test:vitest": "vitest"\n  }\n}\n' \
    >"$repository/explorer/frontend/package.json"

PATH=$stub_bin:$PATH
export PATH
GATE_TEST_RECORD=$record_file
export GATE_TEST_RECORD
EXPLORER_GATE_LOG_DIR=$log_dir
export EXPLORER_GATE_LOG_DIR
EXPLORER_GATE_BUDGET_SECONDS=120
export EXPLORER_GATE_BUDGET_SECONDS

run_gate() {
    : >"$record_file"
    rm -rf "$log_dir"
    set +e
    "$repository/tools/explorer/gate-test.sh" >"$output_file" 2>&1
    gate_status=$?
    set -e
}

# One mix invocation per umbrella application, each carrying its own suites in
# path order and nothing else, then the three frontend legs.
unset GATE_TEST_FAIL_ON
run_gate
assert_status 0 "$gate_status" 'a gate whose every leg passes'

cat >"$expected_file" <<'EXPECTED'
mix test apps/block_scout_web/test/block_scout_web/paxeer_x_view_test.exs
mix test apps/ethereum_jsonrpc/test/ethereum_jsonrpc/paxeer_x_variant_test.exs
mix test apps/explorer/test/explorer/chain/paxeer_x/receipt_test.exs apps/explorer/test/explorer/chain/paxeer_x/status_test.exs
mix test apps/indexer/test/indexer/transform/paxeer_x_logs_test.exs
yarn install --frozen-lockfile
yarn lint:tsc
yarn test:vitest run
EXPECTED
assert_record 'the gate runs one mix invocation per umbrella application, then the frontend legs'

for application in block_scout_web ethereum_jsonrpc explorer indexer; do
    assert_file "$log_dir/backend-$application.log" "log for the $application leg"
done

# The application that fails stops the gate on its own exit code, before the
# next application and before the frontend.
GATE_TEST_FAIL_ON='apps/explorer/'
export GATE_TEST_FAIL_ON
GATE_TEST_FAIL_STATUS=4
export GATE_TEST_FAIL_STATUS
run_gate
assert_status 4 "$gate_status" 'a gate whose explorer application fails'
assert_says 'FAILED: the backend Paxeer X explorer leg'
assert_says 'exit code: 4'
assert_says "log: $log_dir/backend-explorer.log"

cat >"$expected_file" <<'EXPECTED'
mix test apps/block_scout_web/test/block_scout_web/paxeer_x_view_test.exs
mix test apps/ethereum_jsonrpc/test/ethereum_jsonrpc/paxeer_x_variant_test.exs
mix test apps/explorer/test/explorer/chain/paxeer_x/receipt_test.exs apps/explorer/test/explorer/chain/paxeer_x/status_test.exs
EXPECTED
assert_record 'the gate stops at the application that failed'
refute_file "$log_dir/backend-indexer.log" 'log for the indexer leg'

if [ "$failures" -eq 0 ]; then
    printf 'gate-test-test: every case holds\n'
    exit 0
fi

printf 'gate-test-test: %s case(s) failed\n' "$failures" >&2
exit 1
