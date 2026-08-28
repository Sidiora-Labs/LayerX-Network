#!/usr/bin/env bash
set -euo pipefail

[[ $# -eq 0 ]] || { echo "usage: walkthrough.sh" >&2; exit 64; }
registry=$(layerx --output json program registry list)
mapfile -t candidates < <(jq -er '.data.program_ids[]' <<<"$registry")
program_id=
discovery=
for candidate in "${candidates[@]}"; do
  observed=$(layerx --output json program discover "$candidate")
  if [[ $(jq -r '.data.lifecycle' <<<"$observed") == active ]]; then
    program_id=$candidate; discovery=$observed; break
  fi
done
[[ -n $program_id ]]

interface_digest=absent
if [[ $(jq -r '.data.interface_status // "published"' <<<"$discovery") == published ]]; then
  interface=$(layerx --output json program interface get "$program_id")
  interface_digest=$(jq -er '.data.interface_digest' <<<"$interface")
fi
account=$(layerx --output json account get)
account_sequence=$(jq -er '.data.next_sequence // .data.account_sequence' <<<"$account")
not_before_ms=$(jq -er '.data.observed_at' <<<"$discovery")
expires_at_ms=$((not_before_ms + 300000))
nonce=$(printf '%s:%s:%s' "$program_id" "$account_sequence" "$not_before_ms" | sha256sum | cut -d' ' -f1)
call_nonce=$(printf 'call:%s' "$nonce" | sha256sum | cut -d' ' -f1)

simulation=$(layerx --output json program simulate "$program_id" --fuel 1000000 --fee-limit 0 \
  --idempotency-key "$nonce" --account-sequence "$account_sequence" \
  --not-before-ms "$not_before_ms" --expires-at-ms "$expires_at_ms")
[[ $(jq -r '.data.committed' <<<"$simulation") == false ]]
if [[ $(jq -er '.data.outcome.status' <<<"$simulation") == refused ]]; then
  jq -n --arg program_id "$program_id" --arg interface_digest "$interface_digest" \
    --argjson refusal "$(jq '.data.outcome.failure' <<<"$simulation")" \
    '{program_id:$program_id,interface_digest:$interface_digest,status:"typed-refusal",refusal:$refusal}'
  exit 0
fi

call_output=$(mktemp)
call_error=$(mktemp)
trap 'rm -f "$call_output" "$call_error"' EXIT
if ! layerx --output json program call "$program_id" --fuel 1000000 --fee-limit 0 \
  --idempotency-key "$call_nonce" --account-sequence "$account_sequence" \
  --not-before-ms "$not_before_ms" --expires-at-ms "$expires_at_ms" >"$call_output" 2>"$call_error"; then
  failure=$(<"$call_error")
  jq -n --arg program_id "$program_id" --arg refusal "$failure" \
    '{program_id:$program_id,status:"local-or-deterministic-refusal",refusal:$refusal,submitted:false}'
  exit 0
fi
result=$(<"$call_output")
if [[ $(jq -r '.data.outcome.status' <<<"$result") == unknown ]]; then
  retained_activity=$(jq -er '.data.activity_id' <<<"$result")
  if resolved=$(layerx --output json receipt get "$retained_activity" 2>/dev/null) \
    && [[ $(jq -r '.data.receipt // empty' <<<"$resolved") != "" ]]; then
    jq -n --arg program_id "$program_id" --arg idempotency_key "$call_nonce" --arg activity_id "$retained_activity" \
      --argjson receipt "$(jq '.data' <<<"$resolved")" \
      '{program_id:$program_id,idempotency_key:$idempotency_key,activity_id:$activity_id,status:"resolved-after-unknown",receipt:$receipt}'
  else
    jq -n --arg program_id "$program_id" --arg idempotency_key "$call_nonce" --arg activity_id "$retained_activity" \
      '{program_id:$program_id,idempotency_key:$idempotency_key,activity_id:$activity_id,status:"unknown-fate",retained_bytes:true,action:"activity-bound receipt lookup returned no verified receipt; do not retry with a different idempotency key"}'
  fi
  exit 0
fi
jq -e '.data.outcome.status == "completed" or .data.outcome.status == "refused"' <<<"$result" >/dev/null
jq -n --arg program_id "$program_id" --arg interface_digest "$interface_digest" \
  --arg receipt_digest "$(jq -er '.data.receipt_digest' <<<"$result")" \
  --argjson outcome "$(jq '.data.outcome' <<<"$result")" \
  '{program_id:$program_id,interface_digest:$interface_digest,receipt_digest:$receipt_digest,status:"called",outcome:$outcome}'
