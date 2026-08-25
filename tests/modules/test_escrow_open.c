#include "layerx/lx_escrow.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <string.h>

static size_t emitted_legs;
static uint16_t emitted_reason;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    emitted_legs = set->leg_count;
    emitted_reason = set->legs[0].reason;
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
    lxp_transfer_asset_state asset_state;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *escrow_account;
    lx_escrow_store store;
    lx_escrow_open_request request;
    lx_escrow_record *stored;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    const char *owner_name = "agent:did:key:alice:main";
    const char *escrow_name = "agent:did:key:alice:escrow:order-7";

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    asset.symbol_length = 3U;
    (void)memcpy(asset.symbol, "USD", 4U);
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    (void)memset(&store, 0, sizeof(store));
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U,
                          (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)owner_name, strlen(owner_name),
                              1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &owner) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)escrow_name, strlen(escrow_name),
                              2U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &escrow_account) != LXP_OK ||
        !lxp_u128_is_zero(owner->balance) ||
        !lxp_u128_is_zero(escrow_account->balance) ||
        lxp_ledger_bootstrap_balance(owner, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 10U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;

    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.owner = owner;
    request.escrow_account = escrow_account;
    request.asset = &asset;
    request.amount = (lxp_u128){ 0U, 40U };
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = owner;
    (void)memcpy(request.context.authorized_from, owner->id, 32U);
    request.record.escrow_id[0] = 9U;
    (void)memcpy(request.record.owner, owner->id, 32U);
    (void)memcpy(request.record.escrow_account, escrow_account->id, 32U);
    request.record.beneficiary[0] = 7U;
    request.record.arbiter[0] = 8U;
    (void)memcpy(request.record.asset_id, asset.asset_id, 32U);
    request.record.locked_amount = request.amount;
    request.record.state = LX_ESCROW_STATE_OPEN;
    request.record.expiry = 5000U;
    request.record.dispute_window = 600U;
    request.record.terms_hash[0] = 10U;
    request.record.agreement_reference[0] = 11U;

    store.count = LX_ESCROW_STORE_CAPACITY + 1U;
    if (lx_escrow_state_put(&store, &request.record) !=
            LXP_ERR_NON_CANONICAL ||
        lx_escrow_open_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NON_CANONICAL ||
        owner->balance.lo != 100U || !lxp_u128_is_zero(escrow_account->balance))
        return 1;
    store.count = 0U;
    if (lx_escrow_open_execute(&ctx, &request, &receipt) != LXP_OK ||
        emitted_legs != 1U || emitted_reason != LXP_REASON_ESCROW_LOCK ||
        owner->balance.hi != 0U || owner->balance.lo != 60U ||
        escrow_account->balance.hi != 0U || escrow_account->balance.lo != 40U ||
        lx_escrow_lookup(&store, request.record.escrow_id, &stored) != LXP_OK ||
        memcmp(stored, &request.record, sizeof(*stored)) != 0 ||
        lx_escrow_open_execute(&ctx, &request, &receipt) != LXP_ERR_ESCROW_STATE ||
        owner->balance.lo != 60U || escrow_account->balance.lo != 40U ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
