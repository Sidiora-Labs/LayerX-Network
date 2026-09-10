#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_state.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "genesis module table line %d: %s\n", __LINE__, \
            #condition); return 1; } } while (0)

static const uint8_t parameter_version_key[32] = {
    'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
};

static const uint16_t flag_slots[] = {
    LXP_MODULE_ESCROW, LXP_MODULE_BUDGET, LXP_MODULE_STREAM,
    LXP_MODULE_SERVICE, LXP_MODULE_PERPS
};

static int public_key_for(const uint8_t private_key[32],
                          uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int valid = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return valid ? 0 : 1;
}

static void programs_parameters(const uint8_t signer_public_key[32],
                                const uint8_t asset_id[32],
                                lx_programs_metering_schedule *metering,
                                lx_programs_fee_genesis_parameters *fees)
{
    (void)memset(metering, 0, sizeof(*metering));
    metering->version = 1U;
    metering->coefficients[0] = 1U;
    metering->coefficients[1] = 1U;
    metering->coefficients[2] = 1U;
    metering->coefficients[3] = 1U;
    metering->coefficients[4] = 1U;
    metering->coefficients[5] = 8U;
    metering->coefficients[6] = 8U;
    metering->coefficients[7] = 64U;
    metering->coefficients[8] = 8U;
    metering->activation_batch = 1U;
    metering->authority_kind = LX_PROGRAMS_METERING_AUTHORITY_GENESIS;
    (void)lxp_hash_payload(signer_public_key, 32U, metering->authority_digest);
    (void)memset(fees, 0, sizeof(*fees));
    fees->schedule = (lx_programs_fee_schedule){1U, 1U, 1U, 2U, 4U, 1U, 1U,
                                                100U};
    (void)memcpy(fees->occupancy_asset_id, asset_id, 32U);
    fees->target_occupancy_byte_batches = 100U;
    fees->response_denominator = 1U;
    fees->maximum_change_numerator = 1U;
    fees->maximum_change_denominator = 10U;
    fees->minimum_fee_units_per_occupancy_byte_batch = 1U;
    fees->maximum_fee_units_per_occupancy_byte_batch = 1000U;
}

/* Draft carrying an optional module-enable parameter.  Genesis parameters are
 * sorted by (module id, key) and every "module-enable:" key sorts below
 * "parameter-version" under the same governance module id. */
static void draft_manifest(lxp_genesis_manifest *draft, const uint8_t *key,
                           uint8_t flag)
{
    size_t index = 0U;
    (void)memset(draft, 0, sizeof(*draft));
    draft->protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    draft->network_id = 42U;
    draft->genesis_timestamp_ms = UINT64_C(1700000000000);
    if (key != NULL) {
        draft->parameters[index].module_id = LXP_MODULE_GOVERNANCE;
        (void)memcpy(draft->parameters[index].key, key, 32U);
        draft->parameters[index].value[31] = flag;
        ++index;
    }
    draft->parameters[index].module_id = LXP_MODULE_GOVERNANCE;
    (void)memcpy(draft->parameters[index].key, parameter_version_key, 32U);
    draft->parameters[index].value[31] = 1U;
    draft->parameter_count = index + 1U;
    draft->guarantor_count = 1U;
    draft->guarantors[0].guarantor_id[0] = 1U;
    draft->guarantors[0].public_key[0] = 2U;
    draft->guarantors[0].public_key[32] = 3U;
    draft->guarantors[0].bond = (lxp_u128){0U, 0U};
}

static int check_table(void)
{
    size_t count = 0U;
    const lxp_genesis_module_entry *table = lxp_genesis_module_table(&count);
    size_t index;
    size_t slot;
    uint8_t key[32];
    REQUIRE(table != NULL);
    REQUIRE(count != 0U && count <= (size_t)LXP_GENESIS_MODULE_TABLE_MAX);
    for (index = 0U; index < count; ++index) {
        const lxp_module_iface *iface = table[index].iface();
        size_t other;
        size_t name_length;
        REQUIRE(iface != NULL);
        REQUIRE(iface->module_id == table[index].module_id);
        REQUIRE(iface->name != NULL);
        REQUIRE(iface->activity_type_count != 0U);
        for (other = 0U; other < index; ++other)
            REQUIRE(table[other].module_id != table[index].module_id);
        name_length = strlen(iface->name);
        if (table[index].gate == LXP_GENESIS_MODULE_GATE_ENABLE_FLAG) {
            REQUIRE(lxp_genesis_module_enable_key(table[index].module_id,
                                                  key) == LXP_OK);
            REQUIRE(memcmp(key, "module-enable:",
                           LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES) == 0);
            REQUIRE(memcmp(key + LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES,
                           iface->name, name_length) == 0);
            REQUIRE(lxp_ct_is_zero(
                key + LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES + name_length,
                32U - LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES - name_length));
        } else {
            REQUIRE(lxp_genesis_module_enable_key(table[index].module_id,
                                                  key) != LXP_OK);
        }
    }
    REQUIRE(lxp_genesis_module_enable_key(0U, key) != LXP_OK);
    REQUIRE(lxp_genesis_module_enable_key(
        LXP_MODULE_RESERVED_COUNT + 1U, key) != LXP_OK);
    for (slot = 0U; slot < sizeof(flag_slots) / sizeof(flag_slots[0]);
         ++slot) {
        bool found = false;
        for (index = 0U; index < count; ++index) {
            if (table[index].module_id != flag_slots[slot]) continue;
            REQUIRE(table[index].gate ==
                    LXP_GENESIS_MODULE_GATE_ENABLE_FLAG);
            found = true;
        }
        REQUIRE(found);
        REQUIRE(lxp_genesis_module_enable_key(flag_slots[slot], key) ==
                LXP_OK);
    }
    return 0;
}

static int check_defaults(void)
{
    lxp_genesis_module_plan plan;
    REQUIRE(lxp_genesis_module_plan_default(
        LXP_PROTOCOL_VERSION_OCCUPANCY, false, &plan) == LXP_OK);
    REQUIRE(plan.count == 1U);
    REQUIRE(plan.modules[0]->module_id == LXP_MODULE_PROGRAMS);
    REQUIRE(lxp_genesis_module_plan_default(
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT, false, &plan) == LXP_OK);
    REQUIRE(plan.count == 3U);
    REQUIRE(plan.modules[0]->module_id == LXP_MODULE_PROGRAMS);
    REQUIRE(plan.modules[1]->module_id == LXP_MODULE_ASSET);
    REQUIRE(plan.modules[2]->module_id == LXP_MODULE_GOVERNANCE);
    REQUIRE(lxp_genesis_module_plan_default(
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true, &plan) == LXP_OK);
    REQUIRE(plan.count == 4U);
    REQUIRE(plan.modules[3]->module_id == LXP_MODULE_BRIDGE);
    REQUIRE(lxp_genesis_module_plan_default(0U, false, &plan) != LXP_OK);
    REQUIRE(lxp_genesis_module_plan_default(
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT + 1U, false, &plan) != LXP_OK);
    REQUIRE(lxp_genesis_module_plan_default(
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT, false, NULL) != LXP_OK);
    return 0;
}

typedef struct built_genesis {
    lxp_genesis_manifest manifest;
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_byte_span encoded_manifest;
    lxp_byte_span snapshot;
} built_genesis;

static lxp_result build(const uint8_t *enable_key, uint8_t flag,
                        lxp_arena *arena, built_genesis *built)
{
    static const uint8_t signer_private_key[32] = {7U};
    static const uint8_t asset_id[32] = {0x85U};
    lxp_genesis_manifest draft;
    lx_programs_metering_schedule metering;
    lx_programs_fee_genesis_parameters fees;
    uint8_t signer_public_key[32];
    if (public_key_for(signer_private_key, signer_public_key) != 0)
        return LXP_ERR_BAD_SIGNATURE;
    draft_manifest(&draft, enable_key, flag);
    programs_parameters(signer_public_key, asset_id, &metering, &fees);
    return lxp_genesis_build_fresh_empty(
        &draft, asset_id, &metering, &fees, signer_private_key, arena,
        &built->manifest, &built->snapshot_manifest,
        &built->encoded_manifest, &built->snapshot);
}

static int check_kernel(const built_genesis *built,
                        const lxp_genesis_module_plan *plan)
{
    static lxp_state_store state;
    static lxp_state_journal journal;
    static lxp_kernel kernel;
    static lx_account_registry accounts;
    size_t index;
    REQUIRE(lx_account_registry_init(&accounts) == LXP_OK);
    REQUIRE(lxp_state_store_init(&state, 1U) == LXP_OK);
    REQUIRE(lxp_state_store_bind_accounts(&state, &accounts) == LXP_OK);
    REQUIRE(lxp_kernel_create(&kernel, &state, &journal, &built->manifest,
                              1U) == LXP_OK);
    REQUIRE(lxp_genesis_module_plan_matches(plan, &kernel) != LXP_OK);
    for (index = 0U; index < plan->count; ++index) {
        REQUIRE(lxp_kernel_register_module(&kernel,
                                           plan->modules[index]) == LXP_OK);
        if (index + 1U < plan->count)
            REQUIRE(lxp_snapshot_load(built->snapshot.bytes,
                                      built->snapshot.length,
                                      &built->snapshot_manifest,
                                      &kernel) != LXP_OK);
    }
    REQUIRE(lxp_genesis_module_plan_matches(plan, &kernel) == LXP_OK);
    REQUIRE(lxp_snapshot_load(built->snapshot.bytes, built->snapshot.length,
                              &built->snapshot_manifest, &kernel) == LXP_OK);
    REQUIRE(accounts.count == LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT);
    REQUIRE(memcmp(kernel.current_state_root,
                   built->snapshot_manifest.receipt_state_root, 32U) == 0);
    REQUIRE(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}

static int check_enable_flag(void)
{
    static uint8_t arena_bytes[8388608U];
    static built_genesis plain;
    static built_genesis enabled;
    static built_genesis disabled;
    lxp_genesis_module_plan plan;
    lxp_genesis_manifest changed;
    lxp_arena arena;
    uint8_t escrow_key[32];
    uint8_t asset_key[32];
    uint8_t unknown_key[32];

    REQUIRE(lxp_genesis_module_enable_key(LXP_MODULE_ESCROW, escrow_key) ==
            LXP_OK);
    REQUIRE(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
            LXP_OK);

    REQUIRE(build(NULL, 0U, &arena, &plain) == LXP_OK);
    REQUIRE(lxp_genesis_module_plan_resolve(&plain.manifest, &plan) ==
            LXP_OK);
    REQUIRE(plan.count == 3U);
    REQUIRE(plan.modules[0]->module_id == LXP_MODULE_PROGRAMS);
    REQUIRE(plan.modules[1]->module_id == LXP_MODULE_ASSET);
    REQUIRE(plan.modules[2]->module_id == LXP_MODULE_GOVERNANCE);
    REQUIRE(check_kernel(&plain, &plan) == 0);
    REQUIRE(lxp_genesis_verify_signature(&plain.manifest, &arena) == LXP_OK);

    REQUIRE(build(escrow_key, 1U, &arena, &enabled) == LXP_OK);
    REQUIRE(lxp_genesis_module_plan_resolve(&enabled.manifest, &plan) ==
            LXP_OK);
    REQUIRE(plan.count == 4U);
    REQUIRE(plan.modules[3]->module_id == LXP_MODULE_ESCROW);
    REQUIRE(check_kernel(&enabled, &plan) == 0);
    REQUIRE(lxp_genesis_verify_signature(&enabled.manifest, &arena) ==
            LXP_OK);
    REQUIRE(memcmp(enabled.manifest.genesis_state_root,
                   plain.manifest.genesis_state_root, 32U) != 0);

    REQUIRE(build(escrow_key, 0U, &arena, &disabled) == LXP_OK);
    REQUIRE(lxp_genesis_module_plan_resolve(&disabled.manifest, &plan) ==
            LXP_OK);
    REQUIRE(plan.count == 3U);
    REQUIRE(check_kernel(&disabled, &plan) == 0);
    REQUIRE(memcmp(disabled.manifest.genesis_state_root,
                   enabled.manifest.genesis_state_root, 32U) != 0);

    changed = enabled.manifest;
    changed.parameters[0].value[31] = 2U;
    REQUIRE(lxp_genesis_module_plan_resolve(&changed, &plan) != LXP_OK);
    changed = enabled.manifest;
    changed.parameters[0].value[0] = 1U;
    REQUIRE(lxp_genesis_module_plan_resolve(&changed, &plan) != LXP_OK);

    REQUIRE(lxp_genesis_module_enable_key(LXP_MODULE_ESCROW, unknown_key) ==
            LXP_OK);
    unknown_key[LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES] = 'z';
    changed = enabled.manifest;
    (void)memcpy(changed.parameters[0].key, unknown_key, 32U);
    REQUIRE(lxp_genesis_module_plan_resolve(&changed, &plan) != LXP_OK);

    (void)memset(asset_key, 0, sizeof(asset_key));
    (void)memcpy(asset_key, "module-enable:",
                 LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES);
    (void)memcpy(asset_key + LXP_GENESIS_MODULE_ENABLE_PREFIX_BYTES,
                 lx_asset_module_iface()->name,
                 strlen(lx_asset_module_iface()->name));
    changed = enabled.manifest;
    (void)memcpy(changed.parameters[0].key, asset_key, 32U);
    REQUIRE(lxp_genesis_module_plan_resolve(&changed, &plan) != LXP_OK);

    REQUIRE(lxp_genesis_module_plan_resolve(NULL, &plan) != LXP_OK);
    REQUIRE(lxp_genesis_module_plan_resolve(&enabled.manifest, NULL) !=
            LXP_OK);
    REQUIRE(lxp_genesis_module_plan_register(&plan, NULL) != LXP_OK);
    REQUIRE(lxp_genesis_module_plan_matches(&plan, NULL) != LXP_OK);
    return 0;
}

int main(void)
{
    REQUIRE(check_table() == 0);
    REQUIRE(check_defaults() == 0);
    REQUIRE(check_enable_flag() == 0);
    return 0;
}
