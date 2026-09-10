#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

const uint8_t lx_service_offer_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'o', 'f', 'f', 'e', 'r', ':', ':', '1'
};
const uint8_t lx_service_agreement_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'a', 'g', 'r', 'e', 'e', ':', ':', '1'
};
const uint8_t lx_service_commitment_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'c', 'o', 'm', 'm', 'i', 't', ':', '1'
};
const uint8_t lx_service_execution_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'e', 'x', 'e', 'c', 'u', 't', ':', '1'
};
const uint8_t lx_service_delivery_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'd', 'e', 'l', 'i', 'v', 'r', ':', '1'
};
const uint8_t lx_service_deliverable_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'd', 'e', 'l', 'i', 't', 'm', ':', '1'
};
const uint8_t lx_service_dispute_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'd', 'i', 's', 'p', 'u', 't', ':', '1'
};
const uint8_t lx_service_progress_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'p', 'r', 'o', 'g', 'r', 's', ':', '1'
};

void lx_service_put_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

uint16_t lx_service_get_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

void lx_service_put_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

uint32_t lx_service_get_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

void lx_service_put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

uint64_t lx_service_get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

lxp_result lx_service_key(const uint8_t prefix[LX_SERVICE_KEY_PREFIX_BYTES],
                          const uint8_t identifier[32],
                          uint8_t key[LX_SERVICE_KEY_BYTES])
{
    if (prefix == NULL || identifier == NULL || key == NULL ||
        lxp_ct_is_zero(identifier, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, prefix, LX_SERVICE_KEY_PREFIX_BYTES);
    (void)memcpy(key + LX_SERVICE_KEY_PREFIX_BYTES, identifier, 32U);
    return LXP_OK;
}

static lxp_result ctx_check(const lxp_module_ctx *ctx)
{
    if (ctx == NULL || ctx->module_id != LXP_MODULE_SERVICE)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result flag_read(uint8_t byte, bool *flag)
{
    if (byte > 1U) return LXP_ERR_NON_CANONICAL;
    *flag = byte != 0U;
    return LXP_OK;
}

lxp_result lx_service_hashes_check(
    const uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32], size_t count)
{
    size_t i;
    size_t j;
    if (hashes == NULL || count == 0U || count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < count; ++i) {
        if (lxp_ct_is_zero(hashes[i], 32U)) return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < i; ++j)
            if (memcmp(hashes[i], hashes[j], 32U) == 0)
                return LXP_ERR_NON_CANONICAL;
    }
    return LXP_OK;
}

void lx_service_hashes_sort(uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32],
                            size_t count)
{
    size_t i;
    for (i = 1U; i < count; ++i) {
        uint8_t current[32];
        size_t position = i;
        (void)memcpy(current, hashes[i], 32U);
        while (position != 0U &&
               memcmp(current, hashes[position - 1U], 32U) < 0) {
            (void)memcpy(hashes[position], hashes[position - 1U], 32U);
            --position;
        }
        (void)memcpy(hashes[position], current, 32U);
    }
}

lxp_result lx_service_offer_encode(
    const lx_service_offer *offer,
    uint8_t bytes[LX_SERVICE_OFFER_RECORD_BYTES])
{
    size_t offset = 0U;
    lxp_result status;
    if (offer == NULL || bytes == NULL ||
        lxp_ct_is_zero(offer->offer_id, 32U) ||
        lxp_ct_is_zero(offer->activity_id, 32U) ||
        lxp_ct_is_zero(offer->offering_agent, 32U) ||
        lxp_ct_is_zero(offer->asset_id, 32U) ||
        lxp_u128_is_zero(offer->price) ||
        lxp_ct_is_zero(offer->terms_hash, 32U) ||
        lxp_ct_is_zero(offer->deliverable_specification_hash, 32U) ||
        offer->delivery_deadline == 0U || offer->acceptance_window == 0U ||
        offer->dispute_window == 0U || offer->offer_expiry == 0U ||
        offer->default_outcome < LX_SERVICE_DEFAULT_ACCEPT ||
        offer->default_outcome > LX_SERVICE_DEFAULT_REJECT)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_SERVICE_OFFER_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, offer->offer_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, offer->activity_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, offer->offering_agent, 32U); offset += 32U;
    (void)memcpy(bytes + offset, offer->asset_id, 32U); offset += 32U;
    status = lxp_u128_to_be(offer->price, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    (void)memcpy(bytes + offset, offer->terms_hash, 32U); offset += 32U;
    (void)memcpy(bytes + offset, offer->deliverable_specification_hash, 32U);
    offset += 32U;
    lx_service_put_u64(bytes + offset, offer->delivery_deadline); offset += 8U;
    lx_service_put_u64(bytes + offset, offer->acceptance_window); offset += 8U;
    lx_service_put_u64(bytes + offset, offer->dispute_window); offset += 8U;
    bytes[offset++] = (uint8_t)offer->default_outcome;
    lx_service_put_u64(bytes + offset, offer->offer_expiry); offset += 8U;
    lx_service_put_u64(bytes + offset, offer->global_sequence); offset += 8U;
    bytes[offset++] = offer->withdrawn ? 1U : 0U;
    bytes[offset++] = offer->accepted ? 1U : 0U;
    return offset == LX_SERVICE_OFFER_RECORD_BYTES ? LXP_OK :
                                                     LXP_FATAL_INVARIANT;
}

lxp_result lx_service_offer_decode(const uint8_t *bytes, size_t length,
                                   lx_service_offer *offer)
{
    size_t offset = 0U;
    lxp_result status;
    if (bytes == NULL || offer == NULL ||
        length != LX_SERVICE_OFFER_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(offer, 0, sizeof(*offer));
    offset = 1U;
    (void)memcpy(offer->offer_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(offer->activity_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(offer->offering_agent, bytes + offset, 32U); offset += 32U;
    (void)memcpy(offer->asset_id, bytes + offset, 32U); offset += 32U;
    status = lxp_u128_from_be(bytes + offset, &offer->price);
    if (status != LXP_OK) return status;
    offset += 16U;
    (void)memcpy(offer->terms_hash, bytes + offset, 32U); offset += 32U;
    (void)memcpy(offer->deliverable_specification_hash, bytes + offset, 32U);
    offset += 32U;
    offer->delivery_deadline = lx_service_get_u64(bytes + offset); offset += 8U;
    offer->acceptance_window = lx_service_get_u64(bytes + offset); offset += 8U;
    offer->dispute_window = lx_service_get_u64(bytes + offset); offset += 8U;
    if (bytes[offset] < LX_SERVICE_DEFAULT_ACCEPT ||
        bytes[offset] > LX_SERVICE_DEFAULT_REJECT)
        return LXP_ERR_NON_CANONICAL;
    offer->default_outcome = (lx_service_default_outcome)bytes[offset];
    offset += 1U;
    offer->offer_expiry = lx_service_get_u64(bytes + offset); offset += 8U;
    offer->global_sequence = lx_service_get_u64(bytes + offset); offset += 8U;
    status = flag_read(bytes[offset], &offer->withdrawn);
    if (status != LXP_OK) return status;
    offset += 1U;
    status = flag_read(bytes[offset], &offer->accepted);
    if (status != LXP_OK) return status;
    offset += 1U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(offer->offer_id, 32U) ||
        lxp_ct_is_zero(offer->activity_id, 32U) ||
        lxp_ct_is_zero(offer->offering_agent, 32U) ||
        lxp_ct_is_zero(offer->asset_id, 32U) ||
        lxp_u128_is_zero(offer->price) ||
        lxp_ct_is_zero(offer->terms_hash, 32U) ||
        lxp_ct_is_zero(offer->deliverable_specification_hash, 32U) ||
        offer->delivery_deadline == 0U || offer->acceptance_window == 0U ||
        offer->dispute_window == 0U || offer->offer_expiry == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_offer_put(lxp_module_ctx *ctx,
                                const lx_service_offer *offer)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_OFFER_RECORD_BYTES];
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = lx_service_offer_encode(offer, bytes);
    if (status != LXP_OK) return status;
    status = lx_service_key(lx_service_offer_prefix, offer->offer_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_service_offer_lookup(lxp_module_ctx *ctx,
                                   const uint8_t offer_id[32],
                                   lx_service_offer *offer)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || offer == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_offer_prefix, offer_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_service_offer_decode(bytes, length, offer);
}

lxp_result lx_service_agreement_encode(
    const lx_service_agreement *agreement,
    uint8_t bytes[LX_SERVICE_AGREEMENT_RECORD_BYTES], size_t *length)
{
    size_t offset = 0U;
    if (agreement == NULL || bytes == NULL || length == NULL ||
        lxp_ct_is_zero(agreement->agreement_id, 32U) ||
        lxp_ct_is_zero(agreement->offer_id, 32U) ||
        lxp_ct_is_zero(agreement->provider, 32U) ||
        lxp_ct_is_zero(agreement->buyer, 32U) ||
        lxp_ct_is_zero(agreement->terms_hash, 32U) ||
        agreement->default_outcome < LX_SERVICE_DEFAULT_ACCEPT ||
        agreement->default_outcome > LX_SERVICE_DEFAULT_REJECT ||
        agreement->state < LX_SERVICE_AGREEMENT_FORMED ||
        agreement->state > LX_SERVICE_AGREEMENT_PROPOSED ||
        agreement->contested_hash_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_SERVICE_AGREEMENT_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, agreement->agreement_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, agreement->offer_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, agreement->provider, 32U); offset += 32U;
    (void)memcpy(bytes + offset, agreement->buyer, 32U); offset += 32U;
    (void)memcpy(bytes + offset, agreement->terms_hash, 32U); offset += 32U;
    (void)memcpy(bytes + offset, agreement->escrow_id, 32U); offset += 32U;
    lx_service_put_u64(bytes + offset, agreement->delivery_deadline);
    offset += 8U;
    lx_service_put_u64(bytes + offset, agreement->acceptance_window_end);
    offset += 8U;
    lx_service_put_u64(bytes + offset, agreement->dispute_window_end);
    offset += 8U;
    bytes[offset++] = (uint8_t)agreement->default_outcome;
    bytes[offset++] = (uint8_t)agreement->state;
    lx_service_put_u64(bytes + offset, agreement->accepted_sequence);
    offset += 8U;
    lx_service_put_u16(bytes + offset, agreement->rejection_reason);
    offset += 2U;
    bytes[offset++] = agreement->default_applied ? 1U : 0U;
    lx_service_put_u64(bytes + offset, agreement->outcome_sequence);
    offset += 8U;
    lx_service_put_u64(bytes + offset, agreement->outcome_timestamp);
    offset += 8U;
    bytes[offset++] = (uint8_t)agreement->contested_hash_count;
    if (offset != LX_SERVICE_AGREEMENT_FIXED_BYTES) return LXP_FATAL_INVARIANT;
    if (agreement->contested_hash_count != 0U) {
        lxp_result status = lx_service_hashes_check(
            agreement->contested_hashes, agreement->contested_hash_count);
        if (status != LXP_OK) return status;
        (void)memcpy(bytes + offset, agreement->contested_hashes,
                     agreement->contested_hash_count * 32U);
        offset += agreement->contested_hash_count * 32U;
    }
    *length = offset;
    return LXP_OK;
}

lxp_result lx_service_agreement_decode(const uint8_t *bytes, size_t length,
                                       lx_service_agreement *agreement)
{
    size_t offset;
    size_t count;
    lxp_result status;
    if (bytes == NULL || agreement == NULL ||
        length < LX_SERVICE_AGREEMENT_FIXED_BYTES ||
        length > LX_SERVICE_AGREEMENT_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(agreement, 0, sizeof(*agreement));
    offset = 1U;
    (void)memcpy(agreement->agreement_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(agreement->offer_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(agreement->provider, bytes + offset, 32U); offset += 32U;
    (void)memcpy(agreement->buyer, bytes + offset, 32U); offset += 32U;
    (void)memcpy(agreement->terms_hash, bytes + offset, 32U); offset += 32U;
    (void)memcpy(agreement->escrow_id, bytes + offset, 32U); offset += 32U;
    agreement->delivery_deadline = lx_service_get_u64(bytes + offset);
    offset += 8U;
    agreement->acceptance_window_end = lx_service_get_u64(bytes + offset);
    offset += 8U;
    agreement->dispute_window_end = lx_service_get_u64(bytes + offset);
    offset += 8U;
    if (bytes[offset] < LX_SERVICE_DEFAULT_ACCEPT ||
        bytes[offset] > LX_SERVICE_DEFAULT_REJECT)
        return LXP_ERR_NON_CANONICAL;
    agreement->default_outcome = (lx_service_default_outcome)bytes[offset];
    offset += 1U;
    if (bytes[offset] < LX_SERVICE_AGREEMENT_FORMED ||
        bytes[offset] > LX_SERVICE_AGREEMENT_PROPOSED)
        return LXP_ERR_NON_CANONICAL;
    agreement->state = (lx_service_agreement_state)bytes[offset];
    offset += 1U;
    agreement->accepted_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    agreement->rejection_reason = lx_service_get_u16(bytes + offset);
    offset += 2U;
    status = flag_read(bytes[offset], &agreement->default_applied);
    if (status != LXP_OK) return status;
    offset += 1U;
    agreement->outcome_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    agreement->outcome_timestamp = lx_service_get_u64(bytes + offset);
    offset += 8U;
    count = bytes[offset++];
    if (count > LX_SERVICE_MAX_DELIVERABLES ||
        offset + count * 32U != length)
        return LXP_ERR_NON_CANONICAL;
    if (count != 0U) {
        (void)memcpy(agreement->contested_hashes, bytes + offset, count * 32U);
        status = lx_service_hashes_check(
            (const uint8_t (*)[32])agreement->contested_hashes, count);
        if (status != LXP_OK) return status;
    }
    agreement->contested_hash_count = count;
    if (lxp_ct_is_zero(agreement->agreement_id, 32U) ||
        lxp_ct_is_zero(agreement->offer_id, 32U) ||
        lxp_ct_is_zero(agreement->provider, 32U) ||
        lxp_ct_is_zero(agreement->buyer, 32U) ||
        lxp_ct_is_zero(agreement->terms_hash, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_agreement_put(lxp_module_ctx *ctx,
                                    const lx_service_agreement *agreement)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_AGREEMENT_RECORD_BYTES];
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = lx_service_agreement_encode(agreement, bytes, &length);
    if (status != LXP_OK) return status;
    status = lx_service_key(lx_service_agreement_prefix,
                            agreement->agreement_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length);
}

lxp_result lx_service_agreement_lookup(lxp_module_ctx *ctx,
                                       const uint8_t agreement_id[32],
                                       lx_service_agreement *agreement)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || agreement == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_agreement_prefix, agreement_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_service_agreement_decode(bytes, length, agreement);
}

lxp_result lx_service_commitment_encode(
    const lx_service_commitment *commitment,
    uint8_t bytes[LX_SERVICE_COMMITMENT_RECORD_BYTES])
{
    size_t offset = 0U;
    if (commitment == NULL || bytes == NULL ||
        lxp_ct_is_zero(commitment->commitment_id, 32U) ||
        lxp_ct_is_zero(commitment->activity_id, 32U) ||
        lxp_ct_is_zero(commitment->provider, 32U) ||
        lxp_ct_is_zero(commitment->agreement_id, 32U) ||
        lxp_ct_is_zero(commitment->task_hash, 32U) ||
        commitment->deadline == 0U || commitment->resource_bound == 0U ||
        (commitment->abandoned && commitment->abandon_reason == 0U) ||
        (!commitment->abandoned && commitment->abandon_reason != 0U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_SERVICE_COMMITMENT_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, commitment->commitment_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, commitment->activity_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, commitment->provider, 32U); offset += 32U;
    (void)memcpy(bytes + offset, commitment->agreement_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, commitment->task_hash, 32U); offset += 32U;
    (void)memcpy(bytes + offset, commitment->escrow_id, 32U); offset += 32U;
    lx_service_put_u64(bytes + offset, commitment->deadline); offset += 8U;
    lx_service_put_u64(bytes + offset, commitment->resource_bound);
    offset += 8U;
    lx_service_put_u64(bytes + offset, commitment->global_sequence);
    offset += 8U;
    bytes[offset++] = commitment->abandoned ? 1U : 0U;
    lx_service_put_u16(bytes + offset, commitment->abandon_reason);
    offset += 2U;
    return offset == LX_SERVICE_COMMITMENT_RECORD_BYTES ? LXP_OK :
                                                          LXP_FATAL_INVARIANT;
}

lxp_result lx_service_commitment_decode(const uint8_t *bytes, size_t length,
                                        lx_service_commitment *commitment)
{
    size_t offset;
    lxp_result status;
    if (bytes == NULL || commitment == NULL ||
        length != LX_SERVICE_COMMITMENT_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(commitment, 0, sizeof(*commitment));
    offset = 1U;
    (void)memcpy(commitment->commitment_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(commitment->activity_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(commitment->provider, bytes + offset, 32U); offset += 32U;
    (void)memcpy(commitment->agreement_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(commitment->task_hash, bytes + offset, 32U); offset += 32U;
    (void)memcpy(commitment->escrow_id, bytes + offset, 32U); offset += 32U;
    commitment->deadline = lx_service_get_u64(bytes + offset); offset += 8U;
    commitment->resource_bound = lx_service_get_u64(bytes + offset);
    offset += 8U;
    commitment->global_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    status = flag_read(bytes[offset], &commitment->abandoned);
    if (status != LXP_OK) return status;
    offset += 1U;
    commitment->abandon_reason = lx_service_get_u16(bytes + offset);
    offset += 2U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(commitment->commitment_id, 32U) ||
        lxp_ct_is_zero(commitment->activity_id, 32U) ||
        lxp_ct_is_zero(commitment->provider, 32U) ||
        lxp_ct_is_zero(commitment->agreement_id, 32U) ||
        lxp_ct_is_zero(commitment->task_hash, 32U) ||
        commitment->deadline == 0U || commitment->resource_bound == 0U ||
        (commitment->abandoned && commitment->abandon_reason == 0U) ||
        (!commitment->abandoned && commitment->abandon_reason != 0U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_commitment_put(lxp_module_ctx *ctx,
                                     const lx_service_commitment *commitment)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_COMMITMENT_RECORD_BYTES];
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = lx_service_commitment_encode(commitment, bytes);
    if (status != LXP_OK) return status;
    status = lx_service_key(lx_service_commitment_prefix,
                            commitment->commitment_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_service_commitment_lookup(lxp_module_ctx *ctx,
                                        const uint8_t commitment_id[32],
                                        lx_service_commitment *commitment)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || commitment == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_commitment_prefix, commitment_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_service_commitment_decode(bytes, length, commitment);
}

lxp_result lx_service_execution_put(lxp_module_ctx *ctx,
                                    const lx_service_execution *execution)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_EXECUTION_RECORD_BYTES];
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || execution == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    bytes[0] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    status = lx_service_execution_encode(execution, bytes + 1U,
                                         sizeof(bytes) - 1U, &length);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_EXECUTION_BYTES) return LXP_FATAL_INVARIANT;
    status = lx_service_key(lx_service_execution_prefix,
                            execution->attestation_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_service_execution_lookup(lxp_module_ctx *ctx,
                                       const uint8_t attestation_id[32],
                                       lx_service_execution *execution)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t preimage[384];
    const uint8_t *bytes;
    size_t length;
    size_t preimage_length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || execution == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_execution_prefix, attestation_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    if (length != LX_SERVICE_EXECUTION_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    status = lx_service_execution_decode(bytes + 1U, length - 1U, execution);
    if (status != LXP_OK) return status;
    status = lx_service_attestation_bytes(execution, preimage,
                                          sizeof(preimage), &preimage_length);
    if (status != LXP_OK) return status;
    if (preimage_length > sizeof(execution->canonical_payload))
        return LXP_ERR_LENGTH_LIMIT;
    execution->canonical_payload_length = (uint16_t)preimage_length;
    (void)memcpy(execution->canonical_payload, preimage, preimage_length);
    return LXP_OK;
}

static lxp_result deliverable_encode(
    const lx_service_deliverable *item,
    uint8_t bytes[LX_SERVICE_DELIVERABLE_RECORD_BYTES])
{
    size_t offset = 0U;
    if (item == NULL || lxp_ct_is_zero(item->hash, 32U) ||
        item->artifact_size == 0U ||
        lxp_ct_is_zero(item->availability_reference, 32U))
        return LXP_ERR_NON_CANONICAL;
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, item->hash, 32U); offset += 32U;
    lx_service_put_u64(bytes + offset, item->artifact_size); offset += 8U;
    (void)memcpy(bytes + offset, item->availability_reference, 32U);
    offset += 32U;
    return offset == LX_SERVICE_DELIVERABLE_RECORD_BYTES ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result deliverable_decode(const uint8_t *bytes, size_t length,
                                     lx_service_deliverable *item)
{
    size_t offset;
    if (bytes == NULL || item == NULL ||
        length != LX_SERVICE_DELIVERABLE_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(item, 0, sizeof(*item));
    offset = 1U;
    (void)memcpy(item->hash, bytes + offset, 32U); offset += 32U;
    item->artifact_size = lx_service_get_u64(bytes + offset); offset += 8U;
    (void)memcpy(item->availability_reference, bytes + offset, 32U);
    offset += 32U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(item->hash, 32U) || item->artifact_size == 0U ||
        lxp_ct_is_zero(item->availability_reference, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result delivery_header_encode(
    const lx_service_delivery *delivery,
    uint8_t bytes[LX_SERVICE_DELIVERY_RECORD_BYTES])
{
    size_t offset = 0U;
    if (delivery == NULL || lxp_ct_is_zero(delivery->delivery_id, 32U) ||
        lxp_ct_is_zero(delivery->activity_id, 32U) ||
        lxp_ct_is_zero(delivery->agreement_id, 32U) ||
        lxp_ct_is_zero(delivery->provider, 32U) ||
        delivery->deliverable_count == 0U ||
        delivery->deliverable_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_SERVICE_DELIVERY_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, delivery->delivery_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, delivery->activity_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, delivery->agreement_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, delivery->provider, 32U); offset += 32U;
    bytes[offset++] = (uint8_t)delivery->deliverable_count;
    lx_service_put_u64(bytes + offset, delivery->global_sequence);
    offset += 8U;
    return offset == LX_SERVICE_DELIVERY_RECORD_BYTES ? LXP_OK :
                                                        LXP_FATAL_INVARIANT;
}

static lxp_result delivery_header_decode(const uint8_t *bytes, size_t length,
                                         lx_service_delivery *delivery)
{
    size_t offset;
    if (bytes == NULL || delivery == NULL ||
        length != LX_SERVICE_DELIVERY_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(delivery, 0, sizeof(*delivery));
    offset = 1U;
    (void)memcpy(delivery->delivery_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(delivery->activity_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(delivery->agreement_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(delivery->provider, bytes + offset, 32U); offset += 32U;
    delivery->deliverable_count = bytes[offset++];
    delivery->global_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (delivery->deliverable_count == 0U ||
        delivery->deliverable_count > LX_SERVICE_MAX_DELIVERABLES ||
        lxp_ct_is_zero(delivery->delivery_id, 32U) ||
        lxp_ct_is_zero(delivery->activity_id, 32U) ||
        lxp_ct_is_zero(delivery->agreement_id, 32U) ||
        lxp_ct_is_zero(delivery->provider, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result deliverable_key(const uint8_t delivery_id[32], size_t index,
                                  uint8_t key[LX_SERVICE_ITEM_KEY_BYTES])
{
    lxp_result status;
    if (index >= LX_SERVICE_MAX_DELIVERABLES) return LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_deliverable_prefix, delivery_id, key);
    if (status != LXP_OK) return status;
    key[LX_SERVICE_KEY_BYTES] = (uint8_t)index;
    return LXP_OK;
}

lxp_result lx_service_delivery_put(lxp_module_ctx *ctx,
                                   const lx_service_delivery *delivery)
{
    uint8_t key[LX_SERVICE_ITEM_KEY_BYTES];
    uint8_t header[LX_SERVICE_DELIVERY_RECORD_BYTES];
    uint8_t item[LX_SERVICE_DELIVERABLE_RECORD_BYTES];
    size_t i;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = delivery_header_encode(delivery, header);
    if (status != LXP_OK) return status;
    for (i = 0U; i < delivery->deliverable_count; ++i) {
        status = deliverable_encode(&delivery->deliverables[i], item);
        if (status != LXP_OK) return status;
        status = deliverable_key(delivery->delivery_id, i, key);
        if (status != LXP_OK) return status;
        status = lxp_ctx_kv_put(ctx, key, LX_SERVICE_ITEM_KEY_BYTES, item,
                                sizeof(item));
        if (status != LXP_OK) return status;
    }
    status = lx_service_key(lx_service_delivery_prefix, delivery->delivery_id,
                            key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, LX_SERVICE_KEY_BYTES, header,
                          sizeof(header));
}

lxp_result lx_service_delivery_lookup(lxp_module_ctx *ctx,
                                      const uint8_t delivery_id[32],
                                      lx_service_delivery *delivery)
{
    uint8_t key[LX_SERVICE_ITEM_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    size_t i;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || delivery == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_delivery_prefix, delivery_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, LX_SERVICE_KEY_BYTES, &bytes, &length);
    if (status != LXP_OK) return status;
    status = delivery_header_decode(bytes, length, delivery);
    if (status != LXP_OK) return status;
    for (i = 0U; i < delivery->deliverable_count; ++i) {
        status = deliverable_key(delivery->delivery_id, i, key);
        if (status != LXP_OK) return status;
        status = lxp_ctx_kv_get(ctx, key, LX_SERVICE_ITEM_KEY_BYTES, &bytes,
                                &length);
        if (status != LXP_OK) return status;
        status = deliverable_decode(bytes, length,
                                    &delivery->deliverables[i]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

typedef struct delivery_scan {
    const uint8_t *agreement_id;
    uint8_t delivery_id[32];
    uint64_t global_sequence;
    bool found;
} delivery_scan;

static lxp_result visit_delivery(const uint8_t *key, size_t key_length,
                                 const uint8_t *value, size_t value_length,
                                 void *user)
{
    delivery_scan *scan = (delivery_scan *)user;
    lx_service_delivery header;
    lxp_result status;
    if (key == NULL || key_length != LX_SERVICE_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = delivery_header_decode(value, value_length, &header);
    if (status != LXP_OK) return status;
    if (memcmp(header.agreement_id, scan->agreement_id, 32U) != 0)
        return LXP_OK;
    if (!scan->found || header.global_sequence >= scan->global_sequence) {
        (void)memcpy(scan->delivery_id, header.delivery_id, 32U);
        scan->global_sequence = header.global_sequence;
        scan->found = true;
    }
    return LXP_OK;
}

lxp_result lx_service_delivery_latest(lxp_module_ctx *ctx,
                                      const uint8_t agreement_id[32],
                                      lx_service_delivery *delivery)
{
    delivery_scan scan;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || agreement_id == NULL || delivery == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    (void)memset(&scan, 0, sizeof(scan));
    scan.agreement_id = agreement_id;
    status = lxp_ctx_kv_iter(ctx, lx_service_delivery_prefix,
                             LX_SERVICE_KEY_PREFIX_BYTES, visit_delivery,
                             &scan);
    if (status != LXP_OK) return status;
    if (!scan.found) return LXP_ERR_UNKNOWN_FIELD;
    return lx_service_delivery_lookup(ctx, scan.delivery_id, delivery);
}

lxp_result lx_service_progress_encode(
    const lx_service_progress *progress,
    uint8_t bytes[LX_SERVICE_PROGRESS_RECORD_BYTES])
{
    size_t offset = 0U;
    if (progress == NULL || bytes == NULL ||
        lxp_ct_is_zero(progress->report_id, 32U) ||
        lxp_ct_is_zero(progress->activity_id, 32U) ||
        lxp_ct_is_zero(progress->commitment_id, 32U) ||
        lxp_ct_is_zero(progress->agreement_id, 32U) ||
        lxp_ct_is_zero(progress->provider, 32U) ||
        lxp_ct_is_zero(progress->note_hash, 32U) ||
        lxp_ct_is_zero(progress->availability_reference, 32U) ||
        progress->progress_bps == 0U ||
        progress->progress_bps > LX_SERVICE_PROGRESS_COMPLETE_BPS)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_SERVICE_PROGRESS_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, progress->report_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->activity_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->commitment_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->agreement_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->provider, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->note_hash, 32U); offset += 32U;
    (void)memcpy(bytes + offset, progress->availability_reference, 32U);
    offset += 32U;
    lx_service_put_u32(bytes + offset, progress->progress_bps); offset += 4U;
    lx_service_put_u64(bytes + offset, progress->reported_at); offset += 8U;
    lx_service_put_u64(bytes + offset, progress->global_sequence); offset += 8U;
    return offset == LX_SERVICE_PROGRESS_RECORD_BYTES ? LXP_OK :
                                                        LXP_FATAL_INVARIANT;
}

lxp_result lx_service_progress_decode(const uint8_t *bytes, size_t length,
                                      lx_service_progress *progress)
{
    size_t offset;
    if (bytes == NULL || progress == NULL ||
        length != LX_SERVICE_PROGRESS_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(progress, 0, sizeof(*progress));
    offset = 1U;
    (void)memcpy(progress->report_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->activity_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->commitment_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->agreement_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->provider, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->note_hash, bytes + offset, 32U); offset += 32U;
    (void)memcpy(progress->availability_reference, bytes + offset, 32U);
    offset += 32U;
    progress->progress_bps = lx_service_get_u32(bytes + offset); offset += 4U;
    progress->reported_at = lx_service_get_u64(bytes + offset); offset += 8U;
    progress->global_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(progress->report_id, 32U) ||
        lxp_ct_is_zero(progress->activity_id, 32U) ||
        lxp_ct_is_zero(progress->commitment_id, 32U) ||
        lxp_ct_is_zero(progress->agreement_id, 32U) ||
        lxp_ct_is_zero(progress->provider, 32U) ||
        lxp_ct_is_zero(progress->note_hash, 32U) ||
        lxp_ct_is_zero(progress->availability_reference, 32U) ||
        progress->progress_bps == 0U ||
        progress->progress_bps > LX_SERVICE_PROGRESS_COMPLETE_BPS)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_progress_put(lxp_module_ctx *ctx,
                                   const lx_service_progress *progress)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_PROGRESS_RECORD_BYTES];
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = lx_service_progress_encode(progress, bytes);
    if (status != LXP_OK) return status;
    status = lx_service_key(lx_service_progress_prefix, progress->report_id,
                            key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_service_progress_lookup(lxp_module_ctx *ctx,
                                      const uint8_t report_id[32],
                                      lx_service_progress *progress)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || progress == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_progress_prefix, report_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_service_progress_decode(bytes, length, progress);
}

typedef struct progress_scan {
    const uint8_t *commitment_id;
    uint32_t progress_bps;
} progress_scan;

static lxp_result visit_progress(const uint8_t *key, size_t key_length,
                                 const uint8_t *value, size_t value_length,
                                 void *user)
{
    progress_scan *scan = (progress_scan *)user;
    lx_service_progress progress;
    lxp_result status;
    if (key == NULL || key_length != LX_SERVICE_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = lx_service_progress_decode(value, value_length, &progress);
    if (status != LXP_OK) return status;
    if (memcmp(progress.commitment_id, scan->commitment_id, 32U) == 0 &&
        progress.progress_bps > scan->progress_bps)
        scan->progress_bps = progress.progress_bps;
    return LXP_OK;
}

lxp_result lx_service_progress_high_water(lxp_module_ctx *ctx,
                                          const uint8_t commitment_id[32],
                                          uint32_t *progress_bps)
{
    progress_scan scan;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || commitment_id == NULL || progress_bps == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    scan.commitment_id = commitment_id;
    scan.progress_bps = 0U;
    status = lxp_ctx_kv_iter(ctx, lx_service_progress_prefix,
                             LX_SERVICE_KEY_PREFIX_BYTES, visit_progress,
                             &scan);
    if (status != LXP_OK) return status;
    *progress_bps = scan.progress_bps;
    return LXP_OK;
}

lxp_result lx_service_dispute_encode(
    const lx_service_dispute *dispute,
    uint8_t bytes[LX_SERVICE_DISPUTE_RECORD_BYTES], size_t *length)
{
    size_t offset = 0U;
    lxp_result status;
    if (dispute == NULL || bytes == NULL || length == NULL ||
        lxp_ct_is_zero(dispute->dispute_id, 32U) ||
        lxp_ct_is_zero(dispute->activity_id, 32U) ||
        lxp_ct_is_zero(dispute->agreement_id, 32U) ||
        lxp_ct_is_zero(dispute->raiser, 32U) ||
        dispute->provider_basis_points > LX_SERVICE_PROGRESS_COMPLETE_BPS ||
        (dispute->resolved && (dispute->ruling == 0U ||
                               lxp_ct_is_zero(dispute->escrow_resolution_id,
                                              32U))))
        return LXP_ERR_NON_CANONICAL;
    status = lx_service_hashes_check(dispute->evidence_hashes,
                                     dispute->evidence_hash_count);
    if (status != LXP_OK) return status;
    (void)memset(bytes, 0, LX_SERVICE_DISPUTE_RECORD_BYTES);
    bytes[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(bytes + offset, dispute->dispute_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, dispute->activity_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, dispute->agreement_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, dispute->raiser, 32U); offset += 32U;
    lx_service_put_u64(bytes + offset, dispute->global_sequence); offset += 8U;
    bytes[offset++] = dispute->resolved ? 1U : 0U;
    lx_service_put_u16(bytes + offset, dispute->ruling); offset += 2U;
    lx_service_put_u32(bytes + offset, dispute->provider_basis_points);
    offset += 4U;
    (void)memcpy(bytes + offset, dispute->escrow_resolution_id, 32U);
    offset += 32U;
    lx_service_put_u64(bytes + offset, dispute->resolution_sequence);
    offset += 8U;
    bytes[offset++] = (uint8_t)dispute->evidence_hash_count;
    if (offset != LX_SERVICE_DISPUTE_FIXED_BYTES) return LXP_FATAL_INVARIANT;
    (void)memcpy(bytes + offset, dispute->evidence_hashes,
                 dispute->evidence_hash_count * 32U);
    offset += dispute->evidence_hash_count * 32U;
    *length = offset;
    return LXP_OK;
}

lxp_result lx_service_dispute_decode(const uint8_t *bytes, size_t length,
                                     lx_service_dispute *dispute)
{
    size_t offset;
    size_t count;
    lxp_result status;
    if (bytes == NULL || dispute == NULL ||
        length < LX_SERVICE_DISPUTE_FIXED_BYTES ||
        length > LX_SERVICE_DISPUTE_RECORD_BYTES ||
        bytes[0] != (uint8_t)LX_SERVICE_RECORD_VERSION)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(dispute, 0, sizeof(*dispute));
    offset = 1U;
    (void)memcpy(dispute->dispute_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(dispute->activity_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(dispute->agreement_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(dispute->raiser, bytes + offset, 32U); offset += 32U;
    dispute->global_sequence = lx_service_get_u64(bytes + offset); offset += 8U;
    status = flag_read(bytes[offset], &dispute->resolved);
    if (status != LXP_OK) return status;
    offset += 1U;
    dispute->ruling = lx_service_get_u16(bytes + offset); offset += 2U;
    dispute->provider_basis_points = lx_service_get_u32(bytes + offset);
    offset += 4U;
    (void)memcpy(dispute->escrow_resolution_id, bytes + offset, 32U);
    offset += 32U;
    dispute->resolution_sequence = lx_service_get_u64(bytes + offset);
    offset += 8U;
    count = bytes[offset++];
    if (offset + count * 32U != length) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(dispute->evidence_hashes, bytes + offset, count * 32U);
    dispute->evidence_hash_count = count;
    status = lx_service_hashes_check(
        (const uint8_t (*)[32])dispute->evidence_hashes, count);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(dispute->dispute_id, 32U) ||
        lxp_ct_is_zero(dispute->activity_id, 32U) ||
        lxp_ct_is_zero(dispute->agreement_id, 32U) ||
        lxp_ct_is_zero(dispute->raiser, 32U) ||
        dispute->provider_basis_points > LX_SERVICE_PROGRESS_COMPLETE_BPS ||
        (dispute->resolved && (dispute->ruling == 0U ||
                               lxp_ct_is_zero(dispute->escrow_resolution_id,
                                              32U))))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_dispute_put(lxp_module_ctx *ctx,
                                  const lx_service_dispute *dispute)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    uint8_t bytes[LX_SERVICE_DISPUTE_RECORD_BYTES];
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK) return status;
    status = lx_service_dispute_encode(dispute, bytes, &length);
    if (status != LXP_OK) return status;
    status = lx_service_key(lx_service_dispute_prefix, dispute->dispute_id,
                            key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length);
}

lxp_result lx_service_dispute_lookup(lxp_module_ctx *ctx,
                                     const uint8_t dispute_id[32],
                                     lx_service_dispute *dispute)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status = ctx_check(ctx);
    if (status != LXP_OK || dispute == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_service_key(lx_service_dispute_prefix, dispute_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_service_dispute_decode(bytes, length, dispute);
}

lxp_result lx_service_emit(lxp_module_ctx *ctx, uint16_t event_type,
                           const uint8_t primary[32],
                           const uint8_t secondary[32], uint8_t code,
                           uint64_t sequence)
{
    uint8_t body[LX_SERVICE_EVENT_BODY_BYTES];
    size_t offset = 0U;
    if (ctx == NULL || primary == NULL || secondary == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(body + offset, primary, 32U); offset += 32U;
    (void)memcpy(body + offset, secondary, 32U); offset += 32U;
    body[offset++] = code;
    lx_service_put_u64(body + offset, sequence); offset += 8U;
    if (offset != sizeof(body)) return LXP_FATAL_INVARIANT;
    return lxp_ctx_emit_event(ctx, event_type, body, sizeof(body));
}
