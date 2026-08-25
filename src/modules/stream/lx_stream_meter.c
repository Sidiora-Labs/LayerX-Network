#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

lxp_result lx_stream_meter_attestation_bytes(
    const lx_stream_meter_attestation *attestation,
    uint8_t *bytes, size_t capacity, size_t *length)
{
    static const uint8_t tag[] = "LXP:STREAM:METER:v1";
    size_t i;
    if (attestation == NULL || bytes == NULL || length == NULL ||
        capacity < sizeof(tag) - 1U + 72U)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(bytes, tag, sizeof(tag) - 1U);
    (void)memcpy(bytes + sizeof(tag) - 1U, attestation->stream_id, 32U);
    for (i = 0U; i < 8U; ++i)
        bytes[sizeof(tag) - 1U + 32U + i] =
            (uint8_t)(attestation->cumulative_reading >> ((7U - i) * 8U));
    (void)memcpy(bytes + sizeof(tag) - 1U + 40U,
                 attestation->authority_key, 32U);
    *length = sizeof(tag) - 1U + 72U;
    return LXP_OK;
}

lxp_result lx_stream_meter_authority_check(
    const lx_stream_record *record,
    const lx_stream_meter_attestation *attestation)
{
    uint8_t message[128];
    size_t message_length;
    size_t i;
    bool found = false;
    lxp_result status;
    if (record == NULL || attestation == NULL ||
        record->meter_authority_count > LX_STREAM_MAX_METER_AUTHORITIES ||
        memcmp(record->stream_id, attestation->stream_id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_METER;
    for (i = 0U; i < record->meter_authority_count; ++i)
        if (memcmp(record->meter_authorities[i],
                   attestation->authority_key, 32U) == 0) {
            found = true;
            break;
        }
    if (!found) return LXP_ERR_UNAUTHORIZED_METER;
    status = lx_stream_meter_attestation_bytes(attestation, message,
                                               sizeof(message),
                                               &message_length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify(attestation->authority_key,
                                    attestation->signature,
                                    LXP_DOMAIN_SIGNATURE_PREIMAGE,
                                    message, message_length);
    return status == LXP_OK ? LXP_OK : LXP_ERR_UNAUTHORIZED_METER;
}

lxp_result lx_stream_metered_accrue(lx_stream_record *record,
                                    uint64_t cumulative_reading,
                                    lxp_u128 *newly_accrued)
{
    uint64_t delta;
    lxp_u256 product;
    lxp_u256 carry;
    lxp_u256 numerator;
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_u128 cap_remaining;
    lxp_u128 updated;
    lxp_result status;
    if (record == NULL || newly_accrued == NULL ||
        record->mode != LX_STREAM_MODE_METERED || record->rate_unit == 0U)
        return LXP_ERR_NON_CANONICAL;
    *newly_accrued = (lxp_u128){ 0U, 0U };
    if (cumulative_reading < record->cumulative_meter)
        return LXP_ERR_METER_REGRESSION;
    delta = cumulative_reading - record->cumulative_meter;
    if (delta == 0U || record->closed ||
        lxp_u128_cmp(record->accrued_total, record->total_cap) >= 0)
        return LXP_OK;
    if (record->paused || record->underfunded) {
        record->cumulative_meter = cumulative_reading;
        return LXP_OK;
    }
    status = lxp_u128_mul(record->rate, (lxp_u128){ 0U, delta }, &product);
    if (status != LXP_OK) return LXP_ERR_ACCRUAL_OVERFLOW;
    (void)memset(&carry, 0, sizeof(carry));
    carry.words[0] = record->remainder_carry.lo;
    carry.words[1] = record->remainder_carry.hi;
    status = lxp_u256_add(product, carry, &numerator);
    if (status == LXP_OK)
        status = lxp_u256_div_floor(
            numerator, (lxp_u128){ 0U, record->rate_unit },
            &quotient, &remainder);
    if (status != LXP_OK) return LXP_ERR_ACCRUAL_OVERFLOW;
    status = lxp_u128_sub(record->total_cap, record->accrued_total,
                          &cap_remaining);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    if (lxp_u128_cmp(quotient, cap_remaining) > 0) {
        quotient = cap_remaining;
        remainder = (lxp_u128){ 0U, 0U };
    }
    status = lxp_u128_add(record->accrued_total, quotient, &updated);
    if (status != LXP_OK) return LXP_ERR_ACCRUAL_OVERFLOW;
    status = lx_stream_carry_apply(record, remainder);
    if (status != LXP_OK) return status;
    record->cumulative_meter = cumulative_reading;
    record->accrued_total = updated;
    *newly_accrued = quotient;
    return LXP_OK;
}

lxp_result lx_stream_meter_execute(
    lx_stream_record *record,
    const lx_stream_meter_attestation *attestation,
    lxp_u128 *newly_accrued)
{
    lxp_result status = lx_stream_meter_authority_check(record, attestation);
    if (status != LXP_OK) return status;
    return lx_stream_metered_accrue(record,
                                    attestation->cumulative_reading,
                                    newly_accrued);
}
