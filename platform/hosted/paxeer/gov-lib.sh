#!/usr/bin/env bash
# Shared pieces of gov-upgrade.sh and gov-consensus-params.sh: logging, chain queries (live over
# RPC or replayed from a fixture directory), the pure height and deadline arithmetic, duration
# parsing and the poll loop used by the confirm steps. Sourced, never executed.
#
# Every function that does arithmetic is pure: it reads only its arguments and prints one result,
# so the tests exercise it without a chain. Times are unix epochs in seconds; block rates are
# decimal seconds per block; durations for consensus params are nanoseconds, the amino JSON form
# the params module decodes.
#
# Queries: with GOV_FIXTURE_DIR set, every query reads DIR/<name>.json instead of contacting the
# RPC, and nothing may be executed. Without it, RPC paths go through curl and module queries go
# through "$PAXD_BIN q ... --node $RPC -o json".

[ -n "${BASH_VERSION:-}" ] || { echo "gov-lib.sh must be sourced from bash" >&2; exit 1; }

GOV_TOOL=${GOV_TOOL:-gov}
GOV_FIXTURE_DIR=${GOV_FIXTURE_DIR:-}
PAXD_BIN=${PAXD_BIN:-paxd}
RPC=${RPC:-}

log() {
    printf '%s %s: %s\n' "$(date -u +%H:%M:%SZ)" "$GOV_TOOL" "$*" >&2
}

fail() {
    echo "$GOV_TOOL: $*" >&2
    exit 1
}

need_value() {
    [ "$#" -ge 2 ] && [ -n "$2" ] || fail "$1 needs a value"
}

require_tools() {
    local tool
    for tool in jq curl awk date; do
        command -v "$tool" >/dev/null 2>&1 || fail "$tool is not available"
    done
    if [ -z "$GOV_FIXTURE_DIR" ]; then
        command -v "$PAXD_BIN" >/dev/null 2>&1 || fail "$PAXD_BIN is not executable; pass --bin"
    fi
}

# --- time helpers -------------------------------------------------------------------------------

# iso_to_epoch 2026-09-26T15:00:00Z -> 1790434800. Fractional seconds are dropped.
iso_to_epoch() {
    local ts=$1 epoch
    [[ "$ts" =~ ^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]+)?Z$ ]] \
        || fail "time $ts is not YYYY-MM-DDTHH:MM:SSZ"
    ts=${ts%%.*}
    ts=${ts%Z}Z
    epoch=$(date -u -d "$ts" +%s 2>/dev/null) || fail "cannot parse time $ts"
    echo "$epoch"
}

epoch_to_iso() {
    date -u -d "@$1" +%Y-%m-%dT%H:%M:%SZ
}

# hours_between NOW LATER -> "+12.5h" or "-3.2h" (negative means LATER is already past).
hours_between() {
    awk -v a="$1" -v b="$2" 'BEGIN { printf "%+.1fh", (b - a) / 3600 }'
}

# --- pure block arithmetic ----------------------------------------------------------------------

# seconds_per_block H1 T1 H2 T2 -> decimal seconds per block between two (height, epoch) samples.
seconds_per_block() {
    local h1=$1 t1=$2 h2=$3 t2=$4
    [[ "$h1" =~ ^[0-9]+$ && "$h2" =~ ^[0-9]+$ ]] || fail "seconds_per_block: heights must be integers"
    [ "$h2" -gt "$h1" ] || fail "seconds_per_block: height did not advance ($h1 -> $h2)"
    [ "$t2" -gt "$t1" ] || fail "seconds_per_block: time did not advance ($t1 -> $t2)"
    awk -v h1="$h1" -v t1="$t1" -v h2="$h2" -v t2="$t2" 'BEGIN { printf "%.6f", (t2 - t1) / (h2 - h1) }'
}

# halt_height CURRENT_HEIGHT NOW_EPOCH TARGET_EPOCH SECONDS_PER_BLOCK -> first height whose expected
# time is at or after the target; rounds to the nearest block.
halt_height() {
    local h=$1 now=$2 target=$3 spb=$4
    [[ "$h" =~ ^[0-9]+$ ]] || fail "halt_height: current height must be an integer"
    [[ "$spb" =~ ^[0-9]*\.?[0-9]+$ ]] || fail "halt_height: seconds per block must be a positive decimal"
    [ "$target" -gt "$now" ] || fail "halt_height: target $(epoch_to_iso "$target") is not after $(epoch_to_iso "$now")"
    awk -v h="$h" -v now="$now" -v target="$target" -v spb="$spb" 'BEGIN {
        if (spb <= 0) { exit 2 }
        printf "%d", h + int((target - now) / spb + 0.5)
    }' || fail "halt_height: seconds per block must be positive"
}

# projected_epoch CURRENT_HEIGHT NOW_EPOCH HEIGHT SECONDS_PER_BLOCK -> epoch at which HEIGHT is expected.
projected_epoch() {
    awk -v h="$1" -v now="$2" -v target_h="$3" -v spb="$4" 'BEGIN { printf "%d", now + (target_h - h) * spb + 0.5 }'
}

# submission_deadline HALT_EPOCH VOTING_SECONDS MARGIN_SECONDS -> latest epoch at which a proposal
# can be submitted (with its full deposit) so that voting ends MARGIN seconds before the halt.
submission_deadline() {
    local halt=$1 voting=$2 margin=$3
    [[ "$voting" =~ ^[0-9]+$ && "$margin" =~ ^[0-9]+$ ]] || fail "submission_deadline: seconds must be integers"
    echo $((halt - voting - margin))
}

# voting_fits NOW_EPOCH HALT_EPOCH VOTING_SECONDS MARGIN_SECONDS -> exit 0 when a proposal submitted
# now finishes voting at least MARGIN seconds before the halt, exit 1 otherwise.
voting_fits() {
    local now=$1 halt=$2 voting=$3 margin=$4 deadline
    deadline=$(submission_deadline "$halt" "$voting" "$margin")
    [ "$now" -le "$deadline" ]
}

# --- durations ----------------------------------------------------------------------------------

# duration_ns 50ms -> 50000000. Accepts ns, us, ms, s, m, h with an integer or decimal count.
duration_ns() {
    local d=$1 count unit
    [[ "$d" =~ ^([0-9]+(\.[0-9]+)?)(ns|us|ms|s|m|h)$ ]] || fail "duration $d must be a number with unit ns, us, ms, s, m or h"
    count=${BASH_REMATCH[1]}
    unit=${BASH_REMATCH[3]}
    awk -v c="$count" -v u="$unit" 'BEGIN {
        mult["ns"] = 1; mult["us"] = 1000; mult["ms"] = 1000000; mult["s"] = 1000000000
        mult["m"] = 60000000000; mult["h"] = 3600000000000
        printf "%d", c * mult[u] + 0.5
    }'
}

# seconds_from_pb 172800s -> 172800; also accepts the amino nanosecond form 172800000000000.
seconds_from_pb() {
    local v=$1
    if [[ "$v" =~ ^([0-9]+)(\.[0-9]+)?s$ ]]; then
        echo "${BASH_REMATCH[1]}"
    elif [[ "$v" =~ ^[0-9]+$ ]]; then
        echo $((v / 1000000000))
    else
        fail "cannot read duration $v"
    fi
}

# ns_to_human 50000000 -> 50ms
ns_to_human() {
    awk -v n="$1" 'BEGIN {
        if (n % 1000000000 == 0) printf "%ds", n / 1000000000
        else if (n % 1000000 == 0) printf "%dms", n / 1000000
        else if (n % 1000 == 0) printf "%dus", n / 1000
        else printf "%dns", n
    }'
}

# --- chain queries ------------------------------------------------------------------------------

fixture_file() {
    local name=$1
    [ -r "$GOV_FIXTURE_DIR/$name.json" ] || fail "fixture $GOV_FIXTURE_DIR/$name.json is missing"
    cat "$GOV_FIXTURE_DIR/$name.json"
}

# rpc_get NAME PATH -> JSON body of the RPC endpoint, unwrapped from a JSON-RPC "result" envelope
# when the node adds one. NAME is the fixture file used in replay mode.
rpc_get() {
    local name=$1 path=$2 body
    if [ -n "$GOV_FIXTURE_DIR" ]; then
        fixture_file "$name"
        return
    fi
    body=$(curl -sf --max-time 20 "$RPC/$path") || fail "GET $RPC/$path failed"
    printf '%s' "$body" | jq -c '(.result // .)' || fail "GET $RPC/$path returned no JSON"
}

# paxd_query NAME ARGS... -> JSON of "paxd q ARGS -o json"; NAME is the fixture file in replay mode.
paxd_query() {
    local name=$1 out
    shift
    if [ -n "$GOV_FIXTURE_DIR" ]; then
        fixture_file "$name"
        return
    fi
    out=$("$PAXD_BIN" q "$@" --node "$RPC" -o json 2>&1) || fail "$PAXD_BIN q $* failed: $out"
    printf '%s\n' "$out" | grep -E '^\{' | tail -n 1
}

# paxd_query_optional NAME ARGS... -> like paxd_query but prints {} when the query reports nothing
# (upgrade plan with none scheduled, proposals with none submitted).
paxd_query_optional() {
    local name=$1 out
    shift
    if [ -n "$GOV_FIXTURE_DIR" ]; then
        fixture_file "$name"
        return
    fi
    if out=$("$PAXD_BIN" q "$@" --node "$RPC" -o json 2>&1); then
        printf '%s\n' "$out" | grep -E '^\{' | tail -n 1
    else
        echo '{}'
    fi
}

status_height() {
    rpc_get status status | jq -r '.sync_info.latest_block_height'
}

status_block_time() {
    rpc_get status status | jq -r '.sync_info.latest_block_time'
}

# block_time_at HEIGHT -> block timestamp; falls back to the node's earliest block when HEIGHT is pruned.
block_header_at() {
    local height=$1
    rpc_get block-lookback "block?height=$height" | jq -c '(.block // .).header | {height, time}'
}

validator_count() {
    rpc_get validators validators | jq -r '.validators | length'
}

# gov_params -> compact JSON {voting, expedited_voting, min_deposit, min_expedited_deposit,
# max_deposit_period, quorum, threshold, veto, expedited_quorum, expedited_threshold}; durations in
# seconds, deposits as "<amount><denom>".
gov_params() {
    paxd_query gov-params gov params | jq -c '{
        voting: .voting_params.voting_period,
        expedited_voting: .voting_params.expedited_voting_period,
        max_deposit_period: .deposit_params.max_deposit_period,
        min_deposit: (.deposit_params.min_deposit[0] | .amount + .denom),
        min_expedited_deposit: ((.deposit_params.min_expedited_deposit // [])[0] // {amount: "", denom: ""} | .amount + .denom),
        quorum: .tally_params.quorum, threshold: .tally_params.threshold, veto: .tally_params.veto_threshold,
        expedited_quorum: .tally_params.expedited_quorum, expedited_threshold: .tally_params.expedited_threshold
    }'
}

consensus_timeouts() {
    rpc_get consensus-params consensus_params | jq -c '.consensus_params.timeout'
}

# --- polling ------------------------------------------------------------------------------------

# poll_until SECONDS INTERVAL FUNCTION [ARGS...] -> runs FUNCTION until it returns 0 or SECONDS pass.
poll_until() {
    local limit=$1 interval=$2 deadline
    shift 2
    deadline=$((SECONDS + limit))
    while :; do
        if "$@"; then
            return 0
        fi
        [ "$SECONDS" -lt "$deadline" ] || return 1
        sleep "$interval"
    done
}

# --- transaction printing -----------------------------------------------------------------------

# tx_flags -> the flags every mutating paxd tx gets; printed in dry runs, passed when executing.
tx_flags() {
    printf -- '--from %q --chain-id %q --node %q --fees %q --gas %q --keyring-backend %q -b sync -y -o json' \
        "$KEY_NAME" "$COSMOS_CHAIN_ID" "$RPC" "$FEES" "$GAS" "$KEYRING_BACKEND"
    [ -z "${HOME_DIR:-}" ] || printf -- ' --home %q' "$HOME_DIR"
}

print_cmd() {
    printf '  %s\n' "$*"
}

# run_tx ARGS... -> broadcasts "paxd tx ARGS <tx_flags>", waits for inclusion and prints the tx JSON.
run_tx() {
    local out hash deadline result
    [ -z "$GOV_FIXTURE_DIR" ] || fail "refusing to execute against fixtures"
    # shellcheck disable=SC2046
    out=$("$PAXD_BIN" tx "$@" $(tx_flags) 2>&1) || fail "tx $* failed to broadcast: $out"
    out=$(printf '%s\n' "$out" | grep -E '^\{' | tail -n 1)
    [ "$(printf '%s' "$out" | jq -r '.code // 0')" = "0" ] || fail "tx $* rejected: $out"
    hash=$(printf '%s' "$out" | jq -r '.txhash')
    [[ "$hash" =~ ^[0-9A-Fa-f]{64}$ ]] || fail "tx $* returned no hash: $out"
    deadline=$((SECONDS + 120))
    while [ "$SECONDS" -lt "$deadline" ]; do
        if result=$("$PAXD_BIN" q tx "$hash" --node "$RPC" -o json 2>/dev/null); then
            [ "$(printf '%s' "$result" | jq -r '.code // 0')" = "0" ] \
                || fail "tx $* failed in block: $(printf '%s' "$result" | jq -r '.raw_log')"
            printf '%s\n' "$result"
            return 0
        fi
        sleep 2
    done
    fail "tx $hash was not included within 120s"
}

# wait_for_tally PROPOSAL_ID WAIT_SECONDS -> polls the proposal until it passes; fails on reject.
tally_passed() {
    local id=$1 status
    status=$(paxd_query proposal gov proposal "$id" | jq -r '.status')
    case "$status" in
        PROPOSAL_STATUS_PASSED) return 0 ;;
        PROPOSAL_STATUS_REJECTED|PROPOSAL_STATUS_FAILED) fail "proposal $id ended $status" ;;
        *) log "proposal $id is $status"; return 1 ;;
    esac
}

wait_for_tally() {
    local id=$1 wait=$2
    poll_until "$wait" 30 tally_passed "$id" || fail "proposal $id did not pass within ${wait}s"
}
