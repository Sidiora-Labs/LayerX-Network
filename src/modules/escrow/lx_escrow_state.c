#include "lx_escrow_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint8_t hold_prefix[LX_ESCROW_HOLD_PREFIX_BYTES] = {
    'h', 'o', 'l', 'd', ':'
};
static const uint8_t result_prefix[LX_ESCROW_RESULT_PREFIX_BYTES] = {
    'r', 'e', 's', 'u', 'l', 't', ':'
};

void lx_escrow_write_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

uint64_t lx_escrow_read_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | (uint64_t)in[i];
    return value;
}

lxp_result lx_escrow_hold_prefix(uint8_t prefix[LX_ESCROW_HOLD_PREFIX_BYTES])
{
    if (prefix == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(prefix, hold_prefix, sizeof(hold_prefix));
    return LXP_OK;
}

lxp_result lx_escrow_hold_key(const uint8_t escrow_id[32],
                              uint8_t key[LX_ESCROW_HOLD_KEY_BYTES])
{
    if (escrow_id == NULL || key == NULL) return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_is_zero(escrow_id, 32U)) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, hold_prefix, sizeof(hold_prefix));
    (void)memcpy(key + sizeof(hold_prefix), escrow_id, 32U);
    return LXP_OK;
}

lxp_result lx_escrow_result_key(const uint8_t idempotency_key[32],
                                uint8_t key[LX_ESCROW_RESULT_KEY_BYTES])
{
    if (idempotency_key == NULL || key == NULL) return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_is_zero(idempotency_key, 32U)) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, result_prefix, sizeof(result_prefix));
    (void)memcpy(key + sizeof(result_prefix), idempotency_key, 32U);
    return LXP_OK;
}

bool lx_escrow_active_state(lx_escrow_status state)
{
    return state == LX_ESCROW_STATE_OPEN ||
           state == LX_ESCROW_STATE_PARTIALLY_CAPTURED;
}

bool lx_escrow_terminal_state(lx_escrow_status state)
{
    return state == LX_ESCROW_STATE_CAPTURED ||
           state == LX_ESCROW_STATE_RELEASED ||
           state == LX_ESCROW_STATE_RESOLVED ||
           state == LX_ESCROW_STATE_TIMED_OUT;
}

lxp_result lx_escrow_timeout_key(const lx_escrow_record *record,
                                 uint8_t key[32])
{
    uint8_t input[40];
    if (record == NULL || key == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(input, record->escrow_id, 32U);
    lx_escrow_write_u64(input + 32U, record->expiry);
    return lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, input, sizeof(input), key);
}

lxp_result lx_escrow_record_encode(const lx_escrow_record *record,
                                   uint8_t bytes[LX_ESCROW_RECORD_BYTES])
{
    lxp_result status;
    if (record == NULL || bytes == NULL) return LXP_ERR_NON_CANONICAL;
    if (record->state < LX_ESCROW_STATE_OPEN ||
        record->state > LX_ESCROW_STATE_TIMED_OUT ||
        lxp_ct_is_zero(record->escrow_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, (size_t)LX_ESCROW_RECORD_BYTES);
    (void)memcpy(bytes, record->escrow_id, 32U);
    (void)memcpy(bytes + 32U, record->owner, 32U);
    (void)memcpy(bytes + 64U, record->escrow_account, 32U);
    (void)memcpy(bytes + 96U, record->beneficiary, 32U);
    (void)memcpy(bytes + 128U, record->arbiter, 32U);
    (void)memcpy(bytes + 160U, record->asset_id, 32U);
    status = lxp_u128_to_be(record->locked_amount, bytes + 192U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->captured_amount, bytes + 208U);
    if (status != LXP_OK) return status;
    bytes[224] = (uint8_t)record->state;
    lx_escrow_write_u64(bytes + 225U, record->expiry);
    lx_escrow_write_u64(bytes + 233U, record->dispute_window);
    (void)memcpy(bytes + 241U, record->terms_hash, 32U);
    (void)memcpy(bytes + 273U, record->agreement_reference, 32U);
    return LXP_OK;
}

lxp_result lx_escrow_record_decode(const uint8_t *bytes, size_t length,
                                   lx_escrow_record *record)
{
    lxp_result status;
    if (bytes == NULL || record == NULL ||
        length != (size_t)LX_ESCROW_RECORD_BYTES)
        return LXP_ERR_NON_CANONICAL;
    if (bytes[224] < (uint8_t)LX_ESCROW_STATE_OPEN ||
        bytes[224] > (uint8_t)LX_ESCROW_STATE_TIMED_OUT ||
        lxp_ct_is_zero(bytes, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->escrow_id, bytes, 32U);
    (void)memcpy(record->owner, bytes + 32U, 32U);
    (void)memcpy(record->escrow_account, bytes + 64U, 32U);
    (void)memcpy(record->beneficiary, bytes + 96U, 32U);
    (void)memcpy(record->arbiter, bytes + 128U, 32U);
    (void)memcpy(record->asset_id, bytes + 160U, 32U);
    status = lxp_u128_from_be(bytes + 192U, &record->locked_amount);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 208U, &record->captured_amount);
    if (status != LXP_OK) return status;
    record->state = (lx_escrow_status)bytes[224];
    record->expiry = lx_escrow_read_u64(bytes + 225U);
    record->dispute_window = lx_escrow_read_u64(bytes + 233U);
    (void)memcpy(record->terms_hash, bytes + 241U, 32U);
    (void)memcpy(record->agreement_reference, bytes + 273U, 32U);
    return LXP_OK;
}

lxp_result lx_escrow_result_encode(const lx_escrow_economic_result *result,
                                   uint8_t bytes[LX_ESCROW_RESULT_BYTES])
{
    lxp_result status;
    if (result == NULL || bytes == NULL) return LXP_ERR_NON_CANONICAL;
    if (result->ordinal == 0U || result->ordinal > 7U ||
        result->state_after < LX_ESCROW_STATE_OPEN ||
        result->state_after > LX_ESCROW_STATE_TIMED_OUT ||
        lxp_ct_is_zero(result->escrow_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bytes, 0, (size_t)LX_ESCROW_RESULT_BYTES);
    (void)memcpy(bytes, result->escrow_id, 32U);
    bytes[32] = (uint8_t)(result->ordinal >> 8U);
    bytes[33] = (uint8_t)(result->ordinal & 0xffU);
    bytes[34] = (uint8_t)result->state_after;
    status = lxp_u128_to_be(result->captured_after, bytes + 35U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(result->locked_after, bytes + 51U);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 67U, result->asset_id, 32U);
    (void)memcpy(bytes + 99U, result->from, 32U);
    (void)memcpy(bytes + 131U, result->to, 32U);
    status = lxp_u128_to_be(result->amount, bytes + 163U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(result->secondary_amount, bytes + 179U);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 195U, result->transfer_set_root, 32U);
    lx_escrow_write_u64(bytes + 227U, result->global_sequence);
    lx_escrow_write_u64(bytes + 235U, result->timestamp);
    return LXP_OK;
}

lxp_result lx_escrow_result_decode(const uint8_t *bytes, size_t length,
                                   lx_escrow_economic_result *result)
{
    uint16_t ordinal;
    lxp_result status;
    if (bytes == NULL || result == NULL ||
        length != (size_t)LX_ESCROW_RESULT_BYTES)
        return LXP_ERR_NON_CANONICAL;
    ordinal = (uint16_t)(((uint16_t)bytes[32] << 8U) | (uint16_t)bytes[33]);
    if (ordinal == 0U || ordinal > 7U ||
        bytes[34] < (uint8_t)LX_ESCROW_STATE_OPEN ||
        bytes[34] > (uint8_t)LX_ESCROW_STATE_TIMED_OUT ||
        lxp_ct_is_zero(bytes, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(result, 0, sizeof(*result));
    (void)memcpy(result->escrow_id, bytes, 32U);
    result->ordinal = ordinal;
    result->state_after = (lx_escrow_status)bytes[34];
    status = lxp_u128_from_be(bytes + 35U, &result->captured_after);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 51U, &result->locked_after);
    if (status != LXP_OK) return status;
    (void)memcpy(result->asset_id, bytes + 67U, 32U);
    (void)memcpy(result->from, bytes + 99U, 32U);
    (void)memcpy(result->to, bytes + 131U, 32U);
    status = lxp_u128_from_be(bytes + 163U, &result->amount);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 179U, &result->secondary_amount);
    if (status != LXP_OK) return status;
    (void)memcpy(result->transfer_set_root, bytes + 195U, 32U);
    result->global_sequence = lx_escrow_read_u64(bytes + 227U);
    result->timestamp = lx_escrow_read_u64(bytes + 235U);
    return LXP_OK;
}

lxp_result lx_escrow_lookup(lxp_module_ctx *ctx, const uint8_t escrow_id[32],
                            lx_escrow_record *record)
{
    uint8_t key[LX_ESCROW_HOLD_KEY_BYTES];
    const uint8_t *value;
    size_t length;
    lxp_result status;
    if (ctx == NULL || escrow_id == NULL || record == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_hold_key(escrow_id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &value, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_ESCROW_STATE;
    if (status != LXP_OK) return status;
    status = lx_escrow_record_decode(value, length, record);
    if (status != LXP_OK) return status;
    return memcmp(record->escrow_id, escrow_id, 32U) == 0 ? LXP_OK :
                                                            LXP_FATAL_INVARIANT;
}

static lxp_result state_write(lxp_module_ctx *ctx,
                              const lx_escrow_record *record, bool expect)
{
    uint8_t key[LX_ESCROW_HOLD_KEY_BYTES];
    uint8_t bytes[LX_ESCROW_RECORD_BYTES];
    lx_escrow_record existing;
    lxp_result status;
    if (ctx == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_record_encode(record, bytes);
    if (status != LXP_OK) return status;
    status = lx_escrow_hold_key(record->escrow_id, key);
    if (status != LXP_OK) return status;
    status = lx_escrow_lookup(ctx, record->escrow_id, &existing);
    if (status != LXP_OK && status != LXP_ERR_ESCROW_STATE) return status;
    if ((status == LXP_OK) != expect) return LXP_ERR_ESCROW_STATE;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_escrow_state_write(lxp_module_ctx *ctx,
                                 const lx_escrow_record *record)
{
    uint8_t key[LX_ESCROW_HOLD_KEY_BYTES];
    uint8_t bytes[LX_ESCROW_RECORD_BYTES];
    lxp_result status;
    if (ctx == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_record_encode(record, bytes);
    if (status != LXP_OK) return status;
    status = lx_escrow_hold_key(record->escrow_id, key);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, sizeof(bytes));
}

lxp_result lx_escrow_state_put(lxp_module_ctx *ctx,
                               const lx_escrow_record *record)
{
    return state_write(ctx, record, false);
}

lxp_result lx_escrow_state_update(lxp_module_ctx *ctx,
                                  const lx_escrow_record *record)
{
    return state_write(ctx, record, true);
}

typedef struct iter_adapter {
    lx_escrow_visit_fn visit;
    void *user;
} iter_adapter;

static lxp_result iter_visit(const uint8_t *key, size_t key_length,
                             const uint8_t *value, size_t value_length,
                             void *user)
{
    iter_adapter *adapter = (iter_adapter *)user;
    lx_escrow_record record;
    lxp_result status;
    if (key == NULL || key_length != (size_t)LX_ESCROW_HOLD_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_record_decode(value, value_length, &record);
    if (status != LXP_OK) return status;
    if (memcmp(record.escrow_id, key + LX_ESCROW_HOLD_PREFIX_BYTES, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    return adapter->visit(&record, adapter->user);
}

lxp_result lx_escrow_state_iter(lxp_module_ctx *ctx, lx_escrow_visit_fn visit,
                                void *user)
{
    iter_adapter adapter;
    if (ctx == NULL || visit == NULL) return LXP_ERR_NON_CANONICAL;
    adapter.visit = visit;
    adapter.user = user;
    return lxp_ctx_kv_iter(ctx, hold_prefix, sizeof(hold_prefix), iter_visit,
                           &adapter);
}

void lx_escrow_receipt_from_result(const lx_escrow_economic_result *result,
                                   lxp_receipt *receipt)
{
    if (result == NULL || receipt == NULL) return;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->result_code = LXP_OK;
    receipt->module_id = LXP_MODULE_ESCROW;
    receipt->module_version = 1U;
    receipt->operation = (uint8_t)result->ordinal;
    receipt->global_sequence = result->global_sequence;
    receipt->timestamp = result->timestamp;
    receipt->amount = result->amount;
    (void)memcpy(receipt->asset, result->asset_id, 32U);
    (void)memcpy(receipt->from, result->from, 32U);
    (void)memcpy(receipt->to, result->to, 32U);
    (void)memcpy(receipt->transfer_set_root, result->transfer_set_root, 32U);
}

lxp_result lx_escrow_receipt_replay(lxp_module_ctx *ctx,
                                    const uint8_t key[32],
                                    lxp_receipt *receipt, bool *found)
{
    uint8_t storage_key[LX_ESCROW_RESULT_KEY_BYTES];
    lx_escrow_economic_result result;
    const uint8_t *value;
    size_t length;
    lxp_result status;
    if (ctx == NULL || key == NULL || receipt == NULL || found == NULL)
        return LXP_ERR_NON_CANONICAL;
    *found = false;
    status = lx_escrow_result_key(key, storage_key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, storage_key, sizeof(storage_key), &value,
                            &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    status = lx_escrow_result_decode(value, length, &result);
    if (status != LXP_OK) return status;
    lx_escrow_receipt_from_result(&result, receipt);
    *found = true;
    return LXP_OK;
}

lxp_result lx_escrow_receipt_record(lxp_module_ctx *ctx,
                                    const uint8_t key[32],
                                    const lx_escrow_economic_result *result)
{
    uint8_t storage_key[LX_ESCROW_RESULT_KEY_BYTES];
    uint8_t bytes[LX_ESCROW_RESULT_BYTES];
    const uint8_t *existing;
    size_t existing_length;
    lxp_result status;
    if (ctx == NULL || key == NULL || result == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lx_escrow_result_key(key, storage_key);
    if (status != LXP_OK) return status;
    status = lx_escrow_result_encode(result, bytes);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, storage_key, sizeof(storage_key), &existing,
                            &existing_length);
    if (status == LXP_OK) return LXP_ERR_IDEMPOTENT_REPLAY;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    return lxp_ctx_kv_put(ctx, storage_key, sizeof(storage_key), bytes,
                          sizeof(bytes));
}

lxp_result lx_escrow_event_body(const lx_escrow_record *record,
                                uint16_t ordinal,
                                uint8_t body[LX_ESCROW_EVENT_BYTES])
{
    lxp_result status;
    if (record == NULL || body == NULL || ordinal == 0U || ordinal > 7U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(body, 0, (size_t)LX_ESCROW_EVENT_BYTES);
    (void)memcpy(body, record->escrow_id, 32U);
    body[32] = (uint8_t)(ordinal >> 8U);
    body[33] = (uint8_t)(ordinal & 0xffU);
    body[34] = (uint8_t)record->state;
    status = lxp_u128_to_be(record->captured_amount, body + 35U);
    if (status != LXP_OK) return status;
    return lxp_u128_to_be(record->locked_amount, body + 51U);
}

lx_escrow_runtime *lx_escrow_require_runtime(lxp_module_ctx *ctx)
{
    lx_escrow_runtime *runtime;
    if (ctx == NULL) return NULL;
    runtime = (lx_escrow_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL ||
        runtime->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        runtime->assets->count > LX_ASSET_REGISTRY_CAPACITY)
        return NULL;
    return runtime;
}

lxp_result lx_escrow_resolve_account(lx_escrow_runtime *runtime,
                                     const uint8_t id[32],
                                     lx_account **account)
{
    size_t i;
    if (runtime == NULL || runtime->accounts == NULL || id == NULL ||
        account == NULL ||
        runtime->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < runtime->accounts->count; ++i)
        if (memcmp(runtime->accounts->accounts[i].id, id, 32U) == 0) {
            *account = &runtime->accounts->accounts[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

lxp_result lx_escrow_settle(lxp_module_ctx *ctx,
                            const lxp_transfer_context *base,
                            const lx_escrow_settlement *settlement,
                            lxp_authorization_kind authority_kind,
                            lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_transfer_asset_state asset_state;
    lxp_result status;
    if (ctx == NULL || base == NULL || settlement == NULL ||
        settlement->from == NULL || settlement->to == NULL ||
        settlement->asset == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (settlement->from->next_sequence == UINT64_MAX) return LXP_ERR_OVERFLOW;
    status = lx_asset_transfer_state(settlement->asset, &asset_state);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    set.context = *base;
    set.legs[0].from = settlement->from;
    set.legs[0].to = settlement->to;
    (void)memcpy(set.legs[0].asset_id, settlement->asset->asset_id, 32U);
    set.legs[0].amount = settlement->amount;
    set.legs[0].reason = settlement->reason;
    set.leg_count = 1U;
    if (settlement->secondary_to != NULL &&
        !lxp_u128_is_zero(settlement->secondary_amount)) {
        set.legs[1] = set.legs[0];
        set.legs[1].to = settlement->secondary_to;
        set.legs[1].amount = settlement->secondary_amount;
        set.leg_count = 2U;
    }
    if (lxp_u128_is_zero(set.legs[0].amount) && set.leg_count == 1U)
        return LXP_ERR_ZERO_AMOUNT;
    (void)memcpy(source.authorized_from, settlement->from->id, 32U);
    source.debit_authority_kind = authority_kind;
    source.protocol_system_capability = false;
    set.context.assets = &asset_state;
    set.context.asset_count = 1U;
    set.context.has_client_balance = false;
    set.context.idempotency_seen = false;
    set.context.protocol_system_capability = false;
    set.context.program_spend_token = 0U;
    set.context.origin_module_id = LXP_MODULE_ESCROW;
    set.context.debit_authority_kind = authority_kind;
    set.context.source_authorities = &source;
    set.context.source_authority_count = 1U;
    set.context.sequence_account = settlement->from;
    set.context.actor_sequence = settlement->from->next_sequence;
    set.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    (void)memcpy(set.context.authorized_from, settlement->from->id, 32U);
    return lxp_ctx_emit_transfer_set(ctx, &set, receipt);
}

lxp_result lx_escrow_commit_result(lxp_module_ctx *ctx,
                                   const lx_escrow_record *record,
                                   const uint8_t idempotency_key[32],
                                   const lx_escrow_settlement *settlement,
                                   uint16_t ordinal, lxp_receipt *receipt)
{
    lx_escrow_economic_result result;
    lxp_result status;
    if (ctx == NULL || record == NULL || idempotency_key == NULL ||
        settlement == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&result, 0, sizeof(result));
    (void)memcpy(result.escrow_id, record->escrow_id, 32U);
    result.ordinal = ordinal;
    result.state_after = record->state;
    result.captured_after = record->captured_amount;
    result.locked_after = record->locked_amount;
    (void)memcpy(result.asset_id, record->asset_id, 32U);
    if (settlement->from != NULL)
        (void)memcpy(result.from, settlement->from->id, 32U);
    if (settlement->to != NULL)
        (void)memcpy(result.to, settlement->to->id, 32U);
    result.amount = settlement->amount;
    result.secondary_amount = settlement->secondary_amount;
    (void)memcpy(result.transfer_set_root, receipt->transfer_set_root, 32U);
    result.global_sequence = lxp_ctx_global_sequence(ctx);
    result.timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    status = lx_escrow_state_write(ctx, record);
    if (status != LXP_OK) return status;
    status = lx_escrow_receipt_record(ctx, idempotency_key, &result);
    if (status != LXP_OK) return status;
    lx_escrow_receipt_from_result(&result, receipt);
    return LXP_OK;
}
