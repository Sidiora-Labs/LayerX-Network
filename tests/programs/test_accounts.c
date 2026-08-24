#include "layerx/programs.h"

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_snapshot.h"

#include <string.h>

static uint8_t nibble(uint8_t value)
{
    return value <= (uint8_t)'9' ? (uint8_t)(value - (uint8_t)'0') :
                                   (uint8_t)(value - (uint8_t)'a' + 10U);
}

static void decode_hex(const char *hex, uint8_t bytes[32])
{
    size_t i;
    for (i = 0U; i < 32U; ++i)
        bytes[i] = (uint8_t)((nibble((uint8_t)hex[i * 2U]) << 4U) |
                             nibble((uint8_t)hex[i * 2U + 1U]));
}

static int derivation_vectors(void)
{
    static const char empty_expected[] =
        "558c786d2c1f6371169ad993b4adb445e3081e410ce50bc7da1752005426fd40";
    static const char vault_expected[] =
        "ae8ecdd739892abd6f799dc19ebf0c5791eddf59db1c41e12f8ee22a590507f2";
    static const char binary_expected[] =
        "694e43962a8b89d0ee449629e50c6e8f2bf8492ba86c3b6e99782158235a29f5";
    static const char maximum_expected[] =
        "295fb65ee3e4ffa67749a0ea0e4c709c0f14f1223034ac8b5e7d1d76346aba24";
    static const uint8_t binary_seed[5] = {0x00U, 0xffU, 0x7fU, 0x80U, 0x01U};
    uint8_t program_id[32];
    uint8_t expected[32];
    uint8_t derived[32];
    uint8_t maximum[LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    (void)memset(program_id, 1, sizeof(program_id));
    decode_hex(empty_expected, expected);
    if (lxp_programs_account_derive(program_id, NULL, 0U, derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    decode_hex(vault_expected, expected);
    if (lxp_programs_account_derive(program_id, (const uint8_t *)"vault", 5U,
                                    derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    {
        size_t i;
        for (i = 0U; i < sizeof(program_id); ++i)
            program_id[i] = (uint8_t)(i + 1U);
    }
    decode_hex(binary_expected, expected);
    if (lxp_programs_account_derive(program_id, binary_seed,
                                    sizeof(binary_seed), derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    (void)memset(program_id, 0xab, sizeof(program_id));
    (void)memset(maximum, 0xcd, sizeof(maximum));
    decode_hex(maximum_expected, expected);
    if (lxp_programs_account_derive(program_id, maximum, sizeof(maximum),
                                    derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0 ||
        lxp_programs_account_derive(program_id, maximum,
            sizeof(maximum) + 1U, derived) != LXP_ERR_LENGTH_LIMIT)
        return 1;
    return 0;
}

static lxp_result count_binding(const lx_programs_account_binding *binding,
                                void *user)
{
    size_t *count = (size_t *)user;
    if (binding == NULL || binding->seed_length != 5U ||
        memcmp(binding->seed, "vault", 5U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    ++*count;
    return LXP_OK;
}

static int registry_boundaries(void)
{
    static const uint8_t module_name[] = "programs";
    lx_account_registry registry;
    lx_account_registration registration;
    lx_account *account;
    uint8_t account_id[32];
    uint8_t asset_id[32];
    bool created;

    (void)memset(account_id, 0x31, sizeof(account_id));
    (void)memset(asset_id, 0x42, sizeof(asset_id));
    if (lx_account_registry_init(&registry) != LXP_OK)
        return 1;
    registry.count = 1U;
    registry.accounts[0].kind = LX_ACCOUNT_AGENT_MAIN;
    (void)memcpy(registry.accounts[0].id, account_id, sizeof(account_id));
    if (lx_account_module_value_prepare(
            &registry, module_name, sizeof(module_name) - 1U, account_id,
            asset_id, 1U, &registration, &account, &created) !=
        LXP_ERR_ACCOUNT_ID_MISMATCH)
        return 1;

    if (lx_account_registry_init(&registry) != LXP_OK)
        return 1;
    registry.count = LX_ACCOUNT_REGISTRY_CAPACITY;
    if (lx_account_module_value_prepare(
            &registry, module_name, sizeof(module_name) - 1U, account_id,
            asset_id, 1U, &registration, &account, &created) !=
        LXP_ERR_ARENA_EXHAUSTED)
        return 1;
    return 0;
}

static int registration_law(void)
{
    static const uint8_t program_prefix[] = "program\0";
    static const uint8_t owner_prefix[] = "program-owner\0";
    uint8_t arena_bytes[4096];
    static uint8_t snapshot_bytes[1048576];
    lxp_arena arena;
    lxp_arena snapshot_arena;
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lx_account_registry accounts;
    lxp_transfer_asset_state assets[2];
    lx_programs_transfer_runtime runtime;
    lxp_module_ctx deploy_ctx;
    lxp_module_ctx legacy_ctx;
    lxp_module_ctx account_ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    lxp_authority_resolved authority;
    const lxp_module_registration *registration;
    const lxp_module_registration *legacy_registration;
    lxp_result module_result = LXP_OK;
    uint64_t parameters = 1U;
    uint8_t program_id[32];
    uint8_t expected_id[32];
    uint8_t deploy_key[sizeof(program_prefix) - 1U + 32U];
    uint8_t owner_key[sizeof(owner_prefix) - 1U + 32U];
    uint8_t deploy_record[71];
    uint8_t owner_record[33];
    uint8_t registration_payload[78];
    uint8_t before_root[32];
    uint8_t preview_root[32];
    uint8_t committed_root[32];
    lx_account *account;
    lx_programs_account_binding binding;
    bool created;
    size_t visited = 0U;
    lxp_byte_span snapshot;
    lxp_snapshot_manifest_record manifest;
    lxp_state_store restored_store;
    lxp_state_journal restored_journal;
    lxp_kernel restored_kernel;
    lx_account_registry restored_accounts;
    lx_programs_transfer_runtime restored_runtime;

    (void)memset(program_id, 1, sizeof(program_id));
    (void)memset(assets, 0, sizeof(assets));
    (void)memset(&runtime, 0, sizeof(runtime));
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(registration_payload, 0, sizeof(registration_payload));
    (void)memset(deploy_record, 0, sizeof(deploy_record));
    (void)memset(owner_record, 0, sizeof(owner_record));
    (void)memcpy(assets[0].asset_id, "asset-one", 9U);
    (void)memcpy(assets[1].asset_id, "asset-two", 9U);
    assets[0].registered = true;
    assets[1].registered = true;
    runtime.accounts = &accounts;
    runtime.assets = assets;
    runtime.asset_count = 2U;
    (void)memcpy(deploy_key, program_prefix, sizeof(program_prefix) - 1U);
    (void)memcpy(deploy_key + sizeof(program_prefix) - 1U,
                 program_id, 32U);
    (void)memcpy(owner_key, owner_prefix, sizeof(owner_prefix) - 1U);
    (void)memcpy(owner_key + sizeof(owner_prefix) - 1U, program_id, 32U);
    (void)memcpy(registration_payload, program_id, 32U);
    (void)memcpy(registration_payload + 32U, "LXPA1", 5U);
    (void)memcpy(registration_payload + 37U, assets[0].asset_id, 32U);
    registration_payload[72] = 5U;
    (void)memcpy(registration_payload + 73U, "vault", 5U);
    activity.activity_type = LX_PROGRAMS_ACCOUNT;
    activity.payload = (lxp_byte_span){registration_payload,
                                       sizeof(registration_payload)};
    (void)memset(authority.principal, 0x44, 32U);
    deploy_record[0] = 1U;
    (void)memcpy(deploy_record + 1U, authority.principal, 32U);
    (void)memset(deploy_record + 33U, 0x51, 32U);
    deploy_record[66] = 1U;
    deploy_record[70] = 1U;
    owner_record[0] = 1U;
    (void)memcpy(owner_record + 1U, authority.principal, 32U);

    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_arena_init(&snapshot_arena, snapshot_bytes,
                       sizeof(snapshot_bytes)) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lxp_state_store_init(&store, 7U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_set_epoch(&kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v2()) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS,
                                       &runtime) != LXP_OK ||
        lxp_module_ctx_init(&deploy_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            1U, 1U, 7U, 10000U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&deploy_ctx, deploy_key, sizeof(deploy_key),
                       deploy_record, sizeof(deploy_record)) != LXP_OK ||
        lxp_ctx_kv_put(&deploy_ctx, owner_key, sizeof(owner_key),
                       owner_record, sizeof(owner_record)) != LXP_OK ||
        lxp_module_ctx_commit(&deploy_ctx) != LXP_OK ||
        lxp_state_root(&kernel, before_root) != LXP_OK ||
        lxp_state_journal_open(&store, 7U, &journal) != LXP_OK ||
        lxp_module_ctx_init(&legacy_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            2U, 1U, 7U, 10000U, &arena, false) != LXP_OK)
        return 1;
    legacy_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&legacy_ctx, &effects) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_REGISTRY, 1U,
                                       &legacy_registration) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_ACCOUNT, 1U,
                                       &registration) != LXP_OK ||
        lxp_programs_account_derive(program_id, (const uint8_t *)"vault", 5U,
                                    expected_id) != LXP_OK)
        return 1;
    activity.activity_type = LX_PROGRAMS_REGISTRY;
    if (lxp_kernel_dispatch(legacy_registration, &legacy_ctx, &activity,
                            &authority, &effects, &module_result) != LXP_OK ||
        module_result != LXP_OK || legacy_ctx.staged_account_count != 0U ||
        legacy_ctx.staged_count != 0U || effects.count != 1U ||
        effects.effects[0].event_type != LX_PROGRAMS_EVENT_REGISTRY_READ)
        return 1;
    lxp_module_ctx_rollback(&legacy_ctx);
    if (lxp_module_ctx_init(&account_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            3U, 1U, 7U, 10000U, &arena, false) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&account_ctx, &effects) != LXP_OK)
        return 1;
    activity.activity_type = LX_PROGRAMS_ACCOUNT;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    module_result = LXP_OK;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_VERSION_UNSUPPORTED ||
        account_ctx.staged_account_count != 0U ||
        account_ctx.staged_count != 0U || effects.count != 0U)
        return 1;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    (void)memset(authority.principal, 0x45, 32U);
    module_result = LXP_OK;
    if (lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_AUTH_SCOPE ||
        account_ctx.staged_account_count != 0U ||
        account_ctx.staged_count != 0U || effects.count != 0U)
        return 1;
    (void)memset(authority.principal, 0x44, 32U);
    module_result = LXP_OK;
    if (lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_OK || accounts.count != 0U ||
        account_ctx.staged_account_count != 1U ||
        account_ctx.staged_count != 2U ||
        account_ctx.staged_accounts[0].account.kind !=
            LX_ACCOUNT_MODULE_VALUE ||
        !account_ctx.staged_accounts[0].account.has_asset ||
        memcmp(account_ctx.staged_accounts[0].account.id,
               expected_id, 32U) != 0 ||
        memcmp(account_ctx.staged_accounts[0].account.asset_id,
               assets[0].asset_id, 32U) != 0 ||
        effects.count != 1U ||
        effects.effects[0].event_type !=
            LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED ||
        !store.account_root_required ||
        lxp_module_ctx_prepare_commit(&account_ctx) != LXP_OK ||
        lxp_module_ctx_preview_state_root(&account_ctx, &journal,
                                          preview_root) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&account_ctx) != LXP_OK ||
        accounts.count != 1U || kernel.module_kv_count != 4U ||
        lx_account_open(&accounts, accounts.accounts[0].name,
                        accounts.accounts[0].name_length, expected_id, 8U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &account) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        lxp_state_root(&kernel, committed_root) != LXP_OK ||
        memcmp(preview_root, committed_root, 32U) != 0 ||
        memcmp(before_root, committed_root, 32U) == 0)
        return 1;

    if (lxp_state_journal_open(&store, 8U, &journal) != LXP_OK ||
        lxp_module_ctx_init(&account_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            4U, 1U, 8U, 10000U, &arena, true) != LXP_OK)
        return 1;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&account_ctx, &effects) != LXP_OK ||
        lxp_programs_account_register(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            assets[0].asset_id, &account, &created) != LXP_OK ||
        created || account != &accounts.accounts[0] || effects.count != 0U ||
        lxp_programs_account_lookup(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            &binding, &account) != LXP_OK ||
        account != &accounts.accounts[0] ||
        lxp_programs_account_lookup_id(
            &account_ctx, expected_id, &binding, &account) != LXP_OK ||
        lxp_programs_account_iter(&account_ctx, program_id, count_binding,
                                  &visited) != LXP_OK ||
        visited != 1U ||
        lxp_programs_account_register(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            assets[1].asset_id, &account, &created) !=
                LXP_ERR_ASSET_MISMATCH)
        return 1;
    lxp_module_ctx_rollback(&account_ctx);
    if (lxp_state_journal_rollback(&journal) != LXP_OK ||
        accounts.count != 1U || kernel.module_kv_count != 4U)
        return 1;

    if (lxp_snapshot_write(&kernel, 7U, &snapshot_arena, &snapshot) != LXP_OK ||
        lxp_snapshot_manifest_build(snapshot.bytes, snapshot.length, 7U,
                                    committed_root, &manifest) != LXP_OK ||
        lx_account_registry_init(&restored_accounts) != LXP_OK ||
        lxp_state_store_init(&restored_store, 0U) != LXP_OK ||
        lxp_kernel_create(&restored_kernel, &restored_store,
                          &restored_journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&restored_kernel,
                                   programs_module_registration()) != LXP_OK ||
        lxp_kernel_set_epoch(&restored_kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&restored_kernel,
                                   programs_module_registration_v2()) !=
            LXP_OK)
        return 1;
    restored_runtime = runtime;
    restored_runtime.accounts = &restored_accounts;
    if (lxp_kernel_bind_module_runtime(&restored_kernel, LXP_MODULE_PROGRAMS,
                                       &restored_runtime) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          committed_root, &restored_kernel) != LXP_OK ||
        restored_accounts.count != 1U ||
        memcmp(restored_accounts.accounts[0].id, expected_id, 32U) != 0 ||
        lxp_state_root(&restored_kernel, before_root) != LXP_OK ||
        memcmp(before_root, committed_root, 32U) != 0 ||
        lxp_state_store_destroy(&restored_store) != LXP_OK ||
        lxp_state_store_destroy(&store) != LXP_OK)
        return 1;
    return 0;
}

int main(void)
{
    if (derivation_vectors() != 0) return 1;
    if (registry_boundaries() != 0) return 1;
    return registration_law();
}
