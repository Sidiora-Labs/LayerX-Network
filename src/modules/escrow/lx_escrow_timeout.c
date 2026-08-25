#include "layerx/lx_escrow.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <stdbool.h>
#include <string.h>

static bool active_state(lx_escrow_status state)
{
    return state == LX_ESCROW_STATE_OPEN ||
           state == LX_ESCROW_STATE_PARTIALLY_CAPTURED;
}

static lxp_result execute_release(lxp_module_ctx *ctx,
                                  const lx_escrow_release_request *request,
                                  bool timeout, lxp_receipt *receipt)
{
    lx_escrow_record *record;
    lxp_transfer_set set;
    lxp_u128 remaining;
    lxp_result status;
    bool replayed;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->escrow_id == NULL || request->escrow_account == NULL ||
        request->owner_account == NULL || request->asset == NULL ||
        receipt == NULL || lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_receipt_replay(request->store,
                                      request->idempotency_key,
                                      receipt, &replayed);
    if (status != LXP_OK || replayed) return status;
    if (request->store->economic_result_count ==
        LX_ESCROW_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = lx_escrow_lookup(request->store, request->escrow_id, &record);
    if (status != LXP_OK)
        return LXP_ERR_ESCROW_STATE;
    if (record->state == LX_ESCROW_STATE_DISPUTED)
        return LXP_ERR_HOLD_DISPUTED;
    if (!active_state(record->state)) return LXP_ERR_ESCROW_STATE;
    if (memcmp(record->owner, request->owner_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    if (timeout) {
        if (record->expiry == 0U ||
            lxp_ctx_batch_timestamp_ms(ctx) < record->expiry)
            return LXP_ERR_NOT_YET_VALID;
    } else if (request->authority == NULL ||
               memcmp(request->authority->principal, record->owner, 32U) != 0) {
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    }
    status = lx_escrow_remaining(record, request->escrow_account, &remaining);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(remaining)) {
        (void)memset(&set, 0, sizeof(set));
        set.leg_count = 1U;
        set.legs[0].from = request->escrow_account;
        set.legs[0].to = request->owner_account;
        (void)memcpy(set.legs[0].asset_id, record->asset_id, 32U);
        set.legs[0].amount = remaining;
        set.legs[0].reason = LXP_REASON_ESCROW_RELEASE;
        set.context = request->context;
        status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
        if (status != LXP_OK) return status;
    }
    record->state = timeout ? LX_ESCROW_STATE_TIMED_OUT :
                              LX_ESCROW_STATE_RELEASED;
    record->locked_amount = (lxp_u128){ 0U, 0U };
    return lx_escrow_receipt_record(request->store,
                                    request->idempotency_key, receipt);
}

lxp_result lx_escrow_release_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_release_request *request,
                                     lxp_receipt *receipt)
{
    return execute_release(ctx, request, false, receipt);
}

lxp_result lx_escrow_timeout_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_release_request *request,
                                     lxp_receipt *receipt)
{
    return execute_release(ctx, request, true, receipt);
}

static lx_account *account_by_id(lx_account_registry *accounts,
                                 const uint8_t id[32])
{
    size_t i;
    if (accounts == NULL || id == NULL ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return NULL;
    for (i = 0U; i < accounts->count; ++i)
        if (memcmp(accounts->accounts[i].id, id, 32U) == 0)
            return &accounts->accounts[i];
    return NULL;
}

static lxp_result timeout_key(const lx_escrow_record *record, uint8_t key[32])
{
    uint8_t input[40];
    size_t i;
    (void)memcpy(input, record->escrow_id, 32U);
    for (i = 0U; i < 8U; ++i)
        input[32U + i] = (uint8_t)(record->expiry >> ((7U - i) * 8U));
    return lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, input, sizeof(input), key);
}

lxp_result lx_escrow_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp)
{
    lx_escrow_runtime *runtime;
    size_t i;
    if (ctx == NULL || epoch != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    runtime = (lx_escrow_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL) return lxp_ctx_charge_gas(ctx, 1U);
    if (runtime->store == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL ||
        runtime->store->count > LX_ESCROW_STORE_CAPACITY ||
        runtime->store->economic_result_count >
            LX_ESCROW_IDEMPOTENCY_CAPACITY ||
        runtime->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        runtime->assets->count > LX_ASSET_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < runtime->store->count; ++i) {
        lx_escrow_record *record = &runtime->store->records[i];
        lx_account *escrow_account;
        lx_account *owner_account;
        lx_asset_record *asset;
        lxp_transfer_asset_state asset_state;
        lx_escrow_release_request request;
        lxp_receipt receipt;
        lxp_result status;
        if (!active_state(record->state) || record->expiry == 0U ||
            timestamp < record->expiry)
            continue;
        escrow_account = account_by_id(runtime->accounts,
                                       record->escrow_account);
        owner_account = account_by_id(runtime->accounts, record->owner);
        if (escrow_account == NULL || owner_account == NULL)
            return LXP_ERR_ESCROW_STATE;
        status = lx_asset_lookup(runtime->assets, record->asset_id, &asset);
        if (status != LXP_OK) return status;
        status = lx_asset_transfer_state(asset, &asset_state);
        if (status != LXP_OK) return status;
        (void)memset(&request, 0, sizeof(request));
        (void)memset(&receipt, 0, sizeof(receipt));
        request.store = runtime->store;
        request.escrow_id = record->escrow_id;
        request.escrow_account = escrow_account;
        request.owner_account = owner_account;
        request.asset = asset;
        request.context.assets = &asset_state;
        request.context.asset_count = 1U;
        request.context.sequence_account = escrow_account;
        request.context.actor_sequence = escrow_account->next_sequence;
        request.context.batch_timestamp = timestamp;
        (void)memcpy(request.context.authorized_from,
                     escrow_account->id, 32U);
        status = timeout_key(record, request.idempotency_key);
        if (status == LXP_OK)
            status = lx_escrow_timeout_execute(ctx, &request, &receipt);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}
