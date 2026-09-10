#include "layerx/lx_budget.h"

#include <string.h>

void lx_budget_bind_source_authority(lxp_transfer_set *set,
                                     lxp_transfer_source_authority *source)
{
    if (set == NULL || source == NULL) return;
    (void)memset(source, 0, sizeof(*source));
    (void)memcpy(source->authorized_from, set->context.authorized_from, 32U);
    source->debit_authority_kind = set->context.debit_authority_kind;
    set->context.source_authorities = source;
    set->context.source_authority_count = 1U;
}

lxp_result lx_budget_allowance_debit(lx_budget_record *record,
                                     lxp_u128 amount)
{
    lxp_u128 available;
    lxp_u128 updated;
    lxp_result status;
    if (record == NULL || lxp_u128_is_zero(amount))
        return LXP_ERR_INVALID_AMOUNT;
    status = lxp_u128_sub(record->per_period_limit,
                          record->spent_this_period, &available);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    if (lxp_u128_cmp(amount, available) > 0)
        return LXP_ERR_BUDGET_ALLOWANCE_EXCEEDED;
    status = lxp_u128_add(record->spent_this_period, amount, &updated);
    if (status != LXP_OK) return status;
    record->spent_this_period = updated;
    return LXP_OK;
}

lxp_result lx_budget_remaining(lx_budget_record *record,
                               const lx_account *budget_account,
                               lxp_u128 *remaining)
{
    lxp_u128 allowance;
    lxp_u128 balance;
    lxp_result status;
    if (record == NULL || budget_account == NULL || remaining == NULL ||
        memcmp(record->budget_account, budget_account->id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_sub(record->per_period_limit,
                          record->spent_this_period, &allowance);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    status = lxp_state_balance_get(budget_account, record->asset_id, &balance);
    if (status != LXP_OK) return status;
    *remaining = lxp_u128_cmp(allowance, balance) < 0 ? allowance : balance;
    return LXP_OK;
}

lxp_result lx_budget_spend_prepare(lx_budget_record *record,
                                   uint64_t batch_timestamp,
                                   lxp_u128 balance, lxp_u128 amount)
{
    lxp_u128 allowance;
    lxp_result status;
    if (record == NULL) return LXP_ERR_NON_CANONICAL;
    if (record->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (record->revoked) return LXP_ERR_BUDGET_REVOKED;
    if (record->expiry <= batch_timestamp) return LXP_ERR_EXPIRED;
    status = lx_budget_rollover(record, batch_timestamp);
    if (status != LXP_OK) return status;
    status = lxp_u128_sub(record->per_period_limit,
                          record->spent_this_period, &allowance);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    if (lxp_u128_cmp(amount, allowance) > 0)
        return LXP_ERR_BUDGET_ALLOWANCE_EXCEEDED;
    if (lxp_u128_cmp(amount, balance) > 0)
        return LXP_ERR_INSUFFICIENT_BUDGET_FUNDS;
    return lx_budget_allowance_debit(record, amount);
}

lxp_result lx_budget_spend_execute(lxp_module_ctx *ctx,
                                   const lx_budget_spend_request *request,
                                   lxp_receipt *receipt)
{
    lx_budget_record *record;
    lx_budget_record updated;
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_u128 balance;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->budget_id == NULL || request->budget_account == NULL ||
        request->recipient == NULL || request->asset == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_budget_lookup(request->store, request->budget_id, &record);
    if (status != LXP_OK || record->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (record->revoked) return LXP_ERR_BUDGET_REVOKED;
    if (record->expiry <= lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_EXPIRED;
    status = lx_budget_rollover(record, lxp_ctx_batch_timestamp_ms(ctx));
    if (status != LXP_OK) return status;
    if (memcmp(record->budget_account, request->budget_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0 ||
        request->budget_account->kind != LX_ACCOUNT_AGENT_BUDGET ||
        request->recipient->kind != LX_ACCOUNT_AGENT_MAIN)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_balance_get(request->budget_account, record->asset_id,
                                   &balance);
    if (status != LXP_OK) return status;
    updated = *record;
    status = lx_budget_spend_prepare(&updated, lxp_ctx_batch_timestamp_ms(ctx),
                                     balance, request->amount);
    if (status != LXP_OK) return status;

    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.leg_count = 1U;
    set.legs[0].from = request->budget_account;
    set.legs[0].to = request->recipient;
    (void)memcpy(set.legs[0].asset_id, record->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_BUDGET_SPEND;
    set.context = request->context;
    set.context.debit_authority_kind = LXP_AUTH_BUDGET_ALLOWANCE;
    (void)memcpy(set.context.authorized_from,
                 request->budget_account->id, 32U);
    lx_budget_bind_source_authority(&set, &source);
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    *record = updated;
    return LXP_OK;
}
