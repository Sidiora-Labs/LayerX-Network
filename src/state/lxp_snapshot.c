#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_crypto.h"
#include "lxp_state_internal.h"

#include <stdlib.h>
#include <string.h>

enum { LXP_SNAPSHOT_STRUCTURE_TAG = 0x1804 };

static int bytes_order(const uint8_t *left, size_t left_length,
                       const uint8_t *right, size_t right_length)
{
    size_t common = left_length < right_length ? left_length : right_length;
    int order = memcmp(left, right, common);
    if (order != 0) return order;
    return left_length < right_length ? -1 : left_length != right_length;
}

static void sort_cells(const lxp_state_store *state, size_t *indices)
{
    size_t i;
    for (i = 0U; i < state->count; ++i) indices[i] = i;
    for (i = 1U; i < state->count; ++i) {
        size_t value = indices[i];
        size_t at = i;
        while (at != 0U && memcmp(state->cells[indices[at - 1U]].key,
                                  state->cells[value].key, 32U) > 0) {
            indices[at] = indices[at - 1U];
            --at;
        }
        indices[at] = value;
    }
}

static void sort_idempotency(const lxp_state_store *state, size_t *indices)
{
    size_t i;
    for (i = 0U; i < state->idempotency_count; ++i) indices[i] = i;
    for (i = 1U; i < state->idempotency_count; ++i) {
        size_t value = indices[i];
        size_t at = i;
        while (at != 0U && memcmp(
            state->idempotency[indices[at - 1U]].key_hash,
            state->idempotency[value].key_hash, 32U) > 0) {
            indices[at] = indices[at - 1U];
            --at;
        }
        indices[at] = value;
    }
}

static int kv_order(const lxp_module_kv_entry *left,
                    const lxp_module_kv_entry *right)
{
    if (left->module_id != right->module_id)
        return left->module_id < right->module_id ? -1 : 1;
    return bytes_order(left->key, left->key_length,
                       right->key, right->key_length);
}

static void sort_kv(const lxp_kernel *kernel, size_t *indices)
{
    size_t i;
    for (i = 0U; i < kernel->module_kv_count; ++i) indices[i] = i;
    for (i = 1U; i < kernel->module_kv_count; ++i) {
        size_t value = indices[i];
        size_t at = i;
        while (at != 0U && kv_order(&kernel->module_kv[indices[at - 1U]],
                                    &kernel->module_kv[value]) > 0) {
            indices[at] = indices[at - 1U];
            --at;
        }
        indices[at] = value;
    }
}

static lxp_result snapshot_size(const lxp_kernel *kernel,
                                size_t module_root_count, size_t *size)
{
    size_t total = 4U + 8U + 4U + 4U + 4U + 4U + 2U +
                   module_root_count * 36U;
    size_t i;
    if (kernel->state->count > LXP_STATE_MAX_CELLS ||
        kernel->state->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        kernel->module_count > LXP_KERNEL_MAX_MODULE_REGISTRATIONS ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_FATAL_INVARIANT;
    if (kernel->state->count > (SIZE_MAX - total) / 52U)
        return LXP_ERR_LENGTH_LIMIT;
    total += kernel->state->count * 52U;
    for (i = 0U; i < kernel->state->idempotency_count; ++i) {
        size_t length = kernel->state->idempotency[i].receipt_length;
        if (length > LXP_STATE_MAX_RECEIPT_BYTES ||
            length > SIZE_MAX - total - 40U) return LXP_ERR_LENGTH_LIMIT;
        total += 40U + length;
    }
    for (i = 0U; i < kernel->module_count; ++i) {
        size_t count = kernel->modules[i].activity_type_count;
        if (count > LXP_MODULE_MAX_ACTIVITY_TYPES ||
            count > (SIZE_MAX - total - 27U) / 4U)
            return LXP_ERR_LENGTH_LIMIT;
        total += 27U + count * 4U;
    }
    for (i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->key_length > LXP_MODULE_MAX_KEY_BYTES ||
            entry->value_length > LXP_MODULE_MAX_VALUE_BYTES ||
            entry->key_length + entry->value_length > SIZE_MAX - total - 10U)
            return LXP_ERR_LENGTH_LIMIT;
        total += 10U + entry->key_length + entry->value_length;
    }
    *size = total;
    return LXP_OK;
}

lxp_result lxp_snapshot_write(const lxp_kernel *kernel,
                              uint64_t global_sequence, lxp_arena *arena,
                              lxp_byte_span *snapshot)
{
    lxp_codec_writer writer;
    size_t *cell_order;
    size_t *idem_order;
    size_t *kv_indices;
    void *memory;
    size_t capacity;
    size_t module_root_count;
    size_t i;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || arena == NULL ||
        snapshot == NULL || global_sequence == UINT64_MAX ||
        kernel->state->next_sequence != global_sequence + 1U)
        return LXP_ERR_SEQUENCE_MISMATCH;
    status = lxp_state_module_root_count(kernel, &module_root_count);
    if (status != LXP_OK) return status;
    status = snapshot_size(kernel, module_root_count, &capacity);
    if (status != LXP_OK) return status;
    status = lxp_arena_alloc(arena, kernel->state->count * sizeof(size_t),
                             _Alignof(size_t), &memory);
    if (status != LXP_OK) return status;
    cell_order = (size_t *)memory;
    status = lxp_arena_alloc(arena,
        kernel->state->idempotency_count * sizeof(size_t),
        _Alignof(size_t), &memory);
    if (status != LXP_OK) return status;
    idem_order = (size_t *)memory;
    status = lxp_arena_alloc(arena, kernel->module_kv_count * sizeof(size_t),
                             _Alignof(size_t), &memory);
    if (status != LXP_OK) return status;
    kv_indices = (size_t *)memory;
    sort_cells(kernel->state, cell_order);
    sort_idempotency(kernel->state, idem_order);
    sort_kv(kernel, kv_indices);
    status = lxp_codec_writer_init(&writer, arena, capacity);
    if (status == LXP_OK)
        status = lxp_codec_write_struct_header(&writer,
                                               LXP_SNAPSHOT_STRUCTURE_TAG);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, global_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer, (uint32_t)kernel->state->count);
    for (i = 0U; status == LXP_OK && i < kernel->state->count; ++i) {
        const lxp_state_cell *cell = &kernel->state->cells[cell_order[i]];
        status = lxp_codec_write_bytes(&writer, cell->key, 32U, 32U);
        if (status == LXP_OK)
            status = lxp_codec_write_u128(&writer, cell->value);
    }
    if (status == LXP_OK) status = lxp_codec_write_u32(
        &writer, (uint32_t)kernel->state->idempotency_count);
    for (i = 0U; status == LXP_OK &&
         i < kernel->state->idempotency_count; ++i) {
        const lxp_idempotency_key_state *entry =
            &kernel->state->idempotency[idem_order[i]];
        status = lxp_codec_write_bytes(&writer, entry->key_hash, 32U, 32U);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, entry->receipt,
                entry->receipt_length, LXP_STATE_MAX_RECEIPT_BYTES);
    }
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer, (uint32_t)kernel->module_count);
    for (i = 0U; status == LXP_OK && i < kernel->module_count; ++i) {
        const lxp_module_registration *registration = &kernel->modules[i];
        size_t j;
        status = lxp_codec_write_u16(&writer, registration->module_id);
        if (status == LXP_OK)
            status = lxp_codec_write_u32(&writer, registration->abi_version);
        if (status == LXP_OK)
            status = lxp_codec_write_u64(&writer, registration->enabled_epoch);
        if (status == LXP_OK)
            status = lxp_codec_write_u64(&writer, registration->disabled_epoch);
        if (status == LXP_OK)
            status = lxp_codec_write_u8(&writer,
                                        registration->enabled ? 1U : 0U);
        if (status == LXP_OK)
            status = lxp_codec_write_u32(&writer,
                        (uint32_t)registration->activity_type_count);
        for (j = 0U; status == LXP_OK &&
             j < registration->activity_type_count; ++j)
            status = lxp_codec_write_u32(&writer,
                                         registration->activity_types[j]);
    }
    if (status == LXP_OK)
        status = lxp_codec_write_u32(&writer,
                                     (uint32_t)kernel->module_kv_count);
    for (i = 0U; status == LXP_OK && i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry =
            &kernel->module_kv[kv_indices[i]];
        status = lxp_codec_write_u16(&writer, entry->module_id);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, entry->key,
                entry->key_length, LXP_MODULE_MAX_KEY_BYTES);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, entry->value,
                entry->value_length, LXP_MODULE_MAX_VALUE_BYTES);
    }
    if (status == LXP_OK)
        status = lxp_codec_write_u16(&writer,
                                     (uint16_t)module_root_count);
    for (i = 0U; status == LXP_OK && i < module_root_count; ++i) {
        uint8_t root[32];
        status = lxp_state_subtree_root(kernel, (uint16_t)i, root);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, root, 32U, 32U);
    }
    if (status != LXP_OK) return status;
    if (writer.length != capacity) return LXP_FATAL_INVARIANT;
    snapshot->bytes = writer.bytes;
    snapshot->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_snapshot_manifest_build(const uint8_t *snapshot,
                                       size_t snapshot_length,
                                       uint64_t global_sequence,
                                       const uint8_t state_root[32],
                                       lxp_snapshot_manifest_record *manifest)
{
    if ((snapshot == NULL && snapshot_length != 0U) || state_root == NULL ||
        manifest == NULL) return LXP_ERR_NON_CANONICAL;
    manifest->global_sequence = global_sequence;
    (void)memcpy(manifest->state_root, state_root, 32U);
    return lxp_hash_domain(LXP_DOMAIN_SNAPSHOT, snapshot, snapshot_length,
                           manifest->snapshot_digest);
}

lxp_result lxp_snapshot_manifest(const uint8_t *snapshot,
                                 size_t snapshot_length,
                                 uint64_t global_sequence,
                                 const uint8_t state_root[32],
                                 lxp_snapshot_manifest_record *manifest)
{
    return lxp_snapshot_manifest_build(snapshot, snapshot_length,
                                       global_sequence, state_root, manifest);
}

lxp_result lxp_snapshot_verify_root(const lxp_kernel *kernel,
                                    const lxp_snapshot_manifest_record *manifest,
                                    const uint8_t receipt_state_root[32])
{
    uint8_t computed[32];
    lxp_result status;
    if (kernel == NULL || manifest == NULL || receipt_state_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_root(kernel, computed);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(computed, manifest->state_root, 32U) == 0 &&
           lxp_ct_memcmp(computed, receipt_state_root, 32U) == 0 ?
           LXP_OK : LXP_ERR_SNAPSHOT_MISMATCH;
}

static lxp_result read_fixed(lxp_codec_reader *reader, uint8_t *output,
                             uint32_t length)
{
    lxp_byte_span span;
    lxp_result status = lxp_codec_read_bytes(reader, &span, length);
    if (status != LXP_OK) return status;
    if (span.length != length) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(output, span.bytes, length);
    return LXP_OK;
}

lxp_result lxp_snapshot_load(const uint8_t *snapshot, size_t snapshot_length,
                             const lxp_snapshot_manifest_record *manifest,
                             const uint8_t receipt_state_root[32],
                             lxp_kernel *kernel)
{
    lxp_kernel *candidate;
    lxp_state_store *state;
    lxp_codec_reader reader;
    uint8_t digest[32];
    uint64_t sequence = 0U;
    uint32_t count = 0U;
    uint16_t root_count = 0U;
    size_t i;
    lxp_result status;
    if ((snapshot == NULL && snapshot_length != 0U) || manifest == NULL ||
        receipt_state_root == NULL || kernel == NULL || kernel->state == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_domain(LXP_DOMAIN_SNAPSHOT, snapshot, snapshot_length,
                             digest);
    if (status != LXP_OK || lxp_ct_memcmp(
        digest, manifest->snapshot_digest, 32U) != 0)
        return status != LXP_OK ? status : LXP_ERR_SNAPSHOT_MISMATCH;
    candidate = malloc(sizeof(*candidate));
    state = malloc(sizeof(*state));
    if (candidate == NULL || state == NULL) {
        free(candidate); free(state); return LXP_ERR_IO;
    }
    *candidate = *kernel;
    (void)memset(state, 0, sizeof(*state));
    candidate->state = state;
    candidate->module_kv_count = 0U;
    status = lxp_codec_reader_init(&reader, snapshot, snapshot_length);
    if (status == LXP_OK)
        status = lxp_codec_read_struct_header(&reader,
                                              LXP_SNAPSHOT_STRUCTURE_TAG);
    if (status == LXP_OK) status = lxp_codec_read_u64(&reader, &sequence);
    if (status == LXP_OK && sequence != manifest->global_sequence)
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_STATE_MAX_CELLS)
        status = LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        status = read_fixed(&reader, state->cells[i].key, 32U);
        if (status == LXP_OK)
            status = lxp_codec_read_u128(&reader, &state->cells[i].value);
        if (status == LXP_OK && i != 0U &&
            memcmp(state->cells[i - 1U].key, state->cells[i].key, 32U) >= 0)
            status = LXP_ERR_NON_CANONICAL;
    }
    state->count = status == LXP_OK ? count : 0U;
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_STATE_MAX_IDEMPOTENCY)
        status = LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_byte_span receipt;
        status = read_fixed(&reader, state->idempotency[i].key_hash, 32U);
        if (status == LXP_OK)
            status = lxp_codec_read_bytes(&reader, &receipt,
                                          LXP_STATE_MAX_RECEIPT_BYTES);
        if (status == LXP_OK) {
            state->idempotency[i].receipt_length = (uint32_t)receipt.length;
            (void)memcpy(state->idempotency[i].receipt, receipt.bytes,
                         receipt.length);
        }
        if (status == LXP_OK && i != 0U && memcmp(
            state->idempotency[i - 1U].key_hash,
            state->idempotency[i].key_hash, 32U) >= 0)
            status = LXP_ERR_NON_CANONICAL;
    }
    state->idempotency_count = status == LXP_OK ? count : 0U;
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count != kernel->module_count)
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_module_registration *registration = &candidate->modules[i];
        uint16_t module_id = 0U;
        uint32_t abi_version = 0U;
        uint32_t type_count = 0U;
        uint8_t enabled = 0U;
        size_t j;
        status = lxp_codec_read_u16(&reader, &module_id);
        if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &abi_version);
        if (status == LXP_OK && (module_id != registration->module_id ||
            abi_version != registration->abi_version))
            status = LXP_ERR_SNAPSHOT_MISMATCH;
        if (status == LXP_OK)
            status = lxp_codec_read_u64(&reader,
                                        &registration->enabled_epoch);
        if (status == LXP_OK)
            status = lxp_codec_read_u64(&reader,
                                        &registration->disabled_epoch);
        if (status == LXP_OK) status = lxp_codec_read_u8(&reader, &enabled);
        if (status == LXP_OK && enabled > 1U) status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK) registration->enabled = enabled == 1U;
        if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &type_count);
        if (status == LXP_OK && type_count > LXP_MODULE_MAX_ACTIVITY_TYPES)
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK) registration->activity_type_count = type_count;
        for (j = 0U; status == LXP_OK && j < type_count; ++j)
            status = lxp_codec_read_u32(&reader,
                                        &registration->activity_types[j]);
    }
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK && count > LXP_KERNEL_MAX_MODULE_KV)
        status = LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_module_kv_entry *entry = &candidate->module_kv[i];
        lxp_byte_span key;
        lxp_byte_span value;
        status = lxp_codec_read_u16(&reader, &entry->module_id);
        if (status == LXP_OK)
            status = lxp_codec_read_bytes(&reader, &key,
                                          LXP_MODULE_MAX_KEY_BYTES);
        if (status == LXP_OK)
            status = lxp_codec_read_bytes(&reader, &value,
                                          LXP_MODULE_MAX_VALUE_BYTES);
        if (status == LXP_OK) {
            entry->key_length = (uint16_t)key.length;
            entry->value_length = (uint32_t)value.length;
            (void)memcpy(entry->key, key.bytes, key.length);
            (void)memcpy(entry->value, value.bytes, value.length);
        }
        if (status == LXP_OK && (key.length == 0U || (i != 0U &&
            kv_order(&candidate->module_kv[i - 1U], entry) >= 0)))
            status = LXP_ERR_NON_CANONICAL;
    }
    candidate->module_kv_count = status == LXP_OK ? count : 0U;
    if (status == LXP_OK) state->next_sequence = sequence + 1U;
    if (status == LXP_OK) status = lxp_codec_read_u16(&reader, &root_count);
    if (status == LXP_OK && root_count > LXP_SNAPSHOT_MODULE_ROOT_COUNT)
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK) {
        size_t expected_root_count;
        status = lxp_state_module_root_count(candidate, &expected_root_count);
        if (status == LXP_OK && root_count != expected_root_count)
            status = LXP_ERR_SNAPSHOT_MISMATCH;
    }
    for (i = 0U; status == LXP_OK && i < root_count; ++i) {
        uint8_t recorded[32];
        uint8_t computed[32];
        status = read_fixed(&reader, recorded, 32U);
        if (status == LXP_OK)
            status = lxp_state_subtree_root(candidate, (uint16_t)i, computed);
        if (status == LXP_OK && lxp_ct_memcmp(recorded, computed, 32U) != 0)
            status = LXP_ERR_SNAPSHOT_MISMATCH;
    }
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK)
        status = lxp_snapshot_verify_root(candidate, manifest,
                                          receipt_state_root);
    if (status == LXP_OK) {
        kernel->state->count = state->count;
        (void)memcpy(kernel->state->cells, state->cells,
                     state->count * sizeof(state->cells[0]));
        kernel->state->idempotency_count = state->idempotency_count;
        (void)memcpy(kernel->state->idempotency, state->idempotency,
                     state->idempotency_count * sizeof(state->idempotency[0]));
        kernel->state->next_sequence = state->next_sequence;
        (void)memcpy(kernel->modules, candidate->modules,
                     candidate->module_count * sizeof(candidate->modules[0]));
        kernel->module_kv_count = candidate->module_kv_count;
        (void)memcpy(kernel->module_kv, candidate->module_kv,
                     candidate->module_kv_count *
                     sizeof(candidate->module_kv[0]));
        (void)memcpy(kernel->current_state_root, manifest->state_root, 32U);
    }
    free(state);
    free(candidate);
    return status;
}
