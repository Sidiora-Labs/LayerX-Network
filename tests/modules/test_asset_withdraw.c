#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <string.h>

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

int main(void)
{
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *agent;
    lx_account *withdrawals;
    lx_account *reserve;
    lxp_transfer_asset_state asset_state;
    lx_asset_transfer_request transfer;
    lx_withdrawal_request withdrawal;
    lx_withdrawal_store store;
    lx_finalized_checkpoint checkpoint;
    uint8_t nullifier[32];
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    lxp_u128 total;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    (void)memcpy(asset.symbol, "A", 2U); asset.symbol_length = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U; asset.custody_reference_length = 1U;
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U, (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"agent:did:key:a:main", 20U, 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &agent) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"system:paxeer-withdrawals", 25U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, &withdrawals) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"system:paxeer-reserve", 21U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, &reserve) != LXP_OK ||
        lxp_ledger_bootstrap_balance(agent, asset.asset_id,
            (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ASSET, 10U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK) return 1;
    (void)memset(&transfer, 0, sizeof(transfer));
    transfer.from = agent; transfer.to = withdrawals; transfer.asset = &asset;
    transfer.amount = (lxp_u128){ 0U, 40U };
    transfer.context.assets = &asset_state; transfer.context.asset_count = 1U;
    transfer.context.sequence_account = agent;
    (void)memcpy(transfer.context.authorized_from, agent->id, 32U);
    (void)memset(&withdrawal, 0, sizeof(withdrawal));
    withdrawal.network_id = 7U; withdrawal.withdrawal_id[0] = 2U;
    (void)memcpy(withdrawal.account_id, agent->id, 32U);
    (void)memcpy(withdrawal.asset_id, asset.asset_id, 32U);
    withdrawal.amount = transfer.amount; withdrawal.checkpoint_id[0] = 3U;
    (void)memset(&store, 0, sizeof(store));
    if (lx_withdrawal_nullifier(&withdrawal, nullifier) != LXP_OK ||
        lx_asset_withdraw_request(&ctx, &transfer, &withdrawal, &store,
                                  &receipt) != LXP_OK ||
        agent->balance.lo != 60U || withdrawals->balance.lo != 40U ||
        reserve->balance.lo != 0U || !lx_asset_nullifier_seen(&store, nullifier) ||
        lx_asset_total_units(&assets, &accounts, asset.asset_id, &total) != LXP_OK ||
        total.lo != 100U) return 1;
    if (lx_asset_withdraw_request(&ctx, &transfer, &withdrawal, &store,
                                  &receipt) !=
            LXP_ERR_WITHDRAWAL_ALREADY_SETTLED || agent->balance.lo != 60U)
        return 1;
    withdrawal.withdrawal_id[0] = 4U;
    transfer.amount = (lxp_u128){ 0U, 70U };
    withdrawal.amount = transfer.amount;
    transfer.context.actor_sequence = 1U;
    if (lx_asset_withdraw_request(&ctx, &transfer, &withdrawal, &store,
                                  &receipt) != LXP_ERR_INSUFFICIENT_BALANCE ||
        agent->balance.lo != 60U || withdrawals->balance.lo != 40U) return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.checkpoint_id[0] = 3U; checkpoint.state_root[0] = 5U;
    checkpoint.finalized = true;
    {
        lxp_transfer_context settlement = { 0 };
        settlement.assets = &asset_state; settlement.asset_count = 1U;
        settlement.protocol_system_capability = true;
        if (lx_asset_withdraw_settle(&ctx, withdrawals, reserve, &asset,
                                     &checkpoint, nullifier, &store, settlement,
                                     &receipt) != LXP_OK ||
            withdrawals->balance.lo != 0U || reserve->balance.lo != 40U ||
            lx_asset_total_units(&assets, &accounts, asset.asset_id, &total) !=
                LXP_OK || total.lo != 100U ||
            lx_asset_withdraw_settle(&ctx, withdrawals, reserve, &asset,
                                     &checkpoint, nullifier, &store, settlement,
                                     &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED) return 1;
    }
    store.count = LX_DEPOSIT_NULLIFIER_CAPACITY + 1U;
    if (!lx_asset_nullifier_seen(&store, nullifier) ||
        lx_asset_withdraw_request(&ctx, &transfer, &withdrawal, &store,
                                  &receipt) != LXP_ERR_NON_CANONICAL)
        return 1;
    {
        lxp_transfer_context settlement = { 0 };
        if (lx_asset_withdraw_settle(&ctx, withdrawals, reserve, &asset,
                                     &checkpoint, nullifier, &store,
                                     settlement, &receipt) !=
            LXP_ERR_NON_CANONICAL)
            return 1;
    }
    if (lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
}
