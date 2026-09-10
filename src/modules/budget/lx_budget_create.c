#include "layerx/lx_budget.h"

#include "layerx/lxp_crypto.h"

#include <stdbool.h>
#include <string.h>

lxp_result lx_budget_lookup(lx_budget_store *store,
                            const uint8_t budget_id[32],
                            lx_budget_record **record)
{
    size_t i;
    if (store == NULL || budget_id == NULL || record == NULL ||
        store->count > LX_BUDGET_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->records[i].budget_id, budget_id, 32U) == 0) {
            *record = &store->records[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_budget_record_validate(const lx_budget_record *record)
{
    if (record == NULL || lxp_ct_is_zero(record->budget_id, 32U) ||
        lxp_u128_is_zero(record->per_period_limit) ||
        record->period_length == 0U || record->expiry == 0U ||
        record->expiry <= record->period_start ||
        record->rollover_policy < LX_BUDGET_ROLLOVER_NONE ||
        record->rollover_policy > LX_BUDGET_ROLLOVER_CAPPED ||
        record->delegate_count > LX_BUDGET_MAX_DELEGATES)
        return LXP_ERR_NON_CANONICAL;
    if (record->rollover_policy == LX_BUDGET_ROLLOVER_NONE &&
        !lxp_u128_is_zero(record->carry_cap)) return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_budget_state_put(lx_budget_store *store,
                               const lx_budget_record *record)
{
    lx_budget_record *existing;
    lxp_result status = lx_budget_record_validate(record);
    if (store == NULL || status != LXP_OK ||
        store->count > LX_BUDGET_STORE_CAPACITY)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    if (lx_budget_lookup(store, record->budget_id, &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    if (store->count == LX_BUDGET_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    store->records[store->count++] = *record;
    if (lxp_u128_is_zero(store->records[store->count - 1U].configured_period_limit))
        store->records[store->count - 1U].configured_period_limit =
            record->per_period_limit;
    return LXP_OK;
}

static lxp_result emit_fund(lxp_module_ctx *ctx,
                            const lx_budget_fund_request *request,
                            lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    if (ctx == NULL || request == NULL || request->owner == NULL ||
        request->budget_account == NULL || request->asset == NULL ||
        receipt == NULL || request->owner->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->budget_account->kind != LX_ACCOUNT_AGENT_BUDGET ||
        lxp_u128_is_zero(request->amount)) return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.leg_count = 1U;
    set.legs[0].from = request->owner;
    set.legs[0].to = request->budget_account;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_BUDGET_FUND;
    set.context = request->context;
    set.context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(set.context.authorized_from, request->owner->id, 32U);
    lx_budget_bind_source_authority(&set, &source);
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_budget_create_execute(lxp_module_ctx *ctx,
                                    const lx_budget_fund_request *request,
                                    lxp_receipt *receipt)
{
    lx_budget_record *existing;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->owner == NULL || request->budget_account == NULL ||
        request->asset == NULL || receipt == NULL ||
        request->store->count > LX_BUDGET_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lx_budget_record_validate(&request->record);
    if (status != LXP_OK) return status;
    if (lx_budget_lookup(request->store, request->record.budget_id,
                         &existing) == LXP_OK ||
        request->store->count == LX_BUDGET_STORE_CAPACITY)
        return LXP_ERR_SEQUENCE_REUSED;
    if (memcmp(request->record.owner, request->owner->id, 32U) != 0 ||
        memcmp(request->record.budget_account,
               request->budget_account->id, 32U) != 0 ||
        memcmp(request->record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = emit_fund(ctx, request, receipt);
    if (status != LXP_OK) return status;
    return lx_budget_state_put(request->store, &request->record);
}

lxp_result lx_budget_fund_execute(lxp_module_ctx *ctx,
                                  const lx_budget_fund_request *request,
                                  lxp_receipt *receipt)
{
    lx_budget_record *record;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->owner == NULL || request->budget_account == NULL ||
        request->asset == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_budget_lookup(request->store, request->record.budget_id, &record);
    if (status != LXP_OK || record->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (memcmp(record->owner, request->owner->id, 32U) != 0 ||
        memcmp(record->budget_account, request->budget_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return emit_fund(ctx, request, receipt);
}
