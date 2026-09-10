#include "layerx/lx_budget.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static size_t transfer_calls;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    ++transfer_calls;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

int main(void)
{
    lx_account budget_account;
    lx_account owner;
    lx_account recipient;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_budget_store store;
    lx_budget_spend_request spend;
    lx_budget_close_request close_request;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    lxp_transfer_leg direct;
    lxp_transfer_context direct_context;
    lxp_transfer_result direct_result;

    (void)memset(&budget_account, 0, sizeof(budget_account));
    (void)memset(&owner, 0, sizeof(owner));
    (void)memset(&recipient, 0, sizeof(recipient));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    budget_account.id[0] = 1U; budget_account.kind = LX_ACCOUNT_AGENT_BUDGET;
    owner.id[0] = 2U; owner.kind = LX_ACCOUNT_AGENT_MAIN;
    recipient.id[0] = 3U; recipient.kind = LX_ACCOUNT_AGENT_MAIN;
    asset.asset_id[0] = 4U;
    if (lxp_ledger_bootstrap_balance(&budget_account, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&owner, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&recipient, asset.asset_id,
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
    store.count = 1U;
    store.records[0].budget_id[0] = 5U;
    (void)memcpy(store.records[0].owner, owner.id, 32U);
    (void)memcpy(store.records[0].budget_account, budget_account.id, 32U);
    (void)memcpy(store.records[0].asset_id, asset.asset_id, 32U);
    store.records[0].per_period_limit = (lxp_u128){ 0U, 100U };
    store.records[0].configured_period_limit = (lxp_u128){ 0U, 100U };
    store.records[0].period_start = 1U;
    store.records[0].period_length = 1000U;
    store.records[0].expiry = 1000U;
    store.records[0].revocation_sequence = 1U;
    (void)memset(&spend, 0, sizeof(spend));
    spend.store = &store;
    spend.budget_id = store.records[0].budget_id;
    spend.budget_account = &budget_account;
    spend.recipient = &recipient;
    spend.asset = &asset;
    spend.amount = (lxp_u128){ 0U, 20U };
    spend.context.assets = &asset_state;
    spend.context.asset_count = 1U;
    spend.context.sequence_account = &budget_account;
    (void)memcpy(spend.context.authorized_from, budget_account.id, 32U);
    if (lx_budget_spend_execute(&ctx, &spend, &receipt) != LXP_OK ||
        recipient.balance.lo != 20U || budget_account.balance.lo != 80U)
        return 1;

    (void)memset(&direct, 0, sizeof(direct));
    (void)memset(&direct_context, 0, sizeof(direct_context));
    direct.from = &budget_account;
    direct.to = &owner;
    (void)memcpy(direct.asset_id, asset.asset_id, 32U);
    direct.amount = (lxp_u128){ 0U, 1U };
    direct.reason = LXP_REASON_PAYMENT;
    direct_context.assets = &asset_state;
    direct_context.asset_count = 1U;
    direct_context.sequence_account = &budget_account;
    direct_context.actor_sequence = budget_account.next_sequence;
    direct_context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(direct_context.authorized_from, budget_account.id, 32U);
    if (lxp_apply_transfer(&direct, &direct_context, &direct_result) !=
        LXP_ERR_UNAUTHORIZED_DEBIT) return 1;

    (void)memset(&close_request, 0, sizeof(close_request));
    close_request.store = &store;
    close_request.budget_id = store.records[0].budget_id;
    close_request.budget_account = &budget_account;
    close_request.owner = &owner;
    close_request.asset = &asset;
    close_request.amount = (lxp_u128){ 0U, 30U };
    close_request.context.assets = &asset_state;
    close_request.context.asset_count = 1U;
    close_request.context.sequence_account = &budget_account;
    close_request.context.actor_sequence = budget_account.next_sequence;
    (void)memcpy(close_request.context.authorized_from,
                 budget_account.id, 32U);
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 2U,
                            1000U, &arena, true) != LXP_OK ||
        lx_budget_defund_execute(&ctx, &close_request, &receipt) != LXP_OK ||
        budget_account.balance.lo != 50U || owner.balance.lo != 30U ||
        store.records[0].per_period_limit.lo != 70U)
        return 1;
    close_request.context.actor_sequence = budget_account.next_sequence;
    close_request.revocation_sequence = 1U;
    if (lx_budget_revoke_execute(&ctx, &close_request, &receipt) !=
        LXP_ERR_STALE_REVOCATION) return 1;
    close_request.revocation_sequence = 2U;
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 100U, 0U, 3U,
                            1000U, &arena, true) != LXP_OK ||
        lx_budget_revoke_execute(&ctx, &close_request, &receipt) != LXP_OK ||
        budget_account.balance.lo != 0U || owner.balance.lo != 80U ||
        !store.records[0].revoked || transfer_calls != 3U)
        return 1;
    spend.context.actor_sequence = budget_account.next_sequence;
    spend.amount = (lxp_u128){ 0U, 1U };
    if (lx_budget_spend_execute(&ctx, &spend, &receipt) !=
            LXP_ERR_BUDGET_REVOKED || recipient.balance.lo != 20U)
        return 1;

    store.records[0].revoked = false;
    store.records[0].closed = false;
    close_request.revocation_sequence = 3U;
    close_request.context.actor_sequence = budget_account.next_sequence;
    if (lx_budget_close_execute(&ctx, &close_request, &receipt) != LXP_OK ||
        transfer_calls != 3U || !store.records[0].closed ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
