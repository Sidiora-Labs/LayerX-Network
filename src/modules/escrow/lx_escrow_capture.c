#include "lx_escrow_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_escrow_remaining(const lx_escrow_record *record,
                               const lx_account *escrow_account,
                               lxp_u128 *remaining)
{
    if (record == NULL || escrow_account == NULL || remaining == NULL ||
        memcmp(record->escrow_account, escrow_account->id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    return lxp_state_balance_get(escrow_account, record->asset_id, remaining);
}

static bool authority_can_capture(const lx_escrow_record *record,
                                  const lxp_authority_resolved *authority)
{
    if (authority == NULL) return false;
    if (memcmp(authority->principal, record->beneficiary, 32U) == 0)
        return true;
    return authority->kind == LXP_AUTHORITY_DELEGATED_CAPABILITY &&
           memcmp(authority->principal, record->owner, 32U) == 0;
}

static lxp_result execute_capture(lxp_module_ctx *ctx,
                                  const lx_escrow_capture_request *request,
                                  bool full, lxp_receipt *receipt)
{
    lx_escrow_record record;
    lx_escrow_settlement settlement;
    lxp_u128 remaining;
    lxp_u128 captured;
    lxp_u128 locked_after;
    lxp_u128 amount;
    lxp_result release;
    lxp_result status;
    bool replayed;
    if (ctx == NULL || request == NULL || request->escrow_id == NULL ||
        request->escrow_account == NULL ||
        request->beneficiary_account == NULL || request->asset == NULL ||
        receipt == NULL || lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_receipt_replay(ctx, request->idempotency_key, receipt,
                                      &replayed);
    if (status != LXP_OK || replayed) return status;
    status = lx_escrow_lookup(ctx, request->escrow_id, &record);
    if (status != LXP_OK) return status;
    if (record.state == LX_ESCROW_STATE_TIMED_OUT)
        return LXP_ERR_HOLD_EXPIRED;
    if (record.state == LX_ESCROW_STATE_DISPUTED)
        return LXP_ERR_HOLD_DISPUTED;
    /* An expired hold is refused without touching hold state, ledger balances
     * or the idempotency record; the expiry sweep is the only writer of the
     * timed-out transition. */
    if (record.expiry != 0U &&
        lxp_ctx_batch_timestamp_ms(ctx) >= record.expiry)
        return LXP_ERR_HOLD_EXPIRED;
    if (!lx_escrow_active_state(record.state)) return LXP_ERR_ESCROW_STATE;
    if (!authority_can_capture(&record, request->authority))
        return LXP_ERR_UNAUTHORIZED_CAPTURE;
    if (memcmp(record.beneficiary, request->beneficiary_account->id,
               32U) != 0 ||
        memcmp(record.escrow_account, request->escrow_account->id, 32U) != 0 ||
        memcmp(record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    status = lx_escrow_remaining(&record, request->escrow_account, &remaining);
    if (status != LXP_OK) return status;
    amount = full ? remaining : request->amount;
    if (lxp_u128_is_zero(amount) || lxp_u128_cmp(amount, remaining) > 0)
        return LXP_ERR_CAPTURE_EXCEEDS_HOLD;
    if (!full && lxp_u128_cmp(amount, remaining) == 0)
        return LXP_ERR_CAPTURE_EXCEEDS_HOLD;
    status = lxp_u128_add(record.captured_amount, amount, &captured);
    if (status != LXP_OK) return status;
    status = lxp_u128_sub(remaining, amount, &locked_after);
    if (status != LXP_OK) return status;
    (void)memset(&settlement, 0, sizeof(settlement));
    settlement.from = request->escrow_account;
    settlement.to = request->beneficiary_account;
    settlement.asset = request->asset;
    settlement.amount = amount;
    settlement.reason = LXP_REASON_ESCROW_CAPTURE;
    status = lxp_module_staged_reserve(ctx, 2U);
    if (status != LXP_OK) return status;
    status = lx_escrow_settle(ctx, &request->context, &settlement,
                              LXP_AUTH_ESCROW, receipt);
    release = lxp_module_staged_release(ctx, 2U);
    if (status != LXP_OK) return status;
    if (release != LXP_OK) return release;
    record.captured_amount = captured;
    record.locked_amount = locked_after;
    record.state = full ? LX_ESCROW_STATE_CAPTURED :
                          LX_ESCROW_STATE_PARTIALLY_CAPTURED;
    return lx_escrow_commit_result(ctx, &record, request->idempotency_key,
                                   &settlement, full ? 2U : 3U, receipt);
}

lxp_result lx_escrow_capture_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_capture_request *request,
                                     lxp_receipt *receipt)
{
    return execute_capture(ctx, request, true, receipt);
}

lxp_result lx_escrow_partial_capture_execute(
    lxp_module_ctx *ctx, const lx_escrow_capture_request *request,
    lxp_receipt *receipt)
{
    return execute_capture(ctx, request, false, receipt);
}
