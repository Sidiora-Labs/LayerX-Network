#include "layerx/lx_perps.h"

#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result magnitude_difference(lxp_u128 left, lxp_u128 right,
                                       lxp_u128 *difference,
                                       bool *left_smaller)
{
    int comparison;
    if (difference == NULL || left_smaller == NULL)
        return LXP_ERR_NON_CANONICAL;
    comparison = lxp_u128_cmp(left, right);
    *left_smaller = comparison < 0;
    return comparison < 0 ? lxp_u128_sub(right, left, difference) :
                            lxp_u128_sub(left, right, difference);
}

lxp_result lx_perps_pnl_compute(lx_perps_side side, lxp_u128 entry_price,
                                lxp_u128 mark_price, lxp_u128 size,
                                lxp_u128 price_scale, lxp_i128 *pnl)
{
    lxp_u128 difference;
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_u256 product;
    bool mark_below;
    bool loss;
    lxp_result status;
    if (pnl == NULL || (side != LX_PERPS_SIDE_BUY &&
                        side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(size) || lxp_u128_is_zero(price_scale))
        return LXP_ERR_NON_CANONICAL;
    status = magnitude_difference(mark_price, entry_price, &difference,
                                  &mark_below);
    if (status != LXP_OK) return status;
    status = lxp_u128_mul(difference, size, &product);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    status = lxp_u256_div_floor(product, price_scale, &quotient, &remainder);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    loss = side == LX_PERPS_SIDE_BUY ? mark_below : !mark_below;
    if (lxp_u128_is_zero(difference)) loss = false;
    if (loss && !lxp_u128_is_zero(remainder)) {
        status = lxp_u128_add(quotient, (lxp_u128){ 0U, 1U }, &quotient);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    pnl->negative = loss;
    pnl->magnitude = quotient;
    if (lxp_u128_is_zero(quotient)) pnl->negative = false;
    return LXP_OK;
}

lxp_result lx_perps_funding_rate(const lx_perps_market *market,
                                 lxp_u128 oracle_price,
                                 lxp_u128 reference_price,
                                 uint32_t maximum_rate_bps,
                                 lxp_i128 *rate_bps)
{
    lxp_u128 difference;
    lxp_u128 quotient;
    lxp_u128 remainder;
    lxp_u256 product;
    bool oracle_below;
    lxp_result status;
    if (market == NULL || rate_bps == NULL || market->halted ||
        lxp_u128_is_zero(oracle_price) || lxp_u128_is_zero(reference_price) ||
        maximum_rate_bps > LXP_BASIS_POINTS_ONE)
        return market != NULL && market->halted ? LXP_ERR_MARKET_HALTED :
                                                  LXP_ERR_NON_CANONICAL;
    status = magnitude_difference(oracle_price, reference_price, &difference,
                                  &oracle_below);
    if (status != LXP_OK) return status;
    status = lxp_u128_mul(difference,
                          (lxp_u128){ 0U, LXP_BASIS_POINTS_ONE }, &product);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    status = lxp_u256_div_floor(product, reference_price, &quotient, &remainder);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    if (!lxp_u128_is_zero(remainder) && oracle_below) {
        status = lxp_u128_add(quotient, (lxp_u128){ 0U, 1U }, &quotient);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    if (quotient.hi != 0U || quotient.lo > maximum_rate_bps)
        quotient = (lxp_u128){ 0U, maximum_rate_bps };
    rate_bps->negative = oracle_below && !lxp_u128_is_zero(quotient);
    rate_bps->magnitude = quotient;
    return LXP_OK;
}

lxp_result lx_perps_funding_index_update(lxp_i128 current,
                                         lxp_i128 rate_bps,
                                         uint64_t elapsed_intervals,
                                         lxp_i128 *updated)
{
    lxp_u256 product;
    lxp_u128 delta;
    lxp_u128 remainder;
    lxp_i128 signed_delta;
    lxp_result status;
    if (updated == NULL || elapsed_intervals == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul(rate_bps.magnitude,
                          (lxp_u128){ 0U, elapsed_intervals }, &product);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    status = lxp_u256_div_floor(product, (lxp_u128){ 0U, 1U },
                                &delta, &remainder);
    if (status != LXP_OK || !lxp_u128_is_zero(remainder))
        return LXP_ERR_OVERFLOW;
    signed_delta.negative = rate_bps.negative;
    signed_delta.magnitude = delta;
    status = lxp_i128_add(current, signed_delta, updated);
    return status == LXP_OK ? LXP_OK : LXP_ERR_OVERFLOW;
}

lxp_result lx_perps_funding_tick_execute(
    lxp_module_ctx *ctx, const lx_perps_funding_tick_request *request,
    lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_u128 amount;
    lxp_i128 next_index;
    uint64_t timestamp;
    uint64_t elapsed;
    uint64_t intervals;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->market == NULL ||
        request->long_funding_account == NULL ||
        request->short_funding_account == NULL || request->asset == NULL ||
        request->last_funding_timestamp_ms == NULL ||
        request->funding_index == NULL || receipt == NULL ||
        request->market->funding_interval_ms == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (request->market->halted) return LXP_ERR_MARKET_HALTED;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < *request->last_funding_timestamp_ms)
        return LXP_ERR_TIMESTAMP_REGRESSION;
    elapsed = timestamp - *request->last_funding_timestamp_ms;
    intervals = elapsed / request->market->funding_interval_ms;
    if (intervals == 0U) return LXP_ERR_NOT_YET_VALID;
    status = lx_perps_funding_index_update(*request->funding_index,
                                           request->funding_rate_bps,
                                           intervals, &next_index);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(request->funding_rate_bps.magnitude)) {
        *request->funding_index = next_index;
        *request->last_funding_timestamp_ms +=
            intervals * request->market->funding_interval_ms;
        return LXP_OK;
    }
    if (request->funding_rate_bps.magnitude.hi != 0U ||
        request->funding_rate_bps.magnitude.lo > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_PARAMETER_BOUNDS;
    status = lxp_u128_mul_bps_ceil(
        request->open_notional,
        (uint32_t)request->funding_rate_bps.magnitude.lo, &amount);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    if (lxp_u128_is_zero(amount)) return LXP_ERR_ZERO_AMOUNT;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.leg_count = 1U;
    set.legs[0].from = request->funding_rate_bps.negative ?
        request->short_funding_account : request->long_funding_account;
    set.legs[0].to = request->funding_rate_bps.negative ?
        request->long_funding_account : request->short_funding_account;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = LXP_REASON_FUNDING;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.context = request->context;
    set.context.assets = request->asset;
    set.context.asset_count = 1U;
    set.context.protocol_system_capability = true;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(set.context.authorized_from, set.legs[0].from->id, 32U);
    (void)memcpy(source.authorized_from, set.legs[0].from->id, 32U);
    source.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    source.protocol_system_capability = true;
    set.context.source_authorities = &source;
    set.context.source_authority_count = 1U;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    *request->funding_index = next_index;
    *request->last_funding_timestamp_ms +=
        intervals * request->market->funding_interval_ms;
    return LXP_OK;
}

lxp_result lx_perps_funding_owed(const lx_perps_position *position,
                                 lxp_i128 funding_index, lxp_i128 *owed)
{
    lxp_i128 delta;
    lxp_u128 quotient;
    lxp_u128 remainder;
    bool trader_pays;
    lxp_result status;
    if (position == NULL || owed == NULL ||
        (position->side != LX_PERPS_SIDE_BUY &&
         position->side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(position->entry_notional))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_i128_sub(funding_index, position->funding_index_at_entry,
                          &delta);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    if (lxp_u128_is_zero(delta.magnitude)) {
        owed->negative = false;
        owed->magnitude = (lxp_u128){ 0U, 0U };
        return LXP_OK;
    }
    trader_pays = position->side == LX_PERPS_SIDE_BUY ? !delta.negative :
                                                        delta.negative;
    status = lxp_u128_mul_div_floor(position->entry_notional, delta.magnitude,
                                    (lxp_u128){ 0U, LXP_BASIS_POINTS_ONE },
                                    &quotient, &remainder);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    if (trader_pays && !lxp_u128_is_zero(remainder)) {
        status = lxp_u128_add(quotient, (lxp_u128){ 0U, 1U }, &quotient);
        if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    }
    owed->magnitude = quotient;
    owed->negative = !trader_pays && !lxp_u128_is_zero(quotient);
    return LXP_OK;
}
