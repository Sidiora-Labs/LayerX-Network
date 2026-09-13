#include "layerx/lxp_gateway.h"
#include "lxp_gateway_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <stdlib.h>
#include <string.h>

enum { LXP_GATEWAY_KV_INITIAL_CAPACITY = 16 };

static const uint8_t lxp_gateway_invoice_prefix[6] = {
    'g', 'w', 'i', 'n', 'v', ':'
};
static const uint8_t lxp_gateway_idempotency_prefix[7] = {
    'g', 'w', 'i', 'd', 'e', 'm', ':'
};

static int gateway_bytes_compare(
    const uint8_t *left, size_t left_length,
    const uint8_t *right, size_t right_length)
{
    size_t common = left_length < right_length ? left_length : right_length;
    int comparison = memcmp(left, right, common);
    if (comparison != 0) return comparison;
    if (left_length < right_length) return -1;
    return left_length == right_length ? 0 : 1;
}

static bool gateway_kv_locate(
    const lxp_gateway_kv *kv, const uint8_t *key, size_t key_length,
    size_t *index)
{
    size_t low = 0U;
    size_t high = kv->count;
    while (low < high) {
        size_t middle = low + (high - low) / 2U;
        int comparison = gateway_bytes_compare(
            kv->entries[middle].key, kv->entries[middle].key_length,
            key, key_length);
        if (comparison == 0) {
            *index = middle;
            return true;
        }
        if (comparison < 0) low = middle + 1U;
        else high = middle;
    }
    *index = low;
    return false;
}

static lxp_result gateway_kv_reserve_entries(lxp_gateway_kv *kv)
{
    lxp_gateway_kv_entry *grown;
    size_t capacity;
    if (kv->count < kv->capacity) return LXP_OK;
    if (kv->count == SIZE_MAX) return LXP_ERR_OVERFLOW;
    capacity = kv->capacity == 0U ?
        (size_t)LXP_GATEWAY_KV_INITIAL_CAPACITY : kv->capacity;
    while (capacity <= kv->count) {
        if (capacity > SIZE_MAX / 2U) return LXP_ERR_OVERFLOW;
        capacity *= 2U;
    }
    if (capacity > SIZE_MAX / sizeof(*grown)) return LXP_ERR_OVERFLOW;
    grown = (lxp_gateway_kv_entry *)realloc(
        kv->entries, capacity * sizeof(*grown));
    if (grown == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    kv->entries = grown;
    kv->capacity = capacity;
    return LXP_OK;
}

static lxp_result gateway_kv_reserve_undo(lxp_gateway_kv *kv)
{
    lxp_gateway_kv_undo *grown;
    size_t capacity;
    if (kv->undo_count < kv->undo_capacity) return LXP_OK;
    if (kv->undo_count == SIZE_MAX) return LXP_ERR_OVERFLOW;
    capacity = kv->undo_capacity == 0U ?
        (size_t)LXP_GATEWAY_KV_INITIAL_CAPACITY : kv->undo_capacity;
    while (capacity <= kv->undo_count) {
        if (capacity > SIZE_MAX / 2U) return LXP_ERR_OVERFLOW;
        capacity *= 2U;
    }
    if (capacity > SIZE_MAX / sizeof(*grown)) return LXP_ERR_OVERFLOW;
    grown = (lxp_gateway_kv_undo *)realloc(
        kv->undo, capacity * sizeof(*grown));
    if (grown == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    kv->undo = grown;
    kv->undo_capacity = capacity;
    return LXP_OK;
}

static lxp_result gateway_kv_charge(
    lxp_meter_ctx *meter, size_t added, size_t removed)
{
    int64_t delta;
    if (meter == NULL) return LXP_OK;
    if (added > (size_t)INT64_MAX || removed > (size_t)INT64_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    delta = (int64_t)added - (int64_t)removed;
    return lxp_meter_charge_storage(meter, delta);
}

lxp_result lxp_gateway_kv_init(lxp_gateway_kv *kv)
{
    if (kv == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(kv, 0, sizeof(*kv));
    return LXP_OK;
}

void lxp_gateway_kv_release(lxp_gateway_kv *kv)
{
    size_t i;
    if (kv == NULL) return;
    for (i = 0U; i < kv->count; ++i) {
        free(kv->entries[i].key);
        free(kv->entries[i].value);
    }
    for (i = 0U; i < kv->undo_count; ++i) free(kv->undo[i].value);
    free(kv->entries);
    free(kv->undo);
    (void)memset(kv, 0, sizeof(*kv));
}

lxp_result lxp_gateway_kv_get(
    const lxp_gateway_kv *kv, const uint8_t *key, size_t key_length,
    const uint8_t **value, size_t *value_length)
{
    size_t index = 0U;
    if (kv == NULL || key == NULL || key_length == 0U || value == NULL ||
        value_length == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!gateway_kv_locate(kv, key, key_length, &index))
        return LXP_ERR_UNKNOWN_FIELD;
    *value = kv->entries[index].value;
    *value_length = kv->entries[index].value_length;
    return LXP_OK;
}

lxp_result lxp_gateway_kv_put(
    lxp_gateway_kv *kv, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length, lxp_meter_ctx *meter)
{
    lxp_gateway_kv_undo *record;
    uint8_t *stored_value;
    uint8_t *stored_key;
    size_t index = 0U;
    bool present;
    lxp_result status;
    if (kv == NULL || key == NULL || key_length == 0U || value == NULL ||
        value_length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (key_length > SIZE_MAX - value_length) return LXP_ERR_LENGTH_LIMIT;
    present = gateway_kv_locate(kv, key, key_length, &index);
    status = gateway_kv_charge(
        meter, present ? value_length : key_length + value_length,
        present ? kv->entries[index].value_length : 0U);
    if (status != LXP_OK) return status;
    status = gateway_kv_reserve_undo(kv);
    if (status != LXP_OK) return status;
    stored_value = (uint8_t *)malloc(value_length);
    if (stored_value == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(stored_value, value, value_length);
    record = &kv->undo[kv->undo_count];
    if (present) {
        record->index = index;
        record->value = kv->entries[index].value;
        record->value_length = kv->entries[index].value_length;
        record->created = false;
        ++kv->undo_count;
        kv->stored_bytes -= (uint64_t)kv->entries[index].value_length;
        kv->entries[index].value = stored_value;
        kv->entries[index].value_length = value_length;
        kv->stored_bytes += (uint64_t)value_length;
        return LXP_OK;
    }
    status = gateway_kv_reserve_entries(kv);
    if (status != LXP_OK) {
        free(stored_value);
        return status;
    }
    stored_key = (uint8_t *)malloc(key_length);
    if (stored_key == NULL) {
        free(stored_value);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    (void)memcpy(stored_key, key, key_length);
    if (index < kv->count)
        (void)memmove(&kv->entries[index + 1U], &kv->entries[index],
                      (kv->count - index) * sizeof(kv->entries[0]));
    kv->entries[index].key = stored_key;
    kv->entries[index].key_length = key_length;
    kv->entries[index].value = stored_value;
    kv->entries[index].value_length = value_length;
    ++kv->count;
    kv->stored_bytes += (uint64_t)key_length + (uint64_t)value_length;
    record->index = index;
    record->value = NULL;
    record->value_length = 0U;
    record->created = true;
    ++kv->undo_count;
    return LXP_OK;
}

size_t lxp_gateway_kv_mark(const lxp_gateway_kv *kv)
{
    return kv == NULL ? 0U : kv->undo_count;
}

lxp_result lxp_gateway_kv_rollback(lxp_gateway_kv *kv, size_t mark)
{
    if (kv == NULL || mark > kv->undo_count) return LXP_FATAL_INVARIANT;
    while (kv->undo_count > mark) {
        lxp_gateway_kv_undo *record;
        lxp_gateway_kv_entry *entry;
        --kv->undo_count;
        record = &kv->undo[kv->undo_count];
        if (record->index >= kv->count) return LXP_FATAL_INVARIANT;
        entry = &kv->entries[record->index];
        if (record->created) {
            kv->stored_bytes -=
                (uint64_t)entry->key_length + (uint64_t)entry->value_length;
            free(entry->key);
            free(entry->value);
            if (record->index + 1U < kv->count)
                (void)memmove(entry, entry + 1U,
                              (kv->count - record->index - 1U) *
                                  sizeof(kv->entries[0]));
            --kv->count;
            (void)memset(&kv->entries[kv->count], 0,
                         sizeof(kv->entries[0]));
        } else {
            kv->stored_bytes -= (uint64_t)entry->value_length;
            free(entry->value);
            entry->value = record->value;
            entry->value_length = record->value_length;
            kv->stored_bytes += (uint64_t)record->value_length;
        }
        (void)memset(record, 0, sizeof(*record));
    }
    return LXP_OK;
}

void lxp_gateway_kv_commit(lxp_gateway_kv *kv, size_t mark)
{
    if (kv == NULL || mark > kv->undo_count) return;
    while (kv->undo_count > mark) {
        --kv->undo_count;
        free(kv->undo[kv->undo_count].value);
        (void)memset(&kv->undo[kv->undo_count], 0,
                     sizeof(kv->undo[kv->undo_count]));
    }
}

static lxp_result gateway_kv_leaf_hash(
    const lxp_gateway_kv_entry *entry, uint8_t out[32])
{
    lxp_hash_context context;
    uint8_t lengths[8];
    size_t tag_length = 0U;
    const uint8_t *tag = lxp_domain_tag(LXP_DOMAIN_STATE_LEAF, &tag_length);
    lxp_result status;
    size_t i;
    if (tag == NULL) return LXP_FATAL_INVARIANT;
    if (entry->key_length > UINT32_MAX || entry->value_length > UINT32_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < 4U; ++i) {
        lengths[i] = (uint8_t)(entry->key_length >> (24U - i * 8U));
        lengths[4U + i] = (uint8_t)(entry->value_length >> (24U - i * 8U));
    }
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, tag, tag_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, lengths, sizeof(lengths));
    if (status == LXP_OK)
        status = lxp_hash_update(&context, entry->key, entry->key_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, entry->value, entry->value_length);
    if (status == LXP_OK) status = lxp_hash_final(&context, out);
    return status;
}

static lxp_result gateway_kv_node_hash(
    const uint8_t left[32], const uint8_t right[32], uint8_t out[32])
{
    uint8_t pair[64];
    lxp_result status;
    (void)memcpy(pair, left, 32U);
    (void)memcpy(pair + 32U, right, 32U);
    status = lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), out);
    (void)memset(pair, 0, sizeof(pair));
    return status;
}

lxp_result lxp_gateway_kv_root(const lxp_gateway_kv *kv, uint8_t root[32])
{
    uint8_t (*hashes)[32];
    size_t level_count;
    size_t i;
    lxp_result status = LXP_OK;
    if (kv == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    if (kv->count == 0U)
        return lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, NULL, 0U, root);
    if (kv->count > SIZE_MAX / 32U) return LXP_ERR_LENGTH_LIMIT;
    hashes = (uint8_t (*)[32])malloc(kv->count * 32U);
    if (hashes == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    for (i = 0U; status == LXP_OK && i < kv->count; ++i)
        status = gateway_kv_leaf_hash(&kv->entries[i], hashes[i]);
    level_count = kv->count;
    while (status == LXP_OK && level_count > 1U) {
        size_t next_count = (level_count + 1U) / 2U;
        for (i = 0U; status == LXP_OK && i < next_count; ++i) {
            size_t right = i * 2U + 1U;
            if (right >= level_count) right = i * 2U;
            status = gateway_kv_node_hash(
                hashes[i * 2U], hashes[right], hashes[i]);
        }
        level_count = next_count;
    }
    if (status == LXP_OK) (void)memcpy(root, hashes[0], 32U);
    free(hashes);
    return status;
}

void lxp_gateway_invoice_key(
    uint8_t key[LXP_GATEWAY_KV_INVOICE_KEY_BYTES],
    const uint8_t invoice_id[32], const uint8_t idempotency_key[32])
{
    (void)memcpy(key, lxp_gateway_invoice_prefix,
                 sizeof(lxp_gateway_invoice_prefix));
    (void)memcpy(key + sizeof(lxp_gateway_invoice_prefix), invoice_id, 32U);
    (void)memcpy(key + sizeof(lxp_gateway_invoice_prefix) + 32U,
                 idempotency_key, 32U);
}

void lxp_gateway_idempotency_key_bytes(
    uint8_t key[LXP_GATEWAY_KV_IDEMPOTENCY_KEY_BYTES], uint8_t domain,
    const uint8_t idempotency_key[32])
{
    (void)memcpy(key, lxp_gateway_idempotency_prefix,
                 sizeof(lxp_gateway_idempotency_prefix));
    key[sizeof(lxp_gateway_idempotency_prefix)] = domain;
    (void)memcpy(key + sizeof(lxp_gateway_idempotency_prefix) + 1U,
                 idempotency_key, 32U);
}

static lxp_result gateway_projection_write(
    uint8_t bytes[LXP_GATEWAY_KV_PROJECTION_BYTES],
    const lxp_send_receipt_projection *projection)
{
    lxp_result status = lxp_u128_to_be(projection->from_before, bytes);
    if (status == LXP_OK)
        status = lxp_u128_to_be(projection->from_after, bytes + 16U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(projection->to_before, bytes + 32U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(projection->to_after, bytes + 48U);
    if (status != LXP_OK) return status;
    (void)memcpy(bytes + 64U, projection->transfer_set_root, 32U);
    bytes[96] = projection->replayed ? 1U : 0U;
    return LXP_OK;
}

static lxp_result gateway_projection_read(
    const uint8_t bytes[LXP_GATEWAY_KV_PROJECTION_BYTES],
    lxp_send_receipt_projection *projection)
{
    lxp_result status;
    if (bytes[96] > 1U) return LXP_ERR_NON_CANONICAL;
    (void)memset(projection, 0, sizeof(*projection));
    status = lxp_u128_from_be(bytes, &projection->from_before);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 16U, &projection->from_after);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 32U, &projection->to_before);
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + 48U, &projection->to_after);
    if (status != LXP_OK) return status;
    (void)memcpy(projection->transfer_set_root, bytes + 64U, 32U);
    projection->replayed = bytes[96] != 0U;
    return LXP_OK;
}

static void gateway_put_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void gateway_put_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void gateway_put_u64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static uint16_t gateway_get_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

static uint32_t gateway_get_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t gateway_get_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | (uint64_t)bytes[i];
    return value;
}

static lxp_result gateway_effect_canonical(const lxp_effect *effect)
{
    if (effect->body_length > sizeof(effect->body))
        return LXP_ERR_LENGTH_LIMIT;
    if (effect->kind != LXP_EFFECT_STATE && effect->kind != LXP_EFFECT_TRANSFER &&
        effect->kind != LXP_EFFECT_EVENT)
        return LXP_ERR_NON_CANONICAL;
    if (effect->body_length < sizeof(effect->body) &&
        !lxp_ct_is_zero(effect->body + effect->body_length,
                        sizeof(effect->body) - effect->body_length))
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result gateway_receipt_record_length(const lxp_receipt *receipt,
                                                size_t *length)
{
    size_t total = (size_t)LXP_GATEWAY_KV_RECEIPT_FIXED_BYTES;
    size_t i;
    lxp_result status;
    if (receipt->effects.count > (size_t)LXP_MAX_EFFECTS)
        return LXP_ERR_LENGTH_LIMIT;
    if (receipt->program_outcome.present) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < receipt->effects.count; ++i) {
        status = gateway_effect_canonical(&receipt->effects.effects[i]);
        if (status != LXP_OK) return status;
        total += (size_t)LXP_GATEWAY_KV_EFFECT_FIXED_BYTES +
                 (size_t)receipt->effects.effects[i].body_length;
    }
    *length = total;
    return LXP_OK;
}

static lxp_result gateway_receipt_record_write(const lxp_receipt *receipt,
                                               uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    size_t i;
    lxp_result status = LXP_OK;
    gateway_put_u16(bytes + offset, receipt->protocol_version); offset += 2U;
    (void)memcpy(bytes + offset, receipt->activity_id, 32U); offset += 32U;
    gateway_put_u64(bytes + offset, receipt->global_sequence); offset += 8U;
    (void)memcpy(bytes + offset, receipt->previous_state_root, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, receipt->resulting_state_root, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, receipt->activity_root, 32U); offset += 32U;
    gateway_put_u32(bytes + offset, (uint32_t)receipt->result_code);
    offset += 4U;
    status = lxp_u128_to_be(receipt->fee_charged, bytes + offset);
    offset += 16U;
    (void)memcpy(bytes + offset, receipt->batch_id, 32U); offset += 32U;
    gateway_put_u16(bytes + offset, receipt->module_id); offset += 2U;
    gateway_put_u32(bytes + offset, receipt->module_version); offset += 4U;
    gateway_put_u32(bytes + offset, receipt->parameter_version); offset += 4U;
    bytes[offset] = receipt->operation; offset += 1U;
    (void)memcpy(bytes + offset, receipt->asset, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->amount, bytes + offset);
    offset += 16U;
    (void)memcpy(bytes + offset, receipt->from, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->from_balance_before, bytes + offset);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->from_balance_after, bytes + offset);
    offset += 16U;
    gateway_put_u64(bytes + offset, receipt->from_sequence); offset += 8U;
    (void)memcpy(bytes + offset, receipt->to, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->to_balance_before, bytes + offset);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->to_balance_after, bytes + offset);
    offset += 16U;
    gateway_put_u16(bytes + offset, receipt->supply_binding_version);
    offset += 2U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->total_units_before, bytes + offset);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_to_be(receipt->total_units_after, bytes + offset);
    offset += 16U;
    (void)memcpy(bytes + offset, receipt->transfer_set_root, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, receipt->authorization_hash, 32U);
    offset += 32U;
    (void)memcpy(bytes + offset, receipt->context_hash, 32U); offset += 32U;
    gateway_put_u64(bytes + offset, receipt->timestamp); offset += 8U;
    (void)memcpy(bytes + offset, receipt->sequencer_signature, 64U);
    offset += 64U;
    gateway_put_u32(bytes + offset, (uint32_t)receipt->effects.count);
    offset += 4U;
    if (status != LXP_OK) return status;
    if (offset != (size_t)LXP_GATEWAY_KV_RECEIPT_FIXED_BYTES)
        return LXP_FATAL_INVARIANT;
    for (i = 0U; i < receipt->effects.count; ++i) {
        const lxp_effect *effect = &receipt->effects.effects[i];
        gateway_put_u16(bytes + offset, effect->module_id); offset += 2U;
        gateway_put_u16(bytes + offset, effect->ordinal); offset += 2U;
        gateway_put_u16(bytes + offset, effect->event_type); offset += 2U;
        bytes[offset] = (uint8_t)effect->kind; offset += 1U;
        bytes[offset] = effect->monetary ? 1U : 0U; offset += 1U;
        (void)memcpy(bytes + offset, effect->transfer_set_root, 32U);
        offset += 32U;
        gateway_put_u16(bytes + offset, effect->body_length); offset += 2U;
        (void)memcpy(bytes + offset, effect->body,
                     (size_t)effect->body_length);
        offset += (size_t)effect->body_length;
    }
    return offset == length ? LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result gateway_receipt_record_read(const uint8_t *bytes,
                                              size_t length,
                                              lxp_receipt *receipt)
{
    size_t offset = 0U;
    size_t i;
    uint32_t effect_count;
    uint32_t result_bits;
    lxp_result status = LXP_OK;
    if (length < (size_t)LXP_GATEWAY_KV_RECEIPT_FIXED_BYTES)
        return LXP_ERR_TRUNCATED;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->protocol_version = gateway_get_u16(bytes + offset); offset += 2U;
    (void)memcpy(receipt->activity_id, bytes + offset, 32U); offset += 32U;
    receipt->global_sequence = gateway_get_u64(bytes + offset); offset += 8U;
    (void)memcpy(receipt->previous_state_root, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(receipt->resulting_state_root, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(receipt->activity_root, bytes + offset, 32U); offset += 32U;
    result_bits = gateway_get_u32(bytes + offset); offset += 4U;
    (void)memcpy(&receipt->result_code, &result_bits, sizeof(result_bits));
    status = lxp_u128_from_be(bytes + offset, &receipt->fee_charged);
    offset += 16U;
    (void)memcpy(receipt->batch_id, bytes + offset, 32U); offset += 32U;
    receipt->module_id = gateway_get_u16(bytes + offset); offset += 2U;
    receipt->module_version = gateway_get_u32(bytes + offset); offset += 4U;
    receipt->parameter_version = gateway_get_u32(bytes + offset); offset += 4U;
    receipt->operation = bytes[offset]; offset += 1U;
    (void)memcpy(receipt->asset, bytes + offset, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset, &receipt->amount);
    offset += 16U;
    (void)memcpy(receipt->from, bytes + offset, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset,
                                  &receipt->from_balance_before);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset,
                                  &receipt->from_balance_after);
    offset += 16U;
    receipt->from_sequence = gateway_get_u64(bytes + offset); offset += 8U;
    (void)memcpy(receipt->to, bytes + offset, 32U); offset += 32U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset, &receipt->to_balance_before);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset, &receipt->to_balance_after);
    offset += 16U;
    receipt->supply_binding_version = gateway_get_u16(bytes + offset);
    offset += 2U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset,
                                  &receipt->total_units_before);
    offset += 16U;
    if (status == LXP_OK)
        status = lxp_u128_from_be(bytes + offset, &receipt->total_units_after);
    offset += 16U;
    (void)memcpy(receipt->transfer_set_root, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(receipt->authorization_hash, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(receipt->context_hash, bytes + offset, 32U); offset += 32U;
    receipt->timestamp = gateway_get_u64(bytes + offset); offset += 8U;
    (void)memcpy(receipt->sequencer_signature, bytes + offset, 64U);
    offset += 64U;
    effect_count = gateway_get_u32(bytes + offset); offset += 4U;
    if (status != LXP_OK) return status;
    if (offset != (size_t)LXP_GATEWAY_KV_RECEIPT_FIXED_BYTES)
        return LXP_FATAL_INVARIANT;
    if (effect_count > (uint32_t)LXP_MAX_EFFECTS) return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < (size_t)effect_count; ++i) {
        lxp_effect *effect = &receipt->effects.effects[i];
        uint16_t body_length;
        uint8_t kind;
        if (length - offset < (size_t)LXP_GATEWAY_KV_EFFECT_FIXED_BYTES)
            return LXP_ERR_TRUNCATED;
        effect->module_id = gateway_get_u16(bytes + offset); offset += 2U;
        effect->ordinal = gateway_get_u16(bytes + offset); offset += 2U;
        effect->event_type = gateway_get_u16(bytes + offset); offset += 2U;
        kind = bytes[offset]; offset += 1U;
        if (kind != (uint8_t)LXP_EFFECT_STATE &&
            kind != (uint8_t)LXP_EFFECT_TRANSFER &&
            kind != (uint8_t)LXP_EFFECT_EVENT)
            return LXP_ERR_NON_CANONICAL;
        effect->kind = (lxp_effect_kind)kind;
        if (bytes[offset] > 1U) return LXP_ERR_NON_CANONICAL;
        effect->monetary = bytes[offset] != 0U; offset += 1U;
        (void)memcpy(effect->transfer_set_root, bytes + offset, 32U);
        offset += 32U;
        body_length = gateway_get_u16(bytes + offset); offset += 2U;
        if (body_length > sizeof(effect->body)) return LXP_ERR_LENGTH_LIMIT;
        if (length - offset < (size_t)body_length) return LXP_ERR_TRUNCATED;
        effect->body_length = body_length;
        (void)memcpy(effect->body, bytes + offset, (size_t)body_length);
        offset += (size_t)body_length;
    }
    receipt->effects.count = (size_t)effect_count;
    return offset == length ? LXP_OK : LXP_ERR_TRAILING_BYTES;
}

lxp_result lxp_gateway_invoice_record_put(
    lxp_gateway_invoice_registry *registry, const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32], const lxp_receipt *receipt,
    lxp_meter_ctx *meter)
{
    uint8_t key[LXP_GATEWAY_KV_INVOICE_KEY_BYTES];
    size_t record_length = 0U;
    size_t value_length;
    uint8_t *value;
    lxp_result status;
    if (registry == NULL || registry->scratch == NULL || invoice_id == NULL ||
        idempotency_key == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (size_t index = 0U; index < registry->kv.count; ++index) {
        const lxp_gateway_kv_entry *entry = &registry->kv.entries[index];
        if (entry->key_length == LXP_GATEWAY_KV_INVOICE_KEY_BYTES &&
            memcmp(entry->key, lxp_gateway_invoice_prefix,
                   sizeof(lxp_gateway_invoice_prefix)) == 0 &&
            memcmp(entry->key + sizeof(lxp_gateway_invoice_prefix),
                   invoice_id, 32U) == 0)
            return LXP_ERR_INVOICE_ALREADY_SETTLED;
    }
    status = gateway_receipt_record_length(receipt, &record_length);
    if (status != LXP_OK) return status;
    value_length =
        (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES + record_length;
    value = (uint8_t *)malloc(value_length);
    if (value == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(value, invoice_id, 32U);
    (void)memcpy(value + 32U, idempotency_key, 32U);
    status = gateway_receipt_record_write(
        receipt, value + (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES,
        record_length);
    if (status == LXP_OK)
        status = gateway_receipt_record_read(
            value + (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES,
            record_length, registry->scratch);
    if (status == LXP_OK &&
        memcmp(registry->scratch, receipt, sizeof(*receipt)) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        lxp_gateway_invoice_key(key, invoice_id, idempotency_key);
        status = lxp_gateway_kv_put(&registry->kv, key, sizeof(key), value,
                                    value_length, meter);
    }
    free(value);
    return status;
}

lxp_result lxp_gateway_invoice_record_get(
    const lxp_gateway_kv *kv, const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32], lxp_receipt *receipt, bool *settled)
{
    uint8_t key[LXP_GATEWAY_KV_INVOICE_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t value_length = 0U;
    lxp_result status;
    if (kv == NULL || invoice_id == NULL || idempotency_key == NULL ||
        receipt == NULL || settled == NULL)
        return LXP_ERR_NON_CANONICAL;
    *settled = false;
    lxp_gateway_invoice_key(key, invoice_id, idempotency_key);
    status = lxp_gateway_kv_get(kv, key, sizeof(key), &value, &value_length);
    if (status == LXP_ERR_UNKNOWN_FIELD) {
        for (size_t index = 0U; index < kv->count; ++index) {
            const lxp_gateway_kv_entry *entry = &kv->entries[index];
            if (entry->key_length == sizeof(key) &&
                memcmp(entry->key, key, sizeof(lxp_gateway_invoice_prefix) + 32U) == 0)
                return LXP_ERR_INVOICE_ALREADY_SETTLED;
        }
        return LXP_OK;
    }
    if (status != LXP_OK) return status;
    if (value_length <= (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES ||
        lxp_ct_memcmp(value, invoice_id, 32U) != 0 ||
        lxp_ct_memcmp(value + 32U, idempotency_key, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    status = gateway_receipt_record_read(
        value + (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES,
        value_length - (size_t)LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES,
        receipt);
    if (status != LXP_OK) return status;
    *settled = true;
    return LXP_OK;
}

lxp_result lxp_gateway_idempotency_record_put(
    lxp_gateway_kv *kv, uint8_t domain, const lxp_send_store_record *record,
    lxp_meter_ctx *meter)
{
    uint8_t key[LXP_GATEWAY_KV_IDEMPOTENCY_KEY_BYTES];
    uint8_t value[LXP_GATEWAY_KV_IDEMPOTENCY_VALUE_BYTES];
    lxp_result status;
    if (kv == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(value, record->activity_hash, 32U);
    status = gateway_projection_write(value + 32U, &record->receipt);
    if (status != LXP_OK) return status;
    lxp_gateway_idempotency_key_bytes(key, domain, record->idempotency_key);
    return lxp_gateway_kv_put(kv, key, sizeof(key), value, sizeof(value),
                              meter);
}

lxp_result lxp_gateway_idempotency_precheck(
    const lxp_gateway_kv *kv, uint8_t domain,
    const uint8_t idempotency_key[32], const uint8_t activity_hash[32],
    lxp_send_receipt_projection *projection)
{
    uint8_t key[LXP_GATEWAY_KV_IDEMPOTENCY_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t value_length = 0U;
    lxp_result status;
    if (kv == NULL || idempotency_key == NULL || activity_hash == NULL ||
        projection == NULL)
        return LXP_ERR_NON_CANONICAL;
    lxp_gateway_idempotency_key_bytes(key, domain, idempotency_key);
    status = lxp_gateway_kv_get(kv, key, sizeof(key), &value, &value_length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (value_length != (size_t)LXP_GATEWAY_KV_IDEMPOTENCY_VALUE_BYTES)
        return LXP_FATAL_INVARIANT;
    if (lxp_ct_memcmp(value, activity_hash, 32U) == 0)
        return LXP_ERR_SEQUENCE_REUSED;
    status = gateway_projection_read(value + 32U, projection);
    if (status != LXP_OK) return status;
    projection->replayed = true;
    return LXP_ERR_IDEMPOTENT_REPLAY;
}

lxp_result lxp_gateway_window_reserve(
    lxp_gateway_kv *kv, lxp_send_store *store, uint8_t domain,
    lxp_meter_ctx *meter, lxp_send_store **backup)
{
    lxp_send_store *saved;
    size_t i;
    lxp_result status;
    if (kv == NULL || store == NULL || backup == NULL || *backup != NULL)
        return LXP_ERR_NON_CANONICAL;
    if (store->count > (size_t)LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    if (store->count < (size_t)LXP_SEND_STORE_CAPACITY) return LXP_OK;
    saved = (lxp_send_store *)malloc(sizeof(*saved));
    if (saved == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *saved = *store;
    *backup = saved;
    for (i = 0U; i < saved->count; ++i) {
        status = lxp_gateway_idempotency_record_put(
            kv, domain, &saved->records[i], meter);
        if (status != LXP_OK) return status;
    }
    (void)memset(store->records, 0, sizeof(store->records));
    store->count = 0U;
    return LXP_OK;
}
