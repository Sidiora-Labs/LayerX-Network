#!/usr/bin/env bash
# Submits the consensus-parameter change for the fork: timeout commit, bypass commit timeout and
# timeout vote from the command line, with the same vote, tally and confirm flow as gov-upgrade.sh.
#
# Consensus timeouts are on-chain consensus params kept by the params module in the "baseapp"
# subspace under the key TimeoutParams (sdk/x/params/keeper/consensus_params.go), so the change is a
# ParamChange proposal: paxd tx gov submit-proposal param-change FILE. The value is the amino JSON
# of tendermint TimeoutParams, durations as nanosecond strings; the fields not given on the command
# line are copied from the live /consensus_params so the whole record stays explicit.
#
# Nothing is broadcast without --execute; the dry run writes the proposal file and prints the
# commands. With --target-utc the script refuses (exit 2) when voting would end after the halt,
# printing the latest submission time for both the standard and the expedited path. The confirm
# step polls /consensus_params until it reports the new values.
#
# With GOV_FIXTURE_DIR set (tests) every query is replayed from DIR/<name>.json, --now must be given
# and --execute is refused.
set -euo pipefail

# shellcheck source=platform/hosted/paxeer/gov-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/gov-lib.sh"
GOV_TOOL=gov-consensus-params

COSMOS_CHAIN_ID=hyperpax_125-1
KEY_NAME=""
TIMEOUT_COMMIT=""
BYPASS_COMMIT=""
TIMEOUT_VOTE=""
TITLE="Paxeer X consensus timeouts"
DESCRIPTION="Lowers timeout commit and timeout vote to the values proven by the latency benchmark"
DEPOSIT=""
FEES=1000000uhpx
GAS=2000000
KEYRING_BACKEND=os
HOME_DIR=""
EXPEDITED=0
TARGET_UTC=""
MARGIN_SECONDS=1800
SPB_OVERRIDE=""
NOW_OVERRIDE=""
PROPOSAL_FILE=""
PROPOSAL_ID=""
VOTERS=()
WAIT_SECONDS=180000
EXECUTE=0

usage() {
    cat >&2 <<'USAGE'
usage: gov-consensus-params.sh --rpc URL --key NAME --timeout-commit D --bypass-commit-timeout true|false
                               --timeout-vote D [options] [--execute]

required:
  --rpc URL                   Tendermint RPC of a synced node (read queries and broadcast)
  --key NAME                  key in the keyring that submits the proposal and casts its vote
  --timeout-commit D          new commit timeout, e.g. 50ms
  --bypass-commit-timeout B   true to commit as soon as +2/3 precommits arrive, false to wait
  --timeout-vote D            new prevote/precommit timeout, e.g. 50ms
options:
  --target-utc T              halt time; refuse unless voting ends --margin-seconds before it
  --seconds-per-block X       block rate for the halt projection (default: measured over the last 10000 blocks)
  --margin-seconds S          (default 1800)
  --deposit COINS             initial deposit (default: the chain's min deposit for the chosen path)
  --expedited                 submit as expedited
  --title T / --description D proposal text
  --proposal-file PATH        where the param-change JSON is written (default ./consensus-params.proposal.json)
  --voter NAME                validator operator key whose vote command is printed (repeatable)
  --proposal-id N             skip submission; wait for the tally of N and confirm the values
  --wait-seconds S            limit on the tally wait (default 180000)
  --now EPOCH                 wall-clock time to use instead of date +%s (tests)
  --chain-id ID               (default hyperpax_125-1)
  --fees COINS / --gas N      (default 1000000uhpx / 2000000)
  --keyring-backend B         (default os)
  --home DIR                  paxd home holding the keyring
  --bin PATH                  paxd binary (default paxd)
  --execute                   broadcast; without it every mutating command is printed only
USAGE
}

while [ "$#" -gt 0 ]; do
    case "$1" in
        --rpc) need_value "$@"; RPC=$2; shift 2 ;;
        --key) need_value "$@"; KEY_NAME=$2; shift 2 ;;
        --timeout-commit) need_value "$@"; TIMEOUT_COMMIT=$2; shift 2 ;;
        --bypass-commit-timeout) need_value "$@"; BYPASS_COMMIT=$2; shift 2 ;;
        --timeout-vote) need_value "$@"; TIMEOUT_VOTE=$2; shift 2 ;;
        --target-utc) need_value "$@"; TARGET_UTC=$2; shift 2 ;;
        --seconds-per-block) need_value "$@"; SPB_OVERRIDE=$2; shift 2 ;;
        --margin-seconds) need_value "$@"; MARGIN_SECONDS=$2; shift 2 ;;
        --deposit) need_value "$@"; DEPOSIT=$2; shift 2 ;;
        --expedited) EXPEDITED=1; shift ;;
        --title) need_value "$@"; TITLE=$2; shift 2 ;;
        --description) need_value "$@"; DESCRIPTION=$2; shift 2 ;;
        --proposal-file) need_value "$@"; PROPOSAL_FILE=$2; shift 2 ;;
        --voter) need_value "$@"; VOTERS+=("$2"); shift 2 ;;
        --proposal-id) need_value "$@"; PROPOSAL_ID=$2; shift 2 ;;
        --wait-seconds) need_value "$@"; WAIT_SECONDS=$2; shift 2 ;;
        --now) need_value "$@"; NOW_OVERRIDE=$2; shift 2 ;;
        --chain-id) need_value "$@"; COSMOS_CHAIN_ID=$2; shift 2 ;;
        --fees) need_value "$@"; FEES=$2; shift 2 ;;
        --gas) need_value "$@"; GAS=$2; shift 2 ;;
        --keyring-backend) need_value "$@"; KEYRING_BACKEND=$2; shift 2 ;;
        --home) need_value "$@"; HOME_DIR=$2; shift 2 ;;
        --bin) need_value "$@"; PAXD_BIN=$2; shift 2 ;;
        --execute) EXECUTE=1; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage; fail "unknown argument $1" ;;
    esac
done

[ -n "$RPC" ] || [ -n "$GOV_FIXTURE_DIR" ] || { usage; fail "--rpc is required"; }
[ -n "$KEY_NAME" ] || { usage; fail "--key is required"; }
[ -n "$TIMEOUT_COMMIT" ] && [ -n "$BYPASS_COMMIT" ] && [ -n "$TIMEOUT_VOTE" ] \
    || { usage; fail "--timeout-commit, --bypass-commit-timeout and --timeout-vote are required"; }
[ "$BYPASS_COMMIT" = true ] || [ "$BYPASS_COMMIT" = false ] || fail "--bypass-commit-timeout must be true or false"
[[ "$MARGIN_SECONDS" =~ ^[0-9]+$ ]] || fail "--margin-seconds must be a non-negative integer"
[ -z "$SPB_OVERRIDE" ] || [[ "$SPB_OVERRIDE" =~ ^[0-9]*\.?[0-9]+$ ]] || fail "--seconds-per-block must be a positive decimal"
[ -z "$NOW_OVERRIDE" ] || [[ "$NOW_OVERRIDE" =~ ^[0-9]{9,10}$ ]] || fail "--now must be a unix epoch"
[ -z "$PROPOSAL_ID" ] || [[ "$PROPOSAL_ID" =~ ^[1-9][0-9]*$ ]] || fail "--proposal-id must be a positive integer"
[[ "$WAIT_SECONDS" =~ ^[1-9][0-9]{0,6}$ ]] || fail "--wait-seconds must be a positive integer"
[ -z "$DEPOSIT" ] || [[ "$DEPOSIT" =~ ^[1-9][0-9]*[a-z]+$ ]] || fail "--deposit must be <amount><denom>"
[[ "$FEES" =~ ^[1-9][0-9]*[a-z]+$ ]] || fail "--fees must be <amount><denom>"
[[ "$GAS" =~ ^[1-9][0-9]{0,9}$ ]] || fail "--gas must be a positive integer"
if [ -n "$GOV_FIXTURE_DIR" ]; then
    [ "$EXECUTE" -eq 0 ] || fail "--execute is refused with GOV_FIXTURE_DIR set"
    [ -n "$NOW_OVERRIDE" ] || fail "fixture replay needs --now"
fi
[ -n "$PROPOSAL_FILE" ] || PROPOSAL_FILE=$PWD/consensus-params.proposal.json
require_tools

COMMIT_NS=$(duration_ns "$TIMEOUT_COMMIT")
VOTE_NS=$(duration_ns "$TIMEOUT_VOTE")
[ "$COMMIT_NS" -gt 0 ] && [ "$VOTE_NS" -gt 0 ] || fail "timeouts must be positive"
now() { if [ -n "$NOW_OVERRIDE" ]; then echo "$NOW_OVERRIDE"; else date -u +%s; fi; }

# --- 1. governance parameters, validator set and the current timeouts ---------------------------
PARAMS=$(gov_params)
VOTING_SECONDS=$(seconds_from_pb "$(jq -r .voting <<<"$PARAMS")")
EXPEDITED_SECONDS=$(seconds_from_pb "$(jq -r .expedited_voting <<<"$PARAMS")")
MIN_DEPOSIT=$(jq -r .min_deposit <<<"$PARAMS")
MIN_EXPEDITED_DEPOSIT=$(jq -r .min_expedited_deposit <<<"$PARAMS")
VALIDATORS=$(validator_count)
[[ "$VALIDATORS" =~ ^[1-9][0-9]*$ ]] || fail "could not count the validator set"
if [ "$EXPEDITED" -eq 1 ]; then
    PATH_SECONDS=$EXPEDITED_SECONDS; PATH_NAME=expedited
    [ -n "$DEPOSIT" ] || DEPOSIT=$MIN_EXPEDITED_DEPOSIT
else
    PATH_SECONDS=$VOTING_SECONDS; PATH_NAME=standard
    [ -n "$DEPOSIT" ] || DEPOSIT=$MIN_DEPOSIT
fi
[ -n "$DEPOSIT" ] || fail "the chain reports no minimum deposit for the $PATH_NAME path; pass --deposit"
log "gov: voting ${VOTING_SECONDS}s, expedited ${EXPEDITED_SECONDS}s, deposit $DEPOSIT ($PATH_NAME); quorum $(jq -r .quorum <<<"$PARAMS") threshold $(jq -r .threshold <<<"$PARAMS"); $VALIDATORS bonded validators"

CURRENT=$(consensus_timeouts)
[ "$(jq -r '.commit // empty' <<<"$CURRENT")" != "" ] || fail "/consensus_params returned no timeout block"
NEW_VALUE=$(jq -c --arg commit "$COMMIT_NS" --arg vote "$VOTE_NS" --argjson bypass "$BYPASS_COMMIT" '{
    propose: .propose, propose_delta: .propose_delta, vote: $vote, vote_delta: .vote_delta,
    commit: $commit, bypass_commit_timeout: $bypass
}' <<<"$CURRENT")
cat <<EOF
current timeouts   propose $(ns_to_human "$(jq -r .propose <<<"$CURRENT")") propose_delta $(ns_to_human "$(jq -r .propose_delta <<<"$CURRENT")") vote $(ns_to_human "$(jq -r .vote <<<"$CURRENT")") vote_delta $(ns_to_human "$(jq -r .vote_delta <<<"$CURRENT")") commit $(ns_to_human "$(jq -r .commit <<<"$CURRENT")") bypass $(jq -r .bypass_commit_timeout <<<"$CURRENT")
proposed timeouts  vote $(ns_to_human "$VOTE_NS") commit $(ns_to_human "$COMMIT_NS") bypass $BYPASS_COMMIT (other fields unchanged)
EOF
[ "$NEW_VALUE" != "$(jq -c . <<<"$CURRENT")" ] || fail "the chain already has these timeout params"

# --- 2. timing against the halt ------------------------------------------------------------------
NOW_EPOCH=$(now)
if [ -n "$TARGET_UTC" ]; then
    TARGET_EPOCH=$(iso_to_epoch "$TARGET_UTC")
    STATUS=$(rpc_get status status)
    LATEST_HEIGHT=$(jq -r '.sync_info.latest_block_height' <<<"$STATUS")
    if [ -n "$SPB_OVERRIDE" ]; then
        SPB=$SPB_OVERRIDE
    else
        LOOKBACK_HEIGHT=$((LATEST_HEIGHT - 10000))
        EARLIEST=$(jq -r '.sync_info.earliest_block_height' <<<"$STATUS")
        [ "$LOOKBACK_HEIGHT" -ge "$EARLIEST" ] || LOOKBACK_HEIGHT=$EARLIEST
        LOOKBACK=$(block_header_at "$LOOKBACK_HEIGHT")
        SPB=$(seconds_per_block "$(jq -r .height <<<"$LOOKBACK")" "$(iso_to_epoch "$(jq -r .time <<<"$LOOKBACK")")" \
            "$LATEST_HEIGHT" "$(iso_to_epoch "$(jq -r '.sync_info.latest_block_time' <<<"$STATUS")")")
    fi
    HALT_HEIGHT=$(halt_height "$LATEST_HEIGHT" "$NOW_EPOCH" "$TARGET_EPOCH" "$SPB")
    STD_DEADLINE=$(submission_deadline "$TARGET_EPOCH" "$VOTING_SECONDS" "$MARGIN_SECONDS")
    EXP_DEADLINE=$(submission_deadline "$TARGET_EPOCH" "$EXPEDITED_SECONDS" "$MARGIN_SECONDS")
    cat <<EOF
halt target        $TARGET_UTC, about height $HALT_HEIGHT at ${SPB}s/block from $LATEST_HEIGHT
latest submission  standard  (${VOTING_SECONDS}s vote + ${MARGIN_SECONDS}s margin): $(epoch_to_iso "$STD_DEADLINE") ($(hours_between "$NOW_EPOCH" "$STD_DEADLINE"))
latest submission  expedited (${EXPEDITED_SECONDS}s vote + ${MARGIN_SECONDS}s margin): $(epoch_to_iso "$EXP_DEADLINE") ($(hours_between "$NOW_EPOCH" "$EXP_DEADLINE"))
EOF
    if [ -z "$PROPOSAL_ID" ] && ! voting_fits "$NOW_EPOCH" "$TARGET_EPOCH" "$PATH_SECONDS" "$MARGIN_SECONDS"; then
        echo "REFUSED: a $PATH_NAME proposal submitted now would end voting at $(epoch_to_iso $((NOW_EPOCH + PATH_SECONDS))), after the halt at $TARGET_UTC minus the ${MARGIN_SECONDS}s margin" >&2
        if [ "$EXPEDITED" -eq 0 ] && voting_fits "$NOW_EPOCH" "$TARGET_EPOCH" "$EXPEDITED_SECONDS" "$MARGIN_SECONDS"; then
            echo "the expedited path still fits until $(epoch_to_iso "$EXP_DEADLINE"); rerun with --expedited (deposit $MIN_EXPEDITED_DEPOSIT, two-thirds quorum and threshold)" >&2
        else
            echo "no governance path ends before the halt; submit this change after the fork resumes instead" >&2
        fi
        exit 2
    fi
else
    echo "voting ends       $(epoch_to_iso $((NOW_EPOCH + PATH_SECONDS))) if submitted now ($PATH_NAME path; pass --target-utc to check it against the halt)"
fi

# --- 3. the proposal file and transactions ------------------------------------------------------
jq -n --arg title "$TITLE" --arg description "$DESCRIPTION" --arg deposit "$DEPOSIT" --argjson value "$NEW_VALUE" '{
    title: $title, description: $description,
    changes: [{subspace: "baseapp", key: "TimeoutParams", value: $value}],
    deposit: $deposit
}' >"$PROPOSAL_FILE"
echo "proposal file      $PROPOSAL_FILE"

SUBMIT=(gov submit-proposal param-change "$PROPOSAL_FILE")
[ "$EXPEDITED" -eq 0 ] || SUBMIT+=(--is-expedited)

vote_cmd() {
    printf '%q tx gov vote %s yes --from %s --chain-id %q --node %q --fees %q -b sync -y\n' \
        "$PAXD_BIN" "$1" "$2" "$COSMOS_CHAIN_ID" "$RPC" "$FEES"
}

print_votes() {
    local id=$1 i
    echo "vote from every validator operator (${VALIDATORS} bonded):"
    if [ "${#VOTERS[@]}" -gt 0 ]; then
        for v in "${VOTERS[@]}"; do print_cmd "$(vote_cmd "$id" "$v")"; done
    else
        for ((i = 1; i <= VALIDATORS; i++)); do print_cmd "$(vote_cmd "$id" "<validator-$i-operator-key>")"; done
    fi
}

values_applied() {
    local live
    live=$(consensus_timeouts)
    [ "$(jq -r .commit <<<"$live")" = "$COMMIT_NS" ] && [ "$(jq -r .vote <<<"$live")" = "$VOTE_NS" ] \
        && [ "$(jq -r .bypass_commit_timeout <<<"$live")" = "$BYPASS_COMMIT" ]
}

if [ "$EXECUTE" -eq 0 ]; then
    if [ -z "$PROPOSAL_ID" ]; then
        echo "dry run: would submit (add --execute to broadcast)"
        print_cmd "$PAXD_BIN tx $(printf '%q ' "${SUBMIT[@]}")$(tx_flags)"
        print_cmd "$PAXD_BIN q gov proposals --node $RPC -o json | jq '[.proposals[] | select(.content.changes[0].key == \"TimeoutParams\")] | last | {proposal_id, status, voting_end_time}'"
        print_votes "<proposal_id>"
        echo "then wait for PROPOSAL_STATUS_PASSED and confirm the values:"
        print_cmd "$PAXD_BIN q gov proposal <proposal_id> --node $RPC -o json | jq -r .status"
    else
        print_votes "$PROPOSAL_ID"
        echo "dry run: would wait for proposal $PROPOSAL_ID to pass and confirm the values:"
        print_cmd "$PAXD_BIN q gov proposal $PROPOSAL_ID --node $RPC -o json | jq -r .status"
    fi
    print_cmd "curl -s $RPC/consensus_params | jq '(.result // .).consensus_params.timeout'   # expect commit $COMMIT_NS vote $VOTE_NS bypass $BYPASS_COMMIT"
    exit 0
fi

if [ -z "$PROPOSAL_ID" ]; then
    log "submitting the TimeoutParams change with deposit $DEPOSIT ($PATH_NAME path)"
    run_tx "${SUBMIT[@]}" >/dev/null
    PROPOSAL_ID=$(paxd_query_optional proposals gov proposals \
        | jq -r '[(.proposals // [])[] | select((.content.changes // [])[0].key == "TimeoutParams")] | last | .proposal_id // empty')
    [[ "$PROPOSAL_ID" =~ ^[0-9]+$ ]] || fail "could not find the TimeoutParams proposal after submission"
    log "proposal $PROPOSAL_ID submitted; voting ends $(paxd_query proposal gov proposal "$PROPOSAL_ID" | jq -r .voting_end_time)"
    run_tx gov vote "$PROPOSAL_ID" yes >/dev/null
    log "voted yes from $KEY_NAME"
fi
print_votes "$PROPOSAL_ID"
log "waiting up to ${WAIT_SECONDS}s for proposal $PROPOSAL_ID to pass"
wait_for_tally "$PROPOSAL_ID" "$WAIT_SECONDS"
log "proposal $PROPOSAL_ID passed; confirming the consensus params"
poll_until 300 10 values_applied || fail "/consensus_params does not show the new values: $(consensus_timeouts)"
echo "applied: $(consensus_timeouts)"
