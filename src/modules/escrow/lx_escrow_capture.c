#include "layerx/lx_escrow.h"

#include "layerx/lxp_crypto.h"

#include <stdbool.h>
#include <string.h>

lxp_result lx_escrow_receipt_replay(const lx_escrow_store *store,
                                    const uint8_t key[32],
                                    lxp_receipt *receipt, bool *found)
{
    size_t i;
    if (store == NULL || key == NULL || receipt == NULL || found == NULL ||
        store->economic_result_count > LX_ESCROW_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    *found = false;
    for (i = 0U; i < store->economic_result_count; ++i) {
        if (memcmp(store->economic_results[i].key, key, 32U) == 0) {
            *receipt = store->economic_results[i].receipt;
            *found = true;
            return LXP_OK;
        }
    }
    return LXP_OK;
}

lxp_result lx_escrow_receipt_record(lx_escrow_store *store,
                                    const uint8_t key[32],
                                    const lxp_receipt *receipt)
{
    size_t index;
    if (store == NULL || key == NULL || receipt == NULL ||
        store->economic_result_count > LX_ESCROW_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (store->economic_result_count == LX_ESCROW_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    index = store->economic_result_count++;
    (void)memcpy(store->economic_results[index].key, key, 32U);
    store->economic_results[index].receipt = *receipt;
    return LXP_OK;
}

lxp_result lx_escrow_remaining(const lx_escrow_record *record,
                               const lx_account *escrow_account,
                               lxp_u128 *remaining)
{
    if (record == NULL || escrow_account == NULL || remaining == NULL ||
        memcmp(record->escrow_account, escrow_account->id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    return lxp_state_balance_get(escrow_account, record->asset_id, remaining);
}

static bool authority_can_capture(const lx_escrow_record *record,
                                  const lxp_authority_resolved *authority)
{
    if (authority == NULL) return false;
    if (memcmp(authority->principal, record->beneficiary, 32U) == 0)
        return true;
    return authority->kind == LXP_AUTHORITY_DELEGATED_CAPABILITY &&
           memcmp(authority->principal, record->owner, 32U) == 0;
}

static lxp_result execute_capture(lxp_module_ctx *ctx,
                                  const lx_escrow_capture_request *request,
                                  bool full, lxp_receipt *receipt)
{
    lx_escrow_record *record;
    lxp_transfer_set set;
    lxp_u128 remaining;
    lxp_u128 captured;
    lxp_u128 locked_after;
    lxp_u128 amount;
    lxp_result status;
    bool replayed;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->escrow_id == NULL || request->escrow_account == NULL ||
        request->beneficiary_account == NULL || request->asset == NULL ||
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
    if (status != LXP_OK) return LXP_ERR_CAPTURE_EXCEEDS_HOLD;
    if (record->state == LX_ESCROW_STATE_TIMED_OUT)
        return LXP_ERR_HOLD_EXPIRED;
    if (record->state == LX_ESCROW_STATE_DISPUTED)
        return LXP_ERR_HOLD_DISPUTED;
    if (record->expiry != 0U &&
        lxp_ctx_batch_timestamp_ms(ctx) >= record->expiry) {
        lx_escrow_release_request timeout_request;
        if (request->owner_account == NULL) return LXP_ERR_HOLD_EXPIRED;
        (void)memset(&timeout_request, 0, sizeof(timeout_request));
        timeout_request.store = request->store;
        timeout_request.escrow_id = request->escrow_id;
        timeout_request.escrow_account = request->escrow_account;
        timeout_request.owner_account = request->owner_account;
        timeout_request.asset = request->asset;
        (void)memcpy(timeout_request.idempotency_key,
                     request->idempotency_key, 32U);
        timeout_request.context = request->context;
        status = lx_escrow_timeout_execute(ctx, &timeout_request, receipt);
        return status == LXP_OK ? LXP_ERR_HOLD_EXPIRED : status;
    }
    if (record->state != LX_ESCROW_STATE_OPEN &&
        record->state != LX_ESCROW_STATE_PARTIALLY_CAPTURED)
        return LXP_ERR_ESCROW_STATE;
    if (!authority_can_capture(record, request->authority))
        return LXP_ERR_UNAUTHORIZED_CAPTURE;
    if (memcmp(record->beneficiary, request->beneficiary_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ESCROW_STATE;
    status = lx_escrow_remaining(record, request->escrow_account, &remaining);
    if (status != LXP_OK) return status;
    amount = full ? remaining : request->amount;
    if (lxp_u128_is_zero(amount) || lxp_u128_cmp(amount, remaining) > 0)
        return LXP_ERR_CAPTURE_EXCEEDS_HOLD;
    if (!full && lxp_u128_cmp(amount, remaining) == 0)
        return LXP_ERR_CAPTURE_EXCEEDS_HOLD;
    status = lxp_u128_add(record->captured_amount, amount, &captured);
    if (status != LXP_OK) return status;
    status = lxp_u128_sub(remaining, amount, &locked_after);
    if (status != LXP_OK) return status;

    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->escrow_account;
    set.legs[0].to = request->beneficiary_account;
    (void)memcpy(set.legs[0].asset_id, record->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = LXP_REASON_ESCROW_CAPTURE;
    set.context = request->context;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    record->captured_amount = captured;
    record->locked_amount = locked_after;
    record->state = full ? LX_ESCROW_STATE_CAPTURED :
                           LX_ESCROW_STATE_PARTIALLY_CAPTURED;
    return lx_escrow_receipt_record(request->store,
                                    request->idempotency_key, receipt);
}

lxp_result lx_escrow_capture_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_capture_request *request,
                                     lxp_receipt *receipt)
{
    return execute_capture(ctx, request, true, receipt);
}

lxp_result lx_escrow_partial_capture_execute(
    lxp_module_ctx *ctx, const lx_escrow_capture_request *request,
    lxp_receipt *receipt)
{
    return execute_capture(ctx, request, false, receipt);
}
