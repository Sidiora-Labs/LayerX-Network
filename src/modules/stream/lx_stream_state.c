#include "layerx/lx_stream.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t stream_prefix[] = "stream:";
static const uint8_t result_prefix[] = "result:";

typedef struct stream_iter_adapter {
    lx_stream_visit_fn visit;
    void *user;
} stream_iter_adapter;

lxp_result lx_stream_state_key(const uint8_t stream_id[32],
                               uint8_t key[LX_STREAM_KEY_BYTES])
{
    if (stream_id == NULL || key == NULL || lxp_ct_is_zero(stream_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, stream_prefix, sizeof(stream_prefix) - 1U);
    (void)memcpy(key + sizeof(stream_prefix) - 1U, stream_id, 32U);
    return LXP_OK;
}

lxp_result lx_stream_result_key(const uint8_t idempotency_key[32],
                                uint8_t key[LX_STREAM_KEY_BYTES])
{
    if (idempotency_key == NULL || key == NULL ||
        lxp_ct_is_zero(idempotency_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, result_prefix, sizeof(result_prefix) - 1U);
    (void)memcpy(key + sizeof(result_prefix) - 1U, idempotency_key, 32U);
    return LXP_OK;
}

lxp_result lx_stream_load(lxp_module_ctx *ctx, const uint8_t stream_id[32],
                          lx_stream_record *record)
{
    uint8_t key[LX_STREAM_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_STREAM || record == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_state_key(stream_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status != LXP_OK) return status;
    status = lx_stream_record_decode(bytes, length, record);
    if (status != LXP_OK) return status;
    return memcmp(record->stream_id, stream_id, 32U) == 0 ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lx_stream_save(lxp_module_ctx *ctx, const lx_stream_record *record)
{
    uint8_t key[LX_STREAM_KEY_BYTES];
    uint8_t bytes[LX_STREAM_RECORD_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_STREAM || record == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_record_encode(record, bytes);
    if (status != LXP_OK) return status;
    status = lx_stream_state_key(record->stream_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

static lxp_result visit_stream(const uint8_t *key, size_t key_length,
                               const uint8_t *value, size_t value_length,
                               void *user)
{
    stream_iter_adapter *adapter = (stream_iter_adapter *)user;
    lx_stream_record record;
    lxp_result status;
    if (key == NULL || key_length != LX_STREAM_KEY_BYTES) return LXP_OK;
    if (memcmp(key, stream_prefix, sizeof(stream_prefix) - 1U) != 0)
        return LXP_OK;
    status = lx_stream_record_decode(value, value_length, &record);
    if (status != LXP_OK) return status;
    return adapter->visit(&record, adapter->user);
}

lxp_result lx_stream_iter(lxp_module_ctx *ctx, lx_stream_visit_fn visit,
                          void *user)
{
    stream_iter_adapter adapter;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_STREAM || visit == NULL)
        return LXP_ERR_NON_CANONICAL;
    adapter.visit = visit;
    adapter.user = user;
    return lxp_ctx_kv_iter(ctx, stream_prefix, sizeof(stream_prefix) - 1U,
                           visit_stream, &adapter);
}

lxp_result lx_stream_result_load(lxp_module_ctx *ctx,
                                 const uint8_t idempotency_key[32],
                                 lx_stream_economic_result *result,
                                 bool *found)
{
    uint8_t key[LX_STREAM_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_STREAM ||
        result == NULL || found == NULL)
        return LXP_ERR_NON_CANONICAL;
    *found = false;
    status = lx_stream_result_key(idempotency_key, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (length != LX_STREAM_RESULT_BYTES) return LXP_ERR_NON_CANONICAL;
    (void)memset(result, 0, sizeof(*result));
    (void)memcpy(result->transfer_set_root, bytes, 32U);
    status = lxp_u128_from_be(bytes + 32U, &result->paid);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 48U, &result->refunded);
    if (status != LXP_OK) return status;
    result->ordinal = (uint16_t)(((uint16_t)bytes[64] << 8U) |
                                 (uint16_t)bytes[65]);
    result->leg_count = bytes[66];
    (void)memcpy(result->stream_id, bytes + 67U, 32U);
    if (result->ordinal == 0U || result->ordinal > 7U ||
        result->leg_count > 2U)
        return LXP_ERR_NON_CANONICAL;
    *found = true;
    return LXP_OK;
}

lxp_result lx_stream_result_save(lxp_module_ctx *ctx,
                                 const uint8_t idempotency_key[32],
                                 const lx_stream_economic_result *result)
{
    uint8_t key[LX_STREAM_KEY_BYTES];
    uint8_t bytes[LX_STREAM_RESULT_BYTES];
    lxp_result status;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_STREAM || result == NULL ||
        result->ordinal == 0U || result->ordinal > 7U ||
        result->leg_count > 2U)
        return LXP_ERR_NON_CANONICAL;
    status = lx_stream_result_key(idempotency_key, key);
    if (status != LXP_OK) return status;
    (void)memset(bytes, 0, sizeof(bytes));
    (void)memcpy(bytes, result->transfer_set_root, 32U);
    status = lxp_u128_to_be(result->paid, bytes + 32U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(result->refunded, bytes + 48U);
    if (status != LXP_OK) return status;
    bytes[64] = (uint8_t)(result->ordinal >> 8U);
    bytes[65] = (uint8_t)result->ordinal;
    bytes[66] = result->leg_count;
    (void)memcpy(bytes + 67U, result->stream_id, 32U);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_stream_result_receipt(const lx_stream_economic_result *result,
                                    lxp_receipt *receipt)
{
    if (result == NULL || receipt == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(receipt, 0, sizeof(*receipt));
    if (result->leg_count != 0U)
        (void)memcpy(receipt->transfer_set_root, result->transfer_set_root,
                     32U);
    return LXP_OK;
}
