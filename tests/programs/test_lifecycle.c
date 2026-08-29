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

static size_t interface_payload(uint8_t *out, const uint8_t code_hash[32])
{
    static const uint8_t domain[] = "LayerX/program-interface/v1";
    size_t offset = 0U;
    (void)memcpy(out + offset, domain, sizeof(domain)); offset += sizeof(domain);
    (void)memcpy(out + offset, code_hash, 32U); offset += 32U;
    write_u16(out + offset, 1U); offset += 2U;
    write_u16(out + offset, 1U); offset += 2U;
    write_u16(out + offset, 11U); offset += 2U;
    (void)memcpy(out + offset, "layerx_call", 11U); offset += 11U;
    (void)memset(out + offset, 0, 4U); offset += 4U;
    out[offset++] = 1U; out[offset++] = 0x20U; write_u32(out + offset, 64U); offset += 4U;
    out[offset++] = 1U; out[offset++] = 0x20U; write_u32(out + offset, 64U); offset += 4U;
    write_u16(out + offset, 0U); offset += 2U;
    write_u16(out + offset, 0U); offset += 2U;
    write_u16(out + offset, 0U); offset += 2U;
    return offset;
}

static int interface_state_matches(const lxp_kernel *kernel,
                                   const uint8_t program_id[32],
                                   const uint8_t code_hash[32],
                                   uint32_t version)
{
    static const uint8_t prefix[] = "interface";
    size_t index;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *item = &kernel->module_kv[index];
        const uint8_t *value = item->value;
        if (item->module_id != LXP_MODULE_PROGRAMS || item->key_length != 42U ||
            memcmp(item->key, prefix, sizeof(prefix)) != 0 ||
            memcmp(item->key + sizeof(prefix), program_id, 32U) != 0)
            continue;
        return item->value_length >= 134U &&
               memcmp(value, program_id, 32U) == 0 &&
               value[32] == (uint8_t)(version >> 24U) &&
               value[33] == (uint8_t)(version >> 16U) &&
               value[34] == (uint8_t)(version >> 8U) &&
               value[35] == (uint8_t)version &&
               memcmp(value + 100U, code_hash, 32U) == 0 ? 0 : 1;
    }
    return 1;
}

static int interface_state_absent(const lxp_kernel *kernel,
                                  const uint8_t program_id[32])
{
    static const uint8_t prefix[] = "interface";
    size_t index;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *item = &kernel->module_kv[index];
        if (item->module_id == LXP_MODULE_PROGRAMS &&
            item->key_length == 42U &&
            memcmp(item->key, prefix, sizeof(prefix)) == 0 &&
            memcmp(item->key + sizeof(prefix), program_id, 32U) == 0)
            return 1;
    }
    return 0;
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
    static const uint8_t wasm[] = {
        0,97,115,109,1,0,0,0,1,12,2,96,2,127,127,1,127,96,1,127,1,127,
        3,3,2,0,1,5,3,1,0,1,7,41,3,
        11,'l','a','y','e','r','x','_','c','a','l','l',0,0,
        14,'l','a','y','e','r','x','_','r','e','s','e','r','v','e',0,1,
        6,'m','e','m','o','r','y',2,0,10,11,2,4,0,65,0,11,4,0,65,0,11
    };
    static const uint8_t migration_wasm[] = {
        0,97,115,109,1,0,0,0,1,15,3,96,2,127,127,1,127,96,1,127,1,127,96,0,0,
        3,4,3,0,1,2,5,3,1,0,1,7,51,4,
        11,'l','a','y','e','r','x','_','c','a','l','l',0,0,
        14,'l','a','y','e','r','x','_','r','e','s','e','r','v','e',0,1,
        6,'m','e','m','o','r','y',2,0,7,'m','i','g','r','a','t','e',0,2,
        10,14,3,4,0,65,1,11,4,0,65,0,11,2,0,11
    };
    uint8_t deploy[512] = {0};
    uint8_t upgrade[512] = {0};
    uint8_t old_hash[32];
    uint8_t new_hash[32];
    uint8_t legacy_deploy[512] = {0};
    uint8_t legacy_upgrade[512] = {0};
    uint8_t legacy_program[32];
    uint8_t establish[512] = {0};
    uint8_t remove_interface[512] = {0};
    size_t establish_interface_length;
    size_t establish_length;
    size_t deploy_interface_length;
    size_t upgrade_interface_length;
    size_t deploy_length;
    size_t upgrade_length;
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_state_store sandbox_store;
    lxp_state_journal sandbox_journal;
    lxp_kernel sandbox_kernel;
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
    deploy_interface_length = interface_payload(deploy + 108U, old_hash);
    write_u32(deploy + 104U, (uint32_t)deploy_interface_length);
    (void)memcpy(deploy + 108U + deploy_interface_length, wasm, sizeof(wasm));
    deploy_length = 108U + deploy_interface_length + sizeof(wasm);
    (void)memset(upgrade, 0x31, 32U);
    write_u16(upgrade + 32U, 1U);
    upgrade[34] = 1U;
    (void)memcpy(upgrade + 36U, old_hash, 32U);
    (void)memcpy(upgrade + 68U, new_hash, 32U);
    write_u16(upgrade + 100U, 7U);
    write_u32(upgrade + 102U, sizeof(migration_wasm));
    upgrade_interface_length = interface_payload(upgrade + 117U, new_hash);
    write_u32(upgrade + 106U, (uint32_t)upgrade_interface_length);
    (void)memcpy(upgrade + 110U, "migrate", 7U);
    (void)memcpy(upgrade + 117U + upgrade_interface_length, migration_wasm,
                 sizeof(migration_wasm));
    upgrade_length = 117U + upgrade_interface_length + sizeof(migration_wasm);
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) != LXP_OK ||
        dispatch(&kernel, &journal, &authority, 1U, deploy, deploy_length,
                 LXP_OK) != 0 || kernel.blob_count != 1U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0 ||
        interface_state_matches(&kernel, deploy, old_hash, 1U) != 0)
        return 1;
    (void)memset(legacy_program, 0x32, sizeof(legacy_program));
    (void)memcpy(legacy_deploy, legacy_program, 32U);
    write_u16(legacy_deploy + 32U, 1U);
    legacy_deploy[34] = 1U;
    (void)memcpy(legacy_deploy + 36U, authority.principal, 32U);
    (void)memcpy(legacy_deploy + 68U, old_hash, 32U);
    write_u32(legacy_deploy + 100U, sizeof(wasm));
    (void)memcpy(legacy_deploy + 104U, wasm, sizeof(wasm));
    (void)memcpy(legacy_upgrade, legacy_program, 32U);
    write_u16(legacy_upgrade + 32U, 1U);
    (void)memcpy(legacy_upgrade + 36U, old_hash, 32U);
    (void)memcpy(legacy_upgrade + 68U, new_hash, 32U);
    write_u16(legacy_upgrade + 100U, 0U);
    write_u32(legacy_upgrade + 102U, sizeof(migration_wasm));
    (void)memcpy(legacy_upgrade + 106U, migration_wasm,
                 sizeof(migration_wasm));
    if (dispatch(&kernel, &journal, &authority, 1U, legacy_deploy,
                 104U + sizeof(wasm), LXP_OK) != 0 ||
        interface_state_absent(&kernel, legacy_program) != 0 ||
        dispatch(&kernel, &journal, &authority, 2U, legacy_upgrade,
                 106U + sizeof(migration_wasm), LXP_OK) != 0 ||
        interface_state_absent(&kernel, legacy_program) != 0)
        return 1;
    (void)memcpy(establish, legacy_program, 32U);
    write_u16(establish + 32U, 1U);
    (void)memcpy(establish + 36U, new_hash, 32U);
    (void)memcpy(establish + 68U, old_hash, 32U);
    write_u16(establish + 100U, 0U);
    write_u32(establish + 102U, sizeof(wasm));
    establish_interface_length = interface_payload(establish + 110U,
                                                    old_hash);
    write_u32(establish + 106U, (uint32_t)establish_interface_length);
    (void)memcpy(establish + 110U + establish_interface_length,
                 wasm, sizeof(wasm));
    establish_length = 110U + establish_interface_length +
                       sizeof(wasm);
    if (dispatch(&kernel, &journal, &authority, 2U, establish,
                 establish_length, LXP_ERR_UNKNOWN_FIELD) != 0)
        return 1;
    establish[34] = 2U;
    if (dispatch(&kernel, &journal, &authority, 2U, establish,
                 establish_length, LXP_OK) != 0 ||
        interface_state_matches(&kernel, legacy_program, old_hash, 3U) != 0 ||
        dispatch(&kernel, &journal, &authority, 2U, legacy_upgrade,
                 106U + sizeof(migration_wasm),
                 LXP_ERR_CONTEXT_MISMATCH) != 0)
        return 1;
    upgrade[110] = (uint8_t)'x';
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, upgrade_length,
                 LXP_ERR_UNKNOWN_ACTIVITY) != 0 || kernel.blob_count != 2U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0)
        return 1;
    upgrade[110] = (uint8_t)'m';
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, upgrade_length,
                 LXP_OK) != 0 || kernel.blob_count != 2U ||
        blob_matches(&kernel, old_hash, wasm, sizeof(wasm)) != 0 ||
        blob_matches(&kernel, new_hash, migration_wasm,
                     sizeof(migration_wasm)) != 0 ||
        interface_state_matches(&kernel, upgrade, new_hash, 2U) != 0)
        return 1;
    upgrade[32] = 0U;
    upgrade[33] = 2U;
    if (dispatch(&kernel, &journal, &authority, 2U, upgrade, upgrade_length,
                 LXP_ERR_VERSION_UNSUPPORTED) != 0)
        return 1;
    upgrade[32] = 0U;
    upgrade[33] = 1U;
    (void)memcpy(remove_interface, deploy, 32U);
    write_u16(remove_interface + 32U, 1U);
    (void)memcpy(remove_interface + 36U, new_hash, 32U);
    (void)memcpy(remove_interface + 68U, old_hash, 32U);
    write_u16(remove_interface + 100U, 0U);
    write_u32(remove_interface + 102U, sizeof(wasm));
    write_u32(remove_interface + 106U, 0U);
    (void)memcpy(remove_interface + 110U, wasm, sizeof(wasm));
    if (dispatch(&kernel, &journal, &authority, 2U, remove_interface,
                 110U + sizeof(wasm), LXP_ERR_NON_CANONICAL) != 0 ||
        interface_state_matches(&kernel, upgrade, new_hash, 2U) != 0)
        return 1;
    remove_interface[34] = 2U;
    if (dispatch(&kernel, &journal, &authority, 2U, remove_interface,
                 110U + sizeof(wasm), LXP_OK) != 0 ||
        interface_state_absent(&kernel, deploy) != 0)
        return 1;
    (void)memset(deploy, 0x33, 32U);
    if (lxp_state_store_init(&sandbox_store, 0U) != LXP_OK ||
        lxp_kernel_create(&sandbox_kernel, &sandbox_store, &sandbox_journal,
                          &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&sandbox_kernel,
                                   programs_module_registration_v3()) !=
            LXP_OK ||
        dispatch(&sandbox_kernel, &sandbox_journal, &authority, 1U, deploy,
                 deploy_length, LXP_OK) != 0 ||
        lxp_state_store_destroy(&sandbox_store) != LXP_OK)
        return 1;
    return lxp_state_store_destroy(&store) == LXP_OK ? 0 : 1;
}
