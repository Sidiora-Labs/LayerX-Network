#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_state.h"

#include <openssl/evp.h>
#include <stdlib.h>
#include <string.h>

static lxp_result signer_public_key(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key;
    size_t length = 32U;
    int valid;
    if (private_key == NULL || public_key == NULL ||
        lxp_ct_is_zero(private_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    valid = key != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &length) == 1 &&
        length == 32U;
    EVP_PKEY_free(key);
    return valid ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

static lxp_result sign_manifest(
    const uint8_t private_key[32], const uint8_t *bytes, size_t length,
    uint8_t signature[64])
{
    EVP_PKEY *key;
    EVP_MD_CTX *context;
    size_t signature_length = 64U;
    int valid;
    if (private_key == NULL || (bytes == NULL && length != 0U) ||
        signature == NULL)
        return LXP_ERR_NON_CANONICAL;
    key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    context = EVP_MD_CTX_new();
    valid = key != NULL && context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length,
                       bytes, length) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return valid ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

static lxp_result materialize_snapshot(
    const lxp_genesis_manifest *manifest, lxp_arena *arena,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *snapshot)
{
    lxp_state_store *state = NULL;
    lxp_state_journal *journal = NULL;
    lxp_kernel *kernel = NULL;
    lx_account_registry *accounts = NULL;
    lxp_genesis_module_plan plan;
    uint8_t canonical_root[32];
    uint8_t receipt_root[32];
    bool state_open = false;
    lxp_result status;
    state = (lxp_state_store *)malloc(sizeof(*state));
    journal = (lxp_state_journal *)calloc(1U, sizeof(*journal));
    kernel = (lxp_kernel *)malloc(sizeof(*kernel));
    accounts = (lx_account_registry *)malloc(sizeof(*accounts));
    if (state == NULL || journal == NULL || kernel == NULL || accounts == NULL) {
        status = LXP_ERR_IO;
        goto done;
    }
    status = lx_account_registry_init(accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(state, 1U);
        state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(state, accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(kernel, state, journal, manifest, 1U);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_resolve(manifest, &plan);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_register(&plan, kernel);
    if (status == LXP_OK)
        status = lxp_genesis_materialize(manifest, arena, kernel);
    if (status == LXP_OK) status = lxp_state_root(kernel, canonical_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            canonical_root, manifest->genesis_state_root, 32U) != 0)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_genesis_receipt_state_root(
            manifest->network_id, canonical_root, receipt_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            receipt_root, manifest->genesis_receipt_state_root, 32U) != 0)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        (void)memcpy(kernel->current_state_root, receipt_root, 32U);
    if (status == LXP_OK)
        status = lxp_snapshot_write(kernel, 0U, arena, snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest_build(
            snapshot->bytes, snapshot->length, 0U, canonical_root,
            receipt_root, snapshot_manifest);
done:
    if (state_open) {
        lxp_result close_status = lxp_state_store_destroy(state);
        if (status == LXP_OK && close_status != LXP_OK) status = close_status;
    }
    lx_account_registry_release(accounts);
    if (accounts != NULL) lxp_secure_zero(accounts, sizeof(*accounts));
    if (kernel != NULL) lxp_secure_zero(kernel, sizeof(*kernel));
    if (journal != NULL) lxp_secure_zero(journal, sizeof(*journal));
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
    return status;
}

static lxp_result build_fresh(
    const lxp_genesis_manifest *draft, const uint8_t asset_id[32],
    const lx_programs_metering_schedule *metering,
    const lx_programs_fee_genesis_parameters *fees,
    const lxp_bridge_profile *profile,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_genesis_manifest *signed_manifest,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *encoded_manifest, lxp_byte_span *snapshot)
{
    lxp_genesis_manifest *candidate;
    lx_programs_metering_schedule prepared_metering;
    lxp_byte_span signing_preimage;
    size_t mark;
    lxp_result status;
    if (draft == NULL || asset_id == NULL || metering == NULL ||
        fees == NULL || signer_private_key == NULL || arena == NULL ||
        signed_manifest == NULL || snapshot_manifest == NULL ||
        encoded_manifest == NULL || snapshot == NULL ||
        (draft->protocol_version != LXP_PROTOCOL_VERSION &&
         draft->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT) ||
        draft->account_count != 0U || draft->module_value_count > LX_ASSET_REGISTRY_CAPACITY + 1U ||
        !lxp_ct_is_zero(draft->genesis_state_root, 32U) ||
        !lxp_ct_is_zero(draft->genesis_receipt_state_root, 32U) ||
        !lxp_ct_is_zero(draft->signer_public_key, 32U) ||
        !lxp_ct_is_zero(draft->signature, 64U))
        return LXP_ERR_NON_CANONICAL;
    if (draft->module_value_count != 0U) {
        bool asset_present = false, schedule_present = false;
        for (size_t i = 0U; i < draft->module_value_count; ++i) {
            const lxp_genesis_module_value *value = &draft->module_values[i];
            if (value->module_id == LXP_MODULE_ASSET) {
                lx_asset_record record;
                if (lx_asset_record_decode(value->value, value->value_length, &record) != LXP_OK ||
                    memcmp(record.asset_id, value->key, 32U) != 0 || record.issuer_kind == 1U ||
                    !lxp_u128_is_zero(record.total_units)) return LXP_ERR_NON_CANONICAL;
                if (memcmp(record.asset_id, asset_id, 32U) == 0) asset_present = true;
            } else if (value->module_id == LXP_MODULE_GOVERNANCE &&
                memcmp(value->key, "fee.schedule", 12U) == 0 && lxp_ct_is_zero(value->key + 12U, 20U)) {
                lxp_fee_params schedule;
                if (schedule_present || lxp_fee_params_decode(value->value, value->value_length, &schedule) != LXP_OK ||
                    (schedule.version != 2U && schedule.version != 3U)) return LXP_ERR_NON_CANONICAL;
                schedule_present = true;
            } else return LXP_ERR_NON_CANONICAL;
        }
        if (!asset_present || !schedule_present) return LXP_ERR_ASSET_MISMATCH;
    }
    candidate = (lxp_genesis_manifest *)malloc(sizeof(*candidate));
    if (candidate == NULL) return LXP_ERR_IO;
    *candidate = *draft;
    prepared_metering = *metering;
    (void)memset(snapshot_manifest, 0, sizeof(*snapshot_manifest));
    *encoded_manifest = (lxp_byte_span){NULL, 0U};
    *snapshot = (lxp_byte_span){NULL, 0U};
    mark = lxp_arena_mark(arena);
    status = signer_public_key(signer_private_key,
                               candidate->signer_public_key);
    if (status == LXP_OK &&
        lxp_ct_is_zero(prepared_metering.authority_digest, 32U))
        status = lxp_hash_payload(candidate->signer_public_key, 32U,
                                  prepared_metering.authority_digest);
    if (status == LXP_OK)
        status = lxp_genesis_fresh_empty_accounts(candidate, asset_id);
    if (status == LXP_OK)
        status = lxp_programs_metering_genesis_append(candidate,
                                                       &prepared_metering);
    if (status == LXP_OK)
        status = lxp_programs_fee_genesis_append(candidate, fees);
    if (status == LXP_OK && profile != NULL)
        status = lxp_bridge_genesis_append(candidate, profile);
    if (status == LXP_OK)
        status = lxp_genesis_state_root(
            candidate, arena, candidate->genesis_state_root);
    if (status == LXP_OK)
        status = lxp_genesis_receipt_state_root(
            candidate->network_id, candidate->genesis_state_root,
            candidate->genesis_receipt_state_root);
    if (status == LXP_OK)
        status = lxp_genesis_encode(candidate, false, arena,
                                    &signing_preimage);
    if (status == LXP_OK)
        status = sign_manifest(signer_private_key, signing_preimage.bytes,
                               signing_preimage.length,
                               candidate->signature);
    if (lxp_arena_reset(arena, mark) != LXP_OK && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = materialize_snapshot(candidate, arena, snapshot_manifest,
                                      snapshot);
    if (status == LXP_OK)
        status = lxp_genesis_encode(candidate, true, arena,
                                    encoded_manifest);
    if (status == LXP_OK)
        status = lxp_genesis_verify_signature(candidate, arena);
    if (status == LXP_OK) {
        *signed_manifest = *candidate;
    } else {
        (void)lxp_arena_reset(arena, mark);
        (void)memset(snapshot_manifest, 0, sizeof(*snapshot_manifest));
        *encoded_manifest = (lxp_byte_span){NULL, 0U};
        *snapshot = (lxp_byte_span){NULL, 0U};
    }
    lxp_secure_zero(candidate, sizeof(*candidate));
    lxp_secure_zero(&prepared_metering, sizeof(prepared_metering));
    free(candidate);
    return status;
}

lxp_result lxp_genesis_build_fresh_empty(
    const lxp_genesis_manifest *draft, const uint8_t asset_id[32],
    const lx_programs_metering_schedule *metering,
    const lx_programs_fee_genesis_parameters *fees,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_genesis_manifest *signed_manifest,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *encoded_manifest, lxp_byte_span *snapshot)
{
    return build_fresh(draft, asset_id, metering, fees, NULL, signer_private_key,
                        arena, signed_manifest, snapshot_manifest, encoded_manifest, snapshot);
}

lxp_result lxp_genesis_build_fresh_custody(
    const lxp_genesis_manifest *draft, const uint8_t asset_id[32],
    const lx_programs_metering_schedule *metering,
    const lx_programs_fee_genesis_parameters *fees,
    const lxp_bridge_profile *profile,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_genesis_manifest *signed_manifest,
    lxp_snapshot_manifest_record *snapshot_manifest,
    lxp_byte_span *encoded_manifest, lxp_byte_span *snapshot)
{
    if (lxp_bridge_profile_validate(profile) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    return build_fresh(draft, asset_id, metering, fees, profile, signer_private_key,
                        arena, signed_manifest, snapshot_manifest, encoded_manifest, snapshot);
}

lxp_result lxp_genesis_build_snapshot_migration(
    const lxp_genesis_manifest *genesis,
    const lxp_snapshot_manifest_record *source_manifest,
    const uint8_t *source_snapshot, size_t source_snapshot_length,
    const uint8_t signer_private_key[32], lxp_arena *arena,
    lxp_snapshot_manifest_record *target_manifest,
    lxp_byte_span *target_snapshot)
{
    lxp_state_store *state = NULL;
    lxp_state_journal *journal = NULL;
    lxp_kernel *kernel = NULL;
    lx_account_registry *accounts = NULL;
    uint8_t signer[32];
    uint8_t authorization[LXP_SNAPSHOT_MIGRATION_AUTHORIZATION_BYTES];
    size_t authorization_length = 0U;
    size_t renamed = 0U;
    bool state_open = false;
    lxp_result status;
    if (genesis == NULL || source_manifest == NULL ||
        source_snapshot == NULL || source_snapshot_length == 0U ||
        signer_private_key == NULL || arena == NULL ||
        target_manifest == NULL || target_snapshot == NULL ||
        source_manifest->global_sequence == 0U ||
        source_manifest->migration.present)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(target_manifest, 0, sizeof(*target_manifest));
    *target_snapshot = (lxp_byte_span){NULL, 0U};
    status = lxp_genesis_verify_signature(genesis, arena);
    if (status == LXP_OK) status = signer_public_key(signer_private_key, signer);
    if (status == LXP_OK && lxp_ct_memcmp(
            signer, genesis->signer_public_key, 32U) != 0)
        status = LXP_ERR_BAD_SIGNATURE;
    state = (lxp_state_store *)malloc(sizeof(*state));
    journal = (lxp_state_journal *)calloc(1U, sizeof(*journal));
    kernel = (lxp_kernel *)malloc(sizeof(*kernel));
    accounts = (lx_account_registry *)malloc(sizeof(*accounts));
    if (status == LXP_OK &&
        (state == NULL || journal == NULL || kernel == NULL ||
         accounts == NULL))
        status = LXP_ERR_IO;
    if (status == LXP_OK) status = lx_account_registry_init(accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(state, 1U);
        state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(state, accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(kernel, state, journal, genesis, 1U);
    if (status == LXP_OK)
        status = lxp_kernel_register_module(
            kernel, programs_module_registration_v4());
    if (status == LXP_OK &&
        genesis->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = lxp_kernel_register_module(kernel, lx_asset_module_iface());
    if (status == LXP_OK) {
        lxp_bridge_profile bridge;
        bool present = false;
        status = lxp_bridge_genesis_profile(genesis, &bridge, &present);
        if (status == LXP_OK && present)
            status = lxp_kernel_register_module(
                kernel, lxp_bridge_module_iface());
    }
    if (status == LXP_OK)
        status = lxp_snapshot_load_retired_issuance(
            source_snapshot, source_snapshot_length, source_manifest, kernel,
            &renamed);
    if (status == LXP_OK && (renamed == 0U || renamed > UINT16_MAX))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = lxp_state_root(kernel, target_manifest->canonical_state_root);
    if (status == LXP_OK)
        status = lxp_snapshot_migration_receipt_root(
            genesis->network_id, source_manifest->global_sequence,
            source_manifest->canonical_state_root,
            source_manifest->receipt_state_root,
            target_manifest->canonical_state_root,
            target_manifest->receipt_state_root);
    if (status == LXP_OK) {
        target_manifest->global_sequence = source_manifest->global_sequence;
        (void)memcpy(kernel->current_state_root,
                     target_manifest->receipt_state_root, 32U);
        status = lxp_snapshot_write(
            kernel, target_manifest->global_sequence, arena, target_snapshot);
    }
    if (status == LXP_OK) {
        uint8_t canonical[32];
        uint8_t receipt[32];
        (void)memcpy(canonical, target_manifest->canonical_state_root, 32U);
        (void)memcpy(receipt, target_manifest->receipt_state_root, 32U);
        status = lxp_snapshot_manifest_build(
            target_snapshot->bytes, target_snapshot->length,
            source_manifest->global_sequence, canonical, receipt,
            target_manifest);
    }
    if (status == LXP_OK) {
        lxp_snapshot_migration_authorization *migration =
            &target_manifest->migration;
        migration->present = true;
        migration->network_id = genesis->network_id;
        migration->renamed_account_count = (uint16_t)renamed;
        migration->source_global_sequence = source_manifest->global_sequence;
        (void)memcpy(migration->source_canonical_state_root,
                     source_manifest->canonical_state_root, 32U);
        (void)memcpy(migration->source_receipt_state_root,
                     source_manifest->receipt_state_root, 32U);
        (void)memcpy(migration->source_snapshot_digest,
                     source_manifest->snapshot_digest, 32U);
        (void)memcpy(migration->signer_public_key, signer, 32U);
        status = lxp_snapshot_migration_authorization_encode(
            target_manifest, false, authorization, &authorization_length);
    }
    if (status == LXP_OK)
        status = sign_manifest(
            signer_private_key, authorization, authorization_length,
            target_manifest->migration.signature);
    if (status == LXP_OK)
        status = lxp_snapshot_migration_authorization_verify(
            target_manifest, genesis->network_id,
            genesis->signer_public_key);
    while (kernel != NULL && kernel->blob_count != 0U)
        free(kernel->blobs[--kernel->blob_count].bytes);
    if (state_open) {
        lxp_result close_status = lxp_state_store_destroy(state);
        if (status == LXP_OK && close_status != LXP_OK) status = close_status;
    }
    lxp_secure_zero(signer, sizeof(signer));
    lxp_secure_zero(authorization, sizeof(authorization));
    lx_account_registry_release(accounts);
    if (accounts != NULL) lxp_secure_zero(accounts, sizeof(*accounts));
    if (kernel != NULL) lxp_secure_zero(kernel, sizeof(*kernel));
    if (journal != NULL) lxp_secure_zero(journal, sizeof(*journal));
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
    if (status != LXP_OK) {
        (void)memset(target_manifest, 0, sizeof(*target_manifest));
        *target_snapshot = (lxp_byte_span){NULL, 0U};
    }
    return status;
}
