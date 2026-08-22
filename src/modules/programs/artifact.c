#include "artifact.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <stdbool.h>
#include <string.h>

enum {
    ARTIFACT_KEY_BYTES = 40,
    ARTIFACT_FORMAT_VERSION = 1
};

static const uint8_t artifact_prefix[8] = {
    'p', 'r', 'o', 'g', 'c', 'o', 'd', 'e'
};

static void artifact_key(const uint8_t program_id[32],
                         uint8_t key[ARTIFACT_KEY_BYTES])
{
    (void)memcpy(key, artifact_prefix, sizeof(artifact_prefix));
    (void)memcpy(key + sizeof(artifact_prefix), program_id, 32U);
}

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

static uint32_t read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static void write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

typedef struct artifact_catalog_cursor {
    size_t target;
    size_t count;
    bool found;
    uint8_t program_id[32];
    uint8_t code_hash[32];
} artifact_catalog_cursor;

static lxp_result catalog_visit(const uint8_t *key, size_t key_length,
                                const uint8_t *value, size_t value_length,
                                void *user)
{
    artifact_catalog_cursor *cursor = (artifact_catalog_cursor *)user;
    if (key_length != ARTIFACT_KEY_BYTES ||
        memcmp(key, artifact_prefix, sizeof(artifact_prefix)) != 0 ||
        value_length != LX_PROGRAMS_ARTIFACT_MANIFEST_BYTES ||
        read_u16(value) != ARTIFACT_FORMAT_VERSION ||
        read_u32(value + 2U) == 0U ||
        read_u32(value + 2U) > LXP_KERNEL_MAX_BLOB_BYTES)
        return LXP_FATAL_INVARIANT;
    if (cursor->count == cursor->target) {
        (void)memcpy(cursor->program_id, key + sizeof(artifact_prefix), 32U);
        (void)memcpy(cursor->code_hash, value + 6U, 32U);
        cursor->found = true;
    }
    ++cursor->count;
    return LXP_OK;
}

lxp_result lxp_programs_artifact_store(lxp_module_ctx *ctx,
                                       const uint8_t program_id[32],
                                       const uint8_t code_hash[32],
                                       const uint8_t *wasm,
                                       size_t wasm_length)
{
    uint8_t key[ARTIFACT_KEY_BYTES];
    uint8_t manifest[LX_PROGRAMS_ARTIFACT_MANIFEST_BYTES];
    lxp_result status;
    if (ctx == NULL || program_id == NULL || code_hash == NULL || wasm == NULL ||
        wasm_length == 0U || wasm_length > LXP_KERNEL_MAX_BLOB_BYTES ||
        wasm_length > UINT32_MAX || lxp_ct_is_zero(program_id, 32U) ||
        lxp_ct_is_zero(code_hash, 32U)) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_blob_put(ctx, code_hash, wasm, wasm_length);
    if (status != LXP_OK) return status;
    artifact_key(program_id, key);
    write_u16(manifest, ARTIFACT_FORMAT_VERSION);
    write_u32(manifest + 2U, (uint32_t)wasm_length);
    (void)memcpy(manifest + 6U, code_hash, 32U);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), manifest, sizeof(manifest));
}

lxp_result lxp_programs_artifact_open(lxp_module_ctx *ctx,
                                      const uint8_t program_id[32],
                                      const uint8_t expected_hash[32],
                                      const uint8_t **wasm,
                                      size_t *wasm_length)
{
    uint8_t key[ARTIFACT_KEY_BYTES];
    uint8_t digest[32];
    const uint8_t *manifest;
    size_t manifest_length;
    size_t declared_length;
    lxp_result status;
    if (ctx == NULL || program_id == NULL || expected_hash == NULL ||
        wasm == NULL || wasm_length == NULL) return LXP_ERR_NON_CANONICAL;
    artifact_key(program_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &manifest, &manifest_length);
    if (status != LXP_OK) return status;
    if (manifest_length != LX_PROGRAMS_ARTIFACT_MANIFEST_BYTES ||
        read_u16(manifest) != ARTIFACT_FORMAT_VERSION ||
        lxp_ct_memcmp(manifest + 6U, expected_hash, 32U) != 0)
        return LXP_ERR_VERSION_UNSUPPORTED;
    declared_length = read_u32(manifest + 2U);
    if (declared_length == 0U || declared_length > LXP_KERNEL_MAX_BLOB_BYTES)
        return LXP_FATAL_INVARIANT;
    status = lxp_ctx_blob_get(ctx, manifest + 6U, wasm, wasm_length);
    if (status != LXP_OK) return status;
    if (*wasm_length != declared_length) return LXP_FATAL_INVARIANT;
    status = lxp_hash_sha256(*wasm, *wasm_length, digest);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(digest, manifest + 6U, 32U) == 0 ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_programs_artifact_catalog_count(lxp_module_ctx *ctx,
                                               size_t *count)
{
    artifact_catalog_cursor cursor = { SIZE_MAX, 0U, false, { 0 }, { 0 } };
    lxp_result status;
    if (ctx == NULL || count == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_iter(ctx, artifact_prefix, sizeof(artifact_prefix),
                             catalog_visit, &cursor);
    if (status != LXP_OK) return status;
    *count = cursor.count;
    return LXP_OK;
}

lxp_result lxp_programs_artifact_catalog_open(
    lxp_module_ctx *ctx, size_t index, uint8_t program_id[32],
    uint8_t code_hash[32], const uint8_t **wasm, size_t *wasm_length)
{
    artifact_catalog_cursor cursor = { index, 0U, false, { 0 }, { 0 } };
    lxp_result status;
    if (ctx == NULL || program_id == NULL || code_hash == NULL || wasm == NULL ||
        wasm_length == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_kv_iter(ctx, artifact_prefix, sizeof(artifact_prefix),
                             catalog_visit, &cursor);
    if (status != LXP_OK) return status;
    if (!cursor.found) return LXP_ERR_UNKNOWN_FIELD;
    (void)memcpy(program_id, cursor.program_id, 32U);
    (void)memcpy(code_hash, cursor.code_hash, 32U);
    return lxp_programs_artifact_open(ctx, cursor.program_id,
                                      cursor.code_hash, wasm, wasm_length);
}
