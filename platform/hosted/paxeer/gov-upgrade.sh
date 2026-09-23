#!/usr/bin/env bash
# Drives the Paxeer X software-upgrade governance for the fork: computes the halt height from the
# live block rate, submits the software-upgrade proposal with the minimum deposit, prints the vote
# command for every validator, waits for the tally and confirms the scheduled plan through the
# upgrade plan query (the current_plan endpoint).
#
# Two halt-height estimates are printed and one is used (--anchor, default wallclock):
#   timestamp  block rate from block timestamps over --lookback-blocks, projected from the latest
#              block's own timestamp; block timestamps can lag wall clock, so this estimate lands
#              the halt at TARGET in block time, not necessarily in wall-clock time;
#   wallclock  block rate measured against this machine's clock over --measure-seconds (or taken
#              from --seconds-per-block), projected from the wall-clock time of the measurement.
# The gap between the two is printed as the timestamp skew.
#
# Nothing is broadcast without --execute: the dry run prints every paxd command it would run. With
# --proposal-id the submission is skipped and the script resumes at the wait-and-confirm stage.
# Governance voting_period, expedited_voting_period, min deposits and tally thresholds are read
# from the chain; the script refuses (exit 2) when voting would end after the halt for the chosen
# path and prints the latest submission time for both the standard and the expedited path.
#
# With GOV_FIXTURE_DIR set (tests) every query is replayed from DIR/<name>.json, --now and
# --seconds-per-block must be given and --execute is refused.
set -euo pipefail

# shellcheck source=platform/hosted/paxeer/gov-lib.sh
. "$(dirname "${BASH_SOURCE[0]}")/gov-lib.sh"
GOV_TOOL=gov-upgrade

UPGRADE_NAME=v6.6
COSMOS_CHAIN_ID=hyperpax_125-1
KEY_NAME=""
TARGET_UTC=""
UPGRADE_INFO=""
TITLE=""
DESCRIPTION="Paxeer X: adds the LayerX custody, anchor, exchange, bridge and launchpad modules"
DEPOSIT=""
FEES=1000000uhpx
GAS=2000000
KEYRING_BACKEND=os
HOME_DIR=""
EXPEDITED=0
MARGIN_SECONDS=1800
MEASURE_SECONDS=120
LOOKBACK_BLOCKS=10000
SPB_OVERRIDE=""
NOW_OVERRIDE=""
ANCHOR=wallclock
HALT_HEIGHT_OVERRIDE=""
PROPOSAL_ID=""
VOTERS=()
WAIT_SECONDS=180000
EXECUTE=0

usage() {
    cat >&2 <<'USAGE'
usage: gov-upgrade.sh --rpc URL --key NAME --target-utc YYYY-MM-DDTHH:MM:SSZ [options] [--execute]
       gov-upgrade.sh --rpc URL --key NAME --target-utc T --proposal-id N [--execute]   (resume)

required:
  --rpc URL                   Tendermint RPC of a synced node (read queries and broadcast)
  --key NAME                  key in the keyring that submits the proposal and casts its vote
  --target-utc T              wall-clock halt time; the halt height is derived from it
options:
  --upgrade-name NAME         plan name (default v6.6; v6.5 is refused, it is already in the binary)
  --upgrade-info JSON         plan info, e.g. {"binaries":{"linux/amd64":"<url>?checksum=sha256:<sha>"}}
  --title T / --description D proposal text
  --deposit COINS             initial deposit (default: the chain's min deposit for the chosen path)
  --expedited                 submit as expedited (24 h path, higher deposit and 2/3 tally)
  --voter NAME                key name of a validator operator whose vote command is printed
                              (repeatable; default: one placeholder per bonded validator)
  --margin-seconds S          voting must end this long before the halt (default 1800)
  --measure-seconds S         wall-clock block-rate sample length (default 120)
  --seconds-per-block X       skip the wall-clock measurement and use X
  --lookback-blocks N         span of the timestamp-based rate (default 10000)
  --anchor wallclock|timestamp  which estimate becomes the plan height (default wallclock)
  --halt-height H             use this height instead of the estimate (estimates still printed)
  --now EPOCH                 wall-clock time to use instead of date +%s (tests)
  --proposal-id N             skip submission; wait for the tally of N and confirm the plan
  --wait-seconds S            limit on the tally wait (default 180000)
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
        --target-utc) need_value "$@"; TARGET_UTC=$2; shift 2 ;;
        --upgrade-name) need_value "$@"; UPGRADE_NAME=$2; shift 2 ;;
        --upgrade-info) need_value "$@"; UPGRADE_INFO=$2; shift 2 ;;
        --title) need_value "$@"; TITLE=$2; shift 2 ;;
        --description) need_value "$@"; DESCRIPTION=$2; shift 2 ;;
        --deposit) need_value "$@"; DEPOSIT=$2; shift 2 ;;
        --expedited) EXPEDITED=1; shift ;;
        --voter) need_value "$@"; VOTERS+=("$2"); shift 2 ;;
        --margin-seconds) need_value "$@"; MARGIN_SECONDS=$2; shift 2 ;;
        --measure-seconds) need_value "$@"; MEASURE_SECONDS=$2; shift 2 ;;
        --seconds-per-block) need_value "$@"; SPB_OVERRIDE=$2; shift 2 ;;
        --lookback-blocks) need_value "$@"; LOOKBACK_BLOCKS=$2; shift 2 ;;
        --anchor) need_value "$@"; ANCHOR=$2; shift 2 ;;
        --halt-height) need_value "$@"; HALT_HEIGHT_OVERRIDE=$2; shift 2 ;;
        --now) need_value "$@"; NOW_OVERRIDE=$2; shift 2 ;;
        --proposal-id) need_value "$@"; PROPOSAL_ID=$2; shift 2 ;;
        --wait-seconds) need_value "$@"; WAIT_SECONDS=$2; shift 2 ;;
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
[ -n "$TARGET_UTC" ] || { usage; fail "--target-utc is required"; }
[[ "$UPGRADE_NAME" =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$ ]] || fail "--upgrade-name must be letters, digits, '.', '_' or '-'"
[ "$UPGRADE_NAME" != "v6.5" ] || fail "plan name v6.5 is already applied by the running binary; the fork upgrade is v6.6"
[[ "$MARGIN_SECONDS" =~ ^[0-9]+$ ]] || fail "--margin-seconds must be a non-negative integer"
[[ "$MEASURE_SECONDS" =~ ^[1-9][0-9]{0,4}$ ]] || fail "--measure-seconds must be a positive integer"
[[ "$LOOKBACK_BLOCKS" =~ ^[1-9][0-9]{0,7}$ ]] || fail "--lookback-blocks must be a positive integer"
[ -z "$SPB_OVERRIDE" ] || [[ "$SPB_OVERRIDE" =~ ^[0-9]*\.?[0-9]+$ ]] || fail "--seconds-per-block must be a positive decimal"
[ -z "$NOW_OVERRIDE" ] || [[ "$NOW_OVERRIDE" =~ ^[0-9]{9,10}$ ]] || fail "--now must be a unix epoch"
[ "$ANCHOR" = wallclock ] || [ "$ANCHOR" = timestamp ] || fail "--anchor must be wallclock or timestamp"
[ -z "$HALT_HEIGHT_OVERRIDE" ] || [[ "$HALT_HEIGHT_OVERRIDE" =~ ^[1-9][0-9]*$ ]] || fail "--halt-height must be a positive integer"
[ -z "$PROPOSAL_ID" ] || [[ "$PROPOSAL_ID" =~ ^[1-9][0-9]*$ ]] || fail "--proposal-id must be a positive integer"
[[ "$WAIT_SECONDS" =~ ^[1-9][0-9]{0,6}$ ]] || fail "--wait-seconds must be a positive integer"
[ -z "$DEPOSIT" ] || [[ "$DEPOSIT" =~ ^[1-9][0-9]*[a-z]+$ ]] || fail "--deposit must be <amount><denom>"
[[ "$FEES" =~ ^[1-9][0-9]*[a-z]+$ ]] || fail "--fees must be <amount><denom>"
[[ "$GAS" =~ ^[1-9][0-9]{0,9}$ ]] || fail "--gas must be a positive integer"
if [ -n "$GOV_FIXTURE_DIR" ]; then
    [ "$EXECUTE" -eq 0 ] || fail "--execute is refused with GOV_FIXTURE_DIR set"
    [ -n "$NOW_OVERRIDE" ] && [ -n "$SPB_OVERRIDE" ] || fail "fixture replay needs --now and --seconds-per-block"
fi
[ -n "$TITLE" ] || TITLE="Paxeer X $UPGRADE_NAME"
require_tools

TARGET_EPOCH=$(iso_to_epoch "$TARGET_UTC")
now() { if [ -n "$NOW_OVERRIDE" ]; then echo "$NOW_OVERRIDE"; else date -u +%s; fi; }

# --- 1. governance parameters and validator set -------------------------------------------------
PARAMS=$(gov_params)
VOTING_SECONDS=$(seconds_from_pb "$(jq -r .voting <<<"$PARAMS")")
EXPEDITED_SECONDS=$(seconds_from_pb "$(jq -r .expedited_voting <<<"$PARAMS")")
MAX_DEPOSIT_SECONDS=$(seconds_from_pb "$(jq -r .max_deposit_period <<<"$PARAMS")")
MIN_DEPOSIT=$(jq -r .min_deposit <<<"$PARAMS")
MIN_EXPEDITED_DEPOSIT=$(jq -r .min_expedited_deposit <<<"$PARAMS")
VALIDATORS=$(validator_count)
[[ "$VALIDATORS" =~ ^[1-9][0-9]*$ ]] || fail "could not count the validator set"
log "gov: voting ${VOTING_SECONDS}s, expedited ${EXPEDITED_SECONDS}s, max deposit period ${MAX_DEPOSIT_SECONDS}s, min deposit $MIN_DEPOSIT, expedited $MIN_EXPEDITED_DEPOSIT"
log "tally: quorum $(jq -r .quorum <<<"$PARAMS") threshold $(jq -r .threshold <<<"$PARAMS") veto $(jq -r .veto <<<"$PARAMS"); expedited quorum $(jq -r .expedited_quorum <<<"$PARAMS") threshold $(jq -r .expedited_threshold <<<"$PARAMS"); $VALIDATORS bonded validators"

if [ "$EXPEDITED" -eq 1 ]; then
    PATH_SECONDS=$EXPEDITED_SECONDS
    [ -n "$DEPOSIT" ] || DEPOSIT=$MIN_EXPEDITED_DEPOSIT
    PATH_NAME=expedited
else
    PATH_SECONDS=$VOTING_SECONDS
    [ -n "$DEPOSIT" ] || DEPOSIT=$MIN_DEPOSIT
    PATH_NAME=standard
fi
[ -n "$DEPOSIT" ] || fail "the chain reports no minimum deposit for the $PATH_NAME path; pass --deposit"

# --- 2. halt height -------------------------------------------------------------------------------
STATUS=$(rpc_get status status)
LATEST_HEIGHT=$(jq -r '.sync_info.latest_block_height' <<<"$STATUS")
LATEST_TIME=$(jq -r '.sync_info.latest_block_time' <<<"$STATUS")
EARLIEST_HEIGHT=$(jq -r '.sync_info.earliest_block_height' <<<"$STATUS")
[[ "$LATEST_HEIGHT" =~ ^[0-9]+$ ]] || fail "status returned no height"
[ "$(jq -r '.sync_info.catching_up' <<<"$STATUS")" = "false" ] || fail "the RPC node is still catching up"
LATEST_EPOCH=$(iso_to_epoch "$LATEST_TIME")

LOOKBACK_HEIGHT=$((LATEST_HEIGHT - LOOKBACK_BLOCKS))
[ "$LOOKBACK_HEIGHT" -ge "$EARLIEST_HEIGHT" ] || LOOKBACK_HEIGHT=$EARLIEST_HEIGHT
LOOKBACK=$(block_header_at "$LOOKBACK_HEIGHT")
LOOKBACK_HEIGHT=$(jq -r .height <<<"$LOOKBACK")
LOOKBACK_EPOCH=$(iso_to_epoch "$(jq -r .time <<<"$LOOKBACK")")
SPB_TS=$(seconds_per_block "$LOOKBACK_HEIGHT" "$LOOKBACK_EPOCH" "$LATEST_HEIGHT" "$LATEST_EPOCH")

if [ -n "$SPB_OVERRIDE" ]; then
    SPB_WC=$SPB_OVERRIDE
    WC_HEIGHT=$LATEST_HEIGHT
    NOW_EPOCH=$(now)
    log "wall-clock rate taken from --seconds-per-block $SPB_WC"
else
    T1=$(now); H1=$LATEST_HEIGHT
    log "measuring the wall-clock block rate for ${MEASURE_SECONDS}s from height $H1"
    sleep "$MEASURE_SECONDS"
    H2=$(status_height); T2=$(now)
    SPB_WC=$(seconds_per_block "$H1" "$T1" "$H2" "$T2")
    WC_HEIGHT=$H2
    NOW_EPOCH=$T2
    LATEST_TIME=$(status_block_time)
    LATEST_EPOCH=$(iso_to_epoch "$LATEST_TIME")
fi
SKEW=$((NOW_EPOCH - LATEST_EPOCH))

HALT_TS=$(halt_height "$LATEST_HEIGHT" "$LATEST_EPOCH" "$TARGET_EPOCH" "$SPB_TS")
HALT_WC=$(halt_height "$WC_HEIGHT" "$NOW_EPOCH" "$TARGET_EPOCH" "$SPB_WC")
if [ -n "$HALT_HEIGHT_OVERRIDE" ]; then
    HALT_HEIGHT=$HALT_HEIGHT_OVERRIDE
    HALT_SOURCE=--halt-height
elif [ "$ANCHOR" = timestamp ]; then
    HALT_HEIGHT=$HALT_TS
    HALT_SOURCE=timestamp
else
    HALT_HEIGHT=$HALT_WC
    HALT_SOURCE=wallclock
fi
HALT_EPOCH=$(projected_epoch "$WC_HEIGHT" "$NOW_EPOCH" "$HALT_HEIGHT" "$SPB_WC")
[ "$HALT_HEIGHT" -gt "$WC_HEIGHT" ] || fail "halt height $HALT_HEIGHT is not above the current height $WC_HEIGHT"

cat <<EOF
halt target        $TARGET_UTC (epoch $TARGET_EPOCH)
now                $(epoch_to_iso "$NOW_EPOCH") wall clock; latest block $LATEST_HEIGHT at $LATEST_TIME (skew ${SKEW}s)
rate (timestamp)   ${SPB_TS}s/block over blocks $LOOKBACK_HEIGHT..$LATEST_HEIGHT  -> halt height $HALT_TS
rate (wallclock)   ${SPB_WC}s/block                                  -> halt height $HALT_WC
halt height        $HALT_HEIGHT ($HALT_SOURCE), expected at $(epoch_to_iso "$HALT_EPOCH") wall clock
EOF

# --- 3. voting must end before the halt ---------------------------------------------------------
STD_DEADLINE=$(submission_deadline "$HALT_EPOCH" "$VOTING_SECONDS" "$MARGIN_SECONDS")
EXP_DEADLINE=$(submission_deadline "$HALT_EPOCH" "$EXPEDITED_SECONDS" "$MARGIN_SECONDS")
cat <<EOF
latest submission  standard  (${VOTING_SECONDS}s vote + ${MARGIN_SECONDS}s margin): $(epoch_to_iso "$STD_DEADLINE") ($(hours_between "$NOW_EPOCH" "$STD_DEADLINE"))
latest submission  expedited (${EXPEDITED_SECONDS}s vote + ${MARGIN_SECONDS}s margin): $(epoch_to_iso "$EXP_DEADLINE") ($(hours_between "$NOW_EPOCH" "$EXP_DEADLINE"))
EOF

if [ -n "$PROPOSAL_ID" ]; then
    PROPOSAL=$(paxd_query proposal gov proposal "$PROPOSAL_ID")
    PLAN_NAME=$(jq -r '.content.plan.name // empty' <<<"$PROPOSAL")
    PLAN_HEIGHT=$(jq -r '.content.plan.height // empty' <<<"$PROPOSAL")
    VOTING_END=$(iso_to_epoch "$(jq -r '.voting_end_time' <<<"$PROPOSAL")")
    [ "$PLAN_NAME" = "$UPGRADE_NAME" ] || fail "proposal $PROPOSAL_ID is not a $UPGRADE_NAME software upgrade (plan $PLAN_NAME)"
    HALT_HEIGHT=$PLAN_HEIGHT
    HALT_EPOCH=$(projected_epoch "$WC_HEIGHT" "$NOW_EPOCH" "$HALT_HEIGHT" "$SPB_WC")
    echo "proposal $PROPOSAL_ID  plan $PLAN_NAME at $PLAN_HEIGHT (expected $(epoch_to_iso "$HALT_EPOCH")), voting ends $(epoch_to_iso "$VOTING_END")"
    if [ "$VOTING_END" -gt $((HALT_EPOCH - MARGIN_SECONDS)) ]; then
        echo "REFUSED: proposal $PROPOSAL_ID voting ends $(epoch_to_iso "$VOTING_END"), after halt height $HALT_HEIGHT minus the ${MARGIN_SECONDS}s margin ($(epoch_to_iso $((HALT_EPOCH - MARGIN_SECONDS)))); cancel the plan and re-propose" >&2
        exit 2
    fi
elif ! voting_fits "$NOW_EPOCH" "$HALT_EPOCH" "$PATH_SECONDS" "$MARGIN_SECONDS"; then
    VOTING_END=$((NOW_EPOCH + PATH_SECONDS))
    echo "REFUSED: a $PATH_NAME proposal submitted now would end voting at $(epoch_to_iso "$VOTING_END"), after halt height $HALT_HEIGHT minus the ${MARGIN_SECONDS}s margin ($(epoch_to_iso $((HALT_EPOCH - MARGIN_SECONDS)))); the $PATH_NAME submission deadline was $(epoch_to_iso "$(submission_deadline "$HALT_EPOCH" "$PATH_SECONDS" "$MARGIN_SECONDS")")" >&2
    if [ "$EXPEDITED" -eq 0 ] && voting_fits "$NOW_EPOCH" "$HALT_EPOCH" "$EXPEDITED_SECONDS" "$MARGIN_SECONDS"; then
        echo "the expedited path still fits until $(epoch_to_iso "$EXP_DEADLINE"); rerun with --expedited (deposit $MIN_EXPEDITED_DEPOSIT, two-thirds quorum and threshold)" >&2
    else
        echo "no governance path ends before the halt; move the target or run the fork by halt-height coordination" >&2
    fi
    exit 2
fi

# --- 4. the transactions ------------------------------------------------------------------------
SUBMIT=("$PAXD_BIN" tx gov submit-proposal software-upgrade "$UPGRADE_NAME" --upgrade-height "$HALT_HEIGHT"
    --title "$TITLE" --description "$DESCRIPTION" --deposit "$DEPOSIT")
[ -z "$UPGRADE_INFO" ] || SUBMIT+=(--upgrade-info "$UPGRADE_INFO")
[ "$EXPEDITED" -eq 0 ] || SUBMIT+=(--is-expedited)

vote_cmd() {
    printf '%q tx gov vote %s yes --from %s --chain-id %q --node %q --fees %q -b sync -y\n' \
        "$PAXD_BIN" "$1" "$2" "$COSMOS_CHAIN_ID" "$RPC" "$FEES"
}

print_votes() {
    local id=$1 i
    echo "vote from every validator operator (${VALIDATORS} bonded; yes votes must reach quorum and threshold above):"
    if [ "${#VOTERS[@]}" -gt 0 ]; then
        for v in "${VOTERS[@]}"; do print_cmd "$(vote_cmd "$id" "$v")"; done
    else
        for ((i = 1; i <= VALIDATORS; i++)); do print_cmd "$(vote_cmd "$id" "<validator-$i-operator-key>")"; done
    fi
}

plan_scheduled() {
    local plan
    plan=$(paxd_query_optional upgrade-plan upgrade plan)
    [ "$(jq -r '.name // empty' <<<"$plan")" = "$UPGRADE_NAME" ] && [ "$(jq -r '.height // empty' <<<"$plan")" = "$HALT_HEIGHT" ]
}

if [ "$EXECUTE" -eq 0 ]; then
    if [ -z "$PROPOSAL_ID" ]; then
        echo "dry run: would submit (add --execute to broadcast)"
        print_cmd "$(printf '%q ' "${SUBMIT[@]}")$(tx_flags)"
        print_cmd "$PAXD_BIN q gov proposals --node $RPC -o json | jq '[.proposals[] | select(.content.plan.name == \"$UPGRADE_NAME\")] | last | {proposal_id, status, voting_end_time}'"
        print_votes "<proposal_id>"
        echo "then wait for PROPOSAL_STATUS_PASSED and confirm the plan:"
        print_cmd "$PAXD_BIN q gov proposal <proposal_id> --node $RPC -o json | jq -r .status"
    else
        print_votes "$PROPOSAL_ID"
        echo "dry run: would wait for proposal $PROPOSAL_ID to pass and confirm the plan:"
        print_cmd "$PAXD_BIN q gov proposal $PROPOSAL_ID --node $RPC -o json | jq -r .status"
    fi
    print_cmd "$PAXD_BIN q upgrade plan --node $RPC -o json   # expect name $UPGRADE_NAME height $HALT_HEIGHT"
    print_cmd "curl -s <rest>/cosmos/upgrade/v1beta1/current_plan"
    exit 0
fi

if [ -z "$PROPOSAL_ID" ]; then
    log "submitting $UPGRADE_NAME at height $HALT_HEIGHT with deposit $DEPOSIT ($PATH_NAME path)"
    run_tx "${SUBMIT[@]:2}" >/dev/null
    PROPOSAL_ID=$(paxd_query_optional proposals gov proposals \
        | jq -r --arg name "$UPGRADE_NAME" '[(.proposals // [])[] | select(.content.plan.name == $name)] | last | .proposal_id // empty')
    [[ "$PROPOSAL_ID" =~ ^[0-9]+$ ]] || fail "could not find the $UPGRADE_NAME proposal after submission"
    log "proposal $PROPOSAL_ID submitted; voting ends $(paxd_query proposal gov proposal "$PROPOSAL_ID" | jq -r .voting_end_time)"
    run_tx gov vote "$PROPOSAL_ID" yes >/dev/null
    log "voted yes from $KEY_NAME"
fi
print_votes "$PROPOSAL_ID"
log "waiting up to ${WAIT_SECONDS}s for proposal $PROPOSAL_ID to pass"
wait_for_tally "$PROPOSAL_ID" "$WAIT_SECONDS"
log "proposal $PROPOSAL_ID passed; confirming the scheduled plan"
poll_until 300 10 plan_scheduled || fail "upgrade plan does not show $UPGRADE_NAME at $HALT_HEIGHT: $(paxd_query_optional upgrade-plan upgrade plan)"
echo "scheduled: $(paxd_query_optional upgrade-plan upgrade plan | jq -c '{name, height, info}')"
