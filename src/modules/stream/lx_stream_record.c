#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static void put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static uint64_t get_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static bool authorities_canonical(const lx_stream_record *record)
{
    size_t i;
    if (record->meter_authority_count > LX_STREAM_MAX_METER_AUTHORITIES)
        return false;
    for (i = 0U; i < record->meter_authority_count; ++i) {
        if (lxp_ct_is_zero(record->meter_authorities[i], 32U)) return false;
        if (i != 0U && memcmp(record->meter_authorities[i - 1U],
                              record->meter_authorities[i], 32U) >= 0)
            return false;
    }
    for (i = record->meter_authority_count;
         i < LX_STREAM_MAX_METER_AUTHORITIES; ++i)
        if (!lxp_ct_is_zero(record->meter_authorities[i], 32U)) return false;
    return true;
}

lxp_result lx_stream_record_validate(const lx_stream_record *record)
{
    if (record == NULL || lxp_ct_is_zero(record->stream_id, 32U) ||
        lxp_ct_is_zero(record->payer, 32U) ||
        lxp_ct_is_zero(record->stream_account, 32U) ||
        lxp_ct_is_zero(record->recipient, 32U) ||
        lxp_ct_is_zero(record->asset_id, 32U) ||
        memcmp(record->stream_account, record->payer, 32U) == 0 ||
        memcmp(record->stream_account, record->recipient, 32U) == 0 ||
        record->mode < LX_STREAM_MODE_TIME ||
        record->mode > LX_STREAM_MODE_METERED ||
        lxp_u128_is_zero(record->rate) || record->rate_unit == 0U ||
        record->start_timestamp == 0U ||
        record->last_accrual_timestamp < record->start_timestamp ||
        (record->end_timestamp != 0U &&
         record->end_timestamp <= record->start_timestamp) ||
        lxp_u128_is_zero(record->total_cap) ||
        lxp_u128_cmp(record->accrued_total, record->total_cap) > 0 ||
        lxp_u128_cmp(record->settled_total, record->accrued_total) > 0 ||
        lxp_u128_cmp(record->remainder_carry,
                     (lxp_u128){ 0U, record->rate_unit }) >= 0 ||
        !authorities_canonical(record) ||
        (record->mode == LX_STREAM_MODE_METERED &&
         record->meter_authority_count == 0U) ||
        (record->mode == LX_STREAM_MODE_TIME &&
         record->meter_authority_count != 0U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_record_encode(const lx_stream_record *record,
                                   uint8_t bytes[LX_STREAM_RECORD_BYTES])
{
    size_t offset = 0U;
    lxp_result status = lx_stream_record_validate(record);
    if (status != LXP_OK || bytes == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_STREAM_RECORD_BYTES);
    (void)memcpy(bytes + offset, record->stream_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, record->payer, 32U); offset += 32U;
    (void)memcpy(bytes + offset, record->stream_account, 32U); offset += 32U;
    (void)memcpy(bytes + offset, record->recipient, 32U); offset += 32U;
    (void)memcpy(bytes + offset, record->asset_id, 32U); offset += 32U;
    bytes[offset++] = (uint8_t)record->mode;
    status = lxp_u128_to_be(record->rate, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    put_u64(bytes + offset, record->rate_unit); offset += 8U;
    put_u64(bytes + offset, record->start_timestamp); offset += 8U;
    put_u64(bytes + offset, record->last_accrual_timestamp); offset += 8U;
    put_u64(bytes + offset, record->end_timestamp); offset += 8U;
    status = lxp_u128_to_be(record->total_cap, bytes + offset);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->accrued_total, bytes + offset + 16U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->settled_total, bytes + offset + 32U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->remainder_carry, bytes + offset + 48U);
    if (status != LXP_OK) return status;
    offset += 64U;
    put_u64(bytes + offset, record->cumulative_meter); offset += 8U;
    bytes[offset++] = (uint8_t)record->meter_authority_count;
    (void)memcpy(bytes + offset, record->meter_authorities,
                 (size_t)LX_STREAM_MAX_METER_AUTHORITIES * 32U);
    offset += (size_t)LX_STREAM_MAX_METER_AUTHORITIES * 32U;
    bytes[offset++] = record->underfunded ? 1U : 0U;
    bytes[offset++] = record->paused ? 1U : 0U;
    bytes[offset++] = record->closed ? 1U : 0U;
    return offset == LX_STREAM_RECORD_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lx_stream_record_decode(const uint8_t *bytes, size_t length,
                                   lx_stream_record *record)
{
    size_t offset = 0U;
    uint8_t mode;
    lxp_result status;
    if (bytes == NULL || record == NULL || length != LX_STREAM_RECORD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->stream_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(record->payer, bytes + offset, 32U); offset += 32U;
    (void)memcpy(record->stream_account, bytes + offset, 32U); offset += 32U;
    (void)memcpy(record->recipient, bytes + offset, 32U); offset += 32U;
    (void)memcpy(record->asset_id, bytes + offset, 32U); offset += 32U;
    mode = bytes[offset++];
    if (mode != (uint8_t)LX_STREAM_MODE_TIME &&
        mode != (uint8_t)LX_STREAM_MODE_METERED)
        return LXP_ERR_NON_CANONICAL;
    record->mode = mode == (uint8_t)LX_STREAM_MODE_TIME ?
        LX_STREAM_MODE_TIME : LX_STREAM_MODE_METERED;
    status = lxp_u128_from_be(bytes + offset, &record->rate);
    if (status != LXP_OK) return status;
    offset += 16U;
    record->rate_unit = get_u64(bytes + offset); offset += 8U;
    record->start_timestamp = get_u64(bytes + offset); offset += 8U;
    record->last_accrual_timestamp = get_u64(bytes + offset); offset += 8U;
    record->end_timestamp = get_u64(bytes + offset); offset += 8U;
    status = lxp_u128_from_be(bytes + offset, &record->total_cap);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset + 16U, &record->accrued_total);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset + 32U, &record->settled_total);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset + 48U,
                                  &record->remainder_carry);
    if (status != LXP_OK) return status;
    offset += 64U;
    record->cumulative_meter = get_u64(bytes + offset); offset += 8U;
    record->meter_authority_count = bytes[offset++];
    if (record->meter_authority_count > LX_STREAM_MAX_METER_AUTHORITIES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(record->meter_authorities, bytes + offset,
                 (size_t)LX_STREAM_MAX_METER_AUTHORITIES * 32U);
    offset += (size_t)LX_STREAM_MAX_METER_AUTHORITIES * 32U;
    if (bytes[offset] > 1U || bytes[offset + 1U] > 1U ||
        bytes[offset + 2U] > 1U)
        return LXP_ERR_NON_CANONICAL;
    record->underfunded = bytes[offset++] != 0U;
    record->paused = bytes[offset++] != 0U;
    record->closed = bytes[offset++] != 0U;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    return lx_stream_record_validate(record);
}

lxp_result lx_stream_transfer_source(lxp_transfer_source_authority *source,
                                     const lx_account *account,
                                     lxp_authorization_kind kind)
{
    if (source == NULL || account == NULL ||
        (kind != LXP_AUTH_OWNER && kind != LXP_AUTH_PROTOCOL_MODULE))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(source, 0, sizeof(*source));
    (void)memcpy(source->authorized_from, account->id, 32U);
    source->debit_authority_kind = kind;
    return LXP_OK;
}
