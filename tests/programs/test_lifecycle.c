#include "layerx/programs.h"

#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

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

static int blob_matches(const lxp_kernel *kernel, const uint8_t hash[32],
                        const uint8_t *bytes, size_t length)
{
    size_t i;
    for (i = 0U; i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        if (blob->module_id == LXP_MODULE_PROGRAMS &&
            memcmp(blob->key, hash, 32U) == 0)
            return blob->length == length &&
                   memcmp(blob->bytes, bytes, length) == 0 ? 0 : 1;
    }
    return 1;
}

static int dispatch(lxp_kernel *kernel, lxp_state_journal *journal,
                    lxp_authority_resolved *authority, uint16_t ordinal,
                    uint8_t *payload, size_t payload_length,
                    lxp_result expected)
{
    uint8_t arena_bytes[4096];
    lxp_arena arena;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    const lxp_module_registration *registration;
    lxp_result module_result = LXP_OK;
    (void)journal;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = ((uint32_t)LXP_MODULE_PROGRAMS << 16U) | ordinal;
    activity.payload = (lxp_byte_span){payload, payload_length};
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_kernel_module_for_activity(kernel, activity.activity_type, 0U,
                                       &registration) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_PROGRAMS, 1U, 0U,
                            ordinal, 1000000U, &arena, false) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK ||
        lxp_kernel_dispatch(registration, &ctx, &activity, authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != expected)
        return 1;
    if (module_result == LXP_OK)
        return lxp_module_ctx_commit(&ctx) == LXP_OK ? 0 : 1;
    lxp_module_ctx_rollback(&ctx);
    return 0;
}

int main(void)
{
    static const uint8_t wasm[8] = {
        0x00U, 0x61U, 0x73U, 0x6dU, 0x01U, 0x00U, 0x00U, 0x00U
    };
    static const uint8_t migration_wasm[37] = {
        0x00U, 0x61U, 0x73U, 0x6dU, 0x01U, 0x00U, 0x00U, 0x00U,
        0x01U, 0x04U, 0x01U, 0x60U, 0x00U, 0x00U,
        0x03U, 0x02U, 0x01U, 0x00U,
        0x07U, 0x0bU, 0x01U, 0x07U, 'm', 'i', 'g', 'r', 'a', 't', 'e',
        0x00U, 0x00U, 0x0aU, 0x04U, 0x01U, 0x02U, 0x00U, 0x0bU
    };
    uint8_t deploy[112] = {0};
    uint8_t upgrade[150] = {0};
    uint8_t old_hash[32];
    uint8_t new_hash[32];
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_authority_resolved authority;
    uint64_t parameters = 1U;
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(authority.principal, 0x42, sizeof(authority.principal));
    if (lxp_hash_sha256(wasm, sizeof(wasm), old_hash) != LXP_OK ||
        lxp_hash_sha256(migration_wasm, sizeof(migration_wasm), new_hash) !=
            LXP_OK)
        return 1;
    (void)memset(deploy, 0x31, 32U);
    write_u16(deploy + 32U, 1U);
    deploy[34] = 1U;
    (void)memcpy(deploy + 36U, authority.principal, 32U);
    (void)memcpy(deploy + 68U, old_hash, 32U);
    write_u32(deploy + 100U, sizeof(wasm));
    (void)memcpy(deploy + 104U, wasm, sizeof(wasm));
    (void)memset(upgrade, 0x31, 32U);
    write_u16(upgrade + 32U, 1U);
    upgrade[34] = 1U;
    (void)memcpy(upgrade + 36U, old_hash, 32U);
    (void)memcpy(upgrade + 68U, new_hash, 32U);
    write_u16(upgrade + 100U, 7U);
    write_u32(upgrade + 102U, sizeof(migration_wasm));
    (void)memcpy(upgrade + 106U, "migrate", 7U);
    (void)memcpy(upgrade + 113U, migration_wasm, sizeof(migration_wasm));
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) != LXP_OK ||
        dispatch(&kernel, &journal, &authority, 1U, deploy, sizeof(deploy),
                 LXP_OK) != 0 || kernel.blob_count != 1U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0)
        return 1;
    upgrade[106] = (uint8_t)'x';
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, sizeof(upgrade),
                 LXP_ERR_UNKNOWN_ACTIVITY) != 0 || kernel.blob_count != 1U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0)
        return 1;
    upgrade[106] = (uint8_t)'m';
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, sizeof(upgrade),
                 LXP_OK) != 0 || kernel.blob_count != 2U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0 ||
        blob_matches(&kernel, new_hash, migration_wasm,
                     sizeof(migration_wasm)) != 0)
        return 1;
    upgrade[32] = 0U;
    upgrade[33] = 2U;
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, sizeof(upgrade),
                 LXP_ERR_VERSION_UNSUPPORTED) != 0)
        return 1;
    return lxp_state_store_destroy(&store) == LXP_OK ? 0 : 1;
}
