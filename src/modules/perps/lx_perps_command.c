#include "layerx/lx_perps.h"

#include "lx_perps_codec.h"

#include "layerx/lx_oracle.h"
#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_result put_u128(uint8_t *bytes, lxp_u128 value)
{
    return lxp_u128_to_be(value, bytes);
}

static lxp_result get_u128(const uint8_t *bytes, lxp_u128 *value)
{
    return lxp_u128_from_be(bytes, value);
}

lxp_result lx_perps_halt_command_encode(
    const lx_perps_halt_command *command,
    uint8_t bytes[LX_PERPS_HALT_PAYLOAD_BYTES])
{
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    bytes[32] = command->halted ? 1U : 0U;
    return LXP_OK;
}

lxp_result lx_perps_halt_command_decode(const uint8_t *bytes, size_t length,
                                        lx_perps_halt_command *command)
{
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_HALT_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    if (bytes[32] > 1U || lxp_ct_is_zero(command->market_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    command->halted = bytes[32] != 0U;
    return LXP_OK;
}

static lxp_result oracle_observation_from(
    const lx_perps_oracle_command *command,
    lx_oracle_observation *observation)
{
    if (command == NULL || observation == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(observation, 0, sizeof(*observation));
    (void)memcpy(observation->market_id, command->market_id, 32U);
    observation->observation_sequence = command->observation_sequence;
    observation->price = command->price;
    observation->observed_at = command->observed_at;
    observation->source_identifier = command->source_identifier;
    (void)memcpy(observation->oracle_public_key, command->oracle_public_key,
                 32U);
    (void)memcpy(observation->signature, command->signature, 64U);
    return LXP_OK;
}

lxp_result lx_perps_oracle_command_encode(
    const lx_perps_oracle_command *command,
    uint8_t bytes[LX_PERPS_ORACLE_PAYLOAD_BYTES])
{
    lx_oracle_observation observation;
    size_t length = 0U;
    lxp_result status = oracle_observation_from(command, &observation);
    if (status != LXP_OK || bytes == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_oracle_observation_encode(&observation, bytes,
                                          LX_PERPS_ORACLE_PAYLOAD_BYTES,
                                          &length);
    if (status != LXP_OK) return status;
    return length == LX_PERPS_ORACLE_PAYLOAD_BYTES ? LXP_OK :
                                                     LXP_FATAL_INVARIANT;
}

lxp_result lx_perps_oracle_command_decode(const uint8_t *bytes, size_t length,
                                          lx_perps_oracle_command *command)
{
    uint8_t canonical[LX_PERPS_ORACLE_PAYLOAD_BYTES];
    lx_oracle_observation observation;
    size_t canonical_length = 0U;
    lxp_result status;
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_ORACLE_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    command->observation_sequence = lx_perps_get_u64(bytes + 32U);
    status = get_u128(bytes + 40U, &command->price);
    if (status != LXP_OK) return status;
    command->observed_at = lx_perps_get_u64(bytes + 56U);
    command->source_identifier = lx_perps_get_u64(bytes + 64U);
    (void)memset(&observation, 0, sizeof(observation));
    (void)memcpy(observation.market_id, command->market_id, 32U);
    observation.observation_sequence = command->observation_sequence;
    observation.price = command->price;
    observation.observed_at = command->observed_at;
    observation.source_identifier = command->source_identifier;
    status = lx_oracle_observation_encode(&observation, canonical,
                                          sizeof(canonical),
                                          &canonical_length);
    if (status != LXP_OK) return status;
    return canonical_length == length &&
           memcmp(canonical, bytes, length) == 0 ? LXP_OK :
                                                   LXP_ERR_NON_CANONICAL;
}

lxp_result lx_perps_oracle_command_sign(lx_perps_oracle_command *command,
                                        const uint8_t private_key[32])
{
    lx_oracle_observation observation;
    lxp_result status = oracle_observation_from(command, &observation);
    if (status != LXP_OK || private_key == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    status = lx_oracle_observation_sign(&observation, private_key);
    if (status != LXP_OK) return status;
    (void)memcpy(command->oracle_public_key, observation.oracle_public_key,
                 32U);
    (void)memcpy(command->signature, observation.signature, 64U);
    return LXP_OK;
}

lxp_result lx_perps_order_command_encode(
    const lx_perps_order_command *command,
    uint8_t bytes[LX_PERPS_ORDER_PAYLOAD_BYTES])
{
    lxp_result status;
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->order_id, 32U) ||
        lxp_ct_is_zero(command->owner_account_id, 32U) ||
        lxp_u128_is_zero(command->price) ||
        lxp_u128_is_zero(command->quantity))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->order_id, 32U);
    (void)memcpy(bytes + 64U, command->owner_account_id, 32U);
    status = lx_perps_put_side(bytes + 96U, (int)command->side);
    if (status != LXP_OK) return status;
    status = put_u128(bytes + 97U, command->price);
    if (status != LXP_OK) return status;
    return put_u128(bytes + 113U, command->quantity);
}

lxp_result lx_perps_order_command_decode(const uint8_t *bytes, size_t length,
                                         lx_perps_order_command *command)
{
    lxp_result status;
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_ORDER_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->order_id, bytes + 32U, 32U);
    (void)memcpy(command->owner_account_id, bytes + 64U, 32U);
    if (!lx_perps_side_valid(bytes[96])) return LXP_ERR_NON_CANONICAL;
    command->side = bytes[96] == 1U ? LX_PERPS_SIDE_BUY : LX_PERPS_SIDE_SELL;
    status = get_u128(bytes + 97U, &command->price);
    if (status != LXP_OK) return status;
    status = get_u128(bytes + 113U, &command->quantity);
    if (status != LXP_OK) return status;
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->order_id, 32U) ||
           lxp_ct_is_zero(command->owner_account_id, 32U) ||
           lxp_u128_is_zero(command->price) ||
           lxp_u128_is_zero(command->quantity) ? LXP_ERR_NON_CANONICAL :
                                                 LXP_OK;
}

lxp_result lx_perps_cancel_command_encode(
    const lx_perps_cancel_command *command,
    uint8_t bytes[LX_PERPS_CANCEL_PAYLOAD_BYTES])
{
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->order_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->order_id, 32U);
    return LXP_OK;
}

lxp_result lx_perps_cancel_command_decode(const uint8_t *bytes, size_t length,
                                          lx_perps_cancel_command *command)
{
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_CANCEL_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->order_id, bytes + 32U, 32U);
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->order_id, 32U) ? LXP_ERR_NON_CANONICAL :
                                                    LXP_OK;
}

lxp_result lx_perps_open_command_encode(
    const lx_perps_open_command *command,
    uint8_t bytes[LX_PERPS_OPEN_PAYLOAD_BYTES])
{
    lxp_result status;
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->position_id, 32U) ||
        lxp_ct_is_zero(command->margin_account_id, 32U) ||
        lxp_u128_is_zero(command->size) ||
        lxp_u128_is_zero(command->entry_notional) ||
        lxp_u128_is_zero(command->margin_amount))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->position_id, 32U);
    (void)memcpy(bytes + 64U, command->margin_account_id, 32U);
    status = lx_perps_put_side(bytes + 96U, (int)command->side);
    if (status != LXP_OK) return status;
    status = put_u128(bytes + 97U, command->size);
    if (status != LXP_OK) return status;
    status = put_u128(bytes + 113U, command->entry_notional);
    if (status != LXP_OK) return status;
    return put_u128(bytes + 129U, command->margin_amount);
}

lxp_result lx_perps_open_command_decode(const uint8_t *bytes, size_t length,
                                        lx_perps_open_command *command)
{
    lxp_result status;
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_OPEN_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->position_id, bytes + 32U, 32U);
    (void)memcpy(command->margin_account_id, bytes + 64U, 32U);
    if (!lx_perps_side_valid(bytes[96])) return LXP_ERR_NON_CANONICAL;
    command->side = bytes[96] == 1U ? LX_PERPS_SIDE_BUY : LX_PERPS_SIDE_SELL;
    status = get_u128(bytes + 97U, &command->size);
    if (status != LXP_OK) return status;
    status = get_u128(bytes + 113U, &command->entry_notional);
    if (status != LXP_OK) return status;
    status = get_u128(bytes + 129U, &command->margin_amount);
    if (status != LXP_OK) return status;
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->position_id, 32U) ||
           lxp_ct_is_zero(command->margin_account_id, 32U) ||
           lxp_u128_is_zero(command->size) ||
           lxp_u128_is_zero(command->entry_notional) ||
           lxp_u128_is_zero(command->margin_amount) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_increase_command_encode(
    const lx_perps_increase_command *command,
    uint8_t bytes[LX_PERPS_INCREASE_PAYLOAD_BYTES])
{
    lxp_result status;
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->position_id, 32U) ||
        lxp_u128_is_zero(command->size_delta) ||
        lxp_u128_is_zero(command->notional_delta) ||
        lxp_u128_is_zero(command->margin_amount))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->position_id, 32U);
    status = put_u128(bytes + 64U, command->size_delta);
    if (status != LXP_OK) return status;
    status = put_u128(bytes + 80U, command->notional_delta);
    if (status != LXP_OK) return status;
    return put_u128(bytes + 96U, command->margin_amount);
}

lxp_result lx_perps_increase_command_decode(
    const uint8_t *bytes, size_t length, lx_perps_increase_command *command)
{
    lxp_result status;
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_INCREASE_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->position_id, bytes + 32U, 32U);
    status = get_u128(bytes + 64U, &command->size_delta);
    if (status != LXP_OK) return status;
    status = get_u128(bytes + 80U, &command->notional_delta);
    if (status != LXP_OK) return status;
    status = get_u128(bytes + 96U, &command->margin_amount);
    if (status != LXP_OK) return status;
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->position_id, 32U) ||
           lxp_u128_is_zero(command->size_delta) ||
           lxp_u128_is_zero(command->notional_delta) ||
           lxp_u128_is_zero(command->margin_amount) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_close_command_encode(
    const lx_perps_close_command *command,
    uint8_t bytes[LX_PERPS_CLOSE_PAYLOAD_BYTES])
{
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->position_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->position_id, 32U);
    return LXP_OK;
}

lxp_result lx_perps_close_command_decode(const uint8_t *bytes, size_t length,
                                         lx_perps_close_command *command)
{
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_CLOSE_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->position_id, bytes + 32U, 32U);
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->position_id, 32U) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_tick_command_encode(
    const lx_perps_tick_command *command,
    uint8_t bytes[LX_PERPS_TICK_PAYLOAD_BYTES])
{
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    return LXP_OK;
}

lxp_result lx_perps_tick_command_decode(const uint8_t *bytes, size_t length,
                                        lx_perps_tick_command *command)
{
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_TICK_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    return lxp_ct_is_zero(command->market_id, 32U) ? LXP_ERR_NON_CANONICAL :
                                                     LXP_OK;
}

lxp_result lx_perps_liquidate_command_encode(
    const lx_perps_liquidate_command *command,
    uint8_t bytes[LX_PERPS_LIQUIDATE_PAYLOAD_BYTES])
{
    if (command == NULL || bytes == NULL ||
        lxp_ct_is_zero(command->market_id, 32U) ||
        lxp_ct_is_zero(command->position_id, 32U) ||
        lxp_ct_is_zero(command->liquidator_account_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(bytes, command->market_id, 32U);
    (void)memcpy(bytes + 32U, command->position_id, 32U);
    (void)memcpy(bytes + 64U, command->liquidator_account_id, 32U);
    return LXP_OK;
}

lxp_result lx_perps_liquidate_command_decode(
    const uint8_t *bytes, size_t length, lx_perps_liquidate_command *command)
{
    if (bytes == NULL || command == NULL ||
        length != LX_PERPS_LIQUIDATE_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    (void)memcpy(command->position_id, bytes + 32U, 32U);
    (void)memcpy(command->liquidator_account_id, bytes + 64U, 32U);
    return lxp_ct_is_zero(command->market_id, 32U) ||
           lxp_ct_is_zero(command->position_id, 32U) ||
           lxp_ct_is_zero(command->liquidator_account_id, 32U) ?
        LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result lx_perps_adl_command_encode(const lx_perps_adl_command *command,
                                       uint8_t *bytes, size_t capacity,
                                       size_t *length)
{
    size_t required;
    size_t i;
    if (command == NULL || bytes == NULL || length == NULL ||
        command->position_count == 0U ||
        command->position_count > LX_PERPS_ADL_CAPACITY ||
        lxp_ct_is_zero(command->market_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    required = LX_PERPS_ADL_PAYLOAD_MIN_BYTES + command->position_count * 32U;
    if (capacity < required) return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < command->position_count; ++i) {
        if (lxp_ct_is_zero(command->position_ids[i], 32U))
            return LXP_ERR_NON_CANONICAL;
        if (i != 0U && memcmp(command->position_ids[i - 1U],
                              command->position_ids[i], 32U) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    (void)memcpy(bytes, command->market_id, 32U);
    bytes[32] = (uint8_t)command->position_count;
    for (i = 0U; i < command->position_count; ++i)
        (void)memcpy(bytes + 33U + i * 32U, command->position_ids[i], 32U);
    *length = required;
    return LXP_OK;
}

lxp_result lx_perps_adl_command_decode(const uint8_t *bytes, size_t length,
                                       lx_perps_adl_command *command)
{
    size_t count;
    size_t i;
    if (bytes == NULL || command == NULL ||
        length < LX_PERPS_ADL_PAYLOAD_MIN_BYTES + 32U ||
        length > LX_PERPS_ADL_PAYLOAD_MAX_BYTES)
        return LXP_ERR_NON_CANONICAL;
    count = bytes[32];
    if (count == 0U || count > LX_PERPS_ADL_CAPACITY ||
        length != LX_PERPS_ADL_PAYLOAD_MIN_BYTES + count * 32U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(command, 0, sizeof(*command));
    (void)memcpy(command->market_id, bytes, 32U);
    if (lxp_ct_is_zero(command->market_id, 32U)) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < count; ++i) {
        (void)memcpy(command->position_ids[i], bytes + 33U + i * 32U, 32U);
        if (lxp_ct_is_zero(command->position_ids[i], 32U))
            return LXP_ERR_NON_CANONICAL;
        if (i != 0U && memcmp(command->position_ids[i - 1U],
                              command->position_ids[i], 32U) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    command->position_count = count;
    return LXP_OK;
}
