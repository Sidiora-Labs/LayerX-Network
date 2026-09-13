#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_STREAM_OPEN, LX_STREAM_TOP_UP, LX_STREAM_METER, LX_STREAM_SETTLE,
    LX_STREAM_PAUSE, LX_STREAM_RESUME, LX_STREAM_CLOSE
};

typedef union stream_typed_payload {
    lx_stream_open_payload open;
    lx_stream_amount_payload amount;
    lx_stream_meter_attestation meter;
    lx_stream_keyed_payload keyed;
    lx_stream_id_payload id;
} stream_typed_payload;

typedef struct stream_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
    stream_typed_payload *typed;
} stream_decoded;

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result decode_typed(uint16_t ordinal, const uint8_t *payload,
                               size_t length, stream_typed_payload *typed)
{
    switch (ordinal) {
    case 1U: return lx_stream_open_decode(payload, length, &typed->open);
    case 2U: return lx_stream_amount_decode(payload, length, &typed->amount);
    case 3U: return lx_stream_meter_decode(payload, length, &typed->meter);
    case 4U:
    case 7U: return lx_stream_keyed_decode(payload, length, &typed->keyed);
    case 5U:
    case 6U: return lx_stream_id_decode(payload, length, &typed->id);
    default: return LXP_ERR_UNKNOWN_ACTIVITY;
    }
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t length,
                                void **decoded)
{
    stream_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        payload == NULL || length == 0U) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(stream_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (stream_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = length;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value->typed),
                                 _Alignof(stream_typed_payload), &memory);
    if (status != LXP_OK) return status;
    value->typed = (stream_typed_payload *)memory;
    (void)memset(value->typed, 0, sizeof(*value->typed));
    status = decode_typed(ordinal, payload, length, value->typed);
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

static lxp_result validate_typed(lxp_module_ctx *ctx,
                                 const lxp_authority_resolved *authority,
                                 const stream_decoded *value)
{
    lx_stream_record record;
    lx_stream_economic_result result;
    bool found;
    lxp_result status;
    if (value->ordinal == 1U) {
        lx_stream_record candidate = value->typed->open.record;
        (void)memcpy(candidate.payer, authority->principal, 32U);
        status = lx_stream_record_validate(&candidate);
        if (status != LXP_OK) return status;
        status = lx_stream_load(ctx, candidate.stream_id, &record);
        if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
        return status == LXP_ERR_UNKNOWN_FIELD ? LXP_OK : status;
    }
    if (value->ordinal == 4U || value->ordinal == 7U) {
        status = lx_stream_result_load(ctx,
                                       value->typed->keyed.idempotency_key,
                                       &result, &found);
        if (status != LXP_OK) return status;
        if (found) {
            if (result.ordinal != value->ordinal ||
                memcmp(result.stream_id, value->typed->keyed.stream_id, 32U) != 0)
                return LXP_ERR_CONTEXT_MISMATCH;
            return LXP_OK;
        }
        status = lx_stream_load(ctx, value->typed->keyed.stream_id, &record);
    } else if (value->ordinal == 2U) {
        status = lx_stream_load(ctx, value->typed->amount.stream_id, &record);
    } else if (value->ordinal == 3U) {
        status = lx_stream_load(ctx, value->typed->meter.stream_id, &record);
    } else {
        status = lx_stream_load(ctx, value->typed->id.stream_id, &record);
    }
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_STREAM_CLOSED;
    if (value->ordinal != 3U) return LXP_OK;
    if (record.mode != LX_STREAM_MODE_METERED) return LXP_ERR_NON_CANONICAL;
    return lx_stream_meter_authority_check(&record, &value->typed->meter);
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const stream_decoded *value = (const stream_decoded *)decoded;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->typed == NULL || value->ordinal == 0U ||
        value->ordinal > 7U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_activity_module_id(activity->activity_type) != LXP_MODULE_STREAM ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (authority->kind < LXP_AUTHORITY_OWNER ||
        authority->kind > LXP_AUTHORITY_DELEGATED_CAPABILITY ||
        lxp_ct_is_zero(authority->principal, 32U))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = validate_typed(ctx, authority, value);
    if (status != LXP_OK) return status;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

#include "lx_stream_execution.h"

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const stream_decoded *value = (const stream_decoded *)decoded;
    (void)effects;
    return stream_execute_typed(ctx, activity, authority, value);
}

static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, 1U);
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_STREAM, root);
}

const lxp_module_iface *lx_stream_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_STREAM, 1U, "stream", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_state_root, NULL
    };
    return &iface;
}

static lxp_result fund_context(lxp_module_ctx *ctx,
                               const lx_stream_fund_request *request,
                               lxp_transfer_source_authority *source,
                               lxp_transfer_set *set)
{
    lxp_result status = lx_stream_transfer_source(source, request->payer,
                                                  LXP_AUTH_OWNER);
    if (status != LXP_OK) return status;
    (void)memset(set, 0, sizeof(*set));
    set->leg_count = 1U;
    set->legs[0].from = request->payer;
    set->legs[0].to = request->stream_account;
    (void)memcpy(set->legs[0].asset_id, request->asset_id, 32U);
    set->legs[0].amount = request->amount;
    set->legs[0].reason = LXP_REASON_STREAM_FUND;
    set->context = request->context;
    set->context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    set->context.sequence_account = request->payer;
    set->context.actor_sequence = request->payer->next_sequence;
    set->context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(set->context.authorized_from, request->payer->id, 32U);
    set->context.source_authorities = source;
    set->context.source_authority_count = 1U;
    set->context.protocol_system_capability = false;
    set->context.program_spend_token = 0U;
    set->context.has_client_balance = false;
    set->context.idempotency_seen = false;
    return LXP_OK;
}

static lxp_result fund_check(const lx_stream_fund_request *request)
{
    if (request == NULL || request->payer == NULL ||
        request->stream_account == NULL ||
        request->context.assets == NULL ||
        request->context.asset_count == 0U ||
        request->payer->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->stream_account->kind != LX_ACCOUNT_AGENT_STREAM ||
        request->payer->next_sequence == UINT64_MAX ||
        lxp_ct_is_zero(request->asset_id, 32U) ||
        lxp_u128_is_zero(request->amount))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_open_execute(lxp_module_ctx *ctx,
                                  const lx_stream_fund_request *request,
                                  lxp_receipt *receipt)
{
    lx_stream_record existing;
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_result status;
    if (ctx == NULL || receipt == NULL) return LXP_ERR_NON_CANONICAL;
    status = fund_check(request);
    if (status != LXP_OK) return status;
    status = lx_stream_record_validate(&request->record);
    if (status != LXP_OK) return status;
    if (memcmp(request->record.payer, request->payer->id, 32U) != 0 ||
        memcmp(request->record.stream_account,
               request->stream_account->id, 32U) != 0 ||
        memcmp(request->record.asset_id, request->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_u128_is_zero(request->record.accrued_total) ||
        !lxp_u128_is_zero(request->record.settled_total) ||
        !lxp_u128_is_zero(request->record.remainder_carry) ||
        request->record.cumulative_meter != 0U ||
        request->record.underfunded || request->record.paused ||
        request->record.closed ||
        request->record.last_accrual_timestamp !=
            request->record.start_timestamp)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_load(ctx, request->record.stream_id, &existing);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = fund_context(ctx, request, &source, &set);
    if (status != LXP_OK) return status;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    status = lx_stream_save(ctx, &request->record);
    if (status != LXP_OK) lxp_module_ctx_rollback(ctx);
    return status;
}

lxp_result lx_stream_top_up_execute(lxp_module_ctx *ctx,
                                    const lx_stream_fund_request *request,
                                    lxp_receipt *receipt)
{
    lx_stream_record record;
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    uint64_t timestamp;
    lxp_result status;
    if (ctx == NULL || receipt == NULL) return LXP_ERR_NON_CANONICAL;
    status = fund_check(request);
    if (status != LXP_OK) return status;
    status = lx_stream_load(ctx, request->record.stream_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_STREAM_CLOSED;
    if (memcmp(record.payer, request->payer->id, 32U) != 0 ||
        memcmp(record.stream_account, request->stream_account->id, 32U) != 0 ||
        memcmp(record.asset_id, request->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    if (timestamp < record.last_accrual_timestamp)
        return LXP_ERR_NON_MONOTONIC_TIME;
    status = fund_context(ctx, request, &source, &set);
    if (status != LXP_OK) return status;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    if (record.underfunded) {
        record.underfunded = false;
        record.last_accrual_timestamp = timestamp;
    }
    status = lx_stream_save(ctx, &record);
    if (status != LXP_OK) lxp_module_ctx_rollback(ctx);
    return status;
}
