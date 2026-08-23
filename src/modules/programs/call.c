#include "layerx/programs.h"

#include "artifact.h"
#include "event.h"
#include "storage.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <stdint.h>
#include <string.h>

enum {
    PROGRAM_CALL_FIXED_BYTES = 32 + 2 + 2 + 4 + 2 + 4 +
                               LX_PROGRAMS_CALL_BUDGET_FIELDS * 8,
    PROGRAM_RECORD_BYTES = 71,
    PROGRAM_KEY_BYTES = 40
};

typedef struct lxp_programs_call_catalog_entry {
    uint8_t program_id[32];
    uint8_t code_hash[32];
    uint32_t wasm_length;
    uint16_t abi_version;
    struct {
        lxp_programs_storage_cell *cells;
        uint32_t count;
        bool begun;
        bool applied;
    } storage_final[2];
} lxp_programs_call_catalog_entry;

struct lxp_programs_call_activity {
    lxp_module_ctx *ctx;
    uint8_t program_id[32];
    uint8_t code_hash[32];
    uint32_t wasm_length;
    uint16_t abi_version;
    uint16_t entrypoint_length;
    const uint8_t *entrypoint;
    uint32_t calldata_length;
    const uint8_t *calldata;
    uint16_t capabilities_length;
    const uint8_t *capabilities;
    uint32_t response_capacity;
    uint64_t budget[LX_PROGRAMS_CALL_BUDGET_FIELDS];
    const lxp_authority_resolved *authority;
    lxp_effect_buffer *effects;
    lxp_transfer_set *transfer_set;
    lxp_receipt transfer_receipt;
    uint8_t transfer_leg_written[LXP_MAX_TRANSFER_SET_LEGS];
    uint16_t transfer_leg_count;
    bool transfer_applied;
    bool storage_settlement_authorized;
    lxp_programs_call_catalog_entry *catalog;
    uint32_t catalog_count;
    uint32_t catalog_cursor;
    bool receipt_view_active;
    lxp_verified_receipt_facts receipt_view;
    struct {
        lxp_programs_storage_cell *cells;
        uint32_t count;
        bool begun;
        bool applied;
    } storage_final[2];
    struct {
        bool active;
        uint8_t terminal_kind;
        lxp_result result_code;
        uint16_t runtime_version;
        uint16_t abi_version;
        uint32_t fee_schedule_version;
        uint64_t cpu_fuel;
        uint64_t memory_bytes;
        uint64_t storage_read_bytes;
        uint64_t storage_write_bytes;
        uint32_t output_values;
        uint64_t output_bytes;
        lxp_u128 fee_units;
        uint8_t transfer_root[32];
        uint8_t *graph;
        uint32_t graph_length;
        uint8_t *terminal;
        uint32_t terminal_length;
        uint8_t *events;
        uint32_t events_length;
    } terminal;
    struct {
        bool active;
        uint8_t program_id[32];
        uint8_t principal[32];
        uint8_t frame_path[8];
        uint8_t frame_depth;
        uint32_t event_index;
        uint8_t *topic;
        uint16_t topic_length;
        uint8_t *data;
        uint32_t data_length;
    } event;
    uint32_t emitted_event_count;
};

static void call_activity_release(void *state)
{
    (void)state;
}

static lxp_result call_namespace_for_program(const lxp_programs_call_activity *value,
                                             const uint8_t program_id[32],
                                 uint16_t selector, uint8_t bytes[65],
                                 uint16_t *length)
{
    const lxp_call_admission_facts *admission;
    if (value == NULL || length == NULL || selector > 1U)
        return LXP_ERR_NON_CANONICAL;
    admission = lxp_ctx_call_admission(value->ctx);
    if (admission == NULL) return LXP_FATAL_INVARIANT;
    (void)memcpy(bytes, program_id, 32U);
    bytes[32] = (uint8_t)(selector == 0U ? 0U : 1U);
    if (selector == 0U) {
        (void)memcpy(bytes + 33U, admission->payer, 32U);
        *length = 65U;
    } else *length = 33U;
    return LXP_OK;
}

static lxp_result call_namespace(const lxp_programs_call_activity *value,
                                 uint16_t selector, uint8_t bytes[65],
                                 uint16_t *length)
{
    return call_namespace_for_program(value, value->program_id, selector,
                                      bytes, length);
}

static lxp_result storage_cell(const lxp_programs_call_activity *value,
                               uint16_t selector, uint32_t index,
                               const uint8_t **key, uint16_t *key_length,
                               const uint8_t **cell_value,
                               uint32_t *value_length, uint32_t *count)
{
    uint8_t ns[65]; uint16_t ns_length;
    lxp_result status = call_namespace(value, selector, ns, &ns_length);
    if (status != LXP_OK) return status;
    return lxp_programs_storage_cell_at(value->ctx, ns, ns_length, index,
        key, key_length, cell_value, value_length, count);
}

lxp_result layerx_programs_call_storage_cell_count(uint64_t token,
                                                    uint16_t selector)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value; uint16_t key_length;
    uint32_t value_length, count = 0U;
    lxp_result status = storage_cell(value, selector, 0U, &key, &key_length,
                                     &cell_value, &value_length, &count);
    if (status == LXP_ERR_UNKNOWN_FIELD && count == 0U) return 0;
    return status == LXP_OK ? (lxp_result)count : status;
}

lxp_result layerx_programs_call_storage_cell_length(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value; uint16_t key_length;
    uint32_t value_length, count;
    lxp_result status = storage_cell(value, selector, index, &key, &key_length,
                                     &cell_value, &value_length, &count);
    if (status != LXP_OK) return status;
    if (section == 0U) return (lxp_result)key_length;
    if (section == 1U && value_length <= INT32_MAX) return (lxp_result)value_length;
    return section == 1U ? LXP_ERR_LENGTH_LIMIT : LXP_ERR_UNKNOWN_FIELD;
}

lxp_result layerx_programs_call_storage_cell_byte(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section,
    uint32_t offset)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value, *bytes; uint16_t key_length;
    uint32_t value_length, count, length;
    lxp_result status = storage_cell(value, selector, index, &key, &key_length,
                                     &cell_value, &value_length, &count);
    if (status != LXP_OK) return status;
    if (section == 0U) { bytes = key; length = key_length; }
    else if (section == 1U) { bytes = cell_value; length = value_length; }
    else return LXP_ERR_UNKNOWN_FIELD;
    return offset < length ? (lxp_result)bytes[offset] : LXP_ERR_TRUNCATED;
}

lxp_result layerx_programs_call_storage_final_begin(
    uint64_t token, uint16_t selector, uint32_t count)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    void *allocation; lxp_result status;
    if (value == NULL || value->ctx == NULL || selector > 1U ||
        value->storage_final[selector].begun) return LXP_ERR_NON_CANONICAL;
    if (count > INT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    if (count == 0U) { value->storage_final[selector].begun = true; return LXP_OK; }
    if (sizeof(lxp_programs_storage_cell) > SIZE_MAX / count) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_ctx_arena_alloc(value->ctx,
        (size_t)count * sizeof(lxp_programs_storage_cell),
        _Alignof(lxp_programs_storage_cell), &allocation);
    if (status != LXP_OK) return status;
    value->storage_final[selector].begun = true;
    value->storage_final[selector].cells = allocation;
    value->storage_final[selector].count = count;
    (void)memset(allocation, 0, (size_t)count * sizeof(lxp_programs_storage_cell));
    return LXP_OK;
}

lxp_result layerx_programs_call_storage_final_cell(
    uint64_t token, uint16_t selector, uint32_t index,
    uint16_t key_length, uint32_t value_length)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_storage_cell *cell; void *key, *cell_value; lxp_result status;
    if (value == NULL || selector > 1U || index >= value->storage_final[selector].count ||
        key_length == 0U || key_length > LX_PROGRAMS_STORAGE_MAX_KEY_BYTES ||
        value_length > LX_PROGRAMS_STORAGE_MAX_VALUE_BYTES) return LXP_ERR_NON_CANONICAL;
    cell = &value->storage_final[selector].cells[index];
    if (cell->key != NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(value->ctx, key_length, 1U, &key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_arena_alloc(value->ctx, value_length == 0U ? 1U : value_length,
                                 1U, &cell_value);
    if (status != LXP_OK) return status;
    cell->key = key; cell->key_length = key_length;
    cell->value = cell_value; cell->value_length = value_length;
    return LXP_OK;
}

lxp_result layerx_programs_call_storage_final_byte(
    uint64_t token, uint16_t selector, uint32_t index, uint16_t section,
    uint32_t offset, uint8_t byte)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_storage_cell *cell; uint8_t *bytes; uint32_t length;
    if (value == NULL || selector > 1U || index >= value->storage_final[selector].count)
        return LXP_ERR_NON_CANONICAL;
    cell = &value->storage_final[selector].cells[index];
    if (section == 0U) { bytes = (uint8_t *)cell->key; length = cell->key_length; }
    else if (section == 1U) { bytes = (uint8_t *)cell->value; length = cell->value_length; }
    else return LXP_ERR_UNKNOWN_FIELD;
    if (bytes == NULL || offset >= length) return LXP_ERR_TRUNCATED;
    bytes[offset] = byte; return LXP_OK;
}

lxp_result layerx_programs_call_storage_final_apply(uint64_t token,
                                                     uint16_t selector)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    uint8_t ns[65]; uint16_t ns_length; lxp_result status;
    if (value == NULL || selector > 1U || !value->storage_settlement_authorized ||
        !value->storage_final[selector].begun)
        return LXP_ERR_NON_CANONICAL;
    if (value->storage_final[selector].applied) return LXP_ERR_NON_CANONICAL;
    status = call_namespace(value, selector, ns, &ns_length);
    if (status != LXP_OK) return status;
    status = lxp_programs_storage_stage_final(value->ctx, ns, ns_length,
        value->storage_final[selector].cells,
        value->storage_final[selector].count);
    if (status == LXP_OK) value->storage_final[selector].applied = true;
    return status;
}

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static lx_account *account_by_id(lx_account_registry *accounts,
                                 const uint8_t id[32])
{
    size_t index;
    if (accounts == NULL) return NULL;
    for (index = 0U; index < accounts->count; ++index)
        if (lxp_ct_memcmp(accounts->accounts[index].id, id, 32U) == 0)
            return &accounts->accounts[index];
    return NULL;
}

static void program_key(const uint8_t program_id[32], uint8_t key[PROGRAM_KEY_BYTES])
{
    static const uint8_t prefix[8] = {'p', 'r', 'o', 'g', 'r', 'a', 'm', 0};
    (void)memcpy(key, prefix, sizeof(prefix));
    (void)memcpy(key + sizeof(prefix), program_id, 32U);
}

static lxp_result catalog_count_visit(const uint8_t *key, size_t key_length,
                                      const uint8_t *record, size_t record_length,
                                      void *user)
{
    lxp_programs_call_activity *value = user;
    if (value == NULL || key == NULL || record == NULL ||
        key_length != PROGRAM_KEY_BYTES || record_length != PROGRAM_RECORD_BYTES ||
        memcmp(key, "program\0", 8U) != 0 || lxp_ct_is_zero(key + 8U, 32U) ||
        read_u16(record + 65U) != LX_PROGRAMS_ABI_VERSION ||
        lxp_ct_is_zero(record + 33U, 32U) || value->catalog_count == UINT32_MAX)
        return LXP_FATAL_INVARIANT;
    ++value->catalog_count;
    return LXP_OK;
}

static lxp_result catalog_fill_visit(const uint8_t *key, size_t key_length,
                                     const uint8_t *record, size_t record_length,
                                     void *user)
{
    lxp_programs_call_activity *value = user;
    lxp_programs_call_catalog_entry *entry;
    const uint8_t *wasm;
    size_t wasm_length;
    lxp_result status;
    if (value == NULL || key == NULL || record == NULL ||
        key_length != PROGRAM_KEY_BYTES || record_length != PROGRAM_RECORD_BYTES ||
        value->catalog == NULL || value->catalog_count == 0U)
        return LXP_FATAL_INVARIANT;
    if (value->catalog_cursor >= value->catalog_count)
        return LXP_FATAL_INVARIANT;
    entry = &value->catalog[value->catalog_cursor];
    (void)memcpy(entry->program_id, key + 8U, 32U);
    (void)memcpy(entry->code_hash, record + 33U, 32U);
    entry->abi_version = read_u16(record + 65U);
    status = lxp_programs_artifact_open(value->ctx, entry->program_id,
                                        entry->code_hash, &wasm, &wasm_length);
    if (status != LXP_OK) return status;
    if (wasm_length == 0U || wasm_length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    entry->wasm_length = (uint32_t)wasm_length;
    ++value->catalog_cursor;
    return LXP_OK;
}

static lxp_result call_catalog_build(lxp_programs_call_activity *value)
{
    static const uint8_t prefix[8] = {'p','r','o','g','r','a','m',0};
    uint32_t count;
    void *allocation;
    lxp_result status;
    if (value == NULL || value->ctx == NULL || value->catalog != NULL ||
        value->catalog_count != 0U) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_iter(value->ctx, prefix, sizeof(prefix),
                             catalog_count_visit, value);
    if (status != LXP_OK) return status;
    count = value->catalog_count;
    if (count == 0U || count > INT32_MAX ||
        sizeof(*value->catalog) > SIZE_MAX / count)
        return LXP_ERR_LENGTH_LIMIT;
    status = lxp_ctx_arena_alloc(value->ctx,
                                 (size_t)count * sizeof(*value->catalog),
                                 _Alignof(lxp_programs_call_catalog_entry),
                                 &allocation);
    if (status != LXP_OK) return status;
    value->catalog = allocation;
    (void)memset(value->catalog, 0, (size_t)count * sizeof(*value->catalog));
    status = lxp_ctx_kv_iter(value->ctx, prefix, sizeof(prefix),
                             catalog_fill_visit, value);
    if (status != LXP_OK || value->catalog_cursor != count)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static lxp_programs_call_catalog_entry *catalog_entry(
    lxp_programs_call_activity *value, uint32_t index)
{
    if (value == NULL || value->catalog == NULL || index >= value->catalog_count)
        return NULL;
    return &value->catalog[index];
}

lxp_result layerx_programs_call_catalog_count(uint64_t token)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    if (value == NULL || value->catalog == NULL || value->catalog_count == 0U ||
        value->catalog_count > INT32_MAX) return LXP_ERR_NON_CANONICAL;
    return (lxp_result)value->catalog_count;
}

lxp_result layerx_programs_call_catalog_wasm_length(uint64_t token,
                                                     uint32_t index)
{
    lxp_programs_call_catalog_entry *entry = catalog_entry(
        (lxp_programs_call_activity *)(uintptr_t)token, index);
    if (entry == NULL || entry->wasm_length == 0U ||
        entry->wasm_length > INT32_MAX) return LXP_ERR_NON_CANONICAL;
    return (lxp_result)entry->wasm_length;
}

lxp_result layerx_programs_call_catalog_identity_byte(
    uint64_t token, uint32_t index, uint16_t section, uint32_t offset)
{
    lxp_programs_call_catalog_entry *entry = catalog_entry(
        (lxp_programs_call_activity *)(uintptr_t)token, index);
    const uint8_t *bytes;
    if (entry == NULL || offset >= 32U) return LXP_ERR_NON_CANONICAL;
    if (section == 0U) bytes = entry->program_id;
    else if (section == 1U) bytes = entry->code_hash;
    else return LXP_ERR_UNKNOWN_FIELD;
    return (lxp_result)bytes[offset];
}

lxp_result layerx_programs_call_catalog_wasm_byte(uint64_t token,
                                                   uint32_t index,
                                                   uint32_t offset)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_call_catalog_entry *entry = catalog_entry(value, index);
    const uint8_t *wasm;
    size_t wasm_length;
    lxp_result status;
    if (entry == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_programs_artifact_open(value->ctx, entry->program_id,
                                        entry->code_hash, &wasm, &wasm_length);
    if (status != LXP_OK) return status;
    if (wasm_length != entry->wasm_length || offset >= wasm_length)
        return LXP_ERR_TRUNCATED;
    return (lxp_result)wasm[offset];
}

lxp_result layerx_programs_call_receipt_view_begin(
    uint64_t token, uint64_t d0, uint64_t d1, uint64_t d2, uint64_t d3)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    uint8_t digest[32];
    lxp_result status;
    if (value == NULL || value->ctx == NULL) return LXP_ERR_NON_CANONICAL;
    write_u64(digest, d0);
    write_u64(digest + 8U, d1);
    write_u64(digest + 16U, d2);
    write_u64(digest + 24U, d3);
    status = lxp_ctx_verified_receipt_facts(value->ctx, digest,
                                             &value->receipt_view);
    if (status != LXP_OK) {
        value->receipt_view_active = false;
        (void)memset(&value->receipt_view, 0, sizeof(value->receipt_view));
        return status;
    }
    if (lxp_ct_memcmp(value->receipt_view.receipt_digest, digest, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    value->receipt_view_active = true;
    return LXP_OK;
}

lxp_result layerx_programs_call_receipt_view_byte(
    uint64_t token, uint16_t section, uint32_t offset)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    uint8_t result[4];
    uint8_t amount[16];
    const uint8_t *bytes;
    size_t length;
    if (value == NULL || !value->receipt_view_active)
        return LXP_ERR_UNKNOWN_FIELD;
    if (section == 0U) {
        bytes = value->receipt_view.receipt_digest;
        length = 32U;
    } else if (section == 1U) {
        result[0] = (uint8_t)((uint32_t)value->receipt_view.result_code >> 24U);
        result[1] = (uint8_t)((uint32_t)value->receipt_view.result_code >> 16U);
        result[2] = (uint8_t)((uint32_t)value->receipt_view.result_code >> 8U);
        result[3] = (uint8_t)(uint32_t)value->receipt_view.result_code;
        bytes = result;
        length = sizeof(result);
    } else if (section == 2U) {
        bytes = value->receipt_view.asset;
        length = 32U;
    } else if (section == 3U) {
        lxp_u128_to_be(value->receipt_view.amount, amount);
        bytes = amount;
        length = sizeof(amount);
    } else if (section == 4U) {
        bytes = value->receipt_view.resulting_state_root;
        length = 32U;
    } else {
        return LXP_ERR_UNKNOWN_FIELD;
    }
    if (offset >= length) return LXP_ERR_TRUNCATED;
    return (lxp_result)bytes[offset];
}

static lxp_result catalog_storage_cell(const lxp_programs_call_activity *value,
                                       uint32_t program_index,
                                       uint16_t selector, uint32_t index,
                                       const uint8_t **key, uint16_t *key_length,
                                       const uint8_t **cell_value,
                                       uint32_t *value_length, uint32_t *count)
{
    lxp_programs_call_catalog_entry *entry = catalog_entry(
        (lxp_programs_call_activity *)value, program_index);
    uint8_t ns[65];
    uint16_t ns_length;
    lxp_result status;
    if (entry == NULL) return LXP_ERR_NON_CANONICAL;
    status = call_namespace_for_program(value, entry->program_id, selector,
                                        ns, &ns_length);
    if (status != LXP_OK) return status;
    return lxp_programs_storage_cell_at(value->ctx, ns, ns_length, index,
                                        key, key_length, cell_value,
                                        value_length, count);
}

lxp_result layerx_programs_call_catalog_storage_cell_count(
    uint64_t token, uint32_t program_index, uint16_t selector)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value;
    uint16_t key_length;
    uint32_t value_length, count = 0U;
    lxp_result status = catalog_storage_cell(value, program_index, selector, 0U,
                                             &key, &key_length, &cell_value,
                                             &value_length, &count);
    if (status == LXP_ERR_UNKNOWN_FIELD && count == 0U) return 0;
    return status == LXP_OK ? (lxp_result)count : status;
}

lxp_result layerx_programs_call_catalog_storage_cell_length(
    uint64_t token, uint32_t program_index, uint16_t selector,
    uint32_t index, uint16_t section)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value;
    uint16_t key_length;
    uint32_t value_length, count;
    lxp_result status = catalog_storage_cell(value, program_index, selector, index,
                                             &key, &key_length, &cell_value,
                                             &value_length, &count);
    if (status != LXP_OK) return status;
    if (section == 0U) return (lxp_result)key_length;
    if (section == 1U && value_length <= INT32_MAX) return (lxp_result)value_length;
    return section == 1U ? LXP_ERR_LENGTH_LIMIT : LXP_ERR_UNKNOWN_FIELD;
}

lxp_result layerx_programs_call_catalog_storage_cell_byte(
    uint64_t token, uint32_t program_index, uint16_t selector,
    uint32_t index, uint16_t section, uint32_t offset)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *key, *cell_value, *bytes;
    uint16_t key_length;
    uint32_t value_length, count, length;
    lxp_result status = catalog_storage_cell(value, program_index, selector, index,
                                             &key, &key_length, &cell_value,
                                             &value_length, &count);
    if (status != LXP_OK) return status;
    if (section == 0U) { bytes = key; length = key_length; }
    else if (section == 1U) { bytes = cell_value; length = value_length; }
    else return LXP_ERR_UNKNOWN_FIELD;
    return offset < length ? (lxp_result)bytes[offset] : LXP_ERR_TRUNCATED;
}

static lxp_result catalog_storage_final_slot(lxp_programs_call_activity *value,
                                             uint32_t program_index,
                                             uint16_t selector,
                                             lxp_programs_storage_cell **cells,
                                             uint32_t **count)
{
    lxp_programs_call_catalog_entry *entry = catalog_entry(value, program_index);
    if (entry == NULL || selector > 1U || cells == NULL || count == NULL)
        return LXP_ERR_NON_CANONICAL;
    *cells = entry->storage_final[selector].cells;
    *count = &entry->storage_final[selector].count;
    return LXP_OK;
}

lxp_result layerx_programs_call_catalog_storage_final_begin(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t count)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_storage_cell *cells;
    uint32_t *stored_count;
    void *allocation;
    lxp_result status = catalog_storage_final_slot(value, program_index, selector,
                                                    &cells, &stored_count);
    lxp_programs_call_catalog_entry *entry = catalog_entry(value, program_index);
    if (status != LXP_OK || entry == NULL || entry->storage_final[selector].begun)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    if (count > INT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    if (count == 0U) { entry->storage_final[selector].begun = true; return LXP_OK; }
    if (sizeof(*cells) > SIZE_MAX / count) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_ctx_arena_alloc(value->ctx, (size_t)count * sizeof(*cells),
                                 _Alignof(lxp_programs_storage_cell), &allocation);
    if (status != LXP_OK) return status;
    entry->storage_final[selector].begun = true;
    entry->storage_final[selector].cells =
        (lxp_programs_storage_cell *)allocation;
    cells = entry->storage_final[selector].cells;
    *stored_count = count;
    (void)memset(cells, 0, (size_t)count * sizeof(*cells));
    return LXP_OK;
}

lxp_result layerx_programs_call_catalog_storage_final_cell(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t index,
    uint16_t key_length, uint32_t value_length)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_storage_cell *cells;
    uint32_t *count;
    void *key, *cell_value;
    lxp_result status = catalog_storage_final_slot(value, program_index, selector,
                                                    &cells, &count);
    if (status != LXP_OK || cells == NULL || index >= *count || key_length == 0U ||
        key_length > LX_PROGRAMS_STORAGE_MAX_KEY_BYTES ||
        value_length > LX_PROGRAMS_STORAGE_MAX_VALUE_BYTES)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    if (cells[index].key != NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(value->ctx, key_length, 1U, &key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_arena_alloc(value->ctx, value_length == 0U ? 1U : value_length,
                                 1U, &cell_value);
    if (status != LXP_OK) return status;
    cells[index].key = key; cells[index].key_length = key_length;
    cells[index].value = cell_value; cells[index].value_length = value_length;
    return LXP_OK;
}

lxp_result layerx_programs_call_catalog_storage_final_byte(
    uint64_t token, uint32_t program_index, uint16_t selector, uint32_t index,
    uint16_t section, uint32_t offset, uint8_t byte)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_storage_cell *cells;
    uint32_t *count, length;
    uint8_t *bytes;
    lxp_result status = catalog_storage_final_slot(value, program_index, selector,
                                                    &cells, &count);
    if (status != LXP_OK || cells == NULL || index >= *count)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    if (section == 0U) { bytes = (uint8_t *)cells[index].key; length = cells[index].key_length; }
    else if (section == 1U) { bytes = (uint8_t *)cells[index].value; length = cells[index].value_length; }
    else return LXP_ERR_UNKNOWN_FIELD;
    if (bytes == NULL || offset >= length) return LXP_ERR_TRUNCATED;
    bytes[offset] = byte;
    return LXP_OK;
}

lxp_result layerx_programs_call_catalog_storage_final_apply(
    uint64_t token, uint32_t program_index, uint16_t selector)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_programs_call_catalog_entry *entry = catalog_entry(value, program_index);
    uint8_t ns[65];
    uint16_t ns_length;
    lxp_result status;
    if (entry == NULL || selector > 1U) return LXP_ERR_NON_CANONICAL;
    if (!value->storage_settlement_authorized ||
        !entry->storage_final[selector].begun) return LXP_ERR_NON_CANONICAL;
    if (entry->storage_final[selector].applied) return LXP_ERR_NON_CANONICAL;
    status = call_namespace_for_program(value, entry->program_id, selector,
                                        ns, &ns_length);
    if (status != LXP_OK) return status;
    status = lxp_programs_storage_stage_final(value->ctx, ns, ns_length,
        entry->storage_final[selector].cells, entry->storage_final[selector].count);
    if (status == LXP_OK) entry->storage_final[selector].applied = true;
    return status;
}

lxp_result layerx_programs_call_storage_final_authorize(uint64_t token)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    if (value == NULL || value->ctx == NULL || value->storage_settlement_authorized)
        return LXP_ERR_NON_CANONICAL;
    value->storage_settlement_authorized = true;
    return LXP_OK;
}

static lxp_result terminal_buffer(lxp_programs_call_activity *value,
                                  uint16_t section, uint8_t **bytes,
                                  uint32_t *length)
{
    if (value == NULL || !value->terminal.active || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (section == LX_PROGRAMS_TERMINAL_BYTES_GRAPH) {
        *bytes = value->terminal.graph;
        *length = value->terminal.graph_length;
    } else if (section == LX_PROGRAMS_TERMINAL_BYTES_PAYLOAD) {
        *bytes = value->terminal.terminal;
        *length = value->terminal.terminal_length;
    } else if (section == LX_PROGRAMS_TERMINAL_BYTES_EVENTS) {
        *bytes = value->terminal.events;
        *length = value->terminal.events_length;
    } else return LXP_ERR_UNKNOWN_FIELD;
    return *bytes == NULL || *length == 0U ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

lxp_result layerx_programs_call_terminal_begin(
    uint64_t token, uint8_t terminal_kind, lxp_result result_code,
    uint16_t runtime_version,
    uint16_t abi_version, uint32_t fee_schedule_version,
    uint64_t cpu_fuel, uint64_t memory_bytes, uint64_t storage_read_bytes,
    uint64_t storage_write_bytes, uint32_t output_values, uint64_t output_bytes,
    uint64_t fee_hi, uint64_t fee_lo,
    uint64_t transfer0, uint64_t transfer1, uint64_t transfer2, uint64_t transfer3,
    uint32_t graph_length, uint32_t terminal_length, uint32_t events_length)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    void *graph, *terminal, *events;
    uint8_t transfer_root[32];
    lxp_result status;
    write_u64(transfer_root, transfer0);
    write_u64(transfer_root + 8U, transfer1);
    write_u64(transfer_root + 16U, transfer2);
    write_u64(transfer_root + 24U, transfer3);
    if (value == NULL || value->ctx == NULL || value->terminal.active ||
        (terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS &&
         terminal_kind != LXP_PROGRAM_TERMINAL_FAILURE &&
         terminal_kind != LXP_PROGRAM_TERMINAL_RESOURCE) ||
        (terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS && result_code != LXP_OK) ||
        (terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS &&
         (result_code == LXP_OK || lxp_result_is_fatal(result_code))) ||
        runtime_version == 0U || abi_version == 0U || fee_schedule_version == 0U ||
        graph_length == 0U || terminal_length == 0U || events_length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS) {
        if (value->transfer_set == NULL) {
            if (!lxp_ct_is_zero(transfer_root, sizeof(transfer_root)))
                return LXP_ERR_NON_CANONICAL;
        } else if (!value->transfer_applied || lxp_ct_memcmp(
                       transfer_root, value->transfer_receipt.transfer_set_root,
                       sizeof(transfer_root)) != 0) return LXP_ERR_NON_CANONICAL;
    } else if (!lxp_ct_is_zero(transfer_root, sizeof(transfer_root))) {
        return LXP_ERR_NON_CANONICAL;
    }
    status = lxp_ctx_arena_alloc(value->ctx, graph_length, 1U, &graph);
    if (status != LXP_OK) return status;
    status = lxp_ctx_arena_alloc(value->ctx, terminal_length, 1U, &terminal);
    if (status != LXP_OK) return status;
    status = lxp_ctx_arena_alloc(value->ctx, events_length, 1U, &events);
    if (status != LXP_OK) return status;
    value->terminal.active = true;
    value->terminal.terminal_kind = terminal_kind;
    value->terminal.result_code = result_code;
    value->terminal.runtime_version = runtime_version;
    value->terminal.abi_version = abi_version;
    value->terminal.fee_schedule_version = fee_schedule_version;
    value->terminal.cpu_fuel = cpu_fuel;
    value->terminal.memory_bytes = memory_bytes;
    value->terminal.storage_read_bytes = storage_read_bytes;
    value->terminal.storage_write_bytes = storage_write_bytes;
    value->terminal.output_values = output_values;
    value->terminal.output_bytes = output_bytes;
    value->terminal.fee_units = (lxp_u128){fee_hi, fee_lo};
    (void)memcpy(value->terminal.transfer_root, transfer_root,
                 sizeof(transfer_root));
    value->terminal.graph = graph; value->terminal.graph_length = graph_length;
    value->terminal.terminal = terminal; value->terminal.terminal_length = terminal_length;
    value->terminal.events = events; value->terminal.events_length = events_length;
    return LXP_OK;
}

lxp_result layerx_programs_call_terminal_byte(uint64_t token, uint16_t section,
                                               uint32_t offset, uint8_t byte)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    uint8_t *bytes;
    uint32_t length;
    lxp_result status = terminal_buffer(value, section, &bytes, &length);
    if (status != LXP_OK) return status;
    if (offset >= length) return LXP_ERR_TRUNCATED;
    bytes[offset] = byte;
    return LXP_OK;
}

lxp_result layerx_programs_call_terminal_publish(uint64_t token)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lxp_program_outcome outcome;
    lxp_programs_call_outcome event;
    const lxp_call_admission_facts *admission;
    const uint8_t *activity_id;
    uint8_t graph_root[32], terminal_root[32], events_root[32], frame[8] = {0};
    lxp_result status;
    if (value == NULL || value->ctx == NULL || !value->terminal.active)
        return LXP_ERR_NON_CANONICAL;
    admission = lxp_ctx_call_admission(value->ctx);
    activity_id = lxp_ctx_activity_id(value->ctx);
    if (admission == NULL || activity_id == NULL) return LXP_FATAL_INVARIANT;
    status = lxp_hash_sha256(value->terminal.graph, value->terminal.graph_length, graph_root);
    if (status == LXP_OK) status = lxp_hash_sha256(value->terminal.terminal,
                                                    value->terminal.terminal_length,
                                                    terminal_root);
    if (status == LXP_OK) status = lxp_hash_sha256(value->terminal.events,
                                                    value->terminal.events_length,
                                                    events_root);
    if (status != LXP_OK) return status;
    (void)memset(&outcome, 0, sizeof(outcome));
    outcome.present = true;
    outcome.terminal_kind = value->terminal.terminal_kind;
    outcome.result_code = value->terminal.result_code;
    outcome.runtime_version = value->terminal.runtime_version;
    outcome.abi_version = value->terminal.abi_version;
    outcome.fee_schedule_version = value->terminal.fee_schedule_version;
    outcome.cpu_fuel = value->terminal.cpu_fuel;
    outcome.memory_bytes = value->terminal.memory_bytes;
    outcome.storage_read_bytes = value->terminal.storage_read_bytes;
    outcome.storage_write_bytes = value->terminal.storage_write_bytes;
    outcome.output_values = value->terminal.output_values;
    outcome.output_bytes = value->terminal.output_bytes;
    outcome.fee_units = value->terminal.fee_units;
    (void)memcpy(outcome.call_graph_root, graph_root, sizeof(graph_root));
    (void)memcpy(outcome.terminal_payload_root, terminal_root, sizeof(terminal_root));
    if (outcome.terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS)
        return lxp_ctx_bind_program_outcome(value->ctx, &outcome);
    (void)memcpy(outcome.transfer_root, value->terminal.transfer_root, 32U);
    event.program_id = value->program_id;
    event.principal = admission->payer;
    event.activity_id = activity_id;
    event.frame_path = frame;
    event.frame_depth = 0U;
    event.runtime_version = outcome.runtime_version;
    event.abi_version = outcome.abi_version;
    event.fee_schedule_version = outcome.fee_schedule_version;
    event.terminal_result = outcome.result_code;
    event.transfer_set_root = outcome.transfer_root;
    event.call_graph_digest = outcome.call_graph_root;
    event.terminal_detail_digest = outcome.terminal_payload_root;
    event.event_envelope_digest = events_root;
    status = lxp_programs_emit_call_outcome(value->ctx, &event);
    if (status != LXP_OK) return status;
    return lxp_ctx_bind_program_outcome(value->ctx, &outcome);
}

static bool catalog_contains(const lxp_programs_call_activity *value,
                             const uint8_t program_id[32])
{
    uint32_t index;
    if (value == NULL || value->catalog == NULL) return false;
    for (index = 0U; index < value->catalog_count; ++index)
        if (lxp_ct_memcmp(value->catalog[index].program_id, program_id, 32U) == 0)
            return true;
    return false;
}

static bool event_frame_valid(const uint8_t path[8], uint8_t depth)
{
    size_t index;
    if (depth > 8U) return false;
    for (index = 0U; index < 8U; ++index)
        if ((index < depth && path[index] == 0U) ||
            (index >= depth && path[index] != 0U)) return false;
    return true;
}

lxp_result layerx_programs_call_event_begin(
    uint64_t token, uint32_t event_index,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t frame_path, uint8_t frame_depth,
    uint16_t topic_length, uint32_t data_length)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    const lxp_call_admission_facts *admission;
    void *topic, *data;
    lxp_result status;
    if (value == NULL || value->ctx == NULL || value->event.active ||
        event_index != value->emitted_event_count ||
        topic_length > LXP_PROGRAMS_EVENT_MAX_TOPIC_BYTES ||
        data_length > LXP_PROGRAMS_EVENT_MAX_DATA_BYTES)
        return LXP_ERR_NON_CANONICAL;
    admission = lxp_ctx_call_admission(value->ctx);
    if (admission == NULL) return LXP_FATAL_INVARIANT;
    write_u64(value->event.program_id, p0);
    write_u64(value->event.program_id + 8U, p1);
    write_u64(value->event.program_id + 16U, p2);
    write_u64(value->event.program_id + 24U, p3);
    write_u64(value->event.principal, r0);
    write_u64(value->event.principal + 8U, r1);
    write_u64(value->event.principal + 16U, r2);
    write_u64(value->event.principal + 24U, r3);
    write_u64(value->event.frame_path, frame_path);
    if (!catalog_contains(value, value->event.program_id) ||
        lxp_ct_memcmp(value->event.principal, admission->payer, 32U) != 0 ||
        !event_frame_valid(value->event.frame_path, frame_depth))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(value->ctx, topic_length == 0U ? 1U : topic_length,
                                 1U, &topic);
    if (status != LXP_OK) return status;
    status = lxp_ctx_arena_alloc(value->ctx, data_length == 0U ? 1U : data_length,
                                 1U, &data);
    if (status != LXP_OK) return status;
    value->event.active = true;
    value->event.event_index = event_index;
    value->event.frame_depth = frame_depth;
    value->event.topic = topic;
    value->event.topic_length = topic_length;
    value->event.data = data;
    value->event.data_length = data_length;
    return LXP_OK;
}

lxp_result layerx_programs_call_event_byte(uint64_t token, uint16_t section,
                                            uint32_t offset, uint8_t byte)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    uint8_t *bytes;
    uint32_t length;
    if (value == NULL || !value->event.active) return LXP_ERR_NON_CANONICAL;
    if (section == 0U) {
        bytes = value->event.topic;
        length = value->event.topic_length;
    } else if (section == 1U) {
        bytes = value->event.data;
        length = value->event.data_length;
    } else return LXP_ERR_UNKNOWN_FIELD;
    if (bytes == NULL || offset >= length) return LXP_ERR_TRUNCATED;
    bytes[offset] = byte;
    return LXP_OK;
}

lxp_result layerx_programs_call_event_emit(uint64_t token)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *activity_id;
    lxp_programs_guest_event event;
    lxp_result status;
    if (value == NULL || value->ctx == NULL || !value->event.active)
        return LXP_ERR_NON_CANONICAL;
    activity_id = lxp_ctx_activity_id(value->ctx);
    if (activity_id == NULL) return LXP_FATAL_INVARIANT;
    event.program_id = value->event.program_id;
    event.principal = value->event.principal;
    event.activity_id = activity_id;
    event.frame_path = value->event.frame_path;
    event.frame_depth = value->event.frame_depth;
    event.event_index = value->event.event_index;
    event.topic = value->event.topic;
    event.topic_length = value->event.topic_length;
    event.data = value->event.data;
    event.data_length = value->event.data_length;
    status = lxp_programs_emit_guest_event(value->ctx, &event);
    if (status == LXP_OK) {
        value->event.active = false;
        ++value->emitted_event_count;
    }
    return status;
}

static bool valid_entrypoint(const uint8_t *entrypoint, uint16_t length)
{
    size_t index;
    if (length == 0U || length > LX_PROGRAMS_MAX_ENTRYPOINT_BYTES)
        return false;
    for (index = 0U; index < length; ++index) {
        const uint8_t byte = entrypoint[index];
        if (!((byte >= (uint8_t)'a' && byte <= (uint8_t)'z') ||
              (byte >= (uint8_t)'A' && byte <= (uint8_t)'Z') ||
              (byte >= (uint8_t)'0' && byte <= (uint8_t)'9') ||
              byte == (uint8_t)'_' || byte == (uint8_t)'.'))
            return false;
    }
    return true;
}

static lxp_result call_scalar_begin(const lxp_programs_call_activity *value,
                                    const lxp_authority_resolved *authority)
{
    const lxp_call_admission_facts *admission = lxp_ctx_call_admission(value->ctx);
    uint64_t program[4];
    uint64_t principal[4];
    uint64_t authority_hash[4];
    uint64_t binding[4];
    size_t index;
    if (admission == NULL || admission->fee_schedule_version == 0U ||
        admission->parameter_version == 0U)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < 4U; ++index) {
        program[index] = read_u64(value->program_id + index * 8U);
        principal[index] = read_u64(admission->payer + index * 8U);
        authority_hash[index] = read_u64(authority->authority_hash + index * 8U);
        binding[index] = read_u64(admission->activity_binding + index * 8U);
    }
    return layerx_programs_call_begin(
        (uint64_t)(uintptr_t)value,
        program[0], program[1], program[2], program[3],
        principal[0], principal[1], principal[2], principal[3],
        authority_hash[0], authority_hash[1], authority_hash[2], authority_hash[3],
        binding[0], binding[1], binding[2], binding[3],
        admission->signed_fee_limit.hi, admission->signed_fee_limit.lo,
        admission->available_fee_units.hi, admission->available_fee_units.lo,
        admission->fee_schedule_version, admission->parameter_version,
        value->abi_version, value->entrypoint_length, value->wasm_length,
        value->calldata_length,
        value->capabilities_length, value->response_capacity,
        value->budget[0], value->budget[1], value->budget[2], value->budget[3],
        value->budget[4], value->budget[5], value->budget[6]);
}

lxp_result layerx_programs_call_activity_byte(uint64_t token, uint16_t section,
                                              uint32_t offset)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    const uint8_t *bytes;
    uint32_t length;
    size_t blob_length;
    if (value == NULL || token == 0U) return LXP_ERR_NON_CANONICAL;
    switch (section) {
    case LX_PROGRAMS_ACTIVITY_BYTES_WASM:
        {
            lxp_result status = lxp_programs_artifact_open(
                value->ctx, value->program_id, value->code_hash, &bytes,
                &blob_length);
            if (status != LXP_OK) return status;
            if (blob_length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
            length = (uint32_t)blob_length;
        }
        break;
    case LX_PROGRAMS_ACTIVITY_BYTES_ENTRYPOINT:
        bytes = value->entrypoint;
        length = value->entrypoint_length;
        break;
    case LX_PROGRAMS_ACTIVITY_BYTES_CALLDATA:
        bytes = value->calldata;
        length = value->calldata_length;
        break;
    case LX_PROGRAMS_ACTIVITY_BYTES_CAPABILITIES:
        bytes = value->capabilities;
        length = value->capabilities_length;
        break;
    default:
        return LXP_ERR_UNKNOWN_FIELD;
    }
    if (offset >= length) return LXP_ERR_TRUNCATED;
    return (lxp_result)bytes[offset];
}

lxp_result layerx_programs_call_transfer_begin(uint64_t token,
                                               uint16_t leg_count)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    void *allocation;
    lxp_result status;
    if (value == NULL || value->ctx == NULL || value->transfer_set != NULL ||
        leg_count == 0U ||
        leg_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(value->ctx, sizeof(*value->transfer_set),
                                 _Alignof(lxp_transfer_set), &allocation);
    if (status != LXP_OK) return status;
    value->transfer_set = (lxp_transfer_set *)allocation;
    (void)memset(value->transfer_set, 0, sizeof(*value->transfer_set));
    (void)memset(value->transfer_leg_written, 0,
                 sizeof(value->transfer_leg_written));
    value->transfer_leg_count = 0U;
    value->transfer_applied = false;
    value->transfer_set->leg_count = leg_count;
    (void)memset(&value->transfer_receipt, 0, sizeof(value->transfer_receipt));
    return LXP_OK;
}

lxp_result layerx_programs_call_transfer_leg(
    uint64_t token, uint16_t index,
    uint64_t f0, uint64_t f1, uint64_t f2, uint64_t f3,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t amount_hi, uint64_t amount_lo)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lx_programs_transfer_runtime *runtime;
    lxp_transfer_leg *leg;
    uint8_t from[32];
    uint8_t to[32];
    if (value == NULL || value->ctx == NULL || value->authority == NULL ||
        value->transfer_set == NULL || value->transfer_applied ||
        index >= value->transfer_set->leg_count || value->transfer_leg_written[index])
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(value->ctx);
    if (runtime == NULL || runtime->accounts == NULL || runtime->assets == NULL)
        return LXP_ERR_MODULE_DISABLED;
    write_u64(from, f0);
    write_u64(from + 8U, f1);
    write_u64(from + 16U, f2);
    write_u64(from + 24U, f3);
    write_u64(to, t0);
    write_u64(to + 8U, t1);
    write_u64(to + 16U, t2);
    write_u64(to + 24U, t3);
    leg = &value->transfer_set->legs[index];
    leg->from = account_by_id(runtime->accounts, from);
    leg->to = account_by_id(runtime->accounts, to);
    if (leg->from == NULL || leg->to == NULL) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    write_u64(leg->asset_id, a0);
    write_u64(leg->asset_id + 8U, a1);
    write_u64(leg->asset_id + 16U, a2);
    write_u64(leg->asset_id + 24U, a3);
    leg->amount = (lxp_u128){amount_hi, amount_lo};
    leg->reason = LXP_REASON_PAYMENT;
    leg->supply_mode = LXP_TRANSFER_CONSERVED;
    value->transfer_leg_written[index] = 1U;
    value->transfer_leg_count += 1U;
    return LXP_OK;
}

lxp_result layerx_programs_call_transfer_apply(uint64_t token)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)(uintptr_t)token;
    lx_programs_transfer_runtime *runtime;
    lxp_transfer_set *set;
    if (value == NULL || value->ctx == NULL || value->authority == NULL ||
        value->transfer_set == NULL || value->transfer_applied ||
        value->transfer_set->leg_count == 0U ||
        value->transfer_leg_count != value->transfer_set->leg_count)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(value->ctx);
    if (runtime == NULL || runtime->assets == NULL) return LXP_ERR_MODULE_DISABLED;
    set = value->transfer_set;
    set->context.assets = runtime->assets;
    set->context.asset_count = runtime->asset_count;
    (void)memcpy(set->context.authorized_from, value->authority->principal, 32U);
    set->context.actor_sequence = lxp_ctx_global_sequence(value->ctx);
    set->context.batch_timestamp = lxp_ctx_batch_timestamp_ms(value->ctx);
    set->context.sequence_account = set->legs[0].from;
    set->context.debit_authority_kind = LXP_AUTH_OWNER;
    {
        lxp_result status = lxp_ctx_emit_transfer_set(value->ctx, set,
                                                       &value->transfer_receipt);
        if (status == LXP_OK) value->transfer_applied = true;
        return status;
    }
}

lxp_result layerx_programs_call_transfer_root_byte(uint64_t token,
                                                    uint32_t offset)
{
    const lxp_programs_call_activity *value =
        (const lxp_programs_call_activity *)(uintptr_t)token;
    if (value == NULL || offset >= 32U ||
        lxp_ct_is_zero(value->transfer_receipt.transfer_set_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    return (lxp_result)value->transfer_receipt.transfer_set_root[offset];
}

lxp_result lxp_programs_call_decode(lxp_module_ctx *ctx,
                                    const uint8_t *payload,
                                    size_t payload_length, void **decoded)
{
    lxp_programs_call_activity *value;
    size_t cursor;
    size_t expected;
    size_t index;
    void *allocation;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL ||
        payload_length < PROGRAM_CALL_FIXED_BYTES)
        return LXP_ERR_TRUNCATED;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(lxp_programs_call_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (lxp_programs_call_activity *)allocation;
    (void)memset(value, 0, sizeof(*value));
    value->ctx = ctx;
    (void)memcpy(value->program_id, payload, sizeof(value->program_id));
    cursor = 32U;
    value->abi_version = read_u16(payload + cursor);
    cursor += 2U;
    value->entrypoint_length = read_u16(payload + cursor);
    cursor += 2U;
    value->calldata_length = read_u32(payload + cursor);
    cursor += 4U;
    value->capabilities_length = read_u16(payload + cursor);
    cursor += 2U;
    value->response_capacity = read_u32(payload + cursor);
    cursor += 4U;
    for (index = 0U; index < LX_PROGRAMS_CALL_BUDGET_FIELDS; ++index) {
        value->budget[index] = read_u64(payload + cursor);
        cursor += 8U;
    }
    if (lxp_ct_is_zero(value->program_id, sizeof(value->program_id)) ||
        value->abi_version != 1U ||
        value->calldata_length > LX_PROGRAMS_MAX_CALLDATA_BYTES ||
        value->capabilities_length > LX_PROGRAMS_MAX_CAPABILITY_BYTES ||
        value->response_capacity > LX_PROGRAMS_MAX_RESPONSE_BYTES)
        return LXP_ERR_NON_CANONICAL;
    if ((size_t)value->entrypoint_length > SIZE_MAX - cursor)
        return LXP_ERR_LENGTH_LIMIT;
    expected = cursor + (size_t)value->entrypoint_length;
    if ((size_t)value->calldata_length > SIZE_MAX - expected)
        return LXP_ERR_LENGTH_LIMIT;
    expected += (size_t)value->calldata_length;
    if ((size_t)value->capabilities_length > SIZE_MAX - expected)
        return LXP_ERR_LENGTH_LIMIT;
    expected += (size_t)value->capabilities_length;
    if (expected != payload_length ||
        !valid_entrypoint(payload + cursor, value->entrypoint_length))
        return LXP_ERR_NON_CANONICAL;
    value->entrypoint = payload + cursor;
    cursor += value->entrypoint_length;
    value->calldata = payload + cursor;
    cursor += value->calldata_length;
    value->capabilities = payload + cursor;
    *decoded = value;
    return LXP_OK;
}

lxp_result lxp_programs_call_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)decoded;
    uint8_t key[PROGRAM_KEY_BYTES];
    const uint8_t *record;
    const uint8_t *wasm;
    size_t record_length;
    size_t wasm_length;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_CALL ||
        lxp_ct_is_zero(authority->principal, sizeof(authority->principal)) ||
        lxp_ct_is_zero(authority->authority_hash, sizeof(authority->authority_hash)))
        return LXP_ERR_NON_CANONICAL;
    program_key(value->program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &record, &record_length);
    if (status != LXP_OK) return status;
    if (record_length != PROGRAM_RECORD_BYTES) return LXP_FATAL_INVARIANT;
    if (read_u16(record + 65U) != value->abi_version) return LXP_ERR_VERSION_UNSUPPORTED;
    (void)memcpy(value->code_hash, record + 33U, sizeof(value->code_hash));
    status = lxp_programs_artifact_open(ctx, value->program_id, value->code_hash,
                                        &wasm, &wasm_length);
    if (status != LXP_OK) return status;
    if (wasm_length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    value->wasm_length = (uint32_t)wasm_length;
    return lxp_ctx_charge_gas(ctx, (uint64_t)PROGRAM_CALL_FIXED_BYTES +
                              value->entrypoint_length + value->calldata_length +
                              value->capabilities_length);
}

lxp_result lxp_programs_call_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    lxp_programs_call_activity *value =
        (lxp_programs_call_activity *)decoded;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        activity->activity_type != LX_PROGRAMS_CALL)
        return LXP_ERR_NON_CANONICAL;
    value->authority = authority;
    value->effects = effects;
    status = lxp_ctx_bind_activity_state(ctx, value, call_activity_release);
    if (status != LXP_OK) return status;
    status = call_catalog_build(value);
    if (status != LXP_OK) return status;
    /* The Rust boundary consumes this exact arena-owned activity once. It must
     * publish into the existing C journal before reporting success. */
    status = call_scalar_begin(value, authority);
    return status;
}
