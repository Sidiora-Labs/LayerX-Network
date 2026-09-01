#define _POSIX_C_SOURCE 200809L

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

int main(void)
{
    static uint8_t snapshot_storage[4194304];
    static uint8_t read_storage[4194304];
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
    lxp_arena snapshot_arena;
    lxp_arena read_arena;
    uint8_t root[32];
    uint8_t receipt_root[32] = { 0x91U };
    uint8_t original_terminal[32];
    uint8_t restored_terminal[32];
    uint8_t before_truncation_root[32];
    uint8_t asset_id[32] = { 0x41U };
    size_t cut;
    char directory[] = "/tmp/lxp-snapshot-XXXXXX";
    char path[128];
    char link_path[128];
    static uint64_t parameters = 1U;
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
        snapshot.length != 482U ||
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
        restored.module_kv_count != 2U) return 1;
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
        lxp_state_store_require_account_root(&original_state) != LXP_OK ||
        lxp_state_root(&original, root) != LXP_OK ||
        lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.bytes[0] != 0U ||
        snapshot.bytes[1] != LXP_PROTOCOL_VERSION_OCCUPANCY ||
        snapshot.bytes[snapshot.length - 10U * 36U - 2U] != 0U ||
        snapshot.bytes[snapshot.length - 10U * 36U - 1U] != 10U ||
        lxp_snapshot_manifest(snapshot.bytes, snapshot.length, 2U, root,
                              receipt_root, &manifest) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored) != LXP_OK ||
        !restored_state.account_root_required ||
        restored_accounts.count != original_accounts.count ||
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
    if (lxp_state_store_destroy(&original_state) != LXP_OK ||
        lxp_state_store_destroy(&restored_state) != LXP_OK ||
        unlink(path) != 0 || rmdir(directory) != 0) return 1;
    return 0;
}
