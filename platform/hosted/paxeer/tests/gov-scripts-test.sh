#!/usr/bin/env bash
# Verifies gov-upgrade.sh and gov-consensus-params.sh without a live chain: the pure height and
# deadline arithmetic from gov-lib.sh, and both scripts in dry-run mode replaying recorded mainnet
# fixtures (tests/fixtures/gov, captured from a synced full node with host details scrubbed).
#
# The disposable single-validator chain of fork-rehearsal.sh is not started here: it needs the data
# directory of a stopped synced mainnet node, which this test must not assume (and must never take
# from the live paxd on the host). When PAXEER_FORK_ENV names an env file with SOURCE_DATA, OLD_BIN
# and NEW_BIN the rehearsal can be run by hand; this test stays fixture-based either way.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
PAXEER=$(dirname "$HERE")
FIXTURES=$HERE/fixtures/gov
UPGRADE=$PAXEER/gov-upgrade.sh
CONSENSUS=$PAXEER/gov-consensus-params.sh
LIB=$PAXEER/gov-lib.sh
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

PASS=0
FAILED=0
fail_case() {
    echo "FAIL: $*" >&2
    FAILED=$((FAILED + 1))
}
ok() {
    PASS=$((PASS + 1))
}
expect_eq() {
    local name=$1 got=$2 want=$3
    if [ "$got" = "$want" ]; then ok; else fail_case "$name: got '$got', want '$want'"; fi
}
expect_contains() {
    local name=$1 haystack=$2 needle=$3
    if [[ "$haystack" == *"$needle"* ]]; then ok; else fail_case "$name: output lacks '$needle'"; printf '%s\n' "$haystack" | sed 's/^/    /' >&2; fi
}
expect_missing() {
    local name=$1 haystack=$2 needle=$3
    if [[ "$haystack" != *"$needle"* ]]; then ok; else fail_case "$name: output must not contain '$needle'"; fi
}

for f in "$UPGRADE" "$CONSENSUS" "$LIB"; do
    bash -n "$f" || fail_case "syntax: $f"
done
if command -v shellcheck >/dev/null 2>&1; then
    shellcheck -x "$UPGRADE" "$CONSENSUS" "$LIB" "${BASH_SOURCE[0]}" && ok || fail_case "shellcheck"
else
    echo "shellcheck not installed; skipped" >&2
fi

# --- pure arithmetic ---------------------------------------------------------------------------
# shellcheck source=platform/hosted/paxeer/gov-lib.sh
. "$LIB"
expect_eq "seconds_per_block" "$(seconds_per_block 100 1000 200 1040)" "0.400000"
expect_eq "halt_height exact" "$(halt_height 1000 0 100 0.4)" "1250"
expect_eq "halt_height rounds up" "$(halt_height 1000 0 101 0.4)" "1253"
expect_eq "halt_height rounds down" "$(halt_height 23869863 1790179835 1790434800 0.3946)" "24515998"
expect_eq "projected_epoch" "$(projected_epoch 23869863 1790179835 24515998 0.3946)" "1790434800"
expect_eq "submission_deadline standard" "$(submission_deadline 1790434800 172800 1800)" "1790260200"
expect_eq "submission_deadline expedited" "$(submission_deadline 1790434800 86400 1800)" "1790346600"
if voting_fits 1790260200 1790434800 172800 1800; then ok; else fail_case "voting_fits at the deadline"; fi
if voting_fits 1790260201 1790434800 172800 1800; then fail_case "voting_fits one second late"; else ok; fi
if voting_fits 1790346600 1790434800 86400 1800; then ok; else fail_case "voting_fits expedited at the deadline"; fi
expect_eq "iso_to_epoch" "$(iso_to_epoch 2026-09-26T15:00:00Z)" "1790434800"
expect_eq "iso_to_epoch fractional" "$(iso_to_epoch 2026-09-23T16:10:34.772748938Z)" "1790179834"
expect_eq "duration_ns ms" "$(duration_ns 50ms)" "50000000"
expect_eq "duration_ns decimal s" "$(duration_ns 1.5s)" "1500000000"
expect_eq "duration_ns m" "$(duration_ns 2m)" "120000000000"
expect_eq "ns_to_human" "$(ns_to_human 50000000)" "50ms"
expect_eq "ns_to_human s" "$(ns_to_human 1000000000)" "1s"
expect_eq "seconds_from_pb s" "$(seconds_from_pb 172800s)" "172800"
expect_eq "seconds_from_pb ns" "$(seconds_from_pb 172800000000000)" "172800"
if (duration_ns 50 2>/dev/null); then fail_case "duration_ns accepts a bare number"; else ok; fi
if (halt_height 1000 100 100 0.4 2>/dev/null); then fail_case "halt_height accepts a past target"; else ok; fi
if (seconds_per_block 200 1000 100 1040 2>/dev/null); then fail_case "seconds_per_block accepts a receding height"; else ok; fi

# --- gov-upgrade.sh dry run against the fixtures ---------------------------------------------------
export GOV_FIXTURE_DIR=$FIXTURES
TARGET=2026-09-26T15:00:00Z
NOW=1790179835
SPB=0.3946
RPC_PLACEHOLDER=http://rpc.example:26657
common=(--rpc "$RPC_PLACEHOLDER" --key operator --target-utc "$TARGET" --now "$NOW" --seconds-per-block "$SPB")

set +e
out=$(bash "$UPGRADE" "${common[@]}" 2>&1); rc=$?
set -e
expect_eq "upgrade dry run exit" "$rc" "0"
expect_contains "upgrade halt height (wallclock)" "$out" "halt height        24515998 (wallclock), expected at 2026-09-26T15:00:00Z"
expect_contains "upgrade halt height (timestamp)" "$out" "-> halt height 24584454"
expect_contains "upgrade skew" "$out" "(skew 1s)"
expect_contains "upgrade standard deadline" "$out" "standard  (172800s vote + 1800s margin): 2026-09-24T14:30:00Z (+22.3h)"
expect_contains "upgrade expedited deadline" "$out" "expedited (86400s vote + 1800s margin): 2026-09-25T14:30:00Z (+46.3h)"
expect_contains "upgrade dry-run banner" "$out" "dry run: would submit (add --execute to broadcast)"
expect_contains "upgrade submit command" "$out" "paxd tx gov submit-proposal software-upgrade v6.6 --upgrade-height 24515998 --title Paxeer\\ X\\ v6.6 --description"
expect_contains "upgrade min deposit" "$out" "--deposit 10000000uhpx --from operator --chain-id hyperpax_125-1 --node $RPC_PLACEHOLDER --fees 1000000uhpx --gas 2000000 --keyring-backend os -b sync -y -o json"
expect_missing "upgrade not expedited" "$out" "--is-expedited"
expect_contains "upgrade vote validator 1" "$out" "paxd tx gov vote <proposal_id> yes --from <validator-1-operator-key> --chain-id hyperpax_125-1 --node $RPC_PLACEHOLDER --fees 1000000uhpx -b sync -y"
expect_contains "upgrade vote validator 4" "$out" "--from <validator-4-operator-key>"
expect_missing "upgrade no fifth validator" "$out" "validator-5-operator-key"
expect_contains "upgrade confirm plan" "$out" "paxd q upgrade plan --node $RPC_PLACEHOLDER -o json   # expect name v6.6 height 24515998"
expect_contains "upgrade confirm current_plan" "$out" "/cosmos/upgrade/v1beta1/current_plan"
expect_contains "upgrade tally line" "$out" "quorum 0.334000000000000000 threshold 0.500000000000000000"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --voter val-a --voter val-b --upgrade-info '{"binaries":{"linux/amd64":"https://example/paxd?checksum=sha256:00"}}' 2>&1); rc=$?
set -e
expect_eq "upgrade voters exit" "$rc" "0"
expect_contains "upgrade named voter" "$out" "tx gov vote <proposal_id> yes --from val-a"
expect_contains "upgrade second voter" "$out" "--from val-b"
expect_missing "upgrade placeholder replaced" "$out" "validator-1-operator-key"
expect_contains "upgrade info" "$out" "--upgrade-info"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --anchor timestamp 2>&1); rc=$?
set -e
expect_eq "upgrade timestamp anchor exit" "$rc" "0"
expect_contains "upgrade timestamp anchor height" "$out" "--upgrade-height 24584454"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --halt-height 24500000 2>&1); rc=$?
set -e
expect_eq "upgrade explicit height exit" "$rc" "0"
expect_contains "upgrade explicit height" "$out" "halt height        24500000 (--halt-height)"
expect_contains "upgrade explicit height command" "$out" "--upgrade-height 24500000"

# standard path too late, expedited still fits
set +e
out=$(bash "$UPGRADE" --rpc "$RPC_PLACEHOLDER" --key operator --target-utc "$TARGET" --now 1790300000 --seconds-per-block "$SPB" 2>&1); rc=$?
set -e
expect_eq "upgrade late standard exit" "$rc" "2"
expect_contains "upgrade late standard refusal" "$out" "REFUSED: a standard proposal submitted now would end voting at 2026-09-27T01:33:20Z"
expect_contains "upgrade late standard deadline" "$out" "the standard submission deadline was 2026-09-24T14:30:00Z"
expect_contains "upgrade late standard hint" "$out" "the expedited path still fits until 2026-09-25T14:30:00Z; rerun with --expedited (deposit 20000000uhpx"
expect_missing "upgrade late standard prints no tx" "$out" "dry run: would submit"

set +e
out=$(bash "$UPGRADE" --rpc "$RPC_PLACEHOLDER" --key operator --target-utc "$TARGET" --now 1790300000 --seconds-per-block "$SPB" --expedited 2>&1); rc=$?
set -e
expect_eq "upgrade expedited exit" "$rc" "0"
expect_contains "upgrade expedited flag" "$out" "--deposit 20000000uhpx"
expect_contains "upgrade expedited is-expedited" "$out" "--is-expedited"
expect_contains "upgrade expedited tally" "$out" "expedited quorum 0.667000000000000000 threshold 0.667000000000000000"

# no path fits
set +e
out=$(bash "$UPGRADE" --rpc "$RPC_PLACEHOLDER" --key operator --target-utc "$TARGET" --now 1790400000 --seconds-per-block "$SPB" --expedited 2>&1); rc=$?
set -e
expect_eq "upgrade no path exit" "$rc" "2"
expect_contains "upgrade no path refusal" "$out" "REFUSED: a expedited proposal submitted now"
expect_contains "upgrade no path message" "$out" "no governance path ends before the halt"

# resume on a submitted proposal: voting ends 2026-09-25T16:10:35Z, plan height 24515996
set +e
out=$(bash "$UPGRADE" "${common[@]}" --proposal-id 1 2>&1); rc=$?
set -e
expect_eq "upgrade resume exit" "$rc" "0"
expect_contains "upgrade resume plan" "$out" "proposal 1  plan v6.6 at 24515996"
expect_contains "upgrade resume votes" "$out" "tx gov vote 1 yes --from"
expect_contains "upgrade resume confirm" "$out" "expect name v6.6 height 24515996"
expect_missing "upgrade resume does not resubmit" "$out" "submit-proposal software-upgrade"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --proposal-id 1 --margin-seconds 100000 2>&1); rc=$?
set -e
expect_eq "upgrade resume late exit" "$rc" "2"
expect_contains "upgrade resume late refusal" "$out" "REFUSED: proposal 1 voting ends 2026-09-25T16:10:35Z, after halt height 24515996"
expect_contains "upgrade resume late abort path" "$out" "cancel the plan and re-propose"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --upgrade-name v6.5 2>&1); rc=$?
set -e
expect_eq "upgrade v6.5 refused exit" "$rc" "1"
expect_contains "upgrade v6.5 refused" "$out" "plan name v6.5 is already applied by the running binary"

set +e
out=$(bash "$UPGRADE" "${common[@]}" --execute 2>&1); rc=$?
set -e
expect_eq "upgrade execute on fixtures exit" "$rc" "1"
expect_contains "upgrade execute on fixtures refused" "$out" "--execute is refused with GOV_FIXTURE_DIR set"

set +e
out=$(bash "$UPGRADE" --rpc "$RPC_PLACEHOLDER" --key operator --target-utc 2026-09-26 --now "$NOW" --seconds-per-block "$SPB" 2>&1); rc=$?
set -e
expect_eq "upgrade bad target exit" "$rc" "1"
expect_contains "upgrade bad target" "$out" "is not YYYY-MM-DDTHH:MM:SSZ"

# --- gov-consensus-params.sh dry run against the fixtures -----------------------------------------
PROPOSAL=$WORK/consensus.proposal.json
set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now "$NOW" --timeout-commit 20ms --bypass-commit-timeout true \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" 2>&1); rc=$?
set -e
expect_eq "consensus dry run exit" "$rc" "0"
expect_contains "consensus current" "$out" "current timeouts   propose 1s propose_delta 500ms vote 50ms vote_delta 500ms commit 50ms bypass false"
expect_contains "consensus proposed" "$out" "proposed timeouts  vote 30ms commit 20ms bypass true"
expect_contains "consensus voting end" "$out" "voting ends       2026-09-25T16:10:35Z if submitted now (standard path"
expect_contains "consensus submit command" "$out" "paxd tx gov submit-proposal param-change $PROPOSAL --from operator --chain-id hyperpax_125-1 --node $RPC_PLACEHOLDER --fees 1000000uhpx --gas 2000000 --keyring-backend os -b sync -y -o json"
expect_contains "consensus vote command" "$out" "tx gov vote <proposal_id> yes --from <validator-1-operator-key>"
expect_contains "consensus confirm" "$out" "consensus_params | jq '(.result // .).consensus_params.timeout'   # expect commit 20000000 vote 30000000 bypass true"
[ -s "$PROPOSAL" ] && ok || fail_case "consensus proposal file missing"
expect_eq "consensus subspace" "$(jq -r '.changes[0].subspace' "$PROPOSAL")" "baseapp"
expect_eq "consensus key" "$(jq -r '.changes[0].key' "$PROPOSAL")" "TimeoutParams"
expect_eq "consensus value" "$(jq -c '.changes[0].value' "$PROPOSAL")" \
    '{"propose":"1000000000","propose_delta":"500000000","vote":"30000000","vote_delta":"500000000","commit":"20000000","bypass_commit_timeout":true}'
expect_eq "consensus deposit" "$(jq -r '.deposit' "$PROPOSAL")" "10000000uhpx"
expect_eq "consensus one change" "$(jq -r '.changes | length' "$PROPOSAL")" "1"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now "$NOW" --timeout-commit 20ms --bypass-commit-timeout true \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" --target-utc "$TARGET" --seconds-per-block "$SPB" 2>&1); rc=$?
set -e
expect_eq "consensus with target exit" "$rc" "0"
expect_contains "consensus halt height" "$out" "halt target        $TARGET, about height 24515998"
expect_contains "consensus standard deadline" "$out" "standard  (172800s vote + 1800s margin): 2026-09-24T14:30:00Z"
expect_contains "consensus expedited deadline" "$out" "expedited (86400s vote + 1800s margin): 2026-09-25T14:30:00Z"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now 1790300000 --timeout-commit 20ms --bypass-commit-timeout true \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" --target-utc "$TARGET" --seconds-per-block "$SPB" 2>&1); rc=$?
set -e
expect_eq "consensus late exit" "$rc" "2"
expect_contains "consensus late refusal" "$out" "REFUSED: a standard proposal submitted now would end voting at 2026-09-27T01:33:20Z"
expect_contains "consensus late hint" "$out" "the expedited path still fits until 2026-09-25T14:30:00Z"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now 1790300000 --timeout-commit 20ms --bypass-commit-timeout true \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" --target-utc "$TARGET" --seconds-per-block "$SPB" --expedited 2>&1); rc=$?
set -e
expect_eq "consensus expedited exit" "$rc" "0"
expect_contains "consensus expedited flag" "$out" "param-change $PROPOSAL --is-expedited"
expect_eq "consensus expedited deposit" "$(jq -r '.deposit' "$PROPOSAL")" "20000000uhpx"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now "$NOW" --timeout-commit 50ms --bypass-commit-timeout false \
    --timeout-vote 50ms --proposal-file "$PROPOSAL" 2>&1); rc=$?
set -e
expect_eq "consensus unchanged exit" "$rc" "1"
expect_contains "consensus unchanged" "$out" "the chain already has these timeout params"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now "$NOW" --timeout-commit 20ms --bypass-commit-timeout yes \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" 2>&1); rc=$?
set -e
expect_eq "consensus bad bypass exit" "$rc" "1"
expect_contains "consensus bad bypass" "$out" "--bypass-commit-timeout must be true or false"

set +e
out=$(bash "$CONSENSUS" --rpc "$RPC_PLACEHOLDER" --key operator --now "$NOW" --timeout-commit 20ms --bypass-commit-timeout true \
    --timeout-vote 30ms --proposal-file "$PROPOSAL" --execute 2>&1); rc=$?
set -e
expect_eq "consensus execute on fixtures exit" "$rc" "1"
expect_contains "consensus execute on fixtures refused" "$out" "--execute is refused with GOV_FIXTURE_DIR set"

echo "gov-scripts-test: $PASS checks passed, $FAILED failed (fixture replay; rehearsal chain not started)"
[ "$FAILED" -eq 0 ]
