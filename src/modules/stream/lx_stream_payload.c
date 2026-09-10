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

static lxp_result version_write(uint8_t *bytes, size_t capacity,
                                size_t required)
{
    if (bytes == NULL || capacity < required) return LXP_ERR_LENGTH_LIMIT;
    bytes[0] = 0U;
    bytes[1] = (uint8_t)LX_STREAM_PAYLOAD_VERSION;
    return LXP_OK;
}

static lxp_result version_check(const uint8_t *bytes, size_t length,
                                size_t expected)
{
    if (bytes == NULL || length != expected) return LXP_ERR_NON_CANONICAL;
    return bytes[0] == 0U && bytes[1] == (uint8_t)LX_STREAM_PAYLOAD_VERSION ?
        LXP_OK : LXP_ERR_VERSION_UNSUPPORTED;
}

lxp_result lx_stream_open_encode(const lx_stream_open_payload *payload,
                                 uint8_t *bytes, size_t capacity,
                                 size_t *length)
{
    size_t offset = 0U;
    size_t required;
    lxp_result status;
    if (payload == NULL || length == NULL ||
        payload->record.meter_authority_count >
            LX_STREAM_MAX_METER_AUTHORITIES)
        return LXP_ERR_NON_CANONICAL;
    required = (size_t)LX_STREAM_OPEN_PAYLOAD_FIXED +
               payload->record.meter_authority_count * 32U;
    status = version_write(bytes, capacity, required);
    if (status != LXP_OK) return status;
    offset = 2U;
    (void)memcpy(bytes + offset, payload->record.stream_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, payload->record.stream_account, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, payload->record.recipient, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, payload->record.asset_id, 32U);
    offset += 32U;
    bytes[offset++] = (uint8_t)payload->record.mode;
    status = lxp_u128_to_be(payload->record.rate, bytes + offset);
    if (status != LXP_OK) return status;
    offset += 16U;
    put_u64(bytes + offset, payload->record.rate_unit); offset += 8U;
    put_u64(bytes + offset, payload->record.start_timestamp); offset += 8U;
    put_u64(bytes + offset, payload->record.end_timestamp); offset += 8U;
    status = lxp_u128_to_be(payload->record.total_cap, bytes + offset);
    if (status == LXP_OK)
        status = lxp_u128_to_be(payload->initial_funding, bytes + offset + 16U);
    if (status != LXP_OK) return status;
    offset += 32U;
    bytes[offset++] = (uint8_t)payload->record.meter_authority_count;
    (void)memcpy(bytes + offset, payload->record.meter_authorities,
                 payload->record.meter_authority_count * 32U);
    offset += payload->record.meter_authority_count * 32U;
    *length = offset;
    return offset == required ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lx_stream_open_decode(const uint8_t *bytes, size_t length,
                                 lx_stream_open_payload *payload)
{
    size_t offset = 2U;
    size_t i;
    uint8_t mode;
    uint8_t count;
    lxp_result status;
    if (bytes == NULL || payload == NULL ||
        length < (size_t)LX_STREAM_OPEN_PAYLOAD_FIXED ||
        length > (size_t)LX_STREAM_OPEN_PAYLOAD_MAX)
        return LXP_ERR_NON_CANONICAL;
    if (bytes[0] != 0U || bytes[1] != (uint8_t)LX_STREAM_PAYLOAD_VERSION)
        return LXP_ERR_VERSION_UNSUPPORTED;
    count = bytes[LX_STREAM_OPEN_PAYLOAD_FIXED - 1];
    if (count > LX_STREAM_MAX_METER_AUTHORITIES ||
        length != (size_t)LX_STREAM_OPEN_PAYLOAD_FIXED + (size_t)count * 32U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(payload, 0, sizeof(*payload));
    (void)memcpy(payload->record.stream_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(payload->record.stream_account, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(payload->record.recipient, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(payload->record.asset_id, bytes + offset, 32U);
    offset += 32U;
    mode = bytes[offset++];
    if (mode != (uint8_t)LX_STREAM_MODE_TIME &&
        mode != (uint8_t)LX_STREAM_MODE_METERED)
        return LXP_ERR_NON_CANONICAL;
    payload->record.mode = mode == (uint8_t)LX_STREAM_MODE_TIME ?
        LX_STREAM_MODE_TIME : LX_STREAM_MODE_METERED;
    status = lxp_u128_from_be(bytes + offset, &payload->record.rate);
    if (status != LXP_OK) return status;
    offset += 16U;
    payload->record.rate_unit = get_u64(bytes + offset); offset += 8U;
    payload->record.start_timestamp = get_u64(bytes + offset); offset += 8U;
    payload->record.end_timestamp = get_u64(bytes + offset); offset += 8U;
    status = lxp_u128_from_be(bytes + offset, &payload->record.total_cap);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset + 16U,
                                  &payload->initial_funding);
    if (status != LXP_OK) return status;
    offset += 32U;
    payload->record.meter_authority_count = bytes[offset++];
    for (i = 0U; i < count; ++i) {
        (void)memcpy(payload->record.meter_authorities[i], bytes + offset,
                     32U);
        offset += 32U;
    }
    payload->record.last_accrual_timestamp = payload->record.start_timestamp;
    if (offset != length) return LXP_ERR_TRAILING_BYTES;
    if (lxp_ct_is_zero(payload->record.stream_id, 32U) ||
        lxp_ct_is_zero(payload->record.stream_account, 32U) ||
        lxp_ct_is_zero(payload->record.recipient, 32U) ||
        lxp_ct_is_zero(payload->record.asset_id, 32U) ||
        memcmp(payload->record.stream_account, payload->record.recipient,
               32U) == 0 ||
        lxp_u128_is_zero(payload->record.rate) ||
        payload->record.rate_unit == 0U ||
        payload->record.start_timestamp == 0U ||
        (payload->record.end_timestamp != 0U &&
         payload->record.end_timestamp <= payload->record.start_timestamp) ||
        lxp_u128_is_zero(payload->record.total_cap) ||
        lxp_u128_is_zero(payload->initial_funding) ||
        (payload->record.mode == LX_STREAM_MODE_METERED && count == 0U) ||
        (payload->record.mode == LX_STREAM_MODE_TIME && count != 0U))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < count; ++i) {
        if (lxp_ct_is_zero(payload->record.meter_authorities[i], 32U))
            return LXP_ERR_NON_CANONICAL;
        if (i != 0U && memcmp(payload->record.meter_authorities[i - 1U],
                              payload->record.meter_authorities[i], 32U) >= 0)
            return LXP_ERR_NON_CANONICAL;
    }
    return LXP_OK;
}

lxp_result lx_stream_amount_encode(const lx_stream_amount_payload *payload,
                                   uint8_t *bytes, size_t capacity,
                                   size_t *length)
{
    lxp_result status;
    if (payload == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_write(bytes, capacity,
                           (size_t)LX_STREAM_TOP_UP_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 2U, payload->stream_id, 32U);
    status = lxp_u128_to_be(payload->amount, bytes + 34U);
    if (status != LXP_OK) return status;
    *length = (size_t)LX_STREAM_TOP_UP_PAYLOAD_BYTES;
    return LXP_OK;
}

lxp_result lx_stream_amount_decode(const uint8_t *bytes, size_t length,
                                   lx_stream_amount_payload *payload)
{
    lxp_result status;
    if (payload == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_check(bytes, length,
                           (size_t)LX_STREAM_TOP_UP_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(payload, 0, sizeof(*payload));
    (void)memcpy(payload->stream_id, bytes + 2U, 32U);
    status = lxp_u128_from_be(bytes + 34U, &payload->amount);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(payload->stream_id, 32U) ||
        lxp_u128_is_zero(payload->amount))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_meter_encode(const lx_stream_meter_attestation *payload,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length)
{
    lxp_result status;
    if (payload == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_write(bytes, capacity,
                           (size_t)LX_STREAM_METER_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 2U, payload->stream_id, 32U);
    put_u64(bytes + 34U, payload->cumulative_reading);
    (void)memcpy(bytes + 42U, payload->authority_key, 32U);
    (void)memcpy(bytes + 74U, payload->signature, 64U);
    *length = (size_t)LX_STREAM_METER_PAYLOAD_BYTES;
    return LXP_OK;
}

lxp_result lx_stream_meter_decode(const uint8_t *bytes, size_t length,
                                  lx_stream_meter_attestation *payload)
{
    lxp_result status;
    if (payload == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_check(bytes, length,
                           (size_t)LX_STREAM_METER_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(payload, 0, sizeof(*payload));
    (void)memcpy(payload->stream_id, bytes + 2U, 32U);
    payload->cumulative_reading = get_u64(bytes + 34U);
    (void)memcpy(payload->authority_key, bytes + 42U, 32U);
    (void)memcpy(payload->signature, bytes + 74U, 64U);
    if (lxp_ct_is_zero(payload->stream_id, 32U) ||
        lxp_ct_is_zero(payload->authority_key, 32U) ||
        lxp_ct_is_zero(payload->signature, 64U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_keyed_encode(const lx_stream_keyed_payload *payload,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length)
{
    lxp_result status;
    if (payload == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_write(bytes, capacity,
                           (size_t)LX_STREAM_KEYED_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 2U, payload->stream_id, 32U);
    (void)memcpy(bytes + 34U, payload->idempotency_key, 32U);
    *length = (size_t)LX_STREAM_KEYED_PAYLOAD_BYTES;
    return LXP_OK;
}

lxp_result lx_stream_keyed_decode(const uint8_t *bytes, size_t length,
                                  lx_stream_keyed_payload *payload)
{
    lxp_result status;
    if (payload == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_check(bytes, length,
                           (size_t)LX_STREAM_KEYED_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(payload, 0, sizeof(*payload));
    (void)memcpy(payload->stream_id, bytes + 2U, 32U);
    (void)memcpy(payload->idempotency_key, bytes + 34U, 32U);
    if (lxp_ct_is_zero(payload->stream_id, 32U) ||
        lxp_ct_is_zero(payload->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_id_encode(const lx_stream_id_payload *payload,
                               uint8_t *bytes, size_t capacity,
                               size_t *length)
{
    lxp_result status;
    if (payload == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_write(bytes, capacity,
                           (size_t)LX_STREAM_ID_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 2U, payload->stream_id, 32U);
    *length = (size_t)LX_STREAM_ID_PAYLOAD_BYTES;
    return LXP_OK;
}

lxp_result lx_stream_id_decode(const uint8_t *bytes, size_t length,
                               lx_stream_id_payload *payload)
{
    lxp_result status;
    if (payload == NULL) return LXP_ERR_NON_CANONICAL;
    status = version_check(bytes, length,
                           (size_t)LX_STREAM_ID_PAYLOAD_BYTES);
    if (status != LXP_OK) return status;
    (void)memset(payload, 0, sizeof(*payload));
    (void)memcpy(payload->stream_id, bytes + 2U, 32U);
    return lxp_ct_is_zero(payload->stream_id, 32U) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}
