#include "layerx/lx_escrow.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

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

static void write_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static int run_timeout(bool delayed, uint8_t root[32])
{
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *escrow_account;
    lx_escrow_store store;
    lx_escrow_record record;
    lx_escrow_runtime runtime;
    lxp_transfer_asset_state asset_state;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    const char *owner_name = "agent:did:key:owner:main";
    const char *escrow_name = "agent:did:key:owner:escrow:timeout";
    lxp_transfer_set unauthorized;
    lxp_receipt receipt;
    lx_escrow_capture_request capture;
    lxp_authority_resolved authority;
    uint8_t root_input[33];
    volatile uint64_t elapsed_work = 0U;
    uint64_t i;

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
        lxp_ledger_bootstrap_balance(escrow_account, asset.asset_id,
                                     (lxp_u128){ 0U, 50U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 1000U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    (void)memset(&record, 0, sizeof(record));
    record.escrow_id[0] = 1U;
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    (void)memcpy(record.beneficiary, owner->id, 32U);
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 50U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    if (lx_escrow_state_put(&store, &record) != LXP_OK) return 1;
    runtime.store = &store;
    runtime.accounts = &accounts;
    runtime.assets = &assets;
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ESCROW,
                                       &runtime) != LXP_OK)
        return 1;

    (void)memset(&unauthorized, 0, sizeof(unauthorized));
    (void)memset(&receipt, 0, sizeof(receipt));
    unauthorized.leg_count = 1U;
    unauthorized.legs[0].from = escrow_account;
    unauthorized.legs[0].to = owner;
    (void)memcpy(unauthorized.legs[0].asset_id, asset.asset_id, 32U);
    unauthorized.legs[0].amount = (lxp_u128){ 0U, 1U };
    unauthorized.legs[0].reason = LXP_REASON_PAYMENT;
    unauthorized.context.assets = &asset_state;
    unauthorized.context.asset_count = 1U;
    unauthorized.context.sequence_account = escrow_account;
    (void)memcpy(unauthorized.context.authorized_from,
                 escrow_account->id, 32U);
    if (lxp_ctx_emit_transfer_set(&ctx, &unauthorized, &receipt) !=
            LXP_ERR_UNAUTHORIZED_ESCROW_SPEND ||
        escrow_account->balance.lo != 50U)
        return 1;

    if (delayed)
        for (i = 0U; i < UINT64_C(1000000); ++i) elapsed_work += i;
    store.count = LX_ESCROW_STORE_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    store.count = 1U;
    store.economic_result_count = LX_ESCROW_IDEMPOTENCY_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    store.economic_result_count = 0U;
    accounts.count = LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    accounts.count = 2U;
    assets.count = LX_ASSET_REGISTRY_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    assets.count = 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) != LXP_OK ||
        escrow_account->balance.lo != 0U || owner->balance.lo != 50U ||
        store.records[0].state != LX_ESCROW_STATE_TIMED_OUT)
        return 1;

    (void)memset(&capture, 0, sizeof(capture));
    (void)memset(&authority, 0, sizeof(authority));
    capture.store = &store;
    capture.escrow_id = record.escrow_id;
    capture.escrow_account = escrow_account;
    capture.beneficiary_account = owner;
    capture.owner_account = owner;
    capture.asset = &asset;
    capture.amount = (lxp_u128){ 0U, 1U };
    capture.authority = &authority;
    capture.idempotency_key[0] = 9U;
    if (lx_escrow_partial_capture_execute(&ctx, &capture, &receipt) !=
        LXP_ERR_HOLD_EXPIRED)
        return 1;

    write_u64(root_input, owner->balance.hi);
    write_u64(root_input + 8U, owner->balance.lo);
    write_u64(root_input + 16U, escrow_account->balance.hi);
    write_u64(root_input + 24U, escrow_account->balance.lo);
    root_input[32] = (uint8_t)store.records[0].state;
    if (lxp_hash_sha256(root_input, sizeof(root_input), root) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    (void)elapsed_work;
    return 0;
}

static int explicit_release(void)
{
    lx_account owner;
    lx_account escrow_account;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lx_escrow_store store;
    lx_escrow_record record;
    lx_escrow_release_request request;
    lxp_authority_resolved authority;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    lxp_receipt replayed;
    bool found;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    (void)memset(&owner, 0, sizeof(owner));
    (void)memset(&escrow_account, 0, sizeof(escrow_account));
    (void)memset(&asset, 0, sizeof(asset));
    (void)memset(&store, 0, sizeof(store));
    (void)memset(&record, 0, sizeof(record));
    (void)memset(&authority, 0, sizeof(authority));
    owner.id[0] = 1U;
    owner.kind = LX_ACCOUNT_AGENT_MAIN;
    escrow_account.id[0] = 2U;
    escrow_account.kind = LX_ACCOUNT_AGENT_ESCROW;
    asset.asset_id[0] = 3U;
    if (lxp_ledger_bootstrap_balance(&owner, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&escrow_account, asset.asset_id,
                                     (lxp_u128){ 0U, 20U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 500U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    record.escrow_id[0] = 4U;
    (void)memcpy(record.owner, owner.id, 32U);
    (void)memcpy(record.escrow_account, escrow_account.id, 32U);
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 20U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    if (lx_escrow_state_put(&store, &record) != LXP_OK) return 1;
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, owner.id, 32U);
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.escrow_id = record.escrow_id;
    request.escrow_account = &escrow_account;
    request.owner_account = &owner;
    request.asset = &asset;
    request.authority = &authority;
    request.idempotency_key[0] = 5U;
    request.context.assets = &asset_state;
    request.context.asset_count = 1U;
    request.context.sequence_account = &escrow_account;
    (void)memcpy(request.context.authorized_from, escrow_account.id, 32U);
    store.economic_result_count = LX_ESCROW_IDEMPOTENCY_CAPACITY + 1U;
    if (lx_escrow_receipt_replay(&store, request.idempotency_key,
                                 &replayed, &found) !=
            LXP_ERR_NON_CANONICAL ||
        lx_escrow_receipt_record(&store, request.idempotency_key,
                                 &receipt) != LXP_ERR_NON_CANONICAL)
        return 1;
    store.economic_result_count = 0U;
    if (lx_escrow_release_execute(&ctx, &request, &receipt) != LXP_OK ||
        escrow_account.balance.lo != 0U || owner.balance.lo != 20U ||
        store.records[0].state != LX_ESCROW_STATE_RELEASED ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

int main(void)
{
    uint8_t immediate[32];
    uint8_t delayed[32];
    if (explicit_release() != 0 || run_timeout(false, immediate) != 0 ||
        run_timeout(true, delayed) != 0 || memcmp(immediate, delayed, 32U) != 0)
        return 1;
    return 0;
}
