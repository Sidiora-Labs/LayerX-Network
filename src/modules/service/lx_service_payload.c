#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_result version_check(const uint8_t *payload, size_t length,
                                size_t minimum)
{
    if (payload == NULL || length < minimum) return LXP_ERR_TRUNCATED;
    if (payload[0] != 0U || payload[1] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    return LXP_OK;
}

static lxp_result offer_publish_decode(const uint8_t *payload, size_t length,
                                       lx_service_offer_publish_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(payload, length,
                                      LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->offer_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->asset_id, payload + offset, 32U); offset += 32U;
    status = lxp_u128_from_be(payload + offset, &out->price);
    if (status != LXP_OK) return status;
    offset += 16U;
    (void)memcpy(out->terms_hash, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->deliverable_specification_hash, payload + offset, 32U);
    offset += 32U;
    out->delivery_deadline = lx_service_get_u64(payload + offset); offset += 8U;
    out->acceptance_window = lx_service_get_u64(payload + offset); offset += 8U;
    out->dispute_window = lx_service_get_u64(payload + offset); offset += 8U;
    if (payload[offset] < LX_SERVICE_DEFAULT_ACCEPT ||
        payload[offset] > LX_SERVICE_DEFAULT_REJECT)
        return LXP_ERR_NON_CANONICAL;
    out->default_outcome = (lx_service_default_outcome)payload[offset];
    offset += 1U;
    out->offer_expiry = lx_service_get_u64(payload + offset); offset += 8U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->offer_id, 32U) ||
        lxp_ct_is_zero(out->asset_id, 32U) || lxp_u128_is_zero(out->price) ||
        lxp_ct_is_zero(out->terms_hash, 32U) ||
        lxp_ct_is_zero(out->deliverable_specification_hash, 32U) ||
        out->delivery_deadline == 0U || out->acceptance_window == 0U ||
        out->dispute_window == 0U || out->offer_expiry == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result identifier_decode(const uint8_t *payload, size_t length,
                                    lx_service_identifier_payload *out)
{
    lxp_result status = version_check(payload, length,
                                      LX_SERVICE_IDENTIFIER_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_IDENTIFIER_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memcpy(out->identifier, payload + LX_SERVICE_PAYLOAD_VERSION_BYTES,
                 32U);
    if (lxp_ct_is_zero(out->identifier, 32U)) return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result agreement_propose_decode(
    const uint8_t *payload, size_t length,
    lx_service_agreement_propose_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(
        payload, length, LX_SERVICE_AGREEMENT_PROPOSE_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_AGREEMENT_PROPOSE_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->agreement_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->offer_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->terms_hash, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->escrow_id, payload + offset, 32U); offset += 32U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->agreement_id, 32U) ||
        lxp_ct_is_zero(out->offer_id, 32U) ||
        lxp_ct_is_zero(out->terms_hash, 32U) ||
        lxp_ct_is_zero(out->escrow_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result commit_task_decode(const uint8_t *payload, size_t length,
                                     lx_service_commit_task_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(payload, length,
                                      LX_SERVICE_COMMIT_TASK_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_COMMIT_TASK_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->commitment_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->agreement_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->task_hash, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->escrow_id, payload + offset, 32U); offset += 32U;
    out->deadline = lx_service_get_u64(payload + offset); offset += 8U;
    out->resource_bound = lx_service_get_u64(payload + offset); offset += 8U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->commitment_id, 32U) ||
        lxp_ct_is_zero(out->agreement_id, 32U) ||
        lxp_ct_is_zero(out->task_hash, 32U) || out->deadline == 0U ||
        out->resource_bound == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result commit_abandon_decode(
    const uint8_t *payload, size_t length,
    lx_service_commit_abandon_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(payload, length,
                                      LX_SERVICE_COMMIT_ABANDON_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_COMMIT_ABANDON_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->commitment_id, payload + offset, 32U); offset += 32U;
    out->abandon_reason = lx_service_get_u16(payload + offset); offset += 2U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->commitment_id, 32U) || out->abandon_reason == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result progress_decode(const uint8_t *payload, size_t length,
                                  lx_service_progress_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(payload, length,
                                      LX_SERVICE_PROGRESS_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_PROGRESS_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->report_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->commitment_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->note_hash, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->availability_reference, payload + offset, 32U);
    offset += 32U;
    out->progress_bps = lx_service_get_u32(payload + offset); offset += 4U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->report_id, 32U) ||
        lxp_ct_is_zero(out->commitment_id, 32U) ||
        lxp_ct_is_zero(out->note_hash, 32U) ||
        lxp_ct_is_zero(out->availability_reference, 32U) ||
        out->progress_bps == 0U ||
        out->progress_bps > LX_SERVICE_PROGRESS_COMPLETE_BPS)
        return LXP_ERR_PARAMETER_BOUNDS;
    return LXP_OK;
}

static lxp_result deliver_decode(const uint8_t *payload, size_t length,
                                 lx_service_deliver_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    size_t count;
    size_t i;
    lxp_result status = version_check(
        payload, length, LX_SERVICE_DELIVER_PAYLOAD_FIXED_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->delivery_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->agreement_id, payload + offset, 32U); offset += 32U;
    count = payload[offset++];
    if (count == 0U || count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_LENGTH_LIMIT;
    if (offset + count * LX_SERVICE_DELIVER_PAYLOAD_ITEM_BYTES != length)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < count; ++i) {
        lx_service_deliverable *item = &out->deliverables[i];
        (void)memcpy(item->hash, payload + offset, 32U); offset += 32U;
        item->artifact_size = lx_service_get_u64(payload + offset);
        offset += 8U;
        (void)memcpy(item->availability_reference, payload + offset, 32U);
        offset += 32U;
        if (lxp_ct_is_zero(item->hash, 32U) || item->artifact_size == 0U ||
            lxp_ct_is_zero(item->availability_reference, 32U))
            return LXP_ERR_NON_CANONICAL;
    }
    out->deliverable_count = count;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->delivery_id, 32U) ||
        lxp_ct_is_zero(out->agreement_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result reject_decode(const uint8_t *payload, size_t length,
                                lx_service_reject_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    size_t count;
    lxp_result status = version_check(
        payload, length, LX_SERVICE_REJECT_PAYLOAD_FIXED_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->agreement_id, payload + offset, 32U); offset += 32U;
    out->rejection_reason = lx_service_get_u16(payload + offset); offset += 2U;
    count = payload[offset++];
    if (count == 0U || count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_LENGTH_LIMIT;
    if (offset + count * 32U != length) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(out->contested_hashes, payload + offset, count * 32U);
    out->contested_hash_count = count;
    status = lx_service_hashes_check(
        (const uint8_t (*)[32])out->contested_hashes, count);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(out->agreement_id, 32U) || out->rejection_reason == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result dispute_open_decode(const uint8_t *payload, size_t length,
                                      lx_service_dispute_open_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    size_t count;
    lxp_result status = version_check(
        payload, length, LX_SERVICE_DISPUTE_OPEN_PAYLOAD_FIXED_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->dispute_id, payload + offset, 32U); offset += 32U;
    (void)memcpy(out->agreement_id, payload + offset, 32U); offset += 32U;
    count = payload[offset++];
    if (count == 0U || count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_LENGTH_LIMIT;
    if (offset + count * 32U != length) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(out->evidence_hashes, payload + offset, count * 32U);
    out->evidence_hash_count = count;
    status = lx_service_hashes_check(
        (const uint8_t (*)[32])out->evidence_hashes, count);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(out->dispute_id, 32U) ||
        lxp_ct_is_zero(out->agreement_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result dispute_resolve_decode(
    const uint8_t *payload, size_t length,
    lx_service_dispute_resolve_payload *out)
{
    size_t offset = LX_SERVICE_PAYLOAD_VERSION_BYTES;
    lxp_result status = version_check(
        payload, length, LX_SERVICE_DISPUTE_RESOLVE_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_DISPUTE_RESOLVE_PAYLOAD_BYTES)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(out, 0, sizeof(*out));
    (void)memcpy(out->dispute_id, payload + offset, 32U); offset += 32U;
    out->ruling = lx_service_get_u16(payload + offset); offset += 2U;
    out->provider_basis_points = lx_service_get_u32(payload + offset);
    offset += 4U;
    (void)memcpy(out->escrow_resolution_id, payload + offset, 32U);
    offset += 32U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(out->dispute_id, 32U) || out->ruling == 0U ||
        out->provider_basis_points > LX_SERVICE_PROGRESS_COMPLETE_BPS ||
        lxp_ct_is_zero(out->escrow_resolution_id, 32U))
        return LXP_ERR_PARAMETER_BOUNDS;
    return LXP_OK;
}

lxp_result lx_service_payload_decode(uint16_t ordinal, const uint8_t *payload,
                                     size_t length,
                                     lx_service_decoded *decoded)
{
    lxp_result status;
    if (payload == NULL || decoded == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(decoded, 0, sizeof(*decoded));
    decoded->ordinal = ordinal;
    decoded->payload_length = length;
    switch (ordinal) {
    case 1U:
        status = offer_publish_decode(payload, length,
                                      &decoded->payload.offer_publish);
        break;
    case 2U:
    case 4U:
    case 10U:
        status = identifier_decode(payload, length,
                                   &decoded->payload.identifier);
        break;
    case 3U:
        status = agreement_propose_decode(payload, length,
                                          &decoded->payload.agreement_propose);
        break;
    case 5U:
        status = commit_task_decode(payload, length,
                                    &decoded->payload.commit_task);
        break;
    case 6U:
        status = commit_abandon_decode(payload, length,
                                       &decoded->payload.commit_abandon);
        break;
    case 7U:
        status = lx_service_execution_decode(payload, length,
                                             &decoded->payload.execution);
        break;
    case 8U:
        status = progress_decode(payload, length, &decoded->payload.progress);
        break;
    case 9U:
        status = deliver_decode(payload, length, &decoded->payload.deliver);
        break;
    case 11U:
        status = reject_decode(payload, length, &decoded->payload.reject);
        break;
    case 12U:
        status = dispute_open_decode(payload, length,
                                     &decoded->payload.dispute_open);
        break;
    case 13U:
        status = dispute_resolve_decode(payload, length,
                                        &decoded->payload.dispute_resolve);
        break;
    default:
        status = LXP_ERR_UNKNOWN_ACTIVITY;
        break;
    }
    return status;
}
