#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

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
    if (batch_timestamp < record->last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
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

lxp_result lx_stream_draw_context(lxp_module_ctx *ctx,
                                  lx_account *stream_account,
                                  const lxp_transfer_context *caller,
                                  lxp_transfer_source_authority *source,
                                  lxp_transfer_context *context)
{
    lxp_result status;
    if (ctx == NULL || stream_account == NULL || caller == NULL ||
        context == NULL || stream_account->next_sequence == UINT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_transfer_source(source, stream_account,
                                       LXP_AUTH_PROTOCOL_MODULE);
    if (status != LXP_OK) return status;
    *context = *caller;
    context->batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    context->sequence_account = stream_account;
    context->actor_sequence = stream_account->next_sequence;
    context->debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memcpy(context->authorized_from, stream_account->id, 32U);
    context->source_authorities = source;
    context->source_authority_count = 1U;
    context->protocol_system_capability = false;
    context->program_spend_token = 0U;
    context->has_client_balance = false;
    context->idempotency_seen = false;
    return LXP_OK;
}

static lxp_result settle_check(const lx_stream_settle_request *request)
{
    if (request == NULL || request->stream_id == NULL ||
        request->stream_account == NULL || request->recipient == NULL ||
        request->context.assets == NULL ||
        request->context.asset_count == 0U ||
        request->stream_account->kind != LX_ACCOUNT_AGENT_STREAM ||
        lxp_ct_is_zero(request->asset_id, 32U) ||
        lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_settle_execute(lxp_module_ctx *ctx,
                                    const lx_stream_settle_request *request,
                                    lxp_receipt *receipt)
{
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_u128 newly_accrued;
    lxp_u128 unsettled;
    lxp_u128 balance;
    lxp_u128 amount;
    lxp_u128 settled_total;
    uint64_t timestamp;
    lxp_result status;
    bool found;
    if (ctx == NULL || receipt == NULL) return LXP_ERR_NON_CANONICAL;
    status = settle_check(request);
    if (status != LXP_OK) return status;
    status = lx_stream_result_load(ctx, request->idempotency_key, &result,
                                   &found);
    if (status != LXP_OK) return status;
    if (found) {
        if (request->stream_id == NULL || result.ordinal != 4U ||
            memcmp(result.stream_id, request->stream_id, 32U) != 0)
            return LXP_ERR_CONTEXT_MISMATCH;
        return lx_stream_result_receipt(&result, receipt);
    }
    status = lx_stream_load(ctx, request->stream_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_STREAM_CLOSED;
    if (memcmp(record.stream_account, request->stream_account->id, 32U) != 0 ||
        memcmp(record.recipient, request->recipient->id, 32U) != 0 ||
        memcmp(record.asset_id, request->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < record.last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
    if (record.mode == LX_STREAM_MODE_TIME) {
        status = lx_stream_accrue(&record, timestamp, &newly_accrued);
        if (status != LXP_OK) return status;
    }
    status = lx_stream_settle_amount(&record, &unsettled);
    if (status != LXP_OK) return status;
    status = lxp_state_balance_get(request->stream_account, record.asset_id,
                                   &balance);
    if (status != LXP_OK) return status;
    amount = lxp_u128_cmp(unsettled, balance) > 0 ? balance : unsettled;
    (void)memset(&result, 0, sizeof(result));
    (void)memset(receipt, 0, sizeof(*receipt));
    result.ordinal = 4U;
    (void)memcpy(result.stream_id, request->stream_id, 32U);
    if (lxp_u128_is_zero(amount)) {
        if (!lxp_u128_is_zero(unsettled)) {
            status = lx_stream_mark_underfunded(&record, timestamp, amount);
            if (status != LXP_OK) return status;
        }
    } else {
        if (lxp_u128_cmp(amount, unsettled) < 0) {
            status = lx_stream_mark_underfunded(&record, timestamp, amount);
        } else {
            status = lxp_u128_add(record.settled_total, amount,
                                  &settled_total);
            if (status == LXP_OK) record.settled_total = settled_total;
        }
        if (status != LXP_OK) return status;
        (void)memset(&set, 0, sizeof(set));
        set.leg_count = 1U;
        set.legs[0].from = request->stream_account;
        set.legs[0].to = request->recipient;
        (void)memcpy(set.legs[0].asset_id, record.asset_id, 32U);
        set.legs[0].amount = amount;
        set.legs[0].reason = LXP_REASON_STREAM_DRAW;
        status = lx_stream_draw_context(ctx, request->stream_account,
                                        &request->context, &source,
                                        &set.context);
        if (status != LXP_OK) return status;
        status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
        if (status != LXP_OK) return status;
        (void)memcpy(result.transfer_set_root, receipt->transfer_set_root,
                     32U);
        result.paid = amount;
        result.leg_count = 1U;
    }
    status = lx_stream_save(ctx, &record);
    if (status == LXP_OK)
        status = lx_stream_result_save(ctx, request->idempotency_key,
                                       &result);
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(ctx);
        return status;
    }
    return LXP_OK;
}
