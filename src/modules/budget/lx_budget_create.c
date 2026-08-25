#include "layerx/lx_budget.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <stdbool.h>
#include <string.h>

static const uint32_t activity_types[] = {
    LX_BUDGET_CREATE, LX_BUDGET_FUND, LX_BUDGET_AMEND,
    LX_BUDGET_DELEGATE_ADD, LX_BUDGET_DELEGATE_REMOVE,
    LX_BUDGET_SPEND, LX_BUDGET_CLOSE
};

typedef struct budget_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
} budget_decoded;

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t length,
                                void **decoded)
{
    budget_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        (payload == NULL && length != 0U)) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(budget_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (budget_decoded *)memory;
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = length;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const budget_decoded *value = (const budget_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const budget_decoded *value = (const budget_decoded *)decoded;
    (void)activity;
    (void)authority;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_emit_event(ctx, value->ordinal, value->payload,
                              value->payload_length);
}

static lxp_result module_epoch_end(lxp_module_ctx *ctx, uint64_t epoch,
                                   uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, 1U);
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_BUDGET, root);
}

const lxp_module_iface *lx_budget_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_BUDGET, 1U, "budget", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        lx_budget_epoch_begin, module_epoch_end, module_state_root, NULL
    };
    return &iface;
}

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

static lxp_result record_validate(const lx_budget_record *record)
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
    lxp_result status = record_validate(record);
    if (store == NULL || status != LXP_OK) return status;
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
    if (ctx == NULL || request == NULL || request->owner == NULL ||
        request->budget_account == NULL || request->asset == NULL ||
        receipt == NULL || request->owner->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->budget_account->kind != LX_ACCOUNT_AGENT_BUDGET ||
        lxp_u128_is_zero(request->amount)) return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->owner;
    set.legs[0].to = request->budget_account;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_BUDGET_FUND;
    set.context = request->context;
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_budget_create_execute(lxp_module_ctx *ctx,
                                    const lx_budget_fund_request *request,
                                    lxp_receipt *receipt)
{
    lx_budget_record *existing;
    lxp_result status;
    if (request == NULL || request->store == NULL) return LXP_ERR_NON_CANONICAL;
    status = record_validate(&request->record);
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
    if (request == NULL || request->store == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_budget_lookup(request->store, request->record.budget_id, &record);
    if (status != LXP_OK || record->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (memcmp(record->owner, request->owner->id, 32U) != 0 ||
        memcmp(record->budget_account, request->budget_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return emit_fund(ctx, request, receipt);
}
