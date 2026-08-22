#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_snapshot.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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
    lxp_snapshot_manifest_record manifest;
    lxp_snapshot_manifest_record stored_manifest;
    lxp_byte_span snapshot;
    lxp_byte_span stored_snapshot;
    lxp_arena snapshot_arena;
    lxp_arena read_arena;
    uint8_t root[32];
    uint8_t original_terminal[32];
    uint8_t restored_terminal[32];
    uint8_t before_truncation_root[32];
    size_t cut;
    char directory[] = "/tmp/lxp-snapshot-XXXXXX";
    char path[128];
    static uint64_t parameters = 1U;
    if (lxp_state_store_init(&original_state, 0U) != LXP_OK ||
        lxp_state_store_init(&restored_state, 0U) != LXP_OK ||
        lxp_kernel_create(&original, &original_state, &original_journal,
                          &parameters, 0U) != LXP_OK ||
        lxp_kernel_create(&restored, &restored_state, &restored_journal,
                          &parameters, 0U) != LXP_OK ||
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
                              &manifest) != LXP_OK ||
        mkdtemp(directory) == NULL ||
        lxp_snapshot_store_write(directory, &manifest, snapshot.bytes,
                                 snapshot.length) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxs", directory, 1U) < 0 ||
        lxp_arena_init(&read_arena, read_storage, sizeof(read_storage)) !=
            LXP_OK ||
        lxp_snapshot_store_read(path, &read_arena, &stored_manifest,
                                &stored_snapshot) != LXP_OK ||
        lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, root, &restored) != LXP_OK ||
        lxp_snapshot_verify_root(&restored, &stored_manifest, root) != LXP_OK)
        return 1;
    if (restored_state.next_sequence != 2U || restored_state.count != 2U ||
        restored.module_kv_count != 2U) return 1;
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
                                  &truncated_manifest) != LXP_OK ||
            lxp_snapshot_load(stored_snapshot.bytes, cut, &truncated_manifest,
                              root, &restored) == LXP_OK ||
            restored.module_kv_count != module_kv_count ||
            restored_state.count != state_count ||
            restored_state.next_sequence != next_sequence ||
            lxp_state_root(&restored, after) != LXP_OK ||
            memcmp(before_truncation_root, after, 32U) != 0)
            return 1;
    }
    ((uint8_t *)stored_snapshot.bytes)[stored_snapshot.length - 1U] ^= 1U;
    if (lxp_snapshot_load(stored_snapshot.bytes, stored_snapshot.length,
                          &stored_manifest, root, &restored) !=
        LXP_ERR_SNAPSHOT_MISMATCH ||
        lxp_kernel_register_module(&original, &program_iface) != LXP_OK ||
        lxp_arena_reset(&snapshot_arena, 0U) != LXP_OK ||
        lxp_snapshot_write(&original, 2U, &snapshot_arena, &snapshot) !=
            LXP_OK ||
        snapshot.bytes[snapshot.length - 10U * 36U - 2U] != 0U ||
        snapshot.bytes[snapshot.length - 10U * 36U - 1U] != 10U)
        return 1;
    if (lxp_state_store_destroy(&original_state) != LXP_OK ||
        lxp_state_store_destroy(&restored_state) != LXP_OK ||
        unlink(path) != 0 || rmdir(directory) != 0) return 1;
    return 0;
}
