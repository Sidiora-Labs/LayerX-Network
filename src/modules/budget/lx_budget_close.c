#include "layerx/lx_budget.h"

#include <string.h>

lxp_result lx_budget_authority_check(const lx_account *account,
                                     lxp_authorization_kind authority_kind,
                                     uint16_t origin_module_id,
                                     uint16_t reason)
{
    if (account == NULL) return LXP_ERR_NON_CANONICAL;
    if (account->kind != LX_ACCOUNT_AGENT_BUDGET) return LXP_OK;
    if (origin_module_id != LXP_MODULE_BUDGET ||
        authority_kind != LXP_AUTH_BUDGET_ALLOWANCE)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (reason != LXP_REASON_BUDGET_SPEND &&
        reason != LXP_REASON_BUDGET_DEFUND)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

static lxp_result validate(const lx_budget_close_request *request,
                           lx_budget_record **record)
{
    lxp_result status;
    if (request == NULL || request->store == NULL ||
        request->budget_id == NULL || request->budget_account == NULL ||
        request->owner == NULL || request->asset == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_budget_lookup(request->store, request->budget_id, record);
    if (status != LXP_OK || (*record)->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (memcmp((*record)->budget_account,
               request->budget_account->id, 32U) != 0 ||
        memcmp((*record)->owner, request->owner->id, 32U) != 0 ||
        memcmp((*record)->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result emit_return(lxp_module_ctx *ctx,
                              const lx_budget_close_request *request,
                              lxp_u128 amount, lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    if (lxp_u128_is_zero(amount)) return LXP_OK;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.leg_count = 1U;
    set.legs[0].from = request->budget_account;
    set.legs[0].to = request->owner;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = LXP_REASON_BUDGET_DEFUND;
    set.context = request->context;
    set.context.debit_authority_kind = LXP_AUTH_BUDGET_ALLOWANCE;
    (void)memcpy(set.context.authorized_from,
                 request->budget_account->id, 32U);
    lx_budget_bind_source_authority(&set, &source);
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_budget_defund_execute(lxp_module_ctx *ctx,
                                    const lx_budget_close_request *request,
                                    lxp_receipt *receipt)
{
    lx_budget_record *record;
    lxp_u128 balance;
    lxp_u128 resulting;
    lxp_u128 allowance;
    lxp_u128 adjusted_limit;
    lxp_result status = validate(request, &record);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(request->amount)) return LXP_ERR_INVALID_AMOUNT;
    status = lxp_state_balance_get(request->budget_account, record->asset_id,
                                   &balance);
    if (status != LXP_OK) return status;
    if (lxp_u128_cmp(request->amount, balance) > 0)
        return LXP_ERR_INSUFFICIENT_BUDGET_FUNDS;
    status = lxp_u128_sub(balance, request->amount, &resulting);
    if (status != LXP_OK) return status;
    status = lxp_u128_sub(record->per_period_limit,
                          record->spent_this_period, &allowance);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    adjusted_limit = record->per_period_limit;
    if (lxp_u128_cmp(allowance, resulting) > 0) {
        status = lxp_u128_add(record->spent_this_period, resulting,
                              &adjusted_limit);
        if (status != LXP_OK) return status;
    }
    status = emit_return(ctx, request, request->amount, receipt);
    if (status != LXP_OK) return status;
    record->per_period_limit = adjusted_limit;
    return LXP_OK;
}

static lxp_result drain(lxp_module_ctx *ctx,
                        const lx_budget_close_request *request,
                        bool revoke, lxp_receipt *receipt)
{
    lx_budget_record *record;
    lxp_u128 balance;
    lxp_result status = validate(request, &record);
    if (status != LXP_OK) return status;
    if (request->revocation_sequence <= record->revocation_sequence)
        return LXP_ERR_STALE_REVOCATION;
    status = lxp_state_balance_get(request->budget_account, record->asset_id,
                                   &balance);
    if (status != LXP_OK) return status;
    status = emit_return(ctx, request, balance, receipt);
    if (status != LXP_OK) return status;
    record->revocation_sequence = request->revocation_sequence;
    record->revoked = revoke;
    record->closed = !revoke;
    record->per_period_limit = record->spent_this_period;
    return LXP_OK;
}

lxp_result lx_budget_revoke_execute(lxp_module_ctx *ctx,
                                    const lx_budget_close_request *request,
                                    lxp_receipt *receipt)
{
    return drain(ctx, request, true, receipt);
}

lxp_result lx_budget_close_execute(lxp_module_ctx *ctx,
                                   const lx_budget_close_request *request,
                                   lxp_receipt *receipt)
{
    return drain(ctx, request, false, receipt);
}
