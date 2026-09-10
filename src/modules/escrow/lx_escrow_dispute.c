#include "lx_escrow_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static bool party_authorized(const lx_escrow_record *record,
                             const lxp_authority_resolved *authority)
{
    return authority != NULL &&
           (memcmp(authority->principal, record->owner, 32U) == 0 ||
            memcmp(authority->principal, record->beneficiary, 32U) == 0);
}

lxp_result lx_escrow_dispute_open_execute(
    lxp_module_ctx *ctx, const lx_escrow_dispute_request *request)
{
    lx_escrow_record record;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->escrow_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_lookup(ctx, request->escrow_id, &record);
    if (status != LXP_OK) return status;
    if (!lx_escrow_active_state(record.state)) return LXP_ERR_ESCROW_STATE;
    if (!party_authorized(&record, request->authority))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (record.dispute_window == 0U ||
        lxp_ctx_batch_timestamp_ms(ctx) > record.dispute_window)
        return LXP_ERR_DISPUTE_WINDOW_CLOSED;
    record.state = LX_ESCROW_STATE_DISPUTED;
    return lx_escrow_state_update(ctx, &record);
}

lxp_result lx_escrow_split_bps(lxp_u128 balance,
                               uint32_t beneficiary_basis_points,
                               lxp_u128 *beneficiary, lxp_u128 *owner)
{
    lxp_result status;
    if (beneficiary == NULL || owner == NULL ||
        beneficiary_basis_points > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_mul_bps_floor(balance, beneficiary_basis_points,
                                    beneficiary);
    if (status != LXP_OK) return status;
    return lxp_u128_sub(balance, *beneficiary, owner);
}

lxp_result lx_escrow_dispute_resolve_execute(
    lxp_module_ctx *ctx, const lx_escrow_dispute_request *request,
    lxp_receipt *receipt)
{
    lx_escrow_record record;
    lx_escrow_settlement settlement;
    lxp_u128 balance;
    lxp_u128 beneficiary_amount;
    lxp_u128 owner_amount;
    lxp_u128 total;
    lxp_u128 captured_after;
    lxp_result release;
    lxp_result status;
    bool replayed;
    if (ctx == NULL || request == NULL || request->escrow_id == NULL ||
        request->escrow_account == NULL ||
        request->beneficiary_account == NULL ||
        request->owner_account == NULL || request->asset == NULL ||
        request->authority == NULL || receipt == NULL ||
        lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_receipt_replay(ctx, request->idempotency_key, receipt,
                                      &replayed);
    if (status != LXP_OK || replayed) return status;
    status = lx_escrow_lookup(ctx, request->escrow_id, &record);
    if (status != LXP_OK) return status;
    if (record.state != LX_ESCROW_STATE_DISPUTED)
        return LXP_ERR_ESCROW_STATE;
    if (memcmp(request->authority->principal, record.arbiter, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (memcmp(request->escrow_account->id, record.escrow_account, 32U) != 0 ||
        memcmp(request->beneficiary_account->id, record.beneficiary,
               32U) != 0 ||
        memcmp(request->owner_account->id, record.owner, 32U) != 0 ||
        memcmp(request->asset->asset_id, record.asset_id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    status = lx_escrow_remaining(&record, request->escrow_account, &balance);
    if (status == LXP_OK)
        status = lx_escrow_split_bps(balance,
                                     request->beneficiary_basis_points,
                                     &beneficiary_amount, &owner_amount);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(beneficiary_amount, owner_amount, &total);
    if (status != LXP_OK || lxp_u128_cmp(total, balance) != 0)
        return LXP_ERR_CONSERVATION;
    status = lxp_u128_add(record.captured_amount, beneficiary_amount,
                          &captured_after);
    if (status != LXP_OK) return status;
    (void)memset(&settlement, 0, sizeof(settlement));
    settlement.from = request->escrow_account;
    settlement.to = request->beneficiary_account;
    settlement.secondary_to = request->owner_account;
    settlement.asset = request->asset;
    settlement.amount = beneficiary_amount;
    settlement.secondary_amount = owner_amount;
    settlement.reason = LXP_REASON_ESCROW_RESOLVE;
    status = lxp_module_staged_reserve(ctx, 2U);
    if (status != LXP_OK) return status;
    status = lx_escrow_settle(ctx, &request->context, &settlement,
                              LXP_AUTH_ESCROW, receipt);
    release = lxp_module_staged_release(ctx, 2U);
    if (status != LXP_OK) return status;
    if (release != LXP_OK) return release;
    record.state = LX_ESCROW_STATE_RESOLVED;
    record.captured_amount = captured_after;
    record.locked_amount = (lxp_u128){ 0U, 0U };
    return lx_escrow_commit_result(ctx, &record, request->idempotency_key,
                                   &settlement, 7U, receipt);
}
