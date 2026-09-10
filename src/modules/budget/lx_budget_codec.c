#include "layerx/lx_budget.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static void budget_u64_write(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static uint64_t budget_u64_read(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

lxp_result lx_budget_state_key(const uint8_t budget_id[32],
                               uint8_t key[LX_BUDGET_STATE_KEY_BYTES])
{
    if (budget_id == NULL || key == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, "budget:", 7U);
    (void)memcpy(key + 7U, budget_id, 32U);
    return LXP_OK;
}

lxp_result lx_budget_record_encode(const lx_budget_record *record,
                                   uint8_t *bytes, size_t capacity,
                                   size_t *length)
{
    size_t encoded;
    size_t i;
    lxp_result status = lx_budget_record_validate(record);
    if (status != LXP_OK) return status;
    if (bytes == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    encoded = (size_t)LX_BUDGET_RECORD_FIXED_BYTES + record->delegate_count * 32U;
    if (capacity < encoded) return LXP_ERR_LENGTH_LIMIT;
    (void)memset(bytes, 0, encoded);
    bytes[0] = 0U;
    bytes[1] = 1U;
    (void)memcpy(bytes + 2U, record->budget_id, 32U);
    (void)memcpy(bytes + 34U, record->owner, 32U);
    (void)memcpy(bytes + 66U, record->budget_account, 32U);
    (void)memcpy(bytes + 98U, record->asset_id, 32U);
    (void)memcpy(bytes + 130U, record->purpose_hash, 32U);
    status = lxp_u128_to_be(record->per_period_limit, bytes + 162U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->configured_period_limit, bytes + 178U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->carry_cap, bytes + 194U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->spent_this_period, bytes + 210U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->carried, bytes + 226U);
    if (status != LXP_OK) return status;
    budget_u64_write(bytes + 242U, record->period_length);
    budget_u64_write(bytes + 250U, record->period_start);
    budget_u64_write(bytes + 258U, record->expiry);
    budget_u64_write(bytes + 266U, record->revocation_sequence);
    bytes[274U] = (uint8_t)record->rollover_policy;
    bytes[275U] = record->closed ? 1U : 0U;
    bytes[276U] = record->revoked ? 1U : 0U;
    bytes[277U] = (uint8_t)record->delegate_count;
    for (i = 0U; i < record->delegate_count; ++i)
        (void)memcpy(bytes + LX_BUDGET_RECORD_FIXED_BYTES + i * 32U,
                     record->delegates[i], 32U);
    *length = encoded;
    return LXP_OK;
}

lxp_result lx_budget_record_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_record *record)
{
    lx_budget_record value;
    size_t count;
    size_t i;
    lxp_result status;
    if (bytes == NULL || record == NULL ||
        length < (size_t)LX_BUDGET_RECORD_FIXED_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U || bytes[275U] > 1U ||
        bytes[276U] > 1U)
        return LXP_ERR_NON_CANONICAL;
    count = bytes[277U];
    if (count > (size_t)LX_BUDGET_MAX_DELEGATES ||
        length != (size_t)LX_BUDGET_RECORD_FIXED_BYTES + count * 32U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    (void)memcpy(value.owner, bytes + 34U, 32U);
    (void)memcpy(value.budget_account, bytes + 66U, 32U);
    (void)memcpy(value.asset_id, bytes + 98U, 32U);
    (void)memcpy(value.purpose_hash, bytes + 130U, 32U);
    status = lxp_u128_from_be(bytes + 162U, &value.per_period_limit);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 178U, &value.configured_period_limit);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 194U, &value.carry_cap);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 210U, &value.spent_this_period);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 226U, &value.carried);
    if (status != LXP_OK) return status;
    value.period_length = budget_u64_read(bytes + 242U);
    value.period_start = budget_u64_read(bytes + 250U);
    value.expiry = budget_u64_read(bytes + 258U);
    value.revocation_sequence = budget_u64_read(bytes + 266U);
    value.rollover_policy = (lx_budget_rollover_policy)bytes[274U];
    value.closed = bytes[275U] != 0U;
    value.revoked = bytes[276U] != 0U;
    value.delegate_count = count;
    for (i = 0U; i < count; ++i) {
        const uint8_t *entry = bytes + LX_BUDGET_RECORD_FIXED_BYTES + i * 32U;
        if (i != 0U && memcmp(entry - 32U, entry, 32U) >= 0)
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(value.delegates[i], entry, 32U);
    }
    status = lx_budget_record_validate(&value);
    if (status != LXP_OK) return status;
    *record = value;
    return LXP_OK;
}

lxp_result lx_budget_create_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_create_payload *payload)
{
    lx_budget_create_payload value;
    lxp_result status;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_CREATE_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    (void)memcpy(value.budget_account, bytes + 34U, 32U);
    (void)memcpy(value.asset_id, bytes + 66U, 32U);
    (void)memcpy(value.purpose_hash, bytes + 98U, 32U);
    status = lxp_u128_from_be(bytes + 130U, &value.per_period_limit);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 146U, &value.carry_cap);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 162U, &value.amount);
    if (status != LXP_OK) return status;
    value.period_length = budget_u64_read(bytes + 178U);
    value.period_start = budget_u64_read(bytes + 186U);
    value.expiry = budget_u64_read(bytes + 194U);
    value.revocation_sequence = budget_u64_read(bytes + 202U);
    value.rollover_policy = bytes[210U];
    if (lxp_ct_is_zero(value.budget_id, 32U) ||
        lxp_ct_is_zero(value.budget_account, 32U) ||
        lxp_ct_is_zero(value.asset_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(value.amount) ||
        lxp_u128_is_zero(value.per_period_limit))
        return LXP_ERR_INVALID_AMOUNT;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_budget_amount_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_amount_payload *payload)
{
    lx_budget_amount_payload value;
    lxp_result status;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_FUND_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    status = lxp_u128_from_be(bytes + 34U, &value.amount);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(value.budget_id, 32U)) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(value.amount)) return LXP_ERR_INVALID_AMOUNT;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_budget_amend_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_amend_payload *payload)
{
    lx_budget_amend_payload value;
    lxp_result status;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_AMEND_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    status = lxp_u128_from_be(bytes + 34U, &value.per_period_limit);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 50U, &value.carry_cap);
    if (status != LXP_OK) return status;
    value.expiry = budget_u64_read(bytes + 66U);
    value.rollover_policy = bytes[74U];
    if (lxp_ct_is_zero(value.budget_id, 32U)) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(value.per_period_limit))
        return LXP_ERR_INVALID_AMOUNT;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_budget_delegate_decode(const uint8_t *bytes, size_t length,
                                     lx_budget_delegate_payload *payload)
{
    lx_budget_delegate_payload value;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_DELEGATE_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    (void)memcpy(value.delegate, bytes + 34U, 32U);
    if (lxp_ct_is_zero(value.budget_id, 32U) ||
        lxp_ct_is_zero(value.delegate, 32U))
        return LXP_ERR_NON_CANONICAL;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_budget_spend_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_spend_payload *payload)
{
    lx_budget_spend_payload value;
    lxp_result status;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_SPEND_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    (void)memcpy(value.recipient, bytes + 34U, 32U);
    status = lxp_u128_from_be(bytes + 66U, &value.amount);
    if (status != LXP_OK) return status;
    if (lxp_ct_is_zero(value.budget_id, 32U) ||
        lxp_ct_is_zero(value.recipient, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_is_zero(value.amount)) return LXP_ERR_INVALID_AMOUNT;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_budget_close_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_close_payload *payload)
{
    lx_budget_close_payload value;
    if (bytes == NULL || payload == NULL ||
        length != (size_t)LX_BUDGET_CLOSE_PAYLOAD_BYTES ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.budget_id, bytes + 2U, 32U);
    value.revocation_sequence = budget_u64_read(bytes + 34U);
    if (lxp_ct_is_zero(value.budget_id, 32U) ||
        value.revocation_sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    *payload = value;
    return LXP_OK;
}
