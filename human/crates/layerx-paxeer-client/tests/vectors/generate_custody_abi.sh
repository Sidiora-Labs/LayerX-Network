#!/usr/bin/env bash
#
# Regenerates tests/vectors/custody_abi.json.
#
# Every byte string in the generated file is produced by an encoder that is
# independent of src/custody.rs: Foundry `cast` for ABI selectors, event
# topics, calldata and return tuples, `sha256sum` for the domain-separated
# identifier digests, and the real `bound-native-withdrawal` fixture and the
# real native state-proof vectors for the evidence arguments.
#
# The identifier layouts mirror the Go definitions that own them:
#   modules/layerxcustody/types/ids.go   - claim ids, nullifier, exit id
#   layerxproof/verify/withdrawal.go     - exit recipient message
#
# Usage: tests/vectors/generate_custody_abi.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/../../../../.." && pwd)"
FIXTURE="${ROOT}/tests/fixtures/asset/bound-native-withdrawal"
OUT="${SCRIPT_DIR}/custody_abi.json"

if [ -d /root/.foundry/bin ]; then
	PATH="${PATH}:/root/.foundry/bin"
fi
for tool in cast jq xxd sha256sum; do
	command -v "${tool}" >/dev/null 2>&1 || {
		echo "generate_custody_abi.sh: ${tool} is required" >&2
		exit 1
	}
done
for file in receipt receipt.proof header header.signature; do
	[ -r "${FIXTURE}/${file}" ] || {
		echo "generate_custody_abi.sh: missing fixture ${FIXTURE}/${file}" >&2
		exit 1
	}
done

# --- encoding helpers ---------------------------------------------------

raw() { printf '%s' "${1#0x}"; }

file_hex() { xxd -p "$1" | tr -d '\n'; }

text_hex() { printf '%s' "$1" | xxd -p | tr -d '\n'; }

# One abi word for a decimal number, as 64 lowercase hex digits.
number_word() { raw "$(cast abi-encode 'f(uint256)' "$1")"; }

be32() {
	local word
	word="$(number_word "$1")"
	printf '%s' "${word: -8}"
}

be64() {
	local word
	word="$(number_word "$1")"
	printf '%s' "${word: -16}"
}

be128() {
	local word
	word="$(number_word "$1")"
	printf '%s' "${word: -32}"
}

# sha256 of a hex string.
digest() { printf '%s' "$1" | xxd -r -p | sha256sum | cut -d' ' -f1; }

# --- declared inputs ----------------------------------------------------

CUSTODY="0x0000000000000000000000000000000000001013"
CHAIN_ID=31337
NETWORK_ID=7332

BENEFICIARY="0x1111111111111111111111111111111111111111111111111111111111111111"
POINTER="0x2222222222222222222222222222222222222222"
TOKEN_AMOUNT="4200000000000000000"
NATIVE_AMOUNT="1234567"
WEI_PER_BASE_UNIT="1000000000000"
NATIVE_WEI="$(cast to-dec "$(cast to-hex $((1234567 * 1000000000000)))")"

ACCOUNT="0x3333333333333333333333333333333333333333333333333333333333333333"
ASSET_ID="0x4444444444444444444444444444444444444444444444444444444444444444"
RECIPIENT="0x5555555555555555555555555555555555555555"
RECIPIENT_SIGNATURE="0x$(printf '66%.0s' $(seq 64))"
BATCH_NUMBER=5
CLAIM_ID="0x7777777777777777777777777777777777777777777777777777777777777777"
NULLIFIER="0x8888888888888888888888888888888888888888888888888888888888888888"
ANCHOR="0x9999999999999999999999999999999999999999999999999999999999999999"
WITHDRAWAL_ID="0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
AMOUNT="123456789012345678"
AVAILABLE_AT=1893456000
DENOM="ulxp"

RECEIPT="0x$(file_hex "${FIXTURE}/receipt")"
PROOF="0x$(file_hex "${FIXTURE}/receipt.proof")"
HEADER="0x$(file_hex "${FIXTURE}/header")"
HEADER_SIGNATURE="0x$(file_hex "${FIXTURE}/header.signature")"
SEQUENCER_PUBLIC="0x$(file_hex "${FIXTURE}/sequencer.public")"
WITNESS="$(jq -r '.vectors[7].proof' "${SCRIPT_DIR}/native-state-proofs.json")"
WITNESS_ROOT="$(jq -r '.vectors[7].root' "${SCRIPT_DIR}/native-state-proofs.json")"

WITHDRAWAL_CLAIM_DOMAIN="LXP/Paxeer/withdrawal-claim/v1"
EXIT_CLAIM_DOMAIN="LXP/Paxeer/emergency-exit/v1"
NULLIFIER_DOMAIN="LX:WITHDRAWAL:v1"
EXIT_WITHDRAWAL_DOMAIN_HEX="$(text_hex 'LXP/v1/emergency-withdrawal-id')00"
EXIT_RECIPIENT_DOMAIN_HEX="$(text_hex 'LX:SETTLE:RECIPIENT:v1')00"

# --- selectors and topics ----------------------------------------------

SIG_DEPOSIT="$(cast sig 'deposit(bytes32)')"
SIG_DEPOSIT_TOKEN="$(cast sig 'depositToken(address,uint256,bytes32)')"
SIG_REQUEST_WITHDRAWAL="$(cast sig 'requestWithdrawal(bytes,bytes,bytes,bytes)')"
SIG_FINALISE_WITHDRAWAL="$(cast sig 'finaliseWithdrawal(bytes,bytes,bytes,bytes)')"
SIG_REQUEST_FORCED_EXIT="$(cast sig 'requestForcedExit(bytes,uint64,bytes32,bytes32,address,bytes)')"
SIG_EXECUTE_FORCED_EXIT="$(cast sig 'executeForcedExit(bytes,uint64,bytes32,bytes32,address,bytes)')"
SIG_GET_CLAIM="$(cast sig 'getClaim(bytes32)')"
SIG_NULLIFIER_STATUS="$(cast sig 'nullifierStatus(bytes32)')"
SIG_GET_ASSET="$(cast sig 'getAsset(bytes32)')"
SIG_EXIT_ELIGIBLE="$(cast sig 'exitEligible()')"
SIG_NATIVE_ASSET_ID="$(cast sig 'nativeAssetId()')"

TOPIC_CUSTODY_DEPOSIT="$(cast sig-event 'CustodyDeposit(bytes32,bytes32,address,bytes32,uint256,uint64)')"
TOPIC_CLAIM_QUEUED="$(cast sig-event 'ClaimQueued(bytes32,bytes32,bytes32,bytes32,address,uint256,uint64)')"
TOPIC_CLAIM_FINALISED="$(cast sig-event 'ClaimFinalised(bytes32,bytes32)')"
TOPIC_CUSTODY_RELEASE="$(cast sig-event 'CustodyRelease(bytes32,bytes32,address,uint256,address)')"
TOPIC_EMERGENCY_EXIT="$(cast sig-event 'EmergencyExitExecuted(bytes32,bytes32,bytes32,bytes32,bytes32,address,uint256)')"

# --- calldata -----------------------------------------------------------

CALL_DEPOSIT="$(cast calldata 'deposit(bytes32)' "${BENEFICIARY}")"
CALL_DEPOSIT_TOKEN="$(cast calldata 'depositToken(address,uint256,bytes32)' \
	"${POINTER}" "${TOKEN_AMOUNT}" "${BENEFICIARY}")"
CALL_REQUEST_WITHDRAWAL="$(cast calldata 'requestWithdrawal(bytes,bytes,bytes,bytes)' \
	"${RECEIPT}" "${PROOF}" "${HEADER}" "${HEADER_SIGNATURE}")"
CALL_FINALISE_WITHDRAWAL="$(cast calldata 'finaliseWithdrawal(bytes,bytes,bytes,bytes)' \
	"${RECEIPT}" "${PROOF}" "${HEADER}" "${HEADER_SIGNATURE}")"
CALL_REQUEST_FORCED_EXIT="$(cast calldata 'requestForcedExit(bytes,uint64,bytes32,bytes32,address,bytes)' \
	"${WITNESS}" "${BATCH_NUMBER}" "${ACCOUNT}" "${ASSET_ID}" "${RECIPIENT}" "${RECIPIENT_SIGNATURE}")"
CALL_EXECUTE_FORCED_EXIT="$(cast calldata 'executeForcedExit(bytes,uint64,bytes32,bytes32,address,bytes)' \
	"${WITNESS}" "${BATCH_NUMBER}" "${ACCOUNT}" "${ASSET_ID}" "${RECIPIENT}" "${RECIPIENT_SIGNATURE}")"
CALL_GET_CLAIM="$(cast calldata 'getClaim(bytes32)' "${CLAIM_ID}")"
CALL_NULLIFIER_STATUS="$(cast calldata 'nullifierStatus(bytes32)' "${NULLIFIER}")"
CALL_GET_ASSET="$(cast calldata 'getAsset(bytes32)' "${ASSET_ID}")"
CALL_EXIT_ELIGIBLE="$(cast calldata 'exitEligible()')"
CALL_NATIVE_ASSET_ID="$(cast calldata 'nativeAssetId()')"

# --- return values ------------------------------------------------------

CLAIM_KIND=1
CLAIM_STATUS=1
RETURN_GET_CLAIM="$(cast abi-encode \
	'f((bytes32,uint8,uint8,bytes32,bytes32,bytes32,bytes32,string,address,uint256,uint64,bytes32,uint64))' \
	"(${CLAIM_ID},${CLAIM_KIND},${CLAIM_STATUS},${NULLIFIER},${WITHDRAWAL_ID},${ACCOUNT},${ASSET_ID},${DENOM},${RECIPIENT},${AMOUNT},${BATCH_NUMBER},${ANCHOR},${AVAILABLE_AT})")"

ASSET_ENABLED=true
ASSET_PAUSED=false
ASSET_MINIMUM_DEPOSIT=1000
ASSET_CUSTODY_CAP=900000000000000000
ASSET_CUSTODIED=450000000000000000
ASSET_RELEASED=125000000000000000
ASSET_PENDING=75000000000000000
RETURN_GET_ASSET="$(cast abi-encode \
	'f((bytes32,string,address,bool,bool,uint256,uint256,uint256,uint256,uint256))' \
	"(${ASSET_ID},${DENOM},${POINTER},${ASSET_ENABLED},${ASSET_PAUSED},${ASSET_MINIMUM_DEPOSIT},${ASSET_CUSTODY_CAP},${ASSET_CUSTODIED},${ASSET_RELEASED},${ASSET_PENDING})")"

NULLIFIER_STATUS_VALUE=2
RETURN_NULLIFIER_STATUS="$(cast abi-encode 'f(uint8)' "${NULLIFIER_STATUS_VALUE}")"
RETURN_EXIT_ELIGIBLE_TRUE="$(cast abi-encode 'f(bool)' true)"
RETURN_EXIT_ELIGIBLE_FALSE="$(cast abi-encode 'f(bool)' false)"
RETURN_NATIVE_ASSET_ID="$(cast abi-encode 'f(bytes32)' "${ASSET_ID}")"

# --- event data ---------------------------------------------------------

RECIPIENT_TOPIC="$(cast abi-encode 'f(address)' "${RECIPIENT}")"
DATA_CLAIM_QUEUED="$(cast abi-encode 'f(bytes32,address,uint256,uint64)' \
	"${ASSET_ID}" "${RECIPIENT}" "${AMOUNT}" "${AVAILABLE_AT}")"
DATA_CUSTODY_RELEASE="$(cast abi-encode 'f(uint256,address)' "${AMOUNT}" "${CUSTODY}")"
DATA_EMERGENCY_EXIT="$(cast abi-encode 'f(bytes32,bytes32,address,uint256)' \
	"${ACCOUNT}" "${ASSET_ID}" "${RECIPIENT}" "${AMOUNT}")"

# --- identifiers --------------------------------------------------------

CLAIM_ID_PREIMAGE="$(cast abi-encode 'f(string,uint256,address,bytes32,address)' \
	"${WITHDRAWAL_CLAIM_DOMAIN}" "${CHAIN_ID}" "${CUSTODY}" "${NULLIFIER}" "${RECIPIENT}")"
WITHDRAWAL_CLAIM_ID="0x$(digest "$(raw "${CLAIM_ID_PREIMAGE}")")"

EXIT_ID_PREIMAGE="$(cast abi-encode 'f(string,uint256,address,bytes32)' \
	"${EXIT_CLAIM_DOMAIN}" "${CHAIN_ID}" "${CUSTODY}" "${NULLIFIER}")"
EXIT_CLAIM_ID="0x$(digest "$(raw "${EXIT_ID_PREIMAGE}")")"

NULLIFIER_PREIMAGE="$(text_hex "${NULLIFIER_DOMAIN}")$(be32 "${NETWORK_ID}")$(raw "${WITHDRAWAL_ID}")$(raw "${ACCOUNT}")$(raw "${ASSET_ID}")$(be128 "${AMOUNT}")$(raw "${ANCHOR}")"
WITHDRAWAL_NULLIFIER="0x$(digest "${NULLIFIER_PREIMAGE}")"

EXIT_WITHDRAWAL_PREIMAGE="${EXIT_WITHDRAWAL_DOMAIN_HEX}$(be32 "${NETWORK_ID}")$(raw "${ACCOUNT}")$(raw "${ASSET_ID}")$(raw "${ANCHOR}")"
EXIT_WITHDRAWAL_ID="0x$(digest "${EXIT_WITHDRAWAL_PREIMAGE}")"

EXIT_RECIPIENT_MESSAGE="0x${EXIT_RECIPIENT_DOMAIN_HEX}$(be32 "${NETWORK_ID}")$(raw "${ACCOUNT}")$(raw "${ASSET_ID}")$(raw "${RECIPIENT}")$(raw "${ANCHOR}")"

# --- document -----------------------------------------------------------

cat >"${OUT}.tmp" <<JSON
{
  "generator": "tests/vectors/generate_custody_abi.sh",
  "encoder": "$(cast --version | head -n 1 | tr -d '\n')",
  "abi": "precompiles/layerxcustody/abi.json",
  "custody_precompile": "${CUSTODY}",
  "chain_id": ${CHAIN_ID},
  "network_id": ${NETWORK_ID},
  "wei_per_base_unit": "${WEI_PER_BASE_UNIT}",
  "inputs": {
    "beneficiary": "${BENEFICIARY}",
    "pointer": "${POINTER}",
    "token_amount": "${TOKEN_AMOUNT}",
    "native_amount": "${NATIVE_AMOUNT}",
    "native_wei": "${NATIVE_WEI}",
    "receipt": "${RECEIPT}",
    "proof": "${PROOF}",
    "header": "${HEADER}",
    "header_signature": "${HEADER_SIGNATURE}",
    "sequencer_public": "${SEQUENCER_PUBLIC}",
    "witness": "${WITNESS}",
    "witness_root": "${WITNESS_ROOT}",
    "batch_number": ${BATCH_NUMBER},
    "account": "${ACCOUNT}",
    "asset_id": "${ASSET_ID}",
    "recipient": "${RECIPIENT}",
    "recipient_topic": "${RECIPIENT_TOPIC}",
    "recipient_signature": "${RECIPIENT_SIGNATURE}",
    "claim_id": "${CLAIM_ID}",
    "nullifier": "${NULLIFIER}",
    "anchor": "${ANCHOR}",
    "withdrawal_id": "${WITHDRAWAL_ID}",
    "amount": "${AMOUNT}",
    "available_at": ${AVAILABLE_AT},
    "denom": "${DENOM}"
  },
  "selectors": {
    "deposit": "${SIG_DEPOSIT}",
    "depositToken": "${SIG_DEPOSIT_TOKEN}",
    "requestWithdrawal": "${SIG_REQUEST_WITHDRAWAL}",
    "finaliseWithdrawal": "${SIG_FINALISE_WITHDRAWAL}",
    "requestForcedExit": "${SIG_REQUEST_FORCED_EXIT}",
    "executeForcedExit": "${SIG_EXECUTE_FORCED_EXIT}",
    "getClaim": "${SIG_GET_CLAIM}",
    "nullifierStatus": "${SIG_NULLIFIER_STATUS}",
    "getAsset": "${SIG_GET_ASSET}",
    "exitEligible": "${SIG_EXIT_ELIGIBLE}",
    "nativeAssetId": "${SIG_NATIVE_ASSET_ID}"
  },
  "topics": {
    "CustodyDeposit": "${TOPIC_CUSTODY_DEPOSIT}",
    "ClaimQueued": "${TOPIC_CLAIM_QUEUED}",
    "ClaimFinalised": "${TOPIC_CLAIM_FINALISED}",
    "CustodyRelease": "${TOPIC_CUSTODY_RELEASE}",
    "EmergencyExitExecuted": "${TOPIC_EMERGENCY_EXIT}"
  },
  "calldata": {
    "deposit": "${CALL_DEPOSIT}",
    "depositToken": "${CALL_DEPOSIT_TOKEN}",
    "requestWithdrawal": "${CALL_REQUEST_WITHDRAWAL}",
    "finaliseWithdrawal": "${CALL_FINALISE_WITHDRAWAL}",
    "requestForcedExit": "${CALL_REQUEST_FORCED_EXIT}",
    "executeForcedExit": "${CALL_EXECUTE_FORCED_EXIT}",
    "getClaim": "${CALL_GET_CLAIM}",
    "nullifierStatus": "${CALL_NULLIFIER_STATUS}",
    "getAsset": "${CALL_GET_ASSET}",
    "exitEligible": "${CALL_EXIT_ELIGIBLE}",
    "nativeAssetId": "${CALL_NATIVE_ASSET_ID}"
  },
  "returns": {
    "getClaim": {
      "encoded": "${RETURN_GET_CLAIM}",
      "kind": ${CLAIM_KIND},
      "status": ${CLAIM_STATUS}
    },
    "getAsset": {
      "encoded": "${RETURN_GET_ASSET}",
      "enabled": ${ASSET_ENABLED},
      "paused": ${ASSET_PAUSED},
      "minimum_deposit": "${ASSET_MINIMUM_DEPOSIT}",
      "custody_cap": "${ASSET_CUSTODY_CAP}",
      "custodied": "${ASSET_CUSTODIED}",
      "released": "${ASSET_RELEASED}",
      "pending": "${ASSET_PENDING}"
    },
    "nullifierStatus": {
      "encoded": "${RETURN_NULLIFIER_STATUS}",
      "status": ${NULLIFIER_STATUS_VALUE}
    },
    "exitEligible": {
      "true": "${RETURN_EXIT_ELIGIBLE_TRUE}",
      "false": "${RETURN_EXIT_ELIGIBLE_FALSE}"
    },
    "nativeAssetId": "${RETURN_NATIVE_ASSET_ID}"
  },
  "events": {
    "ClaimQueued": { "data": "${DATA_CLAIM_QUEUED}" },
    "ClaimFinalised": { "data": "0x" },
    "CustodyRelease": { "data": "${DATA_CUSTODY_RELEASE}" },
    "EmergencyExitExecuted": { "data": "${DATA_EMERGENCY_EXIT}" }
  },
  "ids": {
    "withdrawal_claim_id": {
      "domain": "${WITHDRAWAL_CLAIM_DOMAIN}",
      "preimage": "${CLAIM_ID_PREIMAGE}",
      "digest": "${WITHDRAWAL_CLAIM_ID}"
    },
    "exit_claim_id": {
      "domain": "${EXIT_CLAIM_DOMAIN}",
      "preimage": "${EXIT_ID_PREIMAGE}",
      "digest": "${EXIT_CLAIM_ID}"
    },
    "withdrawal_nullifier": {
      "domain": "${NULLIFIER_DOMAIN}",
      "preimage": "0x${NULLIFIER_PREIMAGE}",
      "digest": "${WITHDRAWAL_NULLIFIER}"
    },
    "exit_withdrawal_id": {
      "domain": "0x${EXIT_WITHDRAWAL_DOMAIN_HEX}",
      "preimage": "0x${EXIT_WITHDRAWAL_PREIMAGE}",
      "digest": "${EXIT_WITHDRAWAL_ID}"
    },
    "exit_recipient_message": {
      "domain": "0x${EXIT_RECIPIENT_DOMAIN_HEX}",
      "message": "${EXIT_RECIPIENT_MESSAGE}"
    }
  }
}
JSON

jq --indent 2 . "${OUT}.tmp" >"${OUT}"
rm -f "${OUT}.tmp"
echo "wrote ${OUT}"
