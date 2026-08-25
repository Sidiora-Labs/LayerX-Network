#include "layerx/lx_perps.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_perps_authority_check(const lx_account *account,
                                    lxp_authorization_kind kind,
                                    uint16_t origin_module_id,
                                    uint16_t reason)
{
    if (account == NULL) return LXP_ERR_NON_CANONICAL;
    if (account->kind != LX_ACCOUNT_AGENT_MARGIN) return LXP_OK;
    if (kind != LXP_AUTH_PROTOCOL_MODULE ||
        origin_module_id != LXP_MODULE_PERPS ||
        (reason != LXP_REASON_MARGIN_RELEASE &&
         reason != LXP_REASON_TRADING_LOSS &&
         reason != LXP_REASON_LIQUIDATION_FEE &&
         reason != LXP_REASON_ADL))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

lxp_result lx_perps_position_lookup(lx_perps_position_store *store,
                                    const uint8_t position_id[32],
                                    lx_perps_position **position)
{
    size_t i;
    if (store == NULL || position_id == NULL || position == NULL ||
        store->count > LX_PERPS_POSITION_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->positions[i].position_id, position_id, 32U) == 0) {
            *position = &store->positions[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

static lxp_result emit_margin(lxp_module_ctx *ctx, lx_account *from,
                              lx_account *to,
                              const lxp_transfer_asset_state *asset,
                              lxp_u128 amount, lxp_transfer_context context,
                              uint16_t reason, lxp_receipt *receipt)
{
    lxp_transfer_set set;
    if (ctx == NULL || from == NULL || to == NULL || asset == NULL ||
        receipt == NULL || lxp_u128_is_zero(amount) || !asset->registered ||
        asset->paused)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = from;
    set.legs[0].to = to;
    (void)memcpy(set.legs[0].asset_id, asset->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = reason;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.context = context;
    set.context.assets = asset;
    set.context.asset_count = 1U;
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_perps_margin_post(lxp_module_ctx *ctx,
                                lx_account *owner_main,
                                lx_account *margin_account,
                                const lxp_transfer_asset_state *asset,
                                lxp_u128 amount,
                                lxp_transfer_context context,
                                lxp_receipt *receipt)
{
    if (owner_main == NULL || margin_account == NULL ||
        owner_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        margin_account->kind != LX_ACCOUNT_AGENT_MARGIN)
        return LXP_ERR_NON_CANONICAL;
    context.debit_authority_kind = context.debit_authority_kind == 0 ?
        LXP_AUTH_OWNER : context.debit_authority_kind;
    return emit_margin(ctx, owner_main, margin_account, asset, amount, context,
                       LXP_REASON_MARGIN_POST, receipt);
}

lxp_result lx_perps_margin_release(lxp_module_ctx *ctx,
                                   lx_account *margin_account,
                                   lx_account *owner_main,
                                   const lxp_transfer_asset_state *asset,
                                   lxp_u128 amount,
                                   lxp_transfer_context context,
                                   lxp_receipt *receipt)
{
    if (owner_main == NULL || margin_account == NULL ||
        owner_main->kind != LX_ACCOUNT_AGENT_MAIN ||
        margin_account->kind != LX_ACCOUNT_AGENT_MARGIN)
        return LXP_ERR_NON_CANONICAL;
    context.protocol_system_capability = true;
    context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(context.authorized_from, margin_account->id, 32U);
    return emit_margin(ctx, margin_account, owner_main, asset, amount, context,
                       LXP_REASON_MARGIN_RELEASE, receipt);
}

static lxp_result position_validate(const lx_perps_position_request *request)
{
    const lx_perps_position *position;
    if (request == NULL || request->store == NULL ||
        request->owner_main == NULL || request->margin_account == NULL ||
        request->asset == NULL || lxp_u128_is_zero(request->margin_amount))
        return LXP_ERR_NON_CANONICAL;
    position = &request->position;
    if (lxp_ct_is_zero(position->position_id, 32U) ||
        lxp_ct_is_zero(position->market_id, 32U) ||
        memcmp(position->owner_main_account_id,
               request->owner_main->id, 32U) != 0 ||
        memcmp(position->margin_account_id,
               request->margin_account->id, 32U) != 0 ||
        memcmp(position->asset_id, request->asset->asset_id, 32U) != 0 ||
        (position->side != LX_PERPS_SIDE_BUY &&
         position->side != LX_PERPS_SIDE_SELL) ||
        lxp_u128_is_zero(position->size) ||
        lxp_u128_is_zero(position->entry_notional))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_perps_position_open_execute(
    lxp_module_ctx *ctx, const lx_perps_position_request *request,
    lxp_receipt *receipt)
{
    lx_perps_position *existing;
    lx_perps_position position;
    lxp_result status = position_validate(request);
    if (status != LXP_OK || ctx == NULL || receipt == NULL)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    if (request->store->count == LX_PERPS_POSITION_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (lx_perps_position_lookup(request->store,
                                 request->position.position_id,
                                 &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    status = lx_perps_margin_post(ctx, request->owner_main,
                                  request->margin_account, request->asset,
                                  request->margin_amount, request->context,
                                  receipt);
    if (status != LXP_OK) return status;
    position = request->position;
    position.open = true;
    request->store->positions[request->store->count++] = position;
    request->margin_account->has_open_reference = true;
    return LXP_OK;
}

lxp_result lx_perps_position_increase_execute(
    lxp_module_ctx *ctx, const lx_perps_position_request *request,
    lxp_receipt *receipt)
{
    lx_perps_position *position;
    lxp_u128 next_size;
    lxp_u128 next_notional;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        receipt == NULL || lxp_u128_is_zero(request->size_delta) ||
        lxp_u128_is_zero(request->notional_delta))
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_position_lookup(request->store,
                                      request->position.position_id,
                                      &position);
    if (status != LXP_OK) return status;
    if (!position->open) return LXP_ERR_MARKET_HALTED;
    status = lxp_u128_add(position->size, request->size_delta, &next_size);
    if (status == LXP_OK)
        status = lxp_u128_add(position->entry_notional,
                              request->notional_delta, &next_notional);
    if (status != LXP_OK) return status;
    status = lx_perps_margin_post(ctx, request->owner_main,
                                  request->margin_account, request->asset,
                                  request->margin_amount, request->context,
                                  receipt);
    if (status != LXP_OK) return status;
    position->size = next_size;
    position->entry_notional = next_notional;
    return LXP_OK;
}

lxp_result lx_perps_position_close_execute(
    lxp_module_ctx *ctx, lx_perps_position_store *store,
    const uint8_t position_id[32], lx_account *margin_account,
    lx_account *owner_main, const lxp_transfer_asset_state *asset,
    lxp_transfer_context context, lxp_receipt *receipt)
{
    lx_perps_position *position;
    lxp_u128 amount;
    lxp_result status;
    if (ctx == NULL || store == NULL || position_id == NULL ||
        margin_account == NULL || owner_main == NULL || asset == NULL ||
        receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_perps_position_lookup(store, position_id, &position);
    if (status != LXP_OK) return status;
    if (!position->open || memcmp(position->margin_account_id,
                                  margin_account->id, 32U) != 0)
        return LXP_ERR_ACCOUNT_NOT_EMPTY;
    amount = margin_account->balance;
    if (lxp_u128_is_zero(amount)) return LXP_ERR_ACCOUNT_NOT_EMPTY;
    status = lx_perps_margin_release(ctx, margin_account, owner_main, asset,
                                     amount, context, receipt);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(margin_account->balance))
        return LXP_FATAL_INVARIANT;
    position->open = false;
    margin_account->has_open_reference = false;
    return LXP_OK;
}
