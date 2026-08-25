#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"

#include <stdbool.h>
#include <string.h>

lxp_result lx_stream_authority_check(const lx_account *account,
                                     lxp_authorization_kind authority_kind,
                                     uint16_t origin_module_id,
                                     uint16_t reason)
{
    if (account == NULL) return LXP_ERR_NON_CANONICAL;
    if (account->kind != LX_ACCOUNT_AGENT_STREAM) return LXP_OK;
    if (origin_module_id != LXP_MODULE_STREAM ||
        authority_kind != LXP_AUTH_PROTOCOL_MODULE ||
        (reason != LXP_REASON_STREAM_DRAW &&
         reason != LXP_REASON_STREAM_REFUND))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

lxp_result lx_stream_settle_amount(const lx_stream_record *record,
                                   lxp_u128 *amount)
{
    if (record == NULL || amount == NULL) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_sub(record->accrued_total, record->settled_total,
                     amount) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

lxp_result lx_stream_mark_underfunded(lx_stream_record *record,
                                      uint64_t batch_timestamp,
                                      lxp_u128 settled_amount)
{
    lxp_u128 settled_total;
    lxp_result status;
    if (record == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_add(record->settled_total, settled_amount,
                          &settled_total);
    if (status != LXP_OK) return status;
    record->settled_total = settled_total;
    record->accrued_total = settled_total;
    record->underfunded = true;
    record->last_accrual_timestamp = batch_timestamp;
    record->remainder_carry = (lxp_u128){ 0U, 0U };
    return LXP_OK;
}

lxp_result lx_stream_receipt_replay(const lx_stream_store *store,
                                    const uint8_t key[32],
                                    lxp_receipt *receipt, bool *found)
{
    size_t i;
    if (store == NULL || key == NULL || receipt == NULL || found == NULL ||
        store->economic_result_count > LX_STREAM_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    *found = false;
    for (i = 0U; i < store->economic_result_count; ++i)
        if (memcmp(store->economic_results[i].key, key, 32U) == 0) {
            *receipt = store->economic_results[i].receipt;
            *found = true;
            break;
        }
    return LXP_OK;
}

lxp_result lx_stream_receipt_record(lx_stream_store *store,
                                    const uint8_t key[32],
                                    const lxp_receipt *receipt)
{
    size_t index;
    if (store == NULL || key == NULL || receipt == NULL ||
        store->economic_result_count > LX_STREAM_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (store->economic_result_count == LX_STREAM_IDEMPOTENCY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    index = store->economic_result_count++;
    (void)memcpy(store->economic_results[index].key, key, 32U);
    store->economic_results[index].receipt = *receipt;
    return LXP_OK;
}

lxp_result lx_stream_settle_execute(lxp_module_ctx *ctx,
                                    const lx_stream_settle_request *request,
                                    lxp_receipt *receipt)
{
    lx_stream_record *record;
    lx_stream_record updated;
    lxp_transfer_set set;
    lxp_u128 newly_accrued;
    lxp_u128 unsettled;
    lxp_u128 balance;
    lxp_u128 amount;
    lxp_u128 settled_total;
    lxp_result status;
    bool found;
    bool underfunded;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->stream_id == NULL || request->stream_account == NULL ||
        request->recipient == NULL || request->asset == NULL || receipt == NULL ||
        lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_receipt_replay(request->store,
                                      request->idempotency_key,
                                      receipt, &found);
    if (status != LXP_OK || found) return status;
    status = lx_stream_lookup(request->store, request->stream_id, &record);
    if (status != LXP_OK) return status;
    if (record->closed) return LXP_ERR_STREAM_CLOSED;
    if (request->store->economic_result_count ==
        LX_STREAM_IDEMPOTENCY_CAPACITY) return LXP_ERR_ARENA_EXHAUSTED;
    if (memcmp(record->stream_account, request->stream_account->id, 32U) != 0 ||
        memcmp(record->recipient, request->recipient->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    updated = *record;
    if (updated.mode == LX_STREAM_MODE_TIME) {
        status = lx_stream_accrue(&updated,
                                  lxp_ctx_batch_timestamp_ms(ctx),
                                  &newly_accrued);
        if (status != LXP_OK) return status;
    }
    status = lx_stream_settle_amount(&updated, &unsettled);
    if (status != LXP_OK || lxp_u128_is_zero(unsettled)) return status;
    status = lxp_state_balance_get(request->stream_account, record->asset_id,
                                   &balance);
    if (status != LXP_OK) return status;
    amount = lxp_u128_cmp(unsettled, balance) > 0 ? balance : unsettled;
    if (lxp_u128_is_zero(amount)) {
        return lx_stream_mark_underfunded(record,
                                          lxp_ctx_batch_timestamp_ms(ctx),
                                          amount);
    }
    underfunded = lxp_u128_cmp(amount, unsettled) < 0;
    if (underfunded) {
        status = lx_stream_mark_underfunded(&updated,
                                            lxp_ctx_batch_timestamp_ms(ctx),
                                            amount);
    } else {
        status = lxp_u128_add(updated.settled_total, amount, &settled_total);
        if (status == LXP_OK) updated.settled_total = settled_total;
    }
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->stream_account;
    set.legs[0].to = request->recipient;
    (void)memcpy(set.legs[0].asset_id, record->asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = LXP_REASON_STREAM_DRAW;
    set.context = request->context;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    *record = updated;
    return lx_stream_receipt_record(request->store,
                                    request->idempotency_key, receipt);
}
