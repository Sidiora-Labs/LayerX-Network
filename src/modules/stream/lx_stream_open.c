#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_STREAM_OPEN, LX_STREAM_TOP_UP, LX_STREAM_METER, LX_STREAM_SETTLE,
    LX_STREAM_PAUSE, LX_STREAM_RESUME, LX_STREAM_CLOSE
};

typedef struct stream_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
} stream_decoded;

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t length,
                                void **decoded)
{
    stream_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        (payload == NULL && length != 0U)) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(stream_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (stream_decoded *)memory;
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = length;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const stream_decoded *value = (const stream_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const stream_decoded *value = (const stream_decoded *)decoded;
    (void)activity;
    (void)authority;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_emit_event(ctx, value->ordinal, value->payload,
                              value->payload_length);
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

lxp_result lx_stream_lookup(lx_stream_store *store,
                            const uint8_t stream_id[32],
                            lx_stream_record **record)
{
    size_t i;
    if (store == NULL || stream_id == NULL || record == NULL ||
        store->count > LX_STREAM_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->records[i].stream_id, stream_id, 32U) == 0) {
            *record = &store->records[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

static lxp_result record_validate(const lx_stream_record *record)
{
    if (record == NULL || lxp_ct_is_zero(record->stream_id, 32U) ||
        record->mode < LX_STREAM_MODE_TIME ||
        record->mode > LX_STREAM_MODE_METERED ||
        lxp_u128_is_zero(record->rate) || record->rate_unit == 0U ||
        record->start_timestamp == 0U ||
        (record->end_timestamp != 0U &&
         record->end_timestamp <= record->start_timestamp) ||
        lxp_u128_is_zero(record->total_cap) ||
        record->meter_authority_count > LX_STREAM_MAX_METER_AUTHORITIES ||
        (record->mode == LX_STREAM_MODE_METERED &&
         record->meter_authority_count == 0U))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_stream_state_put(lx_stream_store *store,
                               const lx_stream_record *record)
{
    lx_stream_record *existing;
    lxp_result status = record_validate(record);
    if (store == NULL || status != LXP_OK ||
        store->count > LX_STREAM_STORE_CAPACITY)
        return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
    if (lx_stream_lookup(store, record->stream_id, &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    if (store->count == LX_STREAM_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    store->records[store->count++] = *record;
    return LXP_OK;
}

static lxp_result emit_fund(lxp_module_ctx *ctx,
                            const lx_stream_fund_request *request,
                            lxp_receipt *receipt)
{
    lxp_transfer_set set;
    if (ctx == NULL || request == NULL || request->payer == NULL ||
        request->stream_account == NULL || request->asset == NULL ||
        receipt == NULL || request->payer->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->stream_account->kind != LX_ACCOUNT_AGENT_STREAM ||
        lxp_u128_is_zero(request->amount)) return LXP_ERR_NON_CANONICAL;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->payer;
    set.legs[0].to = request->stream_account;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_STREAM_FUND;
    set.context = request->context;
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_stream_open_execute(lxp_module_ctx *ctx,
                                  const lx_stream_fund_request *request,
                                  lxp_receipt *receipt)
{
    lx_stream_record *existing;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->payer == NULL || request->stream_account == NULL ||
        request->asset == NULL || receipt == NULL ||
        request->store->count > LX_STREAM_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = record_validate(&request->record);
    if (status != LXP_OK) return status;
    if (lx_stream_lookup(request->store, request->record.stream_id,
                         &existing) == LXP_OK ||
        request->store->count == LX_STREAM_STORE_CAPACITY)
        return LXP_ERR_SEQUENCE_REUSED;
    if (memcmp(request->record.payer, request->payer->id, 32U) != 0 ||
        memcmp(request->record.stream_account,
               request->stream_account->id, 32U) != 0 ||
        memcmp(request->record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = emit_fund(ctx, request, receipt);
    if (status != LXP_OK) return status;
    return lx_stream_state_put(request->store, &request->record);
}

lxp_result lx_stream_top_up_execute(lxp_module_ctx *ctx,
                                    const lx_stream_fund_request *request,
                                    lxp_receipt *receipt)
{
    lx_stream_record *record;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->payer == NULL || request->stream_account == NULL ||
        request->asset == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_lookup(request->store, request->record.stream_id, &record);
    if (status != LXP_OK || record->closed) return LXP_ERR_UNKNOWN_FIELD;
    if (memcmp(record->payer, request->payer->id, 32U) != 0 ||
        memcmp(record->stream_account, request->stream_account->id, 32U) != 0 ||
        memcmp(record->asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = emit_fund(ctx, request, receipt);
    if (status != LXP_OK) return status;
    if (record->underfunded) {
        record->underfunded = false;
        record->last_accrual_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    }
    return LXP_OK;
}
