#include "layerx/lx_perps.h"

#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result position_notional(const lx_perps_position *position,
                                    lxp_u128 mark_price,
                                    lxp_u128 price_scale,
                                    lxp_u128 *notional)
{
    lxp_u128 remainder;
    lxp_result status;
    if (position == NULL || notional == NULL ||
        lxp_u128_is_zero(mark_price) || lxp_u128_is_zero(price_scale) ||
        lxp_u128_is_zero(position->size))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul_div_floor(mark_price, position->size, price_scale,
                                    notional, &remainder);
    return status == LXP_OK ? LXP_OK : LXP_ERR_OVERFLOW;
}

lxp_result lx_perps_maintenance_check(const lx_perps_market *market,
                                      const lx_perps_position *position,
                                      lxp_u128 mark_price,
                                      lxp_u128 price_scale,
                                      lxp_u128 margin_balance,
                                      bool *liquidatable)
{
    lxp_u128 notional;
    lxp_u128 required;
    lxp_result status;
    if (market == NULL || position == NULL || liquidatable == NULL ||
        market->maintenance_margin_ratio_bps == 0U ||
        market->maintenance_margin_ratio_bps > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_NON_CANONICAL;
    status = position_notional(position, mark_price, price_scale, &notional);
    if (status != LXP_OK) return status;
    status = lxp_u128_mul_bps_ceil(
        notional, market->maintenance_margin_ratio_bps, &required);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    *liquidatable = lxp_u128_cmp(margin_balance, required) < 0;
    return LXP_OK;
}

lxp_result lx_perps_liquidation_fee_split(lxp_u128 total_fee,
                                          uint32_t liquidator_share_bps,
                                          lxp_u128 *liquidator_fee,
                                          lxp_u128 *insurance_fee)
{
    lxp_result status;
    if (liquidator_fee == NULL || insurance_fee == NULL ||
        liquidator_share_bps > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul_bps_ceil(
        total_fee, LXP_BASIS_POINTS_ONE - liquidator_share_bps,
        insurance_fee);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    status = lxp_u128_sub(total_fee, *insurance_fee, liquidator_fee);
    return status == LXP_OK ? LXP_OK : LXP_ERR_OVERFLOW;
}

static lxp_result append_leg(lxp_transfer_set *set, lx_account *from,
                             lx_account *to, const uint8_t asset_id[32],
                             lxp_u128 amount, uint16_t reason)
{
    lxp_transfer_leg *leg;
    if (lxp_u128_is_zero(amount)) return LXP_OK;
    if (set->leg_count == LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_TOO_MANY_LEGS;
    leg = &set->legs[set->leg_count++];
    (void)memset(leg, 0, sizeof(*leg));
    leg->from = from;
    leg->to = to;
    (void)memcpy(leg->asset_id, asset_id, 32U);
    leg->amount = amount;
    leg->reason = reason;
    leg->supply_mode = LXP_TRANSFER_CONSERVED;
    return LXP_OK;
}

lxp_result lx_perps_liquidation_legs_build(
    const lx_perps_liquidation_request *request, lxp_transfer_set *set,
    lxp_transfer_source_authority *authorities, size_t capacity,
    size_t *authority_count)
{
    size_t index;
    lxp_u128 notional;
    lxp_u128 total_fee;
    lxp_u128 loss_from_margin;
    lxp_u128 deficit;
    lxp_u128 remaining;
    lxp_u128 fee_from_margin;
    lxp_u128 liquidator_fee;
    lxp_u128 insurance_fee;
    lxp_result status;
    if (request == NULL || set == NULL || authorities == NULL ||
        authority_count == NULL || capacity == 0U ||
        request->position == NULL ||
        request->market == NULL || request->margin_account == NULL ||
        request->market_liquidity_account == NULL ||
        request->liquidator_main_account == NULL ||
        request->insurance_account == NULL ||
        request->owner_main_account == NULL || request->asset == NULL ||
        request->liquidation_fee_bps > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_NON_CANONICAL;
    status = position_notional(request->position, request->mark_price,
                               request->price_scale, &notional);
    if (status != LXP_OK) return status;
    status = lxp_u128_mul_bps_ceil(notional, request->liquidation_fee_bps,
                                   &total_fee);
    if (status != LXP_OK) return LXP_ERR_OVERFLOW;
    loss_from_margin = lxp_u128_cmp(request->trading_loss,
                                    request->margin_account->balance) < 0 ?
        request->trading_loss : request->margin_account->balance;
    status = lxp_u128_sub(request->margin_account->balance, loss_from_margin,
                          &remaining);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    status = lxp_u128_sub(request->trading_loss, loss_from_margin, &deficit);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    if (lxp_u128_cmp(deficit, request->insurance_account->balance) > 0)
        return LXP_ERR_INSUFFICIENT_BALANCE;
    fee_from_margin = lxp_u128_cmp(total_fee, remaining) < 0 ?
        total_fee : remaining;
    status = lxp_u128_sub(remaining, fee_from_margin, &remaining);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    status = lx_perps_liquidation_fee_split(
        fee_from_margin, request->liquidator_share_bps,
        &liquidator_fee, &insurance_fee);
    if (status != LXP_OK) return status;
    (void)memset(set, 0, sizeof(*set));
    status = append_leg(set, request->margin_account,
                        request->market_liquidity_account,
                        request->asset->asset_id, loss_from_margin,
                        LXP_REASON_TRADING_LOSS);
    if (status == LXP_OK)
        status = append_leg(set, request->margin_account,
                            request->liquidator_main_account,
                            request->asset->asset_id, liquidator_fee,
                            LXP_REASON_LIQUIDATION_FEE);
    if (status == LXP_OK)
        status = append_leg(set, request->margin_account,
                            request->insurance_account,
                            request->asset->asset_id, insurance_fee,
                            LXP_REASON_LIQUIDATION_FEE);
    if (status == LXP_OK)
        status = append_leg(set, request->insurance_account,
                            request->market_liquidity_account,
                            request->asset->asset_id, deficit,
                            LXP_REASON_INSURANCE);
    if (status == LXP_OK)
        status = append_leg(set, request->margin_account,
                            request->owner_main_account,
                            request->asset->asset_id, remaining,
                            LXP_REASON_MARGIN_RELEASE);
    if (status != LXP_OK) return status;
    if (set->leg_count == 0U) return LXP_ERR_ZERO_AMOUNT;
    set->context = request->context;
    set->context.assets = request->asset;
    set->context.asset_count = 1U;
    set->context.protocol_system_capability = true;
    set->context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(set->context.authorized_from,
                 request->margin_account->id, 32U);
    *authority_count = 0U;
    for (index = 0U; index < set->leg_count; ++index) {
        status = lx_perps_source_authority_add(
            authorities, capacity, authority_count,
            set->legs[index].from->id, LXP_AUTH_PROTOCOL_MODULE, true);
        if (status != LXP_OK) return status;
    }
    set->context.source_authorities = authorities;
    set->context.source_authority_count = *authority_count;
    return LXP_OK;
}

lxp_result lx_perps_liquidate_execute(
    lxp_module_ctx *ctx, const lx_perps_liquidation_request *request,
    lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority authorities[LX_PERPS_LIQUIDATION_SOURCES];
    size_t authority_count = 0U;
    bool liquidatable;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->position == NULL ||
        request->market == NULL || receipt == NULL ||
        ctx->module_id != LXP_MODULE_PERPS)
        return LXP_ERR_NON_CANONICAL;
    if (request->market->halted) return LXP_ERR_MARKET_HALTED;
    if (!request->position->open) return LXP_ERR_AGREEMENT_STATE;
    status = lx_perps_maintenance_check(
        request->market, request->position, request->mark_price,
        request->price_scale, request->margin_account->balance,
        &liquidatable);
    if (status != LXP_OK) return status;
    if (!liquidatable) return LXP_ERR_MARGIN_INSUFFICIENT;
    status = lx_perps_liquidation_legs_build(request, &set, authorities,
                                             LX_PERPS_LIQUIDATION_SOURCES,
                                             &authority_count);
    if (status != LXP_OK) return status;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(request->margin_account->balance))
        return LXP_FATAL_INVARIANT;
    request->position->open = false;
    request->margin_account->has_open_reference = false;
    return LXP_OK;
}
