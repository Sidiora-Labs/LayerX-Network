#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_crypto.h"
#include "lxp_state_internal.h"

#include <stdlib.h>
#include <string.h>

static void snapshot_put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static void snapshot_put_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static uint32_t snapshot_get_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static uint64_t snapshot_get_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

lxp_result lxp_snapshot_migration_receipt_root(
    uint32_t network_id, uint64_t global_sequence,
    const uint8_t source_canonical_state_root[32],
    const uint8_t source_receipt_state_root[32],
    const uint8_t target_canonical_state_root[32],
    uint8_t target_receipt_state_root[32])
{
    static const uint8_t domain[] =
        "LXP/snapshot-issuance-migration/v1";
    uint8_t fields[108];
    lxp_hash_context hash;
    lxp_result status;
    if (network_id == 0U || global_sequence == 0U ||
        source_canonical_state_root == NULL ||
        source_receipt_state_root == NULL ||
        target_canonical_state_root == NULL ||
        target_receipt_state_root == NULL ||
        lxp_ct_is_zero(source_canonical_state_root, 32U) ||
        lxp_ct_is_zero(source_receipt_state_root, 32U) ||
        lxp_ct_is_zero(target_canonical_state_root, 32U) ||
        lxp_ct_memcmp(source_canonical_state_root,
                      target_canonical_state_root, 32U) == 0)
        return LXP_ERR_NON_CANONICAL;
    snapshot_put_u32(fields, network_id);
    snapshot_put_u64(fields + 4U, global_sequence);
    (void)memcpy(fields + 12U, source_canonical_state_root, 32U);
    (void)memcpy(fields + 44U, source_receipt_state_root, 32U);
    (void)memcpy(fields + 76U, target_canonical_state_root, 32U);
    lxp_hash_init(&hash);
    status = lxp_hash_update(&hash, domain, sizeof(domain) - 1U);
    if (status == LXP_OK)
        status = lxp_hash_update(&hash, fields, sizeof(fields));
    return status == LXP_OK ?
        lxp_hash_final(&hash, target_receipt_state_root) : status;
}

lxp_result lxp_snapshot_migration_authorization_encode(
    const lxp_snapshot_manifest_record *target, bool include_signature,
    uint8_t encoded[LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES],
    size_t *encoded_length)
{
    const lxp_snapshot_migration_authorization *migration;
    size_t offset = 0U;
    if (target == NULL || encoded == NULL || encoded_length == NULL)
        return LXP_ERR_NON_CANONICAL;
    migration = &target->migration;
    if (!migration->present || migration->network_id == 0U ||
        migration->renamed_account_count == 0U ||
        migration->source_global_sequence != target->global_sequence ||
        lxp_ct_is_zero(migration->source_canonical_state_root, 32U) ||
        lxp_ct_is_zero(migration->source_receipt_state_root, 32U) ||
        lxp_ct_is_zero(migration->source_snapshot_digest, 32U) ||
        lxp_ct_is_zero(target->canonical_state_root, 32U) ||
        lxp_ct_is_zero(target->receipt_state_root, 32U) ||
        lxp_ct_is_zero(target->snapshot_digest, 32U) ||
        lxp_ct_is_zero(migration->signer_public_key, 32U) ||
        (include_signature && lxp_ct_is_zero(migration->signature, 64U)))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(encoded + offset, "LXSM", 4U); offset += 4U;
    encoded[offset++] = 1U;
    snapshot_put_u32(encoded + offset, migration->network_id); offset += 4U;
    snapshot_put_u64(encoded + offset, migration->source_global_sequence);
    offset += 8U;
    encoded[offset++] = (uint8_t)(migration->renamed_account_count >> 8U);
    encoded[offset++] = (uint8_t)migration->renamed_account_count;
    (void)memcpy(encoded + offset,
                 migration->source_canonical_state_root, 32U); offset += 32U;
    (void)memcpy(encoded + offset,
                 migration->source_receipt_state_root, 32U); offset += 32U;
    (void)memcpy(encoded + offset,
                 migration->source_snapshot_digest, 32U); offset += 32U;
    snapshot_put_u64(encoded + offset, target->global_sequence); offset += 8U;
    (void)memcpy(encoded + offset, target->canonical_state_root, 32U);
    offset += 32U;
    (void)memcpy(encoded + offset, target->receipt_state_root, 32U);
    offset += 32U;
    (void)memcpy(encoded + offset, target->snapshot_digest, 32U);
    offset += 32U;
    (void)memcpy(encoded + offset, migration->signer_public_key, 32U);
    offset += 32U;
    if (include_signature) {
        (void)memcpy(encoded + offset, migration->signature, 64U);
        offset += 64U;
    }
    *encoded_length = offset;
    return offset == (include_signature ?
            LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES :
            LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES - 64U) ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_snapshot_migration_authorization_decode(
    const uint8_t *encoded, size_t encoded_length,
    lxp_snapshot_manifest_record *target)
{
    lxp_snapshot_migration_authorization migration;
    size_t offset = 0U;
    if (encoded == NULL || target == NULL ||
        encoded_length != LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&migration, 0, sizeof(migration));
    if (memcmp(encoded, "LXSM", 4U) != 0 || encoded[4] != 1U)
        return LXP_ERR_VERSION_UNSUPPORTED;
    offset = 5U;
    migration.present = true;
    migration.network_id = snapshot_get_u32(encoded + offset); offset += 4U;
    migration.source_global_sequence = snapshot_get_u64(encoded + offset);
    offset += 8U;
    migration.renamed_account_count =
        (uint16_t)(((uint16_t)encoded[offset] << 8U) | encoded[offset + 1U]);
    offset += 2U;
    (void)memcpy(migration.source_canonical_state_root,
                 encoded + offset, 32U); offset += 32U;
    (void)memcpy(migration.source_receipt_state_root,
                 encoded + offset, 32U); offset += 32U;
    (void)memcpy(migration.source_snapshot_digest,
                 encoded + offset, 32U); offset += 32U;
    if (snapshot_get_u64(encoded + offset) != target->global_sequence)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    offset += 8U;
    if (lxp_ct_memcmp(encoded + offset,
                      target->canonical_state_root, 32U) != 0)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    offset += 32U;
    if (lxp_ct_memcmp(encoded + offset,
                      target->receipt_state_root, 32U) != 0)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    offset += 32U;
    if (lxp_ct_memcmp(encoded + offset,
                      target->snapshot_digest, 32U) != 0)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    offset += 32U;
    (void)memcpy(migration.signer_public_key, encoded + offset, 32U);
    offset += 32U;
    (void)memcpy(migration.signature, encoded + offset, 64U); offset += 64U;
    if (offset != encoded_length) return LXP_FATAL_INVARIANT;
    target->migration = migration;
    return LXP_OK;
}

lxp_result lxp_snapshot_migration_authorization_verify(
    const lxp_snapshot_manifest_record *target, uint32_t expected_network_id,
    const uint8_t expected_signer_public_key[32])
{
    uint8_t expected_receipt_root[32];
    uint8_t encoded[LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES];
    size_t length;
    lxp_result status;
    if (target == NULL || expected_signer_public_key == NULL ||
        expected_network_id == 0U ||
        target->migration.network_id != expected_network_id ||
        lxp_ct_memcmp(target->migration.signer_public_key,
                      expected_signer_public_key, 32U) != 0 ||
        target->migration.source_global_sequence != target->global_sequence ||
        lxp_ct_memcmp(target->migration.source_canonical_state_root,
                      target->canonical_state_root, 32U) == 0 ||
        lxp_ct_memcmp(target->migration.source_snapshot_digest,
                      target->snapshot_digest, 32U) == 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_snapshot_migration_receipt_root(
        expected_network_id, target->global_sequence,
        target->migration.source_canonical_state_root,
        target->migration.source_receipt_state_root,
        target->canonical_state_root, expected_receipt_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            expected_receipt_root, target->receipt_state_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_snapshot_migration_authorization_encode(
            target, false, encoded, &length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(
            expected_signer_public_key, target->migration.signature,
            encoded, length);
    lxp_secure_zero(encoded, sizeof(encoded));
    return status;
}

static lxp_result snapshot_digest(const uint8_t *snapshot,
                                  size_t snapshot_length,
                                  uint64_t global_sequence,
                                  const uint8_t canonical_state_root[32],
                                  const uint8_t receipt_state_root[32],
                                  uint8_t digest[32])
{
    static const uint8_t format[] = {'L', 'X', 'S', '2'};
    uint8_t fields[80];
    lxp_hash_context context;
    const uint8_t *domain;
    size_t domain_length = 0U;
    lxp_result status;
    if ((snapshot == NULL && snapshot_length != 0U) ||
        canonical_state_root == NULL || receipt_state_root == NULL ||
        digest == NULL || snapshot_length > UINT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    snapshot_put_u64(fields, global_sequence);
    (void)memcpy(fields + 8U, canonical_state_root, 32U);
    (void)memcpy(fields + 40U, receipt_state_root, 32U);
    snapshot_put_u64(fields + 72U, (uint64_t)snapshot_length);
    domain = lxp_domain_tag(LXP_DOMAIN_SNAPSHOT, &domain_length);
    if (domain == NULL) return LXP_ERR_INVALID_TAG;
    lxp_hash_init(&context);
    status = lxp_hash_update(&context, domain, domain_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&context, format, sizeof(format));
    if (status == LXP_OK)
        status = lxp_hash_update(&context, fields, sizeof(fields));
    if (status == LXP_OK)
        status = lxp_hash_update(&context, snapshot, snapshot_length);
    return status == LXP_OK ? lxp_hash_final(&context, digest) : status;
}

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

static int blob_order(const lxp_module_blob *left,
                      const lxp_module_blob *right)
{
    if (left->module_id != right->module_id)
        return left->module_id < right->module_id ? -1 : 1;
    return memcmp(left->key, right->key, 32U);
}

static void sort_blobs(const lxp_kernel *kernel, size_t *indices)
{
    size_t i;
    for (i = 0U; i < kernel->blob_count; ++i) indices[i] = i;
    for (i = 1U; i < kernel->blob_count; ++i) {
        size_t value = indices[i];
        size_t at = i;
        while (at != 0U && blob_order(&kernel->blobs[indices[at - 1U]],
                                      &kernel->blobs[value]) > 0) {
            indices[at] = indices[at - 1U];
            --at;
        }
        indices[at] = value;
    }
}

static bool module_registered(const lxp_kernel *kernel, uint16_t module_id)
{
    size_t i;
    for (i = 0U; i < kernel->module_count; ++i)
        if (kernel->modules[i].module_id == module_id) return true;
    return false;
}

static lxp_result sort_accounts(const lx_account_registry *accounts,
                                size_t *indices)
{
    size_t i;
    lxp_result status = lx_account_registry_index_validate(accounts);
    if (status != LXP_OK) return status;
    for (i = 0U; i < accounts->count; ++i) {
        status = lx_account_registry_index_slot(accounts, i, &indices[i]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result snapshot_size(const lxp_kernel *kernel,
                                size_t module_root_count,
                                bool include_accounts, size_t *size)
{
    size_t total = 4U + 8U + 4U + 4U + 4U + 4U + 2U +
                   module_root_count * 36U;
    size_t blob_total = 0U;
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
    if (kernel->blob_count > LXP_SNAPSHOT_MAX_BLOBS)
        return LXP_ERR_LENGTH_LIMIT;
    if (SIZE_MAX - total < LXP_SNAPSHOT_BLOB_SECTION_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    total += LXP_SNAPSHOT_BLOB_SECTION_BYTES;
    for (i = 0U; i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        if (blob->length == 0U || blob->bytes == NULL)
            return LXP_FATAL_INVARIANT;
        if (blob->length > LXP_SNAPSHOT_MAX_BLOB_BYTES ||
            blob->length >
                (size_t)LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES - blob_total ||
            blob->length > SIZE_MAX - total - LXP_SNAPSHOT_BLOB_ENTRY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        blob_total += blob->length;
        total += LXP_SNAPSHOT_BLOB_ENTRY_BYTES + blob->length;
    }
    if (blob_total != kernel->blob_total_bytes) return LXP_FATAL_INVARIANT;
    if (include_accounts) {
        const lx_account_registry *accounts = kernel->state->accounts;
        if (accounts == NULL) return LXP_FATAL_INVARIANT;
        if (accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
            return LXP_ERR_LENGTH_LIMIT;
        if (SIZE_MAX - total < 40U) return LXP_ERR_LENGTH_LIMIT;
        total += 40U;
        for (i = 0U; i < accounts->count; ++i) {
            const lx_account *account = &accounts->accounts[i];
            if (account->name_length == 0U ||
                account->name_length > LX_ACCOUNT_NAME_MAX ||
                SIZE_MAX - total < 149U + account->name_length)
                return LXP_ERR_LENGTH_LIMIT;
            total += 149U + account->name_length;
        }
    }
    *size = total;
    return LXP_OK;
}

static lxp_result write_account(lxp_codec_writer *writer,
                                const lx_account *account)
{
    lxp_result status;
    status = lxp_codec_write_bytes(writer, account->id, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, account->name,
                                       account->name_length,
                                       LX_ACCOUNT_NAME_MAX);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, (uint8_t)account->kind);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(writer, account->balance);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, account->asset_id, 32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, account->has_asset ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, account->next_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(writer, account->created_at_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(writer, account->frozen ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(
            writer, account->has_open_reference ? 1U : 0U);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(writer, account->authority_key,
                                       32U, 32U);
    if (status == LXP_OK)
        status = lxp_codec_write_u8(
            writer, account->has_authority_key ? 1U : 0U);
    return status;
}

lxp_result lxp_snapshot_write(const lxp_kernel *kernel,
                              uint64_t global_sequence, lxp_arena *arena,
                              lxp_byte_span *snapshot)
{
    lxp_codec_writer writer;
    size_t *cell_order;
    size_t *idem_order;
    size_t *kv_indices;
    size_t *blob_indices;
    size_t *account_indices = NULL;
    void *memory;
    size_t capacity;
    size_t module_root_count;
    size_t i;
    uint16_t snapshot_version;
    uint8_t account_root[32];
    bool include_accounts;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || arena == NULL ||
        snapshot == NULL || global_sequence == UINT64_MAX ||
        kernel->state->next_sequence != global_sequence + 1U)
        return LXP_ERR_SEQUENCE_MISMATCH;
    include_accounts = kernel->state->account_root_required;
    snapshot_version = include_accounts ?
        (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY :
        (uint16_t)LXP_PROTOCOL_VERSION_LEGACY;
    if (include_accounts) {
        if (kernel->state->accounts == NULL) return LXP_FATAL_INVARIANT;
        status = lx_account_registry_root(kernel->state->accounts,
                                          account_root);
        if (status != LXP_OK) return status;
    }
    status = lxp_state_module_root_count(kernel, &module_root_count);
    if (status != LXP_OK) return status;
    status = snapshot_size(kernel, module_root_count, include_accounts,
                           &capacity);
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
    status = lxp_arena_alloc(arena, kernel->blob_count * sizeof(size_t),
                             _Alignof(size_t), &memory);
    if (status != LXP_OK) return status;
    blob_indices = (size_t *)memory;
    if (include_accounts) {
        status = lxp_arena_alloc(
            arena, kernel->state->accounts->count * sizeof(size_t),
            _Alignof(size_t), &memory);
        if (status != LXP_OK) return status;
        account_indices = (size_t *)memory;
    }
    sort_cells(kernel->state, cell_order);
    sort_idempotency(kernel->state, idem_order);
    sort_kv(kernel, kv_indices);
    sort_blobs(kernel, blob_indices);
    if (include_accounts) {
        status = sort_accounts(kernel->state->accounts, account_indices);
        if (status != LXP_OK) return status;
    }
    status = lxp_codec_writer_init(&writer, arena, capacity);
    if (status == LXP_OK)
        status = lxp_codec_write_struct_header_version(
            &writer, (uint16_t)LXP_SNAPSHOT_FORMAT_VERSION,
            snapshot_version);
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
        status = lxp_codec_write_u32(&writer, (uint32_t)kernel->blob_count);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer,
                                     (uint64_t)kernel->blob_total_bytes);
    for (i = 0U; status == LXP_OK && i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[blob_indices[i]];
        if (i != 0U && blob_order(&kernel->blobs[blob_indices[i - 1U]],
                                  blob) >= 0) {
            status = LXP_FATAL_INVARIANT;
            break;
        }
        status = lxp_codec_write_u16(&writer, blob->module_id);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, blob->key, 32U, 32U);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer, blob->bytes, blob->length,
                                           LXP_SNAPSHOT_MAX_BLOB_BYTES);
    }
    if (status == LXP_OK && include_accounts)
        status = lxp_codec_write_u32(
            &writer, (uint32_t)kernel->state->accounts->count);
    for (i = 0U; status == LXP_OK && include_accounts &&
         i < kernel->state->accounts->count; ++i)
        status = write_account(
            &writer,
            &kernel->state->accounts->accounts[account_indices[i]]);
    if (status == LXP_OK && include_accounts)
        status = lxp_codec_write_bytes(&writer, account_root, 32U, 32U);
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
                                       const uint8_t canonical_state_root[32],
                                       const uint8_t receipt_state_root[32],
                                       lxp_snapshot_manifest_record *manifest)
{
    if ((snapshot == NULL && snapshot_length != 0U) ||
        canonical_state_root == NULL || receipt_state_root == NULL ||
        manifest == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(manifest, 0, sizeof(*manifest));
    manifest->global_sequence = global_sequence;
    (void)memcpy(manifest->canonical_state_root, canonical_state_root, 32U);
    (void)memcpy(manifest->receipt_state_root, receipt_state_root, 32U);
    return snapshot_digest(snapshot, snapshot_length, global_sequence,
                           canonical_state_root, receipt_state_root,
                           manifest->snapshot_digest);
}

lxp_result lxp_snapshot_manifest(const uint8_t *snapshot,
                                 size_t snapshot_length,
                                 uint64_t global_sequence,
                                 const uint8_t canonical_state_root[32],
                                 const uint8_t receipt_state_root[32],
                                 lxp_snapshot_manifest_record *manifest)
{
    return lxp_snapshot_manifest_build(snapshot, snapshot_length,
                                       global_sequence, canonical_state_root,
                                       receipt_state_root, manifest);
}

lxp_result lxp_snapshot_verify_root(const lxp_kernel *kernel,
                                    const lxp_snapshot_manifest_record *manifest)
{
    uint8_t computed[32];
    lxp_result status;
    if (kernel == NULL || manifest == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_root(kernel, computed);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(computed, manifest->canonical_state_root, 32U) == 0 ?
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

static lxp_result read_bool(lxp_codec_reader *reader, bool *value)
{
    uint8_t encoded;
    lxp_result status;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_codec_read_u8(reader, &encoded);
    if (status != LXP_OK) return status;
    if (encoded > 1U) return LXP_ERR_NON_CANONICAL;
    *value = encoded == 1U;
    return LXP_OK;
}

static lxp_result read_account(lxp_codec_reader *reader, lx_account *account)
{
    lxp_byte_span name;
    uint8_t kind;
    lxp_result status;
    if (reader == NULL || account == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(account, 0, sizeof(*account));
    status = read_fixed(reader, account->id, 32U);
    if (status == LXP_OK)
        status = lxp_codec_read_bytes(reader, &name, LX_ACCOUNT_NAME_MAX);
    if (status == LXP_OK && name.length == 0U)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        account->name_length = (uint16_t)name.length;
        (void)memcpy(account->name, name.bytes, name.length);
    }
    if (status == LXP_OK) status = lxp_codec_read_u8(reader, &kind);
    if (status == LXP_OK) account->kind = (lx_account_kind)kind;
    if (status == LXP_OK)
        status = lxp_codec_read_u128(reader, &account->balance);
    if (status == LXP_OK)
        status = read_fixed(reader, account->asset_id, 32U);
    if (status == LXP_OK)
        status = read_bool(reader, &account->has_asset);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &account->next_sequence);
    if (status == LXP_OK)
        status = lxp_codec_read_u64(reader, &account->created_at_sequence);
    if (status == LXP_OK) status = read_bool(reader, &account->frozen);
    if (status == LXP_OK)
        status = read_bool(reader, &account->has_open_reference);
    if (status == LXP_OK)
        status = read_fixed(reader, account->authority_key, 32U);
    if (status == LXP_OK)
        status = read_bool(reader, &account->has_authority_key);
    return status;
}

static lxp_result snapshot_load(
    const uint8_t *snapshot, size_t snapshot_length,
    const lxp_snapshot_manifest_record *manifest, lxp_kernel *kernel,
    bool migrate_retired_issuance, size_t *renamed_account_count)
{
    lxp_kernel *candidate;
    lxp_state_store *state;
    lx_account_registry *accounts = NULL;
    lx_account_registry *source_accounts = NULL;
    lx_account_registry *live_accounts;
    lxp_codec_reader reader;
    uint8_t digest[32];
    uint64_t sequence = 0U;
    uint32_t count = 0U;
    uint16_t root_count = 0U;
    uint16_t snapshot_version = 0U;
    uint16_t format_tag = (uint16_t)LXP_SNAPSHOT_FORMAT_VERSION;
    size_t i;
    lxp_result status;
    if ((snapshot == NULL && snapshot_length != 0U) || manifest == NULL ||
        kernel == NULL || kernel->state == NULL ||
        (migrate_retired_issuance && renamed_account_count == NULL))
        return LXP_ERR_NON_CANONICAL;
    if (renamed_account_count != NULL) *renamed_account_count = 0U;
    if (kernel->blob_count > LXP_KERNEL_MAX_BLOBS) return LXP_FATAL_INVARIANT;
    live_accounts = kernel->state->accounts;
    status = snapshot_digest(snapshot, snapshot_length,
                             manifest->global_sequence,
                             manifest->canonical_state_root,
                             manifest->receipt_state_root, digest);
    if (status != LXP_OK || lxp_ct_memcmp(
        digest, manifest->snapshot_digest, 32U) != 0)
        return status != LXP_OK ? status : LXP_ERR_SNAPSHOT_MISMATCH;
    if (snapshot_length >= 2U) {
        uint16_t encoded_version =
            (uint16_t)(((uint16_t)snapshot[0] << 8U) | snapshot[1]);
        if (!lxp_protocol_version_supported(encoded_version))
            return LXP_ERR_VERSION_UNSUPPORTED;
    }
    if (manifest->global_sequence == UINT64_MAX)
        return LXP_ERR_SEQUENCE_MISMATCH;
    if (snapshot_length >= 4U) {
        uint16_t encoded_tag =
            (uint16_t)(((uint16_t)snapshot[2] << 8U) | snapshot[3]);
        if (encoded_tag == (uint16_t)LXP_SNAPSHOT_FORMAT_LEGACY)
            format_tag = encoded_tag;
    }
    candidate = malloc(sizeof(*candidate));
    state = malloc(sizeof(*state));
    if (candidate == NULL || state == NULL) {
        free(candidate); free(state); return LXP_ERR_IO;
    }
    *candidate = *kernel;
    (void)memset(state, 0, sizeof(*state));
    candidate->state = state;
    candidate->module_kv_count = 0U;
    candidate->blob_count = 0U;
    candidate->blob_total_bytes = 0U;
    (void)memset(candidate->blobs, 0, sizeof(candidate->blobs));
    status = lxp_codec_reader_init(&reader, snapshot, snapshot_length);
    if (status == LXP_OK)
        status = lxp_codec_read_struct_header_version(
            &reader, format_tag, &snapshot_version);
    if (status == LXP_OK &&
        snapshot_version != (uint16_t)LXP_PROTOCOL_VERSION_LEGACY &&
        snapshot_version != (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_LEGACY &&
        kernel->state->account_root_required)
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY &&
        live_accounts == NULL)
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY) {
        accounts = malloc(sizeof(*accounts));
        if (migrate_retired_issuance)
            source_accounts = malloc(sizeof(*source_accounts));
        if (accounts == NULL ||
            (migrate_retired_issuance && source_accounts == NULL))
            status = LXP_ERR_IO;
        else {
            status = lx_account_registry_init(accounts);
            if (status == LXP_OK && source_accounts != NULL)
                status = lx_account_registry_init(source_accounts);
            state->accounts = accounts;
            state->account_root_required = true;
        }
    }
    if (status == LXP_OK) status = lxp_codec_read_u64(&reader, &sequence);
    if (status == LXP_OK && sequence == UINT64_MAX)
        status = LXP_ERR_SEQUENCE_MISMATCH;
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
    if (status == LXP_OK &&
        format_tag == (uint16_t)LXP_SNAPSHOT_FORMAT_LEGACY &&
        module_registered(candidate, LXP_MODULE_PROGRAMS))
        status = LXP_ERR_SNAPSHOT_BLOBS_MISSING;
    if (status == LXP_OK &&
        format_tag == (uint16_t)LXP_SNAPSHOT_FORMAT_BLOBS) {
        uint64_t declared_total = 0U;
        status = lxp_codec_read_u32(&reader, &count);
        if (status == LXP_OK && count > LXP_SNAPSHOT_MAX_BLOBS)
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            status = lxp_codec_read_u64(&reader, &declared_total);
        if (status == LXP_OK &&
            declared_total > LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES)
            status = LXP_ERR_LENGTH_LIMIT;
        for (i = 0U; status == LXP_OK && i < count; ++i) {
            lxp_module_blob *blob = &candidate->blobs[i];
            lxp_byte_span bytes;
            status = lxp_codec_read_u16(&reader, &blob->module_id);
            if (status == LXP_OK)
                status = read_fixed(&reader, blob->key, 32U);
            if (status == LXP_OK)
                status = lxp_codec_read_bytes(&reader, &bytes,
                                              LXP_SNAPSHOT_MAX_BLOB_BYTES);
            if (status == LXP_OK && bytes.length >
                (size_t)LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES -
                    candidate->blob_total_bytes)
                status = LXP_ERR_LENGTH_LIMIT;
            if (status == LXP_OK && (blob->module_id == 0U ||
                blob->module_id > LXP_MODULE_RESERVED_COUNT ||
                !module_registered(candidate, blob->module_id)))
                status = LXP_ERR_UNKNOWN_MODULE;
            if (status == LXP_OK && (bytes.length == 0U || (i != 0U &&
                blob_order(&candidate->blobs[i - 1U], blob) >= 0)))
                status = LXP_ERR_NON_CANONICAL;
            if (status == LXP_OK) {
                blob->bytes = (uint8_t *)malloc(bytes.length);
                if (blob->bytes == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
            }
            if (status == LXP_OK) {
                (void)memcpy(blob->bytes, bytes.bytes, bytes.length);
                blob->length = bytes.length;
                blob->deleted = false;
                candidate->blob_total_bytes += bytes.length;
                candidate->blob_count = i + 1U;
            }
        }
        if (status == LXP_OK &&
            candidate->blob_total_bytes != declared_total)
            status = LXP_ERR_NON_CANONICAL;
    }
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY)
        status = lxp_codec_read_u32(&reader, &count);
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY &&
        count > LX_ACCOUNT_REGISTRY_CAPACITY)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY &&
        count != 0U) {
        status = lx_account_registry_reserve(accounts, count);
        if (status == LXP_OK && source_accounts != NULL)
            status = lx_account_registry_reserve(source_accounts, count);
    }
    for (i = 0U; status == LXP_OK &&
         snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY &&
         i < count; ++i) {
        status = read_account(&reader, &accounts->accounts[i]);
        if (status == LXP_OK && source_accounts != NULL) {
            bool renamed = false;
            source_accounts->accounts[i] = accounts->accounts[i];
            status = lx_account_migrate_retired_issuance(
                &accounts->accounts[i], &renamed);
            if (status == LXP_OK && renamed) {
                if (*renamed_account_count == SIZE_MAX)
                    status = LXP_ERR_OVERFLOW;
                else
                    ++*renamed_account_count;
            }
        }
        if (status == LXP_OK && i != 0U &&
            memcmp(accounts->accounts[i - 1U].id,
                   accounts->accounts[i].id, 32U) >= 0)
            status = LXP_ERR_NON_CANONICAL;
    }
    if (accounts != NULL)
        accounts->count = status == LXP_OK ? count : 0U;
    if (source_accounts != NULL)
        source_accounts->count = status == LXP_OK ? count : 0U;
    if (status == LXP_OK && accounts != NULL)
        status = lx_account_registry_index_rebuild(accounts);
    if (status == LXP_OK && source_accounts != NULL)
        status = lx_account_registry_index_rebuild(source_accounts);
    if (status == LXP_OK &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY) {
        uint8_t recorded[32];
        uint8_t computed[32];
        status = read_fixed(&reader, recorded, 32U);
        if (status == LXP_OK && source_accounts == NULL)
            status = lx_account_registry_root(accounts, computed);
        if (status == LXP_OK && source_accounts != NULL)
            status = lx_account_registry_retired_issuance_root(
                source_accounts, computed);
        if (status == LXP_OK &&
            lxp_ct_memcmp(recorded, computed, 32U) != 0)
            status = LXP_ERR_SNAPSHOT_MISMATCH;
    }
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
        if (status == LXP_OK && source_accounts != NULL && i == 0U) {
            uint8_t source_account_root[32];
            status = lx_account_registry_retired_issuance_root(
                source_accounts, source_account_root);
            if (status == LXP_OK)
                status = lxp_state_subtree_root_with_account_override(
                    candidate, 0U, source_account_root, computed);
        } else if (status == LXP_OK)
            status = lxp_state_subtree_root(candidate, (uint16_t)i, computed);
        if (status == LXP_OK && lxp_ct_memcmp(recorded, computed, 32U) != 0)
            status = LXP_ERR_SNAPSHOT_MISMATCH;
    }
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK && source_accounts == NULL)
        status = lxp_snapshot_verify_root(candidate, manifest);
    if (status == LXP_OK && source_accounts != NULL) {
        uint8_t source_account_root[32];
        uint8_t computed[32];
        status = lx_account_registry_retired_issuance_root(
            source_accounts, source_account_root);
        if (status == LXP_OK)
            status = lxp_state_root_with_account_override(
                candidate, source_account_root, computed);
        if (status == LXP_OK && lxp_ct_memcmp(
                computed, manifest->canonical_state_root, 32U) != 0)
            status = LXP_ERR_SNAPSHOT_MISMATCH;
        if (status == LXP_OK && *renamed_account_count == 0U)
            status = LXP_ERR_UNKNOWN_FIELD;
    }
    if (status == LXP_OK && live_accounts != NULL &&
        snapshot_version == (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY)
        status = lx_account_registry_reserve(live_accounts, accounts->count);
    if (status == LXP_OK) {
        for (i = 0U; i < kernel->blob_count; ++i)
            free(kernel->blobs[i].bytes);
        (void)memcpy(kernel->blobs, candidate->blobs, sizeof(kernel->blobs));
        kernel->blob_count = candidate->blob_count;
        kernel->blob_total_bytes = candidate->blob_total_bytes;
        if (snapshot_version ==
                (uint16_t)LXP_PROTOCOL_VERSION_OCCUPANCY) {
            if (live_accounts->count != 0U)
                (void)memset(live_accounts->accounts, 0,
                             live_accounts->count *
                                 sizeof(live_accounts->accounts[0]));
            live_accounts->count = accounts->count;
            if (accounts->count != 0U) {
                (void)memcpy(live_accounts->accounts, accounts->accounts,
                             accounts->count *
                                 sizeof(accounts->accounts[0]));
                (void)memcpy(live_accounts->index, accounts->index,
                             accounts->count * sizeof(accounts->index[0]));
            }
            kernel->state->account_root_required = true;
        }
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
        (void)memcpy(kernel->current_state_root,
                     manifest->receipt_state_root, 32U);
    }
    if (status != LXP_OK)
        for (i = 0U; i < candidate->blob_count; ++i)
            free(candidate->blobs[i].bytes);
    lx_account_registry_release(accounts);
    lx_account_registry_release(source_accounts);
    free(accounts);
    free(source_accounts);
    free(state);
    free(candidate);
    return status;
}

lxp_result lxp_snapshot_load(const uint8_t *snapshot, size_t snapshot_length,
                             const lxp_snapshot_manifest_record *manifest,
                             lxp_kernel *kernel)
{
    return snapshot_load(snapshot, snapshot_length, manifest, kernel, false,
                         NULL);
}

lxp_result lxp_snapshot_load_retired_issuance(
    const uint8_t *snapshot, size_t snapshot_length,
    const lxp_snapshot_manifest_record *manifest, lxp_kernel *kernel,
    size_t *renamed_account_count)
{
    return snapshot_load(snapshot, snapshot_length, manifest, kernel, true,
                         renamed_account_count);
}
