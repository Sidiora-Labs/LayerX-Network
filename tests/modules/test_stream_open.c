#include "layerx/lx_stream.h"
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
    lx_account payer;
    lx_account stream_account;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_stream_store store;
    lx_stream_fund_request request;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    (void)memset(&payer, 0, sizeof(payer));
    (void)memset(&stream_account, 0, sizeof(stream_account));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    payer.id[0] = 1U; payer.kind = LX_ACCOUNT_AGENT_MAIN;
    stream_account.id[0] = 2U; stream_account.kind = LX_ACCOUNT_AGENT_STREAM;
    asset.asset_id[0] = 3U;
    if (lxp_ledger_bootstrap_balance(&payer, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&stream_account, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_stream_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_STREAM, 100U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.payer = &payer;
    request.stream_account = &stream_account;
    request.asset = &asset;
    request.amount = (lxp_u128){ 0U, 40U };
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = &payer;
    (void)memcpy(request.context.authorized_from, payer.id, 32U);
    request.record.stream_id[0] = 4U;
    (void)memcpy(request.record.payer, payer.id, 32U);
    (void)memcpy(request.record.stream_account, stream_account.id, 32U);
    request.record.recipient[0] = 5U;
    (void)memcpy(request.record.asset_id, asset.asset_id, 32U);
    request.record.mode = LX_STREAM_MODE_METERED;
    request.record.rate = (lxp_u128){ 0U, 2U };
    request.record.rate_unit = 1U;
    request.record.start_timestamp = 100U;
    request.record.last_accrual_timestamp = 100U;
    request.record.total_cap = (lxp_u128){ 0U, 1000U };
    request.record.meter_authorities[0][0] = 6U;
    request.record.meter_authority_count = 1U;
    store.count = LX_STREAM_STORE_CAPACITY + 1U;
    if (lx_stream_state_put(&store, &request.record) !=
            LXP_ERR_NON_CANONICAL ||
        lx_stream_open_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NON_CANONICAL ||
        payer.balance.lo != 100U || !lxp_u128_is_zero(stream_account.balance))
        return 1;
    store.count = 0U;
    if (lx_stream_open_execute(&ctx, &request, &receipt) != LXP_OK ||
        legs != 1U || reason != LXP_REASON_STREAM_FUND ||
        payer.balance.lo != 60U || stream_account.balance.lo != 40U ||
        store.count != 1U || store.records[0].meter_authority_count != 1U)
        return 1;
    store.records[0].underfunded = true;
    request.amount = (lxp_u128){ 0U, 10U };
    request.context.actor_sequence = payer.next_sequence;
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_STREAM, 500U, 0U, 2U,
                            1000U, &arena, true) != LXP_OK ||
        lx_stream_top_up_execute(&ctx, &request, &receipt) != LXP_OK ||
        payer.balance.lo != 50U || stream_account.balance.lo != 50U ||
        store.records[0].underfunded ||
        store.records[0].last_accrual_timestamp != 500U ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
