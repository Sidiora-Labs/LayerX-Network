#include "lx_escrow_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result validate_open(lxp_module_ctx *ctx,
                                const lx_escrow_open_request *request)
{
    lx_escrow_record existing;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->owner == NULL ||
        request->escrow_account == NULL || request->asset == NULL ||
        request->owner->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->escrow_account->kind != LX_ACCOUNT_AGENT_ESCROW ||
        lxp_u128_is_zero(request->amount) || request->asset->paused ||
        request->record.state != LX_ESCROW_STATE_OPEN ||
        !lxp_u128_is_zero(request->record.captured_amount) ||
        lxp_ct_is_zero(request->record.escrow_id, 32U) ||
        lxp_u128_cmp(request->record.locked_amount, request->amount) != 0 ||
        memcmp(request->record.owner, request->owner->id, 32U) != 0 ||
        memcmp(request->record.escrow_account,
               request->escrow_account->id, 32U) != 0 ||
        memcmp(request->record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (!request->owner->has_asset || !request->escrow_account->has_asset ||
        memcmp(request->owner->asset_id, request->asset->asset_id, 32U) != 0 ||
        memcmp(request->escrow_account->asset_id,
               request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ASSET_MISMATCH;
    status = lx_escrow_lookup(ctx, request->record.escrow_id, &existing);
    if (status == LXP_OK) return LXP_ERR_ESCROW_STATE;
    return status == LXP_ERR_ESCROW_STATE ? LXP_OK : status;
}

lxp_result lx_escrow_open_execute(lxp_module_ctx *ctx,
                                  const lx_escrow_open_request *request,
                                  lxp_receipt *receipt)
{
    lx_escrow_settlement settlement;
    lx_escrow_record record;
    lxp_result release;
    lxp_result status;
    if (receipt == NULL) return LXP_ERR_NON_CANONICAL;
    status = validate_open(ctx, request);
    if (status != LXP_OK) return status;
    record = request->record;
    (void)memset(&settlement, 0, sizeof(settlement));
    settlement.from = request->owner;
    settlement.to = request->escrow_account;
    settlement.asset = request->asset;
    settlement.amount = request->amount;
    settlement.reason = LXP_REASON_ESCROW_LOCK;
    status = lxp_module_staged_reserve(ctx, 2U);
    if (status != LXP_OK) return status;
    status = lx_escrow_settle(ctx, &request->context, &settlement,
                              LXP_AUTH_OWNER, receipt);
    release = lxp_module_staged_release(ctx, 2U);
    if (status != LXP_OK) return status;
    if (release != LXP_OK) return release;
    return lx_escrow_commit_result(ctx, &record, record.escrow_id,
                                   &settlement, 1U, receipt);
}
