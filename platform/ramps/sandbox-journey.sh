#!/bin/sh
set -eu

: "${LAYERX_RAMP_URL:?}"
: "${LAYERX_RAMP_CA_PEM:?}"
: "${LAYERX_RAMP_CUSTOMER_TOKEN:?}"
: "${LAYERX_RAMP_OPERATOR_URL:?}"
: "${LAYERX_RAMP_OPERATOR_TOKEN:?}"
: "${LAYERX_RAMP_ON_QUOTE_ID:?}"
: "${LAYERX_RAMP_OFF_QUOTE_ID:?}"
: "${LAYERX_RAMP_OFF_GRANT_JSON:?}"
: "${LAYERX_RAMP_ON_ACCOUNT_SEQUENCE:?}"
: "${LAYERX_RAMP_OFF_RECEIVER_SEQUENCE:?}"

customer() {
  curl --fail-with-body --silent --show-error --cacert "${LAYERX_RAMP_CA_PEM}" \
    -H "Authorization: Bearer ${LAYERX_RAMP_CUSTOMER_TOKEN}" "$@"
}

operator() {
  curl --fail-with-body --silent --show-error --cacert "${LAYERX_RAMP_CA_PEM}" \
    -H "Authorization: Bearer ${LAYERX_RAMP_OPERATOR_TOKEN}" "$@"
}

create_order() {
  direction="$1"
  quote="$2"
  grant="$3"
  order_id="sandbox-${direction}-${GITHUB_RUN_ID:-manual}-${GITHUB_RUN_ATTEMPT:-1}"
  customer -H 'Content-Type: application/json' -X POST "${LAYERX_RAMP_URL}/v1/orders" \
    --data "{\"order_id\":\"${order_id}\",\"quote_id\":\"${quote}\",\"payer_grant\":${grant}}"
}

work() {
  digest="$1"
  action="$2"
  sequence="$3"
  operator -H 'Content-Type: application/json' -X POST "${LAYERX_RAMP_OPERATOR_URL}/internal/v1/work" \
    --data "{\"order_digest\":${digest},\"action\":\"${action}\",\"account_sequence\":${sequence}}"
}

wait_stage() {
  digest_hex="$1"
  expected="$2"
  attempt=0
  while [ "${attempt}" -lt 60 ]; do
    response="$(customer "${LAYERX_RAMP_URL}/v1/orders/${digest_hex}")"
    stage="$(printf '%s' "${response}" | jq -r '.stage')"
    case "${stage}" in
      "${expected}") printf '%s' "${response}"; return 0 ;;
      compliance_refused|provider_refused|layerx_refused|manual_review|provider_reversed|reversed)
        printf '%s\n' "${response}" >&2
        return 1
        ;;
    esac
    attempt=$((attempt + 1))
    sleep 5
  done
  return 1
}

on_created="$(create_order on-ramp "${LAYERX_RAMP_ON_QUOTE_ID}" null)"
on_digest="$(printf '%s' "${on_created}" | jq -c '.order_digest')"
on_hex="$(printf '%s' "${on_digest}" | jq -r '.[]' | awk '{printf "%02x", $1}')"
work "${on_digest}" compliance null >/dev/null
work "${on_digest}" submit_provider null >/dev/null
wait_stage "${on_hex}" provider_settled >/dev/null
work "${on_digest}" submit_layerx "${LAYERX_RAMP_ON_ACCOUNT_SEQUENCE}" >/dev/null
on_done="$(wait_stage "${on_hex}" done)"

off_created="$(create_order off-ramp "${LAYERX_RAMP_OFF_QUOTE_ID}" "${LAYERX_RAMP_OFF_GRANT_JSON}")"
off_digest="$(printf '%s' "${off_created}" | jq -c '.order_digest')"
off_hex="$(printf '%s' "${off_digest}" | jq -r '.[]' | awk '{printf "%02x", $1}')"
work "${off_digest}" compliance null >/dev/null
work "${off_digest}" submit_layerx "${LAYERX_RAMP_OFF_RECEIVER_SEQUENCE}" >/dev/null
wait_stage "${off_hex}" layerx_verified >/dev/null
work "${off_digest}" submit_provider null >/dev/null
off_done="$(wait_stage "${off_hex}" done)"

printf '%s' "${on_done}" | jq -e '.presentation.status == "done" and .presentation.receipt_digest != null and .presentation.external_custody_label != ""' >/dev/null
printf '%s' "${off_done}" | jq -e '.presentation.status == "done" and .presentation.receipt_digest != null and .presentation.provider_evidence_digest != null and .presentation.external_custody_label != ""' >/dev/null
