#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_state.h"

#include <openssl/evp.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) return 1; } while (0)

static int public_key_for(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int valid = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return valid ? 0 : 1;
}

static void draft_manifest(lxp_genesis_manifest *draft)
{
    static const uint8_t parameter_key[32] = {
        'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
    };
    (void)memset(draft, 0, sizeof(*draft));
    draft->protocol_version = LXP_PROTOCOL_VERSION;
    draft->network_id = 42U;
    draft->genesis_timestamp_ms = UINT64_C(1700000000000);
    draft->parameter_count = 1U;
    draft->parameters[0].module_id = LXP_MODULE_GOVERNANCE;
    (void)memcpy(draft->parameters[0].key, parameter_key,
                 sizeof(parameter_key));
    draft->parameters[0].value[31] = 1U;
    draft->guarantor_count = 1U;
    draft->guarantors[0].guarantor_id[0] = 1U;
    draft->guarantors[0].public_key[0] = 2U;
    draft->guarantors[0].public_key[32] = 3U;
    draft->guarantors[0].bond = (lxp_u128){0U, 0U};
}

static void programs_parameters(
    const uint8_t signer_public_key[32], const uint8_t asset_id[32],
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
    (void)lxp_hash_payload(signer_public_key, 32U,
                           metering->authority_digest);
    (void)memset(fees, 0, sizeof(*fees));
    fees->schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 100U
    };
    (void)memcpy(fees->occupancy_asset_id, asset_id, 32U);
    fees->target_occupancy_byte_batches = 100U;
    fees->response_denominator = 1U;
    fees->maximum_change_numerator = 1U;
    fees->maximum_change_denominator = 10U;
    fees->minimum_fee_units_per_occupancy_byte_batch = 1U;
    fees->maximum_fee_units_per_occupancy_byte_batch = 1000U;
}

static int zero_supply_accounts(const lxp_genesis_manifest *manifest,
                                const uint8_t asset_id[32])
{
    bool has_fees = false;
    bool has_reserve = false;
    bool has_withdrawals = false;
    size_t index;
    if (manifest->account_count != LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT)
        return 1;
    for (index = 0U; index < manifest->account_count; ++index) {
        const lxp_genesis_account *account = &manifest->accounts[index];
        if (memcmp(account->asset_id, asset_id, 32U) != 0 ||
            !lxp_u128_is_zero(account->balance) || account->locked ||
            !lxp_ct_is_zero(account->parent_account_id, 32U))
            return 1;
        if (account->subaccount_kind == LX_ACCOUNT_SYSTEM_FEES)
            has_fees = true;
        else if (account->subaccount_kind ==
                 LX_ACCOUNT_SYSTEM_PAXEER_RESERVE)
            has_reserve = true;
        else if (account->subaccount_kind ==
                 LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS)
            has_withdrawals = true;
        else
            return 1;
    }
    return has_fees && has_reserve && has_withdrawals ? 0 : 1;
}

int main(void)
{
    static const uint8_t signer_private_key[32] = {7U};
    static uint8_t arena_bytes[8388608U];
    static lxp_genesis_manifest draft;
    static lxp_genesis_manifest manifest;
    static lxp_genesis_manifest decoded;
    static lxp_genesis_manifest changed;
    uint8_t signer_public_key[32];
    uint8_t asset_id[32] = {0x85U};
    uint8_t registration_bytes[LXP_GENESIS_REGISTRATION_BYTES];
    uint8_t expected_receipt_root[32];
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_snapshot_manifest_record changed_snapshot;
    lxp_genesis_bootstrap_registration registration;
    lxp_genesis_bootstrap_registration decoded_registration;
    lx_programs_metering_schedule metering;
    lx_programs_fee_genesis_parameters fee_parameters;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lx_account_registry accounts;
    lxp_byte_span encoded_manifest;
    lxp_byte_span snapshot;
    lxp_arena arena;
    bool enabled = false;

    REQUIRE(public_key_for(signer_private_key, signer_public_key) == 0);
    draft_manifest(&draft);
    programs_parameters(signer_public_key, asset_id, &metering,
                        &fee_parameters);
    REQUIRE(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) == LXP_OK);
    REQUIRE(lxp_genesis_build_fresh_empty(
        &draft, asset_id, &metering, &fee_parameters, signer_private_key,
        &arena, &manifest, &snapshot_manifest, &encoded_manifest,
        &snapshot) == LXP_OK);
    REQUIRE(zero_supply_accounts(&manifest, asset_id) == 0);
    REQUIRE(!lxp_ct_is_zero(manifest.genesis_state_root, 32U));
    REQUIRE(!lxp_ct_is_zero(manifest.signature, 64U));
    REQUIRE(memcmp(manifest.signer_public_key, signer_public_key, 32U) == 0);
    REQUIRE(lxp_genesis_receipt_state_root(
        manifest.network_id, manifest.genesis_state_root,
        expected_receipt_root) == LXP_OK);
    REQUIRE(memcmp(manifest.genesis_receipt_state_root,
                   expected_receipt_root, 32U) == 0);
    REQUIRE(memcmp(snapshot_manifest.canonical_state_root,
                   manifest.genesis_state_root, 32U) == 0);
    REQUIRE(memcmp(snapshot_manifest.receipt_state_root,
                   expected_receipt_root, 32U) == 0);
    REQUIRE(memcmp(snapshot_manifest.canonical_state_root,
                   snapshot_manifest.receipt_state_root, 32U) != 0);
    REQUIRE(lxp_genesis_parse(encoded_manifest.bytes, encoded_manifest.length,
                              LXP_GENESIS_INPUT_MANIFEST, &decoded) == LXP_OK);
    REQUIRE(memcmp(&decoded, &manifest, sizeof(manifest)) == 0);
    REQUIRE(lxp_genesis_verify_signature(&decoded, &arena) == LXP_OK);
    REQUIRE(lxp_programs_metering_genesis_validate(&decoded) == LXP_OK);
    REQUIRE(lxp_programs_fee_genesis_validate(&decoded) == LXP_OK);

    REQUIRE(lx_account_registry_init(&accounts) == LXP_OK);
    REQUIRE(lxp_state_store_init(&state, 1U) == LXP_OK);
    REQUIRE(lxp_state_store_bind_accounts(&state, &accounts) == LXP_OK);
    REQUIRE(lxp_kernel_create(&kernel, &state, &journal, &manifest, 1U) ==
            LXP_OK);
    REQUIRE(lxp_kernel_register_module(
        &kernel, programs_module_registration_v4()) == LXP_OK);
    REQUIRE(lxp_snapshot_load(snapshot.bytes, snapshot.length,
                              &snapshot_manifest, &kernel) == LXP_OK);
    REQUIRE(accounts.count == LXP_GENESIS_FRESH_SYSTEM_ACCOUNT_COUNT);
    REQUIRE(memcmp(kernel.current_state_root,
                   snapshot_manifest.receipt_state_root, 32U) == 0);

    (void)memset(&registration, 0, sizeof(registration));
    registration.network_id = manifest.network_id;
    (void)memcpy(registration.settlement_anchor,
                 manifest.genesis_receipt_state_root, 32U);
    (void)memcpy(registration.state_root,
                 manifest.genesis_receipt_state_root, 32U);
    registration.finalised = true;
    REQUIRE(lxp_genesis_registration_encode(
        &registration, registration_bytes) == LXP_OK);
    REQUIRE(lxp_genesis_registration_parse(
        registration_bytes, sizeof(registration_bytes),
        &decoded_registration) == LXP_OK);
    REQUIRE(memcmp(&decoded_registration, &registration,
                   sizeof(registration)) == 0);
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 42U, true, &snapshot_manifest,
        &kernel, &arena, &enabled) == LXP_OK && enabled);

    enabled = true;
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 42U, false, &snapshot_manifest,
        &kernel, &arena, &enabled) != LXP_OK && !enabled);
    enabled = true;
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 43U, true, &snapshot_manifest,
        &kernel, &arena, &enabled) != LXP_OK && !enabled);
    changed_snapshot = snapshot_manifest;
    changed_snapshot.receipt_state_root[0] ^= 1U;
    enabled = true;
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 42U, true, &changed_snapshot,
        &kernel, &arena, &enabled) != LXP_OK && !enabled);
    changed_snapshot = snapshot_manifest;
    changed_snapshot.canonical_state_root[0] ^= 1U;
    enabled = true;
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 42U, true, &changed_snapshot,
        &kernel, &arena, &enabled) != LXP_OK && !enabled);

    changed = manifest;
    changed.parameters[0].value[31] = 2U;
    REQUIRE(lxp_genesis_verify_signature(&changed, &arena) != LXP_OK);
    changed = manifest;
    changed.module_values[0].value[0] ^= 1U;
    REQUIRE(lxp_genesis_verify_signature(&changed, &arena) != LXP_OK);
    changed = manifest;
    changed.accounts[0].balance.lo = 1U;
    REQUIRE(lxp_genesis_verify_signature(&changed, &arena) != LXP_OK);
    decoded_registration.network_id = 43U;
    enabled = true;
    REQUIRE(lxp_genesis_bootstrap_verify(
        &manifest, &decoded_registration, 42U, true, &snapshot_manifest,
        &kernel, &arena, &enabled) != LXP_OK && !enabled);
    registration_bytes[81] = 0U;
    REQUIRE(lxp_genesis_registration_parse(
        registration_bytes, sizeof(registration_bytes),
        &decoded_registration) != LXP_OK);
    REQUIRE(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}
