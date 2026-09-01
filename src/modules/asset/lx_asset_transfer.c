#include "layerx/lx_asset.h"

#include "layerx/lxp_receipt.h"

#include <string.h>

lxp_result lx_asset_validate(const lx_asset_transfer_request *request)
{
    lxp_u128 computed;
    if (request == NULL || request->from == NULL || request->to == NULL ||
        request->asset == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->direct_balance_write) return LXP_ERR_BALANCE_BYPASS;
    if (request->from->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->to->kind != LX_ACCOUNT_AGENT_MAIN)
        return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (lxp_u128_is_zero(request->amount)) return LXP_ERR_INVALID_AMOUNT;
    if (request->asset->paused) return LXP_ERR_ASSET_PAUSED;
    if (!request->from->has_asset || !request->to->has_asset ||
        memcmp(request->from->asset_id, request->asset->asset_id, 32U) != 0 ||
        memcmp(request->to->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ASSET_MISMATCH;
    if (request->from->frozen || request->to->frozen)
        return LXP_ERR_ACCOUNT_FROZEN;
    if (lxp_u128_cmp(request->from->balance, request->amount) < 0)
        return LXP_ERR_INSUFFICIENT_BALANCE;
    if (lxp_u128_sub(request->from->balance, request->amount, &computed) != LXP_OK)
        return LXP_ERR_UNDERFLOW;
    if (request->from != request->to &&
        lxp_u128_add(request->to->balance, request->amount, &computed) != LXP_OK)
        return LXP_ERR_OVERFLOW;
    return LXP_OK;
}

static lxp_result execute(lxp_module_ctx *ctx,
                          const lx_asset_transfer_request *request,
                          lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_result status = lx_asset_validate(request);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->from;
    set.legs[0].to = request->to;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_PAYMENT;
    set.context = request->context;
    return lxp_ctx_emit_monetary_transfer_set(ctx, &set, receipt);
}

lxp_result lx_asset_send_execute(lxp_module_ctx *ctx,
                                 const lx_asset_transfer_request *request,
                                 lxp_receipt *receipt)
{
    return execute(ctx, request, receipt);
}

lxp_result lx_asset_receive_execute(lxp_module_ctx *ctx,
                                    const lx_asset_transfer_request *request,
                                    lxp_receipt *receipt)
{
    lxp_result status;
    if (request == NULL || request->payer_grant == NULL || request->from == NULL)
        return LXP_ERR_NO_PAYER_GRANT;
    status = lxp_verify_payer_grant(request->payer_grant, request->from);
    if (status != LXP_OK) return status;
    if (memcmp(request->payer_grant->from, request->from->id, 32U) != 0 ||
        memcmp(request->payer_grant->recipient, request->to->id, 32U) != 0 ||
        memcmp(request->payer_grant->asset, request->asset->asset_id, 32U) != 0 ||
        lxp_u128_cmp(request->amount,
                     request->payer_grant->per_draw_maximum) > 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    return execute(ctx, request, receipt);
}
