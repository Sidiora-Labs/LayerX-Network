#include "layerx/lx_budget.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static size_t legs;
static uint16_t reason;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    legs = set->leg_count;
    reason = set->legs[0].reason;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

int main(void)
{
    lx_account owner;
    lx_account budget_account;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_budget_store store;
    lx_budget_fund_request request;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    (void)memset(&owner, 0, sizeof(owner));
    (void)memset(&budget_account, 0, sizeof(budget_account));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    owner.id[0] = 1U; owner.kind = LX_ACCOUNT_AGENT_MAIN;
    budget_account.id[0] = 2U; budget_account.kind = LX_ACCOUNT_AGENT_BUDGET;
    asset.asset_id[0] = 3U;
    if (lxp_ledger_bootstrap_balance(&owner, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&budget_account, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_budget_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.owner = &owner;
    request.budget_account = &budget_account;
    request.asset = &asset;
    request.amount = (lxp_u128){ 0U, 50U };
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = &owner;
    (void)memcpy(request.context.authorized_from, owner.id, 32U);
    request.record.budget_id[0] = 4U;
    (void)memcpy(request.record.owner, owner.id, 32U);
    (void)memcpy(request.record.budget_account, budget_account.id, 32U);
    (void)memcpy(request.record.asset_id, asset.asset_id, 32U);
    request.record.per_period_limit = (lxp_u128){ 0U, 100U };
    request.record.period_length = 1000U;
    request.record.period_start = 100U;
    request.record.rollover_policy = LX_BUDGET_ROLLOVER_CAPPED;
    request.record.carry_cap = (lxp_u128){ 0U, 20U };
    request.record.purpose_hash[0] = 5U;
    request.record.expiry = 10000U;
    request.record.revocation_sequence = 1U;
    store.count = LX_BUDGET_STORE_CAPACITY + 1U;
    if (lx_budget_state_put(&store, &request.record) !=
            LXP_ERR_NON_CANONICAL ||
        lx_budget_create_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NON_CANONICAL ||
        owner.balance.lo != 100U || !lxp_u128_is_zero(budget_account.balance))
        return 1;
    store.count = 0U;
    if (lx_budget_create_execute(&ctx, &request, &receipt) != LXP_OK ||
        legs != 1U || reason != LXP_REASON_BUDGET_FUND ||
        owner.balance.lo != 50U || budget_account.balance.lo != 50U ||
        store.count != 1U || store.records[0].per_period_limit.lo != 100U ||
        lx_budget_create_execute(&ctx, &request, &receipt) !=
            LXP_ERR_SEQUENCE_REUSED || owner.balance.lo != 50U)
        return 1;
    request.amount = (lxp_u128){ 0U, 10U };
    request.context.actor_sequence = owner.next_sequence;
    if (lx_budget_fund_execute(&ctx, &request, &receipt) != LXP_OK ||
        owner.balance.lo != 40U || budget_account.balance.lo != 60U ||
        store.records[0].per_period_limit.lo != 100U ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
