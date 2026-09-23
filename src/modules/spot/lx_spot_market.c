#include "layerx/lx_spot.h"

#include "lx_spot_codec.h"

#include "layerx/lxp_hash.h"

#include <string.h>

static const uint8_t escrow_domain[] = "LX:SPOT:ESCROW:v1";

static bool market_fields_valid(const lx_spot_market *market)
{
    return !lx_spot_zero_id(market->market_id) &&
           !lx_spot_zero_id(market->base_asset) &&
           !lx_spot_zero_id(market->quote_asset) &&
           !lx_spot_zero_id(market->administrator) &&
           memcmp(market->base_asset, market->quote_asset, 32U) != 0 &&
           !lxp_u128_is_zero(market->tick_size) &&
           !lxp_u128_is_zero(market->lot_size);
}

lxp_result lx_spot_escrow_id(const uint8_t market_id[32],
                             const uint8_t asset_id[32], uint8_t out[32])
{
    uint8_t material[sizeof(escrow_domain) - 1U + 64U];
    lxp_result status;
    if (market_id == NULL || asset_id == NULL || out == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(material, escrow_domain, sizeof(escrow_domain) - 1U);
    (void)memcpy(material + sizeof(escrow_domain) - 1U, market_id, 32U);
    (void)memcpy(material + sizeof(escrow_domain) - 1U + 32U, asset_id, 32U);
    status = lxp_hash_sha256(material, sizeof(material), out);
    if (status != LXP_OK) return status;
    return lx_spot_zero_id(out) ? LXP_FATAL_INVARIANT : LXP_OK;
}

lxp_result lx_spot_notional(lxp_u128 price, lxp_u128 quantity,
                            lxp_u128 *notional)
{
    lxp_u256 product;
    lxp_result status;
    if (notional == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul(price, quantity, &product);
    if (status != LXP_OK) return status;
    if (product.words[2] != 0U || product.words[3] != 0U)
        return LXP_ERR_OVERFLOW;
    notional->hi = product.words[1];
    notional->lo = product.words[0];
    return LXP_OK;
}

lxp_result lx_spot_market_encode(const lx_spot_market *market,
                                 uint8_t payload[LX_SPOT_MARKET_PAYLOAD_BYTES])
{
    lxp_result status;
    if (market == NULL || payload == NULL || !market_fields_valid(market))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload, market->market_id, 32U);
    (void)memcpy(payload + 32U, market->base_asset, 32U);
    (void)memcpy(payload + 64U, market->quote_asset, 32U);
    status = lxp_u128_to_be(market->tick_size, payload + 96U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(market->lot_size, payload + 112U);
    if (status != LXP_OK) return status;
    (void)memcpy(payload + 128U, market->administrator, 32U);
    return LXP_OK;
}

lxp_result lx_spot_market_decode(const uint8_t *payload, size_t length,
                                 lx_spot_market *market)
{
    lx_spot_market decoded;
    lxp_result status;
    if (payload == NULL || market == NULL ||
        length != LX_SPOT_MARKET_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded, 0, sizeof(decoded));
    (void)memcpy(decoded.market_id, payload, 32U);
    (void)memcpy(decoded.base_asset, payload + 32U, 32U);
    (void)memcpy(decoded.quote_asset, payload + 64U, 32U);
    status = lxp_u128_from_be(payload + 96U, &decoded.tick_size);
    if (status == LXP_OK)
        status = lxp_u128_from_be(payload + 112U, &decoded.lot_size);
    if (status != LXP_OK) return status;
    (void)memcpy(decoded.administrator, payload + 128U, 32U);
    if (!market_fields_valid(&decoded)) return LXP_ERR_NON_CANONICAL;
    status = lx_spot_escrow_id(decoded.market_id, decoded.base_asset,
                               decoded.base_escrow_id);
    if (status == LXP_OK)
        status = lx_spot_escrow_id(decoded.market_id, decoded.quote_asset,
                                   decoded.quote_escrow_id);
    if (status != LXP_OK) return status;
    *market = decoded;
    return LXP_OK;
}

static bool order_command_valid(const lx_spot_order_command *command)
{
    if (lx_spot_zero_id(command->market_id) ||
        lx_spot_zero_id(command->order_id) ||
        lx_spot_zero_id(command->base_account_id) ||
        lx_spot_zero_id(command->quote_account_id) ||
        memcmp(command->base_account_id, command->quote_account_id, 32U) ==
            0 ||
        !lx_spot_side_valid((uint8_t)command->side) ||
        lxp_u128_is_zero(command->quantity))
        return false;
    if (command->kind == LX_SPOT_ORDER_LIMIT)
        return !lxp_u128_is_zero(command->price) &&
               (command->time_in_force == LX_SPOT_TIF_GTC ||
                command->time_in_force == LX_SPOT_TIF_IOC);
    return command->kind == LX_SPOT_ORDER_MARKET &&
           lxp_u128_is_zero(command->price) &&
           command->time_in_force == LX_SPOT_TIF_IOC;
}

lxp_result lx_spot_order_command_encode(
    const lx_spot_order_command *command,
    uint8_t payload[LX_SPOT_ORDER_PAYLOAD_BYTES])
{
    lxp_result status;
    if (command == NULL || payload == NULL || !order_command_valid(command))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload, command->market_id, 32U);
    (void)memcpy(payload + 32U, command->order_id, 32U);
    (void)memcpy(payload + 64U, command->base_account_id, 32U);
    (void)memcpy(payload + 96U, command->quote_account_id, 32U);
    payload[128] = (uint8_t)command->side;
    payload[129] = (uint8_t)command->kind;
    payload[130] = (uint8_t)command->time_in_force;
    status = lxp_u128_to_be(command->price, payload + 131U);
    if (status != LXP_OK) return status;
    return lxp_u128_to_be(command->quantity, payload + 147U);
}

lxp_result lx_spot_order_command_decode(const uint8_t *payload, size_t length,
                                        lx_spot_order_command *command)
{
    lx_spot_order_command decoded;
    lxp_result status;
    if (payload == NULL || command == NULL ||
        length != LX_SPOT_ORDER_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&decoded, 0, sizeof(decoded));
    (void)memcpy(decoded.market_id, payload, 32U);
    (void)memcpy(decoded.order_id, payload + 32U, 32U);
    (void)memcpy(decoded.base_account_id, payload + 64U, 32U);
    (void)memcpy(decoded.quote_account_id, payload + 96U, 32U);
    decoded.side = (lx_spot_side)payload[128];
    decoded.kind = (lx_spot_order_kind)payload[129];
    decoded.time_in_force = (lx_spot_time_in_force)payload[130];
    status = lxp_u128_from_be(payload + 131U, &decoded.price);
    if (status == LXP_OK)
        status = lxp_u128_from_be(payload + 147U, &decoded.quantity);
    if (status != LXP_OK) return status;
    if (!order_command_valid(&decoded)) return LXP_ERR_NON_CANONICAL;
    *command = decoded;
    return LXP_OK;
}

lxp_result lx_spot_cancel_command_encode(
    const lx_spot_cancel_command *command,
    uint8_t payload[LX_SPOT_CANCEL_PAYLOAD_BYTES])
{
    if (command == NULL || payload == NULL ||
        lx_spot_zero_id(command->market_id) ||
        lx_spot_zero_id(command->order_id))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload, command->market_id, 32U);
    (void)memcpy(payload + 32U, command->order_id, 32U);
    return LXP_OK;
}

lxp_result lx_spot_cancel_command_decode(const uint8_t *payload, size_t length,
                                         lx_spot_cancel_command *command)
{
    lx_spot_cancel_command decoded;
    if (payload == NULL || command == NULL ||
        length != LX_SPOT_CANCEL_PAYLOAD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(decoded.market_id, payload, 32U);
    (void)memcpy(decoded.order_id, payload + 32U, 32U);
    if (lx_spot_zero_id(decoded.market_id) ||
        lx_spot_zero_id(decoded.order_id))
        return LXP_ERR_NON_CANONICAL;
    *command = decoded;
    return LXP_OK;
}

lxp_result lx_spot_market_command_encode(
    const lx_spot_market_command *command,
    uint8_t payload[LX_SPOT_MARKET_ID_PAYLOAD_BYTES])
{
    if (command == NULL || payload == NULL ||
        lx_spot_zero_id(command->market_id))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload, command->market_id, 32U);
    return LXP_OK;
}

lxp_result lx_spot_market_command_decode(const uint8_t *payload,
                                         size_t length,
                                         lx_spot_market_command *command)
{
    if (payload == NULL || command == NULL ||
        length != LX_SPOT_MARKET_ID_PAYLOAD_BYTES ||
        lx_spot_zero_id(payload))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(command->market_id, payload, 32U);
    return LXP_OK;
}
