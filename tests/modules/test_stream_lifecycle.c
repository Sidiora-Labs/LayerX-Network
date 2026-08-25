#include "layerx/lx_stream.h"
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

static void record_init(lx_stream_record *record, const lx_account *payer,
                        const lx_account *stream_account,
                        const lx_account *recipient,
                        const lx_asset_record *asset, lx_stream_mode mode,
                        uint8_t marker)
{
    (void)memset(record, 0, sizeof(*record));
    record->stream_id[0] = marker;
    (void)memcpy(record->payer, payer->id, 32U);
    (void)memcpy(record->stream_account, stream_account->id, 32U);
    (void)memcpy(record->recipient, recipient->id, 32U);
    (void)memcpy(record->asset_id, asset->asset_id, 32U);
    record->mode = mode;
    record->rate = (lxp_u128){ 0U, mode == LX_STREAM_MODE_TIME ? 10U : 1U };
    record->rate_unit = mode == LX_STREAM_MODE_TIME ? 1000U : 1U;
    record->start_timestamp = 100U;
    record->last_accrual_timestamp = 100U;
    record->total_cap = (lxp_u128){ 0U, 500U };
    if (mode == LX_STREAM_MODE_METERED) {
        record->meter_authorities[0][0] = 99U;
        record->meter_authority_count = 1U;
    }
}

static void lifecycle_init(lx_stream_lifecycle_request *request,
                           lx_stream_store *store,
                           const lx_stream_record *record,
                           lx_account *stream_account, lx_account *payer,
                           lx_account *recipient,
                           const lx_asset_record *asset,
                           const lxp_transfer_asset_state *asset_state,
                           const lxp_authority_resolved *authority,
                           uint8_t key)
{
    (void)memset(request, 0, sizeof(*request));
    request->store = store;
    request->stream_id = record->stream_id;
    request->stream_account = stream_account;
    request->payer = payer;
    request->recipient = recipient;
    request->asset = asset;
    request->authority = authority;
    request->idempotency_key[0] = key;
    request->context.assets = asset_state;
    request->context.asset_count = 1U;
    request->context.sequence_account = stream_account;
    (void)memcpy(request->context.authorized_from, stream_account->id, 32U);
}

static void settle_init(lx_stream_settle_request *request,
                        lx_stream_store *store,
                        const lx_stream_record *record,
                        lx_account *stream_account, lx_account *recipient,
                        const lx_asset_record *asset,
                        const lxp_transfer_asset_state *asset_state,
                        uint8_t key)
{
    (void)memset(request, 0, sizeof(*request));
    request->store = store;
    request->stream_id = record->stream_id;
    request->stream_account = stream_account;
    request->recipient = recipient;
    request->asset = asset;
    request->idempotency_key[0] = key;
    request->context.assets = asset_state;
    request->context.asset_count = 1U;
    request->context.sequence_account = stream_account;
    request->context.actor_sequence = stream_account->next_sequence;
    (void)memcpy(request->context.authorized_from, stream_account->id, 32U);
}

static int fixed_lifecycle(lxp_kernel *kernel, lxp_arena *arena,
                           const lx_asset_record *asset,
                           const lxp_transfer_asset_state *asset_state)
{
    lx_account payer;
    lx_account stream_account;
    lx_account recipient;
    lx_stream_store store;
    lx_stream_record record;
    lx_stream_record *stored;
    lx_stream_lifecycle_request request;
    lxp_authority_resolved authority;
    lxp_authority_resolved unauthorized;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    lxp_receipt replayed;
    lxp_u128 accrued;
    bool found;

    (void)memset(&payer, 0, sizeof(payer));
    (void)memset(&stream_account, 0, sizeof(stream_account));
    (void)memset(&recipient, 0, sizeof(recipient));
    (void)memset(&store, 0, sizeof(store));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(&unauthorized, 0, sizeof(unauthorized));
    (void)memset(&receipt, 0, sizeof(receipt));
    payer.id[0] = 1U; payer.kind = LX_ACCOUNT_AGENT_MAIN;
    stream_account.id[0] = 2U; stream_account.kind = LX_ACCOUNT_AGENT_STREAM;
    recipient.id[0] = 3U; recipient.kind = LX_ACCOUNT_AGENT_MAIN;
    (void)memcpy(authority.principal, payer.id, 32U);
    (void)memcpy(unauthorized.principal, recipient.id, 32U);
    if (lxp_ledger_bootstrap_balance(&payer, asset->asset_id,
                                     (lxp_u128){ 0U, 50U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&stream_account, asset->asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&recipient, asset->asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK)
        return 1;
    record_init(&record, &payer, &stream_account, &recipient, asset,
                LX_STREAM_MODE_TIME, 4U);
    if (lx_stream_state_put(&store, &record) != LXP_OK ||
        lx_stream_lookup(&store, record.stream_id, &stored) != LXP_OK)
        return 1;
    lifecycle_init(&request, &store, stored, &stream_account, &payer,
                   &recipient, asset, asset_state, &authority, 5U);
    store.economic_result_count = LX_STREAM_IDEMPOTENCY_CAPACITY + 1U;
    if (lx_stream_receipt_replay(&store, request.idempotency_key,
                                 &replayed, &found) !=
            LXP_ERR_NON_CANONICAL ||
        lx_stream_receipt_record(&store, request.idempotency_key,
                                 &receipt) != LXP_ERR_NON_CANONICAL)
        return 1;
    store.economic_result_count = 0U;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_STREAM, 1100U, 0U, 1U,
                            1000U, arena, true) != LXP_OK ||
        lx_stream_pause_execute(&ctx, &request) != LXP_OK || !stored->paused ||
        stored->accrued_total.lo != 10U ||
        lx_stream_accrue(stored, 4000U, &accrued) != LXP_OK ||
        !lxp_u128_is_zero(accrued) || stored->accrued_total.lo != 10U)
        return 1;
    request.authority = &unauthorized;
    if (lx_stream_resume_execute(&ctx, &request) !=
        LXP_ERR_UNAUTHORIZED_DEBIT) return 1;
    request.authority = &authority;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_STREAM, 5000U, 0U, 2U,
                            1000U, arena, true) != LXP_OK ||
        lx_stream_resume_execute(&ctx, &request) != LXP_OK || stored->paused ||
        stored->last_accrual_timestamp != 5000U)
        return 1;

    request.context.inject_failure = true;
    request.context.failure_after_leg = 0U;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_STREAM, 6000U, 0U, 3U,
                            1000U, arena, true) != LXP_OK ||
        lx_stream_close_execute(&ctx, &request, &receipt) != LXP_ERR_IO ||
        stored->closed || payer.balance.lo != 50U ||
        stream_account.balance.lo != 100U || recipient.balance.lo != 0U)
        return 1;
    request.context.inject_failure = false;
    if (lx_stream_close_execute(&ctx, &request, &receipt) != LXP_OK ||
        !stored->closed || stream_account.balance.lo != 0U ||
        recipient.balance.lo != 20U || payer.balance.lo != 130U)
        return 1;
    replayed = receipt;
    (void)memset(&receipt, 0, sizeof(receipt));
    if (lx_stream_close_execute(&ctx, &request, &receipt) != LXP_OK ||
        memcmp(&receipt, &replayed, sizeof(receipt)) != 0 ||
        lx_stream_pause_execute(&ctx, &request) != LXP_ERR_STREAM_CLOSED ||
        payer.balance.lo + recipient.balance.lo != 150U)
        return 1;
    return 0;
}

static uint32_t random_next(uint32_t *state)
{
    uint32_t value = *state;
    value ^= value << 13U;
    value ^= value >> 17U;
    value ^= value << 5U;
    *state = value;
    return value;
}

static int randomized_lifecycle(lxp_kernel *kernel, lxp_arena *arena,
                                const lx_asset_record *asset,
                                const lxp_transfer_asset_state *asset_state)
{
    uint32_t random = 0x4c585031U;
    size_t run;
    for (run = 0U; run < 32U; ++run) {
        lx_account payer;
        lx_account stream_account;
        lx_account recipient;
        lx_stream_store store;
        lx_stream_record record;
        lx_stream_record *stored;
        lx_stream_lifecycle_request lifecycle;
        lx_stream_settle_request settle;
        lxp_authority_resolved authority;
        lxp_module_ctx ctx;
        lxp_receipt receipt;
        lxp_u128 accrued;
        uint64_t meter = 0U;
        uint64_t timestamp = 100U;
        size_t step;

        (void)memset(&payer, 0, sizeof(payer));
        (void)memset(&stream_account, 0, sizeof(stream_account));
        (void)memset(&recipient, 0, sizeof(recipient));
        (void)memset(&store, 0, sizeof(store));
        (void)memset(&authority, 0, sizeof(authority));
        payer.id[0] = 11U; payer.kind = LX_ACCOUNT_AGENT_MAIN;
        stream_account.id[0] = 12U;
        stream_account.kind = LX_ACCOUNT_AGENT_STREAM;
        recipient.id[0] = 13U; recipient.kind = LX_ACCOUNT_AGENT_MAIN;
        (void)memcpy(authority.principal, payer.id, 32U);
        if (lxp_ledger_bootstrap_balance(&payer, asset->asset_id,
                                         (lxp_u128){ 0U, 500U }, 0U) != LXP_OK ||
            lxp_ledger_bootstrap_balance(&stream_account, asset->asset_id,
                                         (lxp_u128){ 0U, 500U }, 0U) != LXP_OK ||
            lxp_ledger_bootstrap_balance(&recipient, asset->asset_id,
                                         (lxp_u128){ 0U, 0U }, 0U) != LXP_OK)
            return 1;
        record_init(&record, &payer, &stream_account, &recipient, asset,
                    LX_STREAM_MODE_METERED, (uint8_t)(20U + run));
        if (lx_stream_state_put(&store, &record) != LXP_OK ||
            lx_stream_lookup(&store, record.stream_id, &stored) != LXP_OK)
            return 1;
        lifecycle_init(&lifecycle, &store, stored, &stream_account, &payer,
                       &recipient, asset, asset_state, &authority, 240U);
        for (step = 0U; step < 12U; ++step) {
            uint32_t operation = random_next(&random) % 4U;
            timestamp += (uint64_t)(random_next(&random) % 100U) + 1U;
            if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_STREAM,
                                    timestamp, 0U, (uint64_t)step + 10U,
                                    1000U, arena, true) != LXP_OK)
                return 1;
            if (operation == 0U) {
                if (lx_stream_pause_execute(&ctx, &lifecycle) != LXP_OK)
                    return 1;
            } else if (operation == 1U) {
                if (lx_stream_resume_execute(&ctx, &lifecycle) != LXP_OK)
                    return 1;
            } else if (operation == 2U) {
                meter += (uint64_t)(random_next(&random) % 7U);
                if (lx_stream_metered_accrue(stored, meter, &accrued) != LXP_OK)
                    return 1;
            } else {
                settle_init(&settle, &store, stored, &stream_account,
                            &recipient, asset, asset_state,
                            (uint8_t)(step + 1U));
                if (lx_stream_settle_execute(&ctx, &settle, &receipt) != LXP_OK)
                    return 1;
            }
        }
        lifecycle.idempotency_key[0] = 250U;
        lifecycle.context.actor_sequence = stream_account.next_sequence;
        timestamp += 1U;
        if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_STREAM, timestamp, 0U,
                                30U, 1000U, arena, true) != LXP_OK ||
            lx_stream_close_execute(&ctx, &lifecycle, &receipt) != LXP_OK ||
            !stored->closed || !lxp_u128_is_zero(stream_account.balance) ||
            payer.balance.lo + recipient.balance.lo != 1000U)
            return 1;
    }
    return 0;
}

int main(void)
{
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 42U;
    if (lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_stream_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        fixed_lifecycle(&kernel, &arena, &asset, &asset_state) != 0 ||
        randomized_lifecycle(&kernel, &arena, &asset, &asset_state) != 0 ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
