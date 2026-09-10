#include "layerx/lx_perps.h"

#include "lx_perps_codec.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t market_prefix[] = "market:";

typedef struct market_iter_adapter {
    lx_perps_market_visit_fn visit;
    void *user;
} market_iter_adapter;

static bool keys_canonical(const lx_perps_market *market)
{
    size_t i;
    if (market->permitted_oracle_key_count == 0U ||
        market->permitted_oracle_key_count > LX_PERPS_MAX_ORACLE_KEYS)
        return false;
    for (i = 0U; i < market->permitted_oracle_key_count; ++i) {
        if (lxp_ct_is_zero(market->permitted_oracle_keys[i], 32U))
            return false;
        if (i != 0U && memcmp(market->permitted_oracle_keys[i - 1U],
                              market->permitted_oracle_keys[i], 32U) >= 0)
            return false;
    }
    return true;
}

static bool accounts_canonical(const lx_perps_market *market)
{
    const uint8_t *accounts[4];
    size_t i;
    size_t j;
    accounts[0] = market->liquidity_account_id;
    accounts[1] = market->long_funding_account_id;
    accounts[2] = market->short_funding_account_id;
    accounts[3] = market->insurance_account_id;
    if (lxp_ct_is_zero(market->administrator, 32U)) return false;
    for (i = 0U; i < 4U; ++i) {
        if (lxp_ct_is_zero(accounts[i], 32U)) return false;
        for (j = 0U; j < i; ++j)
            if (memcmp(accounts[i], accounts[j], 32U) == 0) return false;
    }
    return true;
}

static lxp_result market_validate(const lx_perps_market *market)
{
    if (market == NULL || lxp_ct_is_zero(market->market_id, 32U) ||
        lxp_ct_is_zero(market->quote_asset, 32U) ||
        lxp_u128_is_zero(market->contract_size) ||
        lxp_u128_is_zero(market->tick_size) ||
        lxp_u128_is_zero(market->lot_size) ||
        lxp_u128_is_zero(market->price_scale) ||
        market->maintenance_margin_ratio_bps == 0U ||
        market->initial_margin_ratio_bps <=
            market->maintenance_margin_ratio_bps ||
        market->initial_margin_ratio_bps > LX_PERPS_MARGIN_RATIO_MAX_BPS ||
        market->liquidation_fee_bps > LXP_BASIS_POINTS_ONE ||
        market->liquidator_share_bps > LXP_BASIS_POINTS_ONE ||
        market->maximum_funding_rate_bps == 0U ||
        market->maximum_funding_rate_bps > LXP_BASIS_POINTS_ONE ||
        market->maximum_deviation_basis_points == 0U ||
        market->maximum_deviation_basis_points > LXP_BASIS_POINTS_ONE ||
        market->funding_interval_ms == 0U ||
        market->maximum_oracle_staleness_ms == 0U ||
        lxp_u128_is_zero(market->minimum_price) ||
        lxp_u128_cmp(market->minimum_price, market->maximum_price) >= 0 ||
        market->parameter_version == 0U || !accounts_canonical(market) ||
        !keys_canonical(market))
        return LXP_ERR_PARAMETER_BOUNDS;
    return LXP_OK;
}

static void market_key(const uint8_t market_id[32],
                       uint8_t key[LX_PERPS_MARKET_KEY_BYTES])
{
    (void)memcpy(key, market_prefix, sizeof(market_prefix) - 1U);
    (void)memcpy(key + sizeof(market_prefix) - 1U, market_id, 32U);
}

lxp_result lx_perps_market_encode(const lx_perps_market *market,
                                  uint8_t bytes[LX_PERPS_MARKET_BYTES])
{
    size_t offset = 0U;
    size_t key_bytes;
    lxp_result status = market_validate(market);
    if (status != LXP_OK || bytes == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, LX_PERPS_MARKET_BYTES);
    (void)memcpy(bytes + offset, market->market_id, 32U); offset += 32U;
    (void)memcpy(bytes + offset, market->quote_asset, 32U); offset += 32U;
    (void)memcpy(bytes + offset, market->administrator, 32U); offset += 32U;
    (void)memcpy(bytes + offset, market->liquidity_account_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, market->long_funding_account_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, market->short_funding_account_id, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, market->insurance_account_id, 32U);
    offset += 32U;
    (void)lxp_u128_to_be(market->contract_size, bytes + offset); offset += 16U;
    (void)lxp_u128_to_be(market->tick_size, bytes + offset); offset += 16U;
    (void)lxp_u128_to_be(market->lot_size, bytes + offset); offset += 16U;
    (void)lxp_u128_to_be(market->price_scale, bytes + offset); offset += 16U;
    lx_perps_put_u32(bytes + offset, market->initial_margin_ratio_bps);
    offset += 4U;
    lx_perps_put_u32(bytes + offset, market->maintenance_margin_ratio_bps);
    offset += 4U;
    lx_perps_put_u32(bytes + offset, market->liquidation_fee_bps);
    offset += 4U;
    lx_perps_put_u32(bytes + offset, market->liquidator_share_bps);
    offset += 4U;
    lx_perps_put_u32(bytes + offset, market->maximum_funding_rate_bps);
    offset += 4U;
    lx_perps_put_u32(bytes + offset, market->maximum_deviation_basis_points);
    offset += 4U;
    lx_perps_put_u64(bytes + offset, market->funding_interval_ms);
    offset += 8U;
    lx_perps_put_u64(bytes + offset, market->maximum_oracle_staleness_ms);
    offset += 8U;
    (void)lxp_u128_to_be(market->minimum_price, bytes + offset); offset += 16U;
    (void)lxp_u128_to_be(market->maximum_price, bytes + offset); offset += 16U;
    bytes[offset++] = market->permitted_oracle_key_count;
    key_bytes = (size_t)market->permitted_oracle_key_count * 32U;
    (void)memcpy(bytes + offset, market->permitted_oracle_keys, key_bytes);
    offset += LX_PERPS_MAX_ORACLE_KEYS * 32U;
    lx_perps_put_u32(bytes + offset, market->parameter_version); offset += 4U;
    bytes[offset++] = market->halted ? 1U : 0U;
    return offset == LX_PERPS_MARKET_BYTES ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lx_perps_market_decode(
    const uint8_t bytes[LX_PERPS_MARKET_BYTES], size_t length,
    lx_perps_market *market)
{
    size_t offset = 0U;
    size_t key_bytes;
    lxp_result status;
    if (bytes == NULL || market == NULL || length != LX_PERPS_MARKET_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(market, 0, sizeof(*market));
    (void)memcpy(market->market_id, bytes + offset, 32U); offset += 32U;
    (void)memcpy(market->quote_asset, bytes + offset, 32U); offset += 32U;
    (void)memcpy(market->administrator, bytes + offset, 32U); offset += 32U;
    (void)memcpy(market->liquidity_account_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(market->long_funding_account_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(market->short_funding_account_id, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(market->insurance_account_id, bytes + offset, 32U);
    offset += 32U;
    status = lxp_u128_from_be(bytes + offset, &market->contract_size);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &market->tick_size);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &market->lot_size);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &market->price_scale);
    if (status != LXP_OK) return status;
    offset += 16U;
    market->initial_margin_ratio_bps = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->maintenance_margin_ratio_bps = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->liquidation_fee_bps = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->liquidator_share_bps = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->maximum_funding_rate_bps = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->maximum_deviation_basis_points = lx_perps_get_u32(bytes + offset);
    offset += 4U;
    market->funding_interval_ms = lx_perps_get_u64(bytes + offset);
    offset += 8U;
    market->maximum_oracle_staleness_ms = lx_perps_get_u64(bytes + offset);
    offset += 8U;
    status = lxp_u128_from_be(bytes + offset, &market->minimum_price);
    if (status != LXP_OK) return status;
    offset += 16U;
    status = lxp_u128_from_be(bytes + offset, &market->maximum_price);
    if (status != LXP_OK) return status;
    offset += 16U;
    market->permitted_oracle_key_count = bytes[offset++];
    if (market->permitted_oracle_key_count > LX_PERPS_MAX_ORACLE_KEYS)
        return LXP_ERR_NON_CANONICAL;
    key_bytes = (size_t)market->permitted_oracle_key_count * 32U;
    (void)memcpy(market->permitted_oracle_keys, bytes + offset, key_bytes);
    if (key_bytes < LX_PERPS_MAX_ORACLE_KEYS * 32U &&
        !lxp_ct_is_zero(bytes + offset + key_bytes,
                        LX_PERPS_MAX_ORACLE_KEYS * 32U - key_bytes))
        return LXP_ERR_NON_CANONICAL;
    offset += LX_PERPS_MAX_ORACLE_KEYS * 32U;
    market->parameter_version = lx_perps_get_u32(bytes + offset); offset += 4U;
    if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
    market->halted = bytes[offset++] != 0U;
    return offset == length ? market_validate(market) : LXP_ERR_TRAILING_BYTES;
}

lxp_result lx_perps_market_put(lxp_module_ctx *ctx,
                               const lx_perps_market *market)
{
    uint8_t key[LX_PERPS_MARKET_KEY_BYTES];
    uint8_t bytes[LX_PERPS_MARKET_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS)
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_market_encode(market, bytes);
    if (status != LXP_OK) return status;
    market_key(market->market_id, key);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_perps_market_lookup(lxp_module_ctx *ctx,
                                  const uint8_t market_id[32],
                                  lx_perps_market *market)
{
    uint8_t key[LX_PERPS_MARKET_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS ||
        market_id == NULL || market == NULL)
        return LXP_ERR_NON_CANONICAL;
    market_key(market_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    return lx_perps_market_decode(bytes, length, market);
}

static lxp_result visit_market(const uint8_t *key, size_t key_length,
                               const uint8_t *value, size_t value_length,
                               void *user)
{
    market_iter_adapter *adapter = (market_iter_adapter *)user;
    lx_perps_market market;
    lxp_result status;
    if (key == NULL || key_length != LX_PERPS_MARKET_KEY_BYTES ||
        memcmp(key, market_prefix, sizeof(market_prefix) - 1U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_market_decode(value, value_length, &market);
    if (status != LXP_OK) return status;
    return adapter->visit(&market, adapter->user);
}

lxp_result lx_perps_market_iter(lxp_module_ctx *ctx,
                                lx_perps_market_visit_fn visit, void *user)
{
    market_iter_adapter adapter;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_PERPS || visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    adapter.visit = visit;
    adapter.user = user;
    return lxp_ctx_kv_iter(ctx, market_prefix, sizeof(market_prefix) - 1U,
                           visit_market, &adapter);
}

lxp_result lx_perps_market_create_execute(lxp_module_ctx *ctx,
                                          const lx_perps_market *market)
{
    lx_perps_market existing;
    lxp_result status;
    if (ctx == NULL || market == NULL) return LXP_ERR_NON_CANONICAL;
    status = market_validate(market);
    if (status != LXP_OK) return status;
    status = lx_perps_market_lookup(ctx, market->market_id, &existing);
    if (status == LXP_OK) return LXP_ERR_MARKET_ALREADY_EXISTS;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    return lx_perps_market_put(ctx, market);
}
