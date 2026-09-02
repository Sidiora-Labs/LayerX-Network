#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_hash.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_transfer.h"

#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int xor_file_byte(const char *path, off_t offset, uint8_t mask)
{
    uint8_t byte = 0U;
    int descriptor = open(path, O_RDWR | O_CLOEXEC);
    int failed = descriptor < 0;
    if (!failed && pread(descriptor, &byte, 1U, offset) != 1) failed = 1;
    byte ^= mask;
    if (!failed && pwrite(descriptor, &byte, 1U, offset) != 1) failed = 1;
    if (!failed && fsync(descriptor) != 0) failed = 1;
    if (descriptor >= 0 && close(descriptor) != 0) failed = 1;
    return failed;
}

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *bytes,
                                 size_t length)
{ (void)ctx; (void)bytes; (void)length; return LXP_OK; }
static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *bytes, size_t length,
                                void **decoded)
{ (void)ctx; (void)ordinal; (void)bytes; (void)length; *decoded = NULL;
  return LXP_OK; }
static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; return LXP_OK; }
static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; (void)effects;
  return LXP_OK; }
static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{ (void)ctx; (void)epoch; (void)timestamp; return LXP_OK; }
static lxp_result module_root(lxp_module_ctx *ctx, uint8_t root[32])
{ (void)ctx; (void)memset(root, 0, 32U); return LXP_OK; }

static const uint32_t program_types[] = { UINT32_C(0x00090001) };
static const lxp_module_iface program_iface = {
    9U, 1U, "programs", program_types, 1U, module_genesis, module_decode,
    module_validate, module_execute, module_epoch, module_epoch, module_root,
    NULL
};

static int apply_value(lxp_state_store *state, lxp_state_journal *journal,
                       uint64_t sequence, uint8_t key_byte, uint64_t value)
{
    uint8_t key[32] = { 0U };
    key[0] = key_byte;
    return lxp_state_journal_open(state, sequence, journal) != LXP_OK ||
           lxp_state_journal_set(journal, key, (lxp_u128){0U, value}) !=
               LXP_OK || lxp_state_journal_commit(journal) != LXP_OK;
}

static int open_funded(lx_account_registry *accounts, const char *name,
                       const uint8_t asset_id[32], uint64_t balance,
                       uint64_t sequence)
{
    lx_account *account;
    uint8_t id[32];
    size_t length = strlen(name);
    return lx_account_id_from_string((const uint8_t *)name, length, id) !=
               LXP_OK ||
           lx_account_open(accounts, (const uint8_t *)name, length, id,
                           sequence, LX_ACCOUNT_OPEN_GENESIS, NULL,
                           &account) != LXP_OK ||
           lxp_ledger_bootstrap_balance(
               account, asset_id, (lxp_u128){0U, balance}, sequence) != LXP_OK;
}

static void put_u32(uint8_t *at, uint32_t value)
{
    at[0] = (uint8_t)(value >> 24U);
    at[1] = (uint8_t)(value >> 16U);
    at[2] = (uint8_t)(value >> 8U);
    at[3] = (uint8_t)value;
}

static void put_u64(uint8_t *at, uint64_t value)
{
    put_u32(at, (uint32_t)(value >> 32U));
    put_u32(at + 4U, (uint32_t)value);
}

static int commit_blob(lxp_kernel *kernel, const uint8_t *bytes,
                       size_t length)
{
    lxp_module_blob *blob;
    uint8_t *copy;
    if (kernel->blob_count >= LXP_KERNEL_MAX_BLOBS) return 1;
    blob = &kernel->blobs[kernel->blob_count];
    copy = (uint8_t *)malloc(length);
    if (copy == NULL) return 1;
    if (lxp_hash_sha256(bytes, length, blob->key) != LXP_OK) {
        free(copy);
        return 1;
    }
    (void)memcpy(copy, bytes, length);
    blob->module_id = LXP_MODULE_PROGRAMS;
    blob->length = length;
    blob->bytes = copy;
    blob->deleted = false;
    kernel->blob_count += 1U;
    kernel->blob_total_bytes += length;
    return 0;
}

static void release_blobs(lxp_kernel *kernel)
{
    while (kernel->blob_count != 0U)
        free(kernel->blobs[--kernel->blob_count].bytes);
    kernel->blob_total_bytes = 0U;
}

static int blob_store_holds(const lxp_kernel *kernel, const uint8_t *bytes,
                            size_t length)
{
    uint8_t key[32];
    return kernel->blob_count == 1U && kernel->blob_total_bytes == length &&
           kernel->blobs[0].module_id == LXP_MODULE_PROGRAMS &&
           lxp_hash_sha256(bytes, length, key) == LXP_OK &&
           memcmp(kernel->blobs[0].key, key, 32U) == 0 &&
           kernel->blobs[0].length == length && !kernel->blobs[0].deleted &&
           kernel->blobs[0].bytes != bytes &&
           memcmp(kernel->blobs[0].bytes, bytes, length) == 0;
}

static int load_refused(const uint8_t *bytes, size_t length,
                        const uint8_t root[32],
                        const uint8_t receipt_root[32], lxp_kernel *kernel,
                        lxp_result expected, int any_failure,
                        const uint8_t *blob_bytes, size_t blob_length)
{
    lxp_snapshot_manifest_record manifest;
    uint8_t before[32];
    uint8_t after[32];
    const uint8_t *live = kernel->blobs[0].bytes;
    size_t module_kv_count = kernel->module_kv_count;
    size_t state_count = kernel->state->count;
    uint64_t next_sequence = kernel->state->next_sequence;
    lxp_result status;
    if (lxp_state_root(kernel, before) != LXP_OK ||
        lxp_snapshot_manifest(bytes, length, 2U, root, receipt_root,
                              &manifest) != LXP_OK)
        return 1;
    status = lxp_snapshot_load(bytes, length, &manifest, kernel);
    if (any_failure ? status == LXP_OK : status != expected) return 1;
    return !blob_store_holds(kernel, blob_bytes, blob_length) ||
           kernel->blobs[0].bytes != live ||
           kernel->module_kv_count != module_kv_count ||
           kernel->state->count != state_count ||
           kernel->state->next_sequence != next_sequence ||
           lxp_state_root(kernel, after) != LXP_OK ||
           memcmp(before, after, 32U) != 0;
}

int main(void)
{
    static uint8_t snapshot_storage[4194304];
    static uint8_t read_storage[4194304];
    static uint8_t scratch[4194304];
    static uint8_t reference[4194304];
    static uint8_t artifact[301];
    static uint8_t stale[64];
    lxp_state_store original_state;
    lxp_state_store restored_state;
    lxp_state_journal original_journal;
    lxp_state_journal restored_journal;
    lxp_kernel original;
    lxp_kernel restored;
    lx_account_registry original_accounts;
    lx_account_registry restored_accounts;
    lxp_snapshot_manifest_record manifest;
    lxp_snapshot_manifest_record stored_manifest;
    lxp_byte_span snapshot;
    lxp_byte_span stored_snapshot;
    lxp_byte_span refused;
    lxp_arena snapshot_arena;
    lxp_arena read_arena;
    uint8_t root[32];
    uint8_t receipt_root[32] = { 0x91U };
    uint8_t original_terminal[32];
    uint8_t restored_terminal[32];
    uint8_t before_truncation_root[32];
    uint8_t asset_id[32] = { 0x41U };
    uint8_t expected_section[LXP_SNAPSHOT_BLOB_SECTION_BYTES +
                             LXP_SNAPSHOT_BLOB_ENTRY_BYTES];
    uint8_t *bytes;
    size_t entry_bytes = LXP_SNAPSHOT_BLOB_ENTRY_BYTES + sizeof(artifact);
    size_t accounts_bytes = 40U + (149U + 16U) + (149U + 11U);
    size_t section;
    size_t payload;
    size_t cut;
    size_t i;
    char directory[] = "/tmp/lxp-snapshot-XXXXXX";
    char path[128];
    char link_path[128];
    static uint64_t parameters = 1U;
    for (i = 0U; i < sizeof(artifact); ++i)
        artifact[i] = (uint8_t)(i * 7U + 3U);
    (void)memset(stale, 0x5a, sizeof(stale));
    if (lx_account_registry_init(&original_accounts) != LXP_OK ||
        lx_account_registry_init(&restored_accounts) != LXP_OK ||
        lxp_state_store_init(&original_state, 0U) != LXP_OK ||
        lxp_state_store_init(&restored_state, 0U) != LXP_OK ||
        lxp_state_store_bind_accounts(&original_state,
                                      &original_accounts) != LXP_OK ||
        lxp_state_store_bind_accounts(&restored_state,
                                      &restored_accounts) != LXP_OK ||
        lxp_kernel_create(&original, &original_state, &original_journal,
                          &parameters, 0U) != LXP_OK ||
        lxp_kernel_create(&restored, &restored_state, &restored_journal,
                          &parameters, 0U) != LXP_OK ||
        open_funded(&original_accounts, "agent:alice:main", asset_id,
                    100U, 0U) != 0 ||
        open_funded(&original_accounts, "system:fees", asset_id,
                    7U, 0U) != 0 ||
        apply_value(&original_state, &original_journal, 0U, 2U, 20U) != 0 ||
        apply_value(&original_state, &original_journal, 1U, 1U, 10U) != 0)
        return 1;
    original.module_kv_count = 2U;
    original.module_kv[0].module_id = 2U;
    original.module_kv[0].key_length = 1U;
    original.module_kv[0].key[0] = 2U;
    original.module_kv[0].value_length = 1U;
    original.module_kv[0].value[0] = 8U;
    original.module_kv[1].module_id = 1U;
    original.module_kv[1].key_length = 1U;
    original.module_kv[1].key[0] = 1U;
    original.module_kv[1].value_length = 1U;
    original.module_kv[1].value[0] = 7U;
    if (lxp_state_root(&original, root) != LXP_OK ||
        lxp_arena_init(&snapshot_arena, snapshot_storage,
                       sizeof(snapshot_storage)) != LXP_OK ||
        lxp_snapshot_write(&original, 1U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.length != 494U ||
        snapshot.bytes[0] != 0U ||
        snapshot.bytes[1] != LXP_PROTOCOL_VERSION_LEGACY ||
        snapshot.bytes[2] != (uint8_t)(LXP_SNAPSHOT_FORMAT_BLOBS >> 8U) ||
        snapshot.bytes[3] != (uint8_t)LXP_SNAPSHOT_FORMAT_BLOBS ||
        snapshot.bytes[snapshot.length - 9U * 36U - 2U] != 0U ||
        snapshot.bytes[snapshot.length - 9U * 36U - 1U] != 9U ||
        lxp_snapshot_manifest(snapshot.bytes, snapshot.length, 1U, root,
                              receipt_root, &manifest) != LXP_OK ||
        mkdtemp(directory) == NULL ||
        lxp_snapshot_store_write(directory, &manifest, snapshot.bytes,
                                 snapshot.length) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxs", directory, 1U) < 0 ||
        lxp_arena_init(&read_arena, read_storage, sizeof(read_storage)) !=
            LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) != LXP_OK ||
        lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, &restored) != LXP_OK ||
        lxp_snapshot_verify_root(&restored, &stored_manifest) != LXP_OK ||
        memcmp(stored_manifest.canonical_state_root, root, 32U) != 0 ||
        memcmp(stored_manifest.receipt_state_root, receipt_root, 32U) != 0 ||
        memcmp(restored.current_state_root, receipt_root, 32U) != 0)
        return 1;
    if (restored_state.next_sequence != 2U || restored_state.count != 2U ||
        restored.module_kv_count != 2U || restored.blob_count != 0U) return 1;
    cut = snapshot.length - 2U - 9U * 36U - LXP_SNAPSHOT_BLOB_SECTION_BYTES;
    for (i = 0U; i < LXP_SNAPSHOT_BLOB_SECTION_BYTES; ++i)
        if (snapshot.bytes[cut + i] != 0U) return 1;
    (void)memcpy(scratch, snapshot.bytes, cut);
    (void)memcpy(scratch + cut,
                 snapshot.bytes + cut + LXP_SNAPSHOT_BLOB_SECTION_BYTES,
                 snapshot.length - cut - LXP_SNAPSHOT_BLOB_SECTION_BYTES);
    scratch[2] = (uint8_t)(LXP_SNAPSHOT_FORMAT_LEGACY >> 8U);
    scratch[3] = (uint8_t)LXP_SNAPSHOT_FORMAT_LEGACY;
    if (commit_blob(&restored, stale, sizeof(stale)) != 0 ||
        restored.blob_count != 1U ||
        lxp_snapshot_manifest(scratch,
                              snapshot.length - LXP_SNAPSHOT_BLOB_SECTION_BYTES,
                              1U, root, receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(scratch,
                          snapshot.length - LXP_SNAPSHOT_BLOB_SECTION_BYTES,
                          &manifest, &restored) != LXP_OK ||
        restored.blob_count != 0U || restored.blob_total_bytes != 0U ||
        lxp_snapshot_verify_root(&restored, &manifest) != LXP_OK ||
        restored_state.next_sequence != 2U || restored_state.count != 2U ||
        restored.module_kv_count != 2U ||
        lxp_state_root(&restored, restored_terminal) != LXP_OK ||
        memcmp(root, restored_terminal, 32U) != 0)
        return 1;
    stored_manifest.receipt_state_root[0] ^= 1U;
    if (lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, &restored) !=
            LXP_ERR_SNAPSHOT_MISMATCH ||
        memcmp(restored.current_state_root, receipt_root, 32U) != 0)
        return 1;
    stored_manifest.receipt_state_root[0] ^= 1U;
    if (xor_file_byte(path, 44, 1U) != 0 ||
        lxp_arena_reset(&read_arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) !=
            LXP_ERR_SNAPSHOT_MISMATCH ||
        memcmp(restored.current_state_root, receipt_root, 32U) != 0 ||
        xor_file_byte(path, 44, 1U) != 0 ||
        lxp_arena_reset(&read_arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) != LXP_OK)
        return 1;
    if (xor_file_byte(path, 3, 3U) != 0 ||
        lxp_arena_reset(&read_arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) !=
            LXP_ERR_SNAPSHOT_MISMATCH ||
        xor_file_byte(path, 3, 3U) != 0 ||
        snprintf(link_path, sizeof(link_path), "%s/link.lxs", directory) < 0 ||
        symlink(path, link_path) != 0 ||
        lxp_arena_reset(&read_arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(link_path, &read_arena, &stored_manifest,
                                &stored_snapshot) == LXP_OK ||
        unlink(link_path) != 0 ||
        lxp_arena_reset(&read_arena, 0U) != LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) != LXP_OK)
        return 1;
    if (apply_value(&original_state, &original_journal, 2U, 3U, 30U) != 0 ||
        apply_value(&restored_state, &restored_journal, 2U, 3U, 30U) != 0 ||
        lxp_state_root(&original, original_terminal) != LXP_OK ||
        lxp_state_root(&restored, restored_terminal) != LXP_OK ||
        memcmp(original_terminal, restored_terminal, 32U) != 0)
        return 1;
    if (lxp_state_root(&restored, before_truncation_root) != LXP_OK) return 1;
    for (cut = 0U; cut < stored_snapshot.length; ++cut) {
        lxp_snapshot_manifest_record truncated_manifest;
        uint8_t after[32];
        size_t module_kv_count = restored.module_kv_count;
        size_t state_count = restored_state.count;
        uint64_t next_sequence = restored_state.next_sequence;
        if (lxp_snapshot_manifest(stored_snapshot.bytes, cut, 1U, root,
                                  receipt_root,
                                  &truncated_manifest) != LXP_OK ||
            lxp_snapshot_load(stored_snapshot.bytes, cut, &truncated_manifest,
                              &restored) == LXP_OK ||
            restored.module_kv_count != module_kv_count ||
            restored_state.count != state_count ||
            restored_state.next_sequence != next_sequence ||
            lxp_state_root(&restored, after) != LXP_OK ||
            memcmp(before_truncation_root, after, 32U) != 0)
            return 1;
    }
    if (lxp_state_store_require_account_root(&restored_state) != LXP_OK ||
        lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, &restored) !=
            LXP_ERR_SNAPSHOT_MISMATCH)
        return 1;
    ((uint8_t *)stored_snapshot.bytes)[stored_snapshot.length - 1U] ^= 1U;
    if (lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, &restored) !=
        LXP_ERR_SNAPSHOT_MISMATCH ||
        lxp_kernel_register_module(&original, &program_iface) != LXP_OK ||
        lxp_kernel_register_module(&restored, &program_iface) != LXP_OK ||
        commit_blob(&original, artifact, sizeof(artifact)) != 0 ||
        commit_blob(&restored, stale, sizeof(stale)) != 0 ||
        lxp_state_store_require_account_root(&original_state) != LXP_OK ||
        lxp_state_root(&original, root) != LXP_OK ||
        lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.bytes[0] != 0U ||
        snapshot.bytes[1] != LXP_PROTOCOL_VERSION_OCCUPANCY ||
        snapshot.bytes[2] != (uint8_t)(LXP_SNAPSHOT_FORMAT_BLOBS >> 8U) ||
        snapshot.bytes[3] != (uint8_t)LXP_SNAPSHOT_FORMAT_BLOBS ||
        snapshot.bytes[snapshot.length - 10U * 36U - 2U] != 0U ||
        snapshot.bytes[snapshot.length - 10U * 36U - 1U] != 10U ||
        lxp_snapshot_manifest(snapshot.bytes, snapshot.length, 2U, root,
                              receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored) != LXP_OK ||
        !restored_state.account_root_required ||
        restored_accounts.count != original_accounts.count ||
        !blob_store_holds(&restored, artifact, sizeof(artifact)) ||
        restored.blobs[0].bytes == original.blobs[0].bytes ||
        lxp_snapshot_verify_root(&restored, &manifest) != LXP_OK ||
        lxp_state_root(&restored, restored_terminal) != LXP_OK ||
        memcmp(root, restored_terminal, 32U) != 0)
        return 1;
    ((uint8_t *)snapshot.bytes)[0] = 0xffU;
    ((uint8_t *)snapshot.bytes)[1] = 0xffU;
    if (lxp_snapshot_manifest(snapshot.bytes, snapshot.length, 2U,
                              root, receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored) != LXP_ERR_VERSION_UNSUPPORTED ||
        restored_state.next_sequence != 3U)
        return 1;
    ((uint8_t *)snapshot.bytes)[0] = 0U;
    ((uint8_t *)snapshot.bytes)[1] = LXP_PROTOCOL_VERSION_OCCUPANCY;
    (void)memset((uint8_t *)snapshot.bytes + 2U, 0xff, 8U);
    if (lxp_snapshot_manifest(snapshot.bytes, snapshot.length, UINT64_MAX,
                              root, receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored) != LXP_ERR_SEQUENCE_MISMATCH ||
        restored_state.next_sequence != 3U)
        return 1;
    if (lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.length < 2U + 10U * 36U + accounts_bytes +
                              LXP_SNAPSHOT_BLOB_SECTION_BYTES + entry_bytes)
        return 1;
    bytes = (uint8_t *)snapshot.bytes;
    (void)memcpy(reference, bytes, snapshot.length);
    section = snapshot.length - 2U - 10U * 36U - accounts_bytes -
              LXP_SNAPSHOT_BLOB_SECTION_BYTES - entry_bytes;
    payload = section + LXP_SNAPSHOT_BLOB_SECTION_BYTES +
              LXP_SNAPSHOT_BLOB_ENTRY_BYTES;
    put_u32(expected_section, 1U);
    put_u64(expected_section + 4U, (uint64_t)sizeof(artifact));
    expected_section[12] = 0U;
    expected_section[13] = (uint8_t)LXP_MODULE_PROGRAMS;
    put_u32(expected_section + 14U, 32U);
    if (lxp_hash_sha256(artifact, sizeof(artifact), expected_section + 18U) !=
        LXP_OK)
        return 1;
    put_u32(expected_section + 50U, (uint32_t)sizeof(artifact));
    if (memcmp(bytes + section, expected_section, sizeof(expected_section)) !=
            0 ||
        memcmp(bytes + payload, artifact, sizeof(artifact)) != 0)
        return 1;
    bytes[payload] ^= 1U;
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_SNAPSHOT_MISMATCH, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    bytes[payload] ^= 1U;
    put_u32(bytes + section, (uint32_t)LXP_SNAPSHOT_MAX_BLOBS + 1U);
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_LENGTH_LIMIT, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    put_u32(bytes + section, 1U);
    put_u64(bytes + section + 4U,
            (uint64_t)LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES + 1U);
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_LENGTH_LIMIT, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    put_u64(bytes + section + 4U, (uint64_t)sizeof(artifact) + 1U);
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_NON_CANONICAL, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    put_u64(bytes + section + 4U, (uint64_t)sizeof(artifact));
    bytes[section + 13U] = 5U;
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_UNKNOWN_MODULE, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    bytes[section + 13U] = (uint8_t)LXP_MODULE_PROGRAMS;
    bytes[3] = (uint8_t)(LXP_SNAPSHOT_FORMAT_BLOBS + 1U);
    if (load_refused(bytes, snapshot.length, root, receipt_root, &restored,
                     LXP_ERR_VERSION_UNSUPPORTED, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    bytes[3] = (uint8_t)LXP_SNAPSHOT_FORMAT_BLOBS;
    if (memcmp(bytes, reference, snapshot.length) != 0) return 1;
    (void)memcpy(scratch, bytes, section);
    put_u32(scratch + section, 0U);
    put_u64(scratch + section + 4U, 0U);
    (void)memcpy(scratch + section + LXP_SNAPSHOT_BLOB_SECTION_BYTES,
                 bytes + section + LXP_SNAPSHOT_BLOB_SECTION_BYTES +
                     entry_bytes,
                 snapshot.length - section - LXP_SNAPSHOT_BLOB_SECTION_BYTES -
                     entry_bytes);
    if (load_refused(scratch, snapshot.length - entry_bytes, root,
                     receipt_root, &restored, LXP_ERR_SNAPSHOT_MISMATCH, 0,
                     artifact, sizeof(artifact)) != 0)
        return 1;
    (void)memcpy(scratch, bytes, section);
    (void)memcpy(scratch + section,
                 bytes + section + LXP_SNAPSHOT_BLOB_SECTION_BYTES +
                     entry_bytes,
                 snapshot.length - section - LXP_SNAPSHOT_BLOB_SECTION_BYTES -
                     entry_bytes);
    scratch[2] = (uint8_t)(LXP_SNAPSHOT_FORMAT_LEGACY >> 8U);
    scratch[3] = (uint8_t)LXP_SNAPSHOT_FORMAT_LEGACY;
    if (load_refused(scratch,
                     snapshot.length - LXP_SNAPSHOT_BLOB_SECTION_BYTES -
                         entry_bytes,
                     root, receipt_root, &restored,
                     LXP_ERR_SNAPSHOT_BLOBS_MISSING, 0, artifact,
                     sizeof(artifact)) != 0)
        return 1;
    for (cut = 0U; cut < snapshot.length; ++cut)
        if (load_refused(bytes, cut, root, receipt_root, &restored, LXP_OK, 1,
                         artifact, sizeof(artifact)) != 0)
            return 1;
    original.blob_count = LXP_SNAPSHOT_MAX_BLOBS + 1U;
    if (lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &refused) !=
            LXP_ERR_LENGTH_LIMIT)
        return 1;
    original.blob_count = 1U;
    original.blobs[0].length = (size_t)LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES + 1U;
    original.blob_total_bytes = original.blobs[0].length;
    if (lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &refused) !=
            LXP_ERR_LENGTH_LIMIT)
        return 1;
    original.blobs[0].length = (size_t)LXP_SNAPSHOT_MAX_BLOB_BYTES + 1U;
    original.blob_total_bytes = original.blobs[0].length;
    if (lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &refused) !=
            LXP_ERR_LENGTH_LIMIT)
        return 1;
    original.blobs[0].length = sizeof(artifact);
    original.blob_total_bytes = sizeof(artifact);
    if (lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.length != section + LXP_SNAPSHOT_BLOB_SECTION_BYTES +
                               entry_bytes + accounts_bytes + 10U * 36U + 2U ||
        memcmp(snapshot.bytes, reference, snapshot.length) != 0 ||
        lxp_snapshot_manifest(snapshot.bytes, snapshot.length, 2U, root,
                              receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored) != LXP_OK ||
        !blob_store_holds(&restored, artifact, sizeof(artifact)) ||
        lxp_snapshot_verify_root(&restored, &manifest) != LXP_OK)
        return 1;
    release_blobs(&original);
    release_blobs(&restored);
    if (lxp_state_store_destroy(&original_state) != LXP_OK ||
        lxp_state_store_destroy(&restored_state) != LXP_OK ||
        unlink(path) != 0 || rmdir(directory) != 0) return 1;
    return 0;
}
