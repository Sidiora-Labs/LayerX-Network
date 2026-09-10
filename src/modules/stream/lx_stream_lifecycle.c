#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <stdbool.h>
#include <string.h>

static lxp_result lifecycle_load(lxp_module_ctx *ctx,
                                 const lx_stream_lifecycle_request *request,
                                 lx_stream_record *record)
{
    lxp_result status;
    if (ctx == NULL || request == NULL || request->stream_id == NULL ||
        request->authority == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_stream_load(ctx, request->stream_id, record);
    if (status != LXP_OK) return status;
    if (record->closed) return LXP_ERR_STREAM_CLOSED;
    if (memcmp(request->authority->principal, record->payer, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return LXP_OK;
}

lxp_result lx_stream_pause_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request)
{
    lx_stream_record record;
    lxp_u128 accrued;
    uint64_t timestamp;
    lxp_result status = lifecycle_load(ctx, request, &record);
    if (status != LXP_OK) return status;
    if (record.paused) return LXP_OK;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < record.last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
    if (record.mode == LX_STREAM_MODE_TIME) {
        status = lx_stream_accrue(&record, timestamp, &accrued);
        if (status != LXP_OK) return status;
    }
    record.paused = true;
    return lx_stream_save(ctx, &record);
}

lxp_result lx_stream_resume_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request)
{
    lx_stream_record record;
    uint64_t timestamp;
    lxp_result status = lifecycle_load(ctx, request, &record);
    if (status != LXP_OK) return status;
    if (!record.paused) return LXP_OK;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < record.last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
    record.last_accrual_timestamp = timestamp;
    record.paused = false;
    return lx_stream_save(ctx, &record);
}

static lxp_result close_accounts_check(
    const lx_stream_lifecycle_request *request,
    const lx_stream_record *record)
{
    if (request->stream_account == NULL || request->payer == NULL ||
        request->recipient == NULL || request->context.assets == NULL ||
        request->context.asset_count == 0U ||
        request->stream_account->kind != LX_ACCOUNT_AGENT_STREAM ||
        memcmp(request->stream_account->id, record->stream_account, 32U) != 0 ||
        memcmp(request->payer->id, record->payer, 32U) != 0 ||
        memcmp(request->recipient->id, record->recipient, 32U) != 0 ||
        memcmp(request->asset_id, record->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static void close_leg(lxp_transfer_leg *leg, lx_account *from,
                      lx_account *to, const uint8_t asset_id[32],
                      lxp_u128 amount, uint16_t reason)
{
    leg->from = from;
    leg->to = to;
    (void)memcpy(leg->asset_id, asset_id, 32U);
    leg->amount = amount;
    leg->reason = reason;
}

lxp_result lx_stream_close_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request,
    lxp_receipt *receipt)
{
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_u128 accrued;
    lxp_u128 unsettled;
    lxp_u128 balance;
    lxp_u128 payment;
    lxp_u128 refund;
    lxp_u128 settled;
    uint64_t timestamp;
    lxp_result status;
    bool found;
    if (ctx == NULL || request == NULL || receipt == NULL ||
        lxp_ct_is_zero(request->idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_result_load(ctx, request->idempotency_key, &result,
                                   &found);
    if (status != LXP_OK) return status;
    if (found) return lx_stream_result_receipt(&result, receipt);
    status = lifecycle_load(ctx, request, &record);
    if (status != LXP_OK) return status;
    status = close_accounts_check(request, &record);
    if (status != LXP_OK) return status;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < record.last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
    if (record.mode == LX_STREAM_MODE_TIME && !record.paused &&
        !record.underfunded) {
        status = lx_stream_accrue(&record, timestamp, &accrued);
        if (status != LXP_OK) return status;
    }
    status = lx_stream_settle_amount(&record, &unsettled);
    if (status == LXP_OK)
        status = lxp_state_balance_get(request->stream_account,
                                       record.asset_id, &balance);
    if (status != LXP_OK) return status;
    payment = lxp_u128_cmp(unsettled, balance) > 0 ? balance : unsettled;
    status = lxp_u128_sub(balance, payment, &refund);
    if (status != LXP_OK) return status;
    (void)memset(&result, 0, sizeof(result));
    (void)memset(&set, 0, sizeof(set));
    (void)memset(receipt, 0, sizeof(*receipt));
    result.ordinal = 7U;
    result.paid = payment;
    result.refunded = refund;
    if (!lxp_u128_is_zero(payment)) {
        close_leg(&set.legs[set.leg_count], request->stream_account,
                  request->recipient, record.asset_id, payment,
                  LXP_REASON_STREAM_DRAW);
        ++set.leg_count;
    }
    if (!lxp_u128_is_zero(refund)) {
        close_leg(&set.legs[set.leg_count], request->stream_account,
                  request->payer, record.asset_id, refund,
                  LXP_REASON_STREAM_REFUND);
        ++set.leg_count;
    }
    if (set.leg_count != 0U) {
        status = lx_stream_draw_context(ctx, request->stream_account,
                                        &request->context, &source,
                                        &set.context);
        if (status == LXP_OK)
            status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
        if (status != LXP_OK) return status;
        (void)memcpy(result.transfer_set_root, receipt->transfer_set_root,
                     32U);
        result.leg_count = (uint8_t)set.leg_count;
    }
    status = lxp_u128_add(record.settled_total, payment, &settled);
    if (status != LXP_OK) return status;
    record.settled_total = settled;
    record.accrued_total = settled;
    record.remainder_carry = (lxp_u128){ 0U, 0U };
    record.last_accrual_timestamp = timestamp;
    record.closed = true;
    record.paused = false;
    record.underfunded = false;
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
