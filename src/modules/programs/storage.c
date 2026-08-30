#include "storage.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum {
    STORAGE_FORMAT_VERSION = 1,
    STORAGE_HEAD_BYTES = 38,
    STORAGE_CELL_FIXED_BYTES = 38
};
static const uint8_t storage_prefix[8] = {'p','r','o','g','s','t','o','r'};

static uint16_t read_u16(const uint8_t *p)
{ return (uint16_t)(((uint16_t)p[0] << 8U) | p[1]); }
static uint32_t read_u32(const uint8_t *p)
{ return ((uint32_t)p[0] << 24U) | ((uint32_t)p[1] << 16U) |
         ((uint32_t)p[2] << 8U) | p[3]; }
static void write_u16(uint8_t *p, uint16_t v)
{ p[0] = (uint8_t)(v >> 8U); p[1] = (uint8_t)v; }
static void write_u32(uint8_t *p, uint32_t v)
{ p[0] = (uint8_t)(v >> 24U); p[1] = (uint8_t)(v >> 16U);
  p[2] = (uint8_t)(v >> 8U); p[3] = (uint8_t)v; }

static int key_compare(const uint8_t *a, size_t an,
                       const uint8_t *b, size_t bn)
{
    const size_t n = an < bn ? an : bn;
    const int compared = memcmp(a, b, n);
    if (compared != 0) return compared;
    return an < bn ? -1 : an > bn ? 1 : 0;
}

static lxp_result namespace_key(const uint8_t *ns, uint16_t ns_length,
                                uint8_t key[73], size_t *key_length)
{
    if (ns == NULL || key_length == NULL ||
        (ns_length != 33U && ns_length != 65U) ||
        lxp_ct_is_zero(ns, 32U) ||
        (ns_length == 33U && ns[32] != 1U) ||
        (ns_length == 65U && (ns[32] != 0U || lxp_ct_is_zero(ns + 33U, 32U))))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key, storage_prefix, sizeof(storage_prefix));
    (void)memcpy(key + sizeof(storage_prefix), ns, ns_length);
    *key_length = sizeof(storage_prefix) + ns_length;
    return LXP_OK;
}

lxp_result lxp_programs_storage_import(
    lxp_module_ctx *ctx, const uint8_t *ns, uint16_t ns_length,
    lxp_programs_storage_import_fn import_cell, void *user)
{
    uint8_t key[73], digest[32];
    size_t key_length, head_length, manifest_length, cursor = 6U, index;
    const uint8_t *head, *manifest;
    const uint8_t *previous_key = NULL;
    uint16_t previous_key_length = 0U;
    lxp_result status;
    if (ctx == NULL || import_cell == NULL) return LXP_ERR_NON_CANONICAL;
    status = namespace_key(ns, ns_length, key, &key_length);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, key_length, &head, &head_length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (head_length != STORAGE_HEAD_BYTES ||
        read_u16(head) != STORAGE_FORMAT_VERSION) return LXP_FATAL_INVARIANT;
    status = lxp_ctx_blob_get(ctx, head + 6U, &manifest, &manifest_length);
    if (status != LXP_OK) return status;
    status = lxp_hash_sha256(manifest, manifest_length, digest);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(digest, head + 6U, 32U) != 0 || manifest_length < 6U ||
        read_u16(manifest) != STORAGE_FORMAT_VERSION ||
        read_u32(manifest + 2U) != read_u32(head + 2U))
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < read_u32(head + 2U); ++index) {
        uint16_t cell_key_length;
        uint32_t value_length;
        const uint8_t *value;
        size_t stored_length;
        if (cursor + 2U > manifest_length) return LXP_FATAL_INVARIANT;
        cell_key_length = read_u16(manifest + cursor); cursor += 2U;
        if (cell_key_length == 0U || cell_key_length > LX_PROGRAMS_STORAGE_MAX_KEY_BYTES ||
            cursor + cell_key_length + STORAGE_CELL_FIXED_BYTES - 2U >
                manifest_length) return LXP_FATAL_INVARIANT;
        if (previous_key != NULL && key_compare(previous_key, previous_key_length,
            manifest + cursor, cell_key_length) >= 0) return LXP_FATAL_INVARIANT;
        value_length = read_u32(manifest + cursor + cell_key_length + 32U);
        if (value_length > LX_PROGRAMS_STORAGE_MAX_VALUE_BYTES) return LXP_FATAL_INVARIANT;
        if (value_length == 0U) {
            static const uint8_t empty = 0U;
            value = &empty;
            stored_length = 0U;
        } else {
            status = lxp_ctx_blob_get(ctx,
                                      manifest + cursor + cell_key_length,
                                      &value, &stored_length);
            if (status != LXP_OK) return status;
            if (stored_length != value_length) return LXP_FATAL_INVARIANT;
        }
        status = lxp_hash_sha256(value, stored_length, digest);
        if (status != LXP_OK || lxp_ct_memcmp(
                digest, manifest + cursor + cell_key_length, 32U) != 0)
            return LXP_FATAL_INVARIANT;
        status = import_cell(user, manifest + cursor, cell_key_length,
                             value, value_length);
        if (status != LXP_OK) return status;
        previous_key = manifest + cursor;
        previous_key_length = cell_key_length;
        cursor += cell_key_length + STORAGE_CELL_FIXED_BYTES - 2U;
    }
    return cursor == manifest_length ? LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_programs_storage_stage_final(
    lxp_module_ctx *ctx, const uint8_t *ns, uint16_t ns_length,
    const lxp_programs_storage_cell *cells, uint32_t count)
{
    uint8_t key[73], head[STORAGE_HEAD_BYTES], digest[32];
    uint8_t *manifest;
    size_t key_length, manifest_length = 6U, cursor = 6U, index;
    void *allocation;
    lxp_result status;
    if (ctx == NULL || (count != 0U && cells == NULL)) return LXP_ERR_NON_CANONICAL;
    status = namespace_key(ns, ns_length, key, &key_length);
    if (status != LXP_OK) return status;
    if (count == 0U) return lxp_ctx_kv_del(ctx, key, key_length);
    for (index = 0U; index < count; ++index) {
        if (cells[index].key == NULL || cells[index].key_length == 0U ||
            cells[index].key_length > LX_PROGRAMS_STORAGE_MAX_KEY_BYTES ||
            cells[index].value == NULL ||
            cells[index].value_length > LX_PROGRAMS_STORAGE_MAX_VALUE_BYTES ||
            (index != 0U && key_compare(cells[index - 1U].key,
              cells[index - 1U].key_length, cells[index].key,
              cells[index].key_length) >= 0)) return LXP_ERR_NON_CANONICAL;
        if (SIZE_MAX - manifest_length <
                (size_t)cells[index].key_length + STORAGE_CELL_FIXED_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        manifest_length += (size_t)cells[index].key_length +
                           STORAGE_CELL_FIXED_BYTES;
    }
    status = lxp_ctx_arena_alloc(ctx, manifest_length, 1U, &allocation);
    if (status != LXP_OK) return status;
    manifest = (uint8_t *)allocation;
    write_u16(manifest, STORAGE_FORMAT_VERSION); write_u32(manifest + 2U, count);
    for (index = 0U; index < count; ++index) {
        status = lxp_hash_sha256(cells[index].value, cells[index].value_length, digest);
        if (status != LXP_OK) return status;
        if (cells[index].value_length != 0U) {
            status = lxp_ctx_blob_put(ctx, digest, cells[index].value,
                                      cells[index].value_length);
            if (status != LXP_OK) return status;
        }
        write_u16(manifest + cursor, cells[index].key_length); cursor += 2U;
        (void)memcpy(manifest + cursor, cells[index].key, cells[index].key_length);
        cursor += cells[index].key_length;
        (void)memcpy(manifest + cursor, digest, 32U); cursor += 32U;
        write_u32(manifest + cursor, cells[index].value_length); cursor += 4U;
    }
    status = lxp_hash_sha256(manifest, manifest_length, digest);
    if (status != LXP_OK) return status;
    status = lxp_ctx_blob_put(ctx, digest, manifest, manifest_length);
    if (status != LXP_OK) return status;
    write_u16(head, STORAGE_FORMAT_VERSION); write_u32(head + 2U, count);
    (void)memcpy(head + 6U, digest, 32U);
    return lxp_ctx_kv_put(ctx, key, key_length, head, sizeof(head));
}

typedef struct indexed_import {
    uint32_t target;
    uint32_t seen;
    const uint8_t *key;
    uint16_t key_length;
    const uint8_t *value;
    uint32_t value_length;
} indexed_import;

static lxp_result capture_index(void *user, const uint8_t *key,
                                uint16_t key_length, const uint8_t *value,
                                uint32_t value_length)
{
    indexed_import *capture = (indexed_import *)user;
    if (capture->seen == capture->target) {
        capture->key = key;
        capture->key_length = key_length;
        capture->value = value;
        capture->value_length = value_length;
    }
    ++capture->seen;
    return LXP_OK;
}

lxp_result lxp_programs_storage_cell_at(
    lxp_module_ctx *ctx, const uint8_t *ns, uint16_t ns_length,
    uint32_t index, const uint8_t **key, uint16_t *key_length,
    const uint8_t **value, uint32_t *value_length, uint32_t *cell_count)
{
    indexed_import capture;
    lxp_result status;
    if (key == NULL || key_length == NULL || value == NULL ||
        value_length == NULL || cell_count == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(&capture, 0, sizeof(capture));
    capture.target = index;
    status = lxp_programs_storage_import(ctx, ns, ns_length,
                                         capture_index, &capture);
    if (status != LXP_OK) return status;
    *cell_count = capture.seen;
    if (index >= capture.seen || capture.key == NULL) return LXP_ERR_UNKNOWN_FIELD;
    *key = capture.key; *key_length = capture.key_length;
    *value = capture.value; *value_length = capture.value_length;
    return LXP_OK;
}
