#include "lx_escrow_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result execute_release(lxp_module_ctx *ctx,
                                  const lx_escrow_release_request *request,
                                  bool timeout, lxp_receipt *receipt)
{
    lx_escrow_record record;
    lx_escrow_settlement settlement;
    lxp_u128 remaining;
    lxp_result release;
    lxp_result status;
    bool replayed;
    if (ctx == NULL || request == NULL || request->escrow_id == NULL ||
        request->escrow_account == NULL || request->owner_account == NULL ||
        request->asset == NULL || receipt == NULL ||
        lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_receipt_replay(ctx, request->idempotency_key, receipt,
                                      &replayed);
    if (status != LXP_OK || replayed) return status;
    status = lx_escrow_lookup(ctx, request->escrow_id, &record);
    if (status != LXP_OK) return status;
    if (record.state == LX_ESCROW_STATE_DISPUTED)
        return LXP_ERR_HOLD_DISPUTED;
    if (!lx_escrow_active_state(record.state)) return LXP_ERR_ESCROW_STATE;
    if (memcmp(record.owner, request->owner_account->id, 32U) != 0 ||
        memcmp(record.escrow_account, request->escrow_account->id, 32U) != 0 ||
        memcmp(record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    if (timeout) {
        if (record.expiry == 0U ||
            lxp_ctx_batch_timestamp_ms(ctx) < record.expiry)
            return LXP_ERR_NOT_YET_VALID;
    } else if (request->authority == NULL ||
               memcmp(request->authority->principal, record.owner, 32U) != 0) {
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    }
    status = lx_escrow_remaining(&record, request->escrow_account, &remaining);
    if (status != LXP_OK) return status;
    (void)memset(&settlement, 0, sizeof(settlement));
    settlement.from = request->escrow_account;
    settlement.to = request->owner_account;
    settlement.asset = request->asset;
    settlement.amount = remaining;
    settlement.reason = LXP_REASON_ESCROW_RELEASE;
    status = lxp_module_staged_reserve(ctx, 2U);
    if (status != LXP_OK) return status;
    if (lxp_u128_is_zero(remaining)) {
        (void)memset(receipt, 0, sizeof(*receipt));
        status = LXP_OK;
    } else {
        status = lx_escrow_settle(ctx, &request->context, &settlement,
                                  LXP_AUTH_ESCROW, receipt);
    }
    release = lxp_module_staged_release(ctx, 2U);
    if (status != LXP_OK) return status;
    if (release != LXP_OK) return release;
    record.state = timeout ? LX_ESCROW_STATE_TIMED_OUT :
                             LX_ESCROW_STATE_RELEASED;
    record.locked_amount = (lxp_u128){ 0U, 0U };
    return lx_escrow_commit_result(ctx, &record, request->idempotency_key,
                                   &settlement, timeout ? 5U : 4U, receipt);
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

typedef struct sweep_state {
    uint64_t timestamp;
    size_t count;
    lx_escrow_record records[LX_ESCROW_SWEEP_CAPACITY];
    lxp_u128 moved[LX_ESCROW_SWEEP_CAPACITY];
} sweep_state;

static lxp_result sweep_collect(const lx_escrow_record *record, void *user)
{
    sweep_state *state = (sweep_state *)user;
    if (!lx_escrow_active_state(record->state) || record->expiry == 0U ||
        state->timestamp < record->expiry)
        return LXP_OK;
    if (state->count == (size_t)LX_ESCROW_SWEEP_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    state->records[state->count++] = *record;
    return LXP_OK;
}

static lxp_result sweep_asset_index(lxp_transfer_asset_state *assets,
                                    size_t *asset_count,
                                    const lx_asset_record *asset)
{
    size_t i;
    for (i = 0U; i < *asset_count; ++i)
        if (memcmp(assets[i].asset_id, asset->asset_id, 32U) == 0)
            return LXP_OK;
    if (*asset_count == (size_t)LX_ESCROW_SWEEP_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    return lx_asset_transfer_state(asset, &assets[(*asset_count)++]);
}

static lxp_result sweep_settle(lxp_module_ctx *ctx,
                               lx_escrow_runtime *runtime,
                               sweep_state *sweep, uint64_t timestamp,
                               lxp_receipt *receipt, bool *applied)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority authorities[LX_ESCROW_SWEEP_CAPACITY];
    lxp_transfer_asset_state assets[LX_ESCROW_SWEEP_CAPACITY];
    size_t asset_count = 0U;
    size_t authority_count = 0U;
    size_t i;
    lxp_result status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(authorities, 0, sizeof(authorities));
    (void)memset(assets, 0, sizeof(assets));
    *applied = false;
    for (i = 0U; i < sweep->count; ++i) {
        const lx_escrow_record *record = &sweep->records[i];
        lx_account *escrow_account;
        lx_account *owner_account;
        lx_asset_record *asset;
        lxp_u128 remaining;
        size_t j;
        status = lx_escrow_resolve_account(runtime, record->escrow_account,
                                           &escrow_account);
        if (status == LXP_OK)
            status = lx_escrow_resolve_account(runtime, record->owner,
                                               &owner_account);
        if (status != LXP_OK) return LXP_ERR_ESCROW_STATE;
        for (j = 0U; j < authority_count; ++j)
            if (memcmp(authorities[j].authorized_from,
                       escrow_account->id, 32U) == 0)
                return LXP_FATAL_INVARIANT;
        status = lx_asset_lookup(runtime->assets, record->asset_id, &asset);
        if (status != LXP_OK) return status;
        status = sweep_asset_index(assets, &asset_count, asset);
        if (status != LXP_OK) return status;
        status = lx_escrow_remaining(record, escrow_account, &remaining);
        if (status != LXP_OK) return status;
        sweep->moved[i] = remaining;
        if (lxp_u128_is_zero(remaining)) continue;
        if (escrow_account->next_sequence == UINT64_MAX)
            return LXP_ERR_OVERFLOW;
        set.legs[set.leg_count].from = escrow_account;
        set.legs[set.leg_count].to = owner_account;
        (void)memcpy(set.legs[set.leg_count].asset_id, record->asset_id, 32U);
        set.legs[set.leg_count].amount = remaining;
        set.legs[set.leg_count].reason = LXP_REASON_ESCROW_RELEASE;
        ++set.leg_count;
        (void)memcpy(authorities[authority_count].authorized_from,
                     escrow_account->id, 32U);
        authorities[authority_count].debit_authority_kind = LXP_AUTH_ESCROW;
        ++authority_count;
    }
    if (set.leg_count == 0U) {
        (void)memset(receipt, 0, sizeof(*receipt));
        return LXP_OK;
    }
    set.context.assets = assets;
    set.context.asset_count = asset_count;
    set.context.source_authorities = authorities;
    set.context.source_authority_count = authority_count;
    set.context.debit_authority_kind = LXP_AUTH_ESCROW;
    set.context.origin_module_id = LXP_MODULE_ESCROW;
    set.context.sequence_account = set.legs[0].from;
    set.context.actor_sequence = set.legs[0].from->next_sequence;
    set.context.batch_timestamp = timestamp;
    (void)memcpy(set.context.authorized_from, set.legs[0].from->id, 32U);
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    *applied = true;
    return LXP_OK;
}

lxp_result lx_escrow_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp)
{
    lx_escrow_runtime *runtime;
    sweep_state sweep;
    lxp_receipt receipt;
    size_t i;
    lxp_result release;
    lxp_result status;
    bool applied = false;
    if (ctx == NULL || epoch != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    if (lxp_ctx_module_runtime(ctx) == NULL)
        return lxp_ctx_charge_gas(ctx, 1U);
    runtime = lx_escrow_require_runtime(ctx);
    if (runtime == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(&sweep, 0, sizeof(sweep));
    (void)memset(&receipt, 0, sizeof(receipt));
    sweep.timestamp = timestamp;
    status = lx_escrow_state_iter(ctx, sweep_collect, &sweep);
    if (status != LXP_OK) return status;
    if (sweep.count == 0U) return lxp_ctx_charge_gas(ctx, 1U);
    status = lxp_ctx_charge_gas(ctx, sweep.count);
    if (status != LXP_OK) return status;
    status = lxp_module_staged_reserve(ctx, sweep.count * 2U);
    if (status != LXP_OK) return status;
    status = sweep_settle(ctx, runtime, &sweep, timestamp, &receipt, &applied);
    release = lxp_module_staged_release(ctx, sweep.count * 2U);
    if (status != LXP_OK) return status;
    if (release != LXP_OK) return release;
    for (i = 0U; i < sweep.count; ++i) {
        lx_escrow_record record = sweep.records[i];
        lx_escrow_economic_result result;
        uint8_t idempotency[32];
        uint8_t body[LX_ESCROW_EVENT_BYTES];
        record.state = LX_ESCROW_STATE_TIMED_OUT;
        record.locked_amount = (lxp_u128){ 0U, 0U };
        status = lx_escrow_timeout_key(&record, idempotency);
        if (status == LXP_OK) status = lx_escrow_state_write(ctx, &record);
        if (status != LXP_OK) return status;
        (void)memset(&result, 0, sizeof(result));
        (void)memcpy(result.escrow_id, record.escrow_id, 32U);
        result.ordinal = 5U;
        result.state_after = record.state;
        result.captured_after = record.captured_amount;
        result.locked_after = record.locked_amount;
        (void)memcpy(result.asset_id, record.asset_id, 32U);
        (void)memcpy(result.from, record.escrow_account, 32U);
        (void)memcpy(result.to, record.owner, 32U);
        result.amount = sweep.moved[i];
        if (applied)
            (void)memcpy(result.transfer_set_root, receipt.transfer_set_root,
                         32U);
        result.global_sequence = lxp_ctx_global_sequence(ctx);
        result.timestamp = timestamp;
        status = lx_escrow_receipt_record(ctx, idempotency, &result);
        if (status == LXP_OK)
            status = lx_escrow_event_body(&record, 5U, body);
        if (status == LXP_OK)
            status = lxp_ctx_emit_event(ctx, 5U, body, sizeof(body));
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}
