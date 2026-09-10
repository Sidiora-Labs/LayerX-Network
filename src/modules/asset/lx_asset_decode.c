#include "layerx/lx_asset.h"

#include <string.h>

static bool utf8_valid(const uint8_t *bytes, size_t length)
{
    size_t cursor = 0U;
    while (cursor < length) {
        uint32_t code = bytes[cursor++];
        uint32_t minimum;
        size_t continuation;
        if (code < 0x80U) continue;
        if (code >= 0xc2U && code <= 0xdfU) {
            code &= 0x1fU;
            minimum = 0x80U;
            continuation = 1U;
        } else if (code >= 0xe0U && code <= 0xefU) {
            code &= 0x0fU;
            minimum = 0x800U;
            continuation = 2U;
        } else if (code >= 0xf0U && code <= 0xf4U) {
            code &= 0x07U;
            minimum = 0x10000U;
            continuation = 3U;
        } else return false;
        if (continuation > length - cursor) return false;
        while (continuation-- != 0U) {
            uint8_t next = bytes[cursor++];
            if ((next & 0xc0U) != 0x80U) return false;
            code = (code << 6U) | (next & 0x3fU);
        }
        if (code < minimum || code > 0x10ffffU ||
            (code >= 0xd800U && code <= 0xdfffU)) return false;
    }
    return true;
}

lxp_result lx_asset_register_decode(const uint8_t *bytes, size_t length,
                                    lx_asset_register_payload *payload)
{
    lx_asset_register_payload value;
    size_t cursor = 66U;
    size_t index;
    if (bytes == NULL || payload == NULL || length < 89U ||
        bytes[0] != 0U || bytes[1] != 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&value, 0, sizeof(value));
    (void)memcpy(value.asset_id, bytes + 2U, 32U);
    (void)memcpy(value.salt, bytes + 34U, 32U);
    value.symbol_length = bytes[cursor++];
    if (value.symbol_length == 0U || value.symbol_length > LX_ASSET_SYMBOL_MAX ||
        value.symbol_length > length - cursor) return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < value.symbol_length; ++index)
        if (bytes[cursor + index] > 0x7fU) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value.symbol, bytes + cursor, value.symbol_length);
    cursor += value.symbol_length;
    if (cursor == length) return LXP_ERR_NON_CANONICAL;
    value.name_length = bytes[cursor++];
    if (value.name_length == 0U || value.name_length > LX_ASSET_NAME_MAX ||
        value.name_length > length - cursor ||
        !utf8_valid(bytes + cursor, value.name_length))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value.name, bytes + cursor, value.name_length);
    cursor += value.name_length;
    if (length - cursor < 19U) return LXP_ERR_NON_CANONICAL;
    value.decimals = bytes[cursor++];
    if (value.decimals > 38U) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_from_be(bytes + cursor, &value.supply_cap) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    cursor += 16U;
    value.issuer_kind = bytes[cursor++];
    value.custody_reference_length = bytes[cursor++];
    if ((value.issuer_kind != 1U && value.issuer_kind != 2U) ||
        value.custody_reference_length > LX_ASSET_CUSTODY_REFERENCE_MAX ||
        value.custody_reference_length != length - cursor ||
        (value.issuer_kind == 1U && value.custody_reference_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value.custody_reference, bytes + cursor,
                 value.custody_reference_length);
    *payload = value;
    return LXP_OK;
}

lxp_result lx_asset_account_open_decode(const uint8_t *bytes, size_t length,
                                        lx_asset_account_open_payload *payload)
{
    if (bytes == NULL || payload == NULL || length != 34U ||
        bytes[0] != 0U || bytes[1] != 1U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload->asset_id, bytes + 2U, 32U);
    return LXP_OK;
}

lxp_result lx_asset_supply_decode(const uint8_t *bytes, size_t length,
                                  lx_asset_supply_payload *payload)
{
    lx_asset_supply_payload value;
    if (bytes == NULL || payload == NULL || length != 82U ||
        bytes[0] != 0U || bytes[1] != 1U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value.asset_id, bytes + 2U, 32U);
    (void)memcpy(value.account_id, bytes + 34U, 32U);
    if (lxp_u128_from_be(bytes + 66U, &value.amount) != LXP_OK ||
        lxp_u128_is_zero(value.amount)) return LXP_ERR_INVALID_AMOUNT;
    *payload = value;
    return LXP_OK;
}

lxp_result lx_asset_grant_revoke_decode(const uint8_t *bytes, size_t length,
                                        lx_asset_grant_revoke_payload *payload)
{
    size_t index;
    if (bytes == NULL || payload == NULL || length != 42U ||
        bytes[0] != 0U || bytes[1] != 1U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(payload->grant_id, bytes + 2U, 32U);
    payload->revocation_sequence = 0U;
    for (index = 34U; index < 42U; ++index)
        payload->revocation_sequence = (payload->revocation_sequence << 8U) |
                                       bytes[index];
    return LXP_OK;
}
