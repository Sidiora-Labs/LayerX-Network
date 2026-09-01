#include "layerx/lxp_genesis_builder.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
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
        status = lxp_kernel_register_module(
            kernel, programs_module_registration_v4());
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
    if (accounts != NULL) lxp_secure_zero(accounts, sizeof(*accounts));
    if (kernel != NULL) lxp_secure_zero(kernel, sizeof(*kernel));
    if (journal != NULL) lxp_secure_zero(journal, sizeof(*journal));
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
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
    lxp_genesis_manifest *candidate;
    lx_programs_metering_schedule prepared_metering;
    lxp_byte_span signing_preimage;
    size_t mark;
    lxp_result status;
    if (draft == NULL || asset_id == NULL || metering == NULL ||
        fees == NULL || signer_private_key == NULL || arena == NULL ||
        signed_manifest == NULL || snapshot_manifest == NULL ||
        encoded_manifest == NULL || snapshot == NULL ||
        draft->protocol_version != LXP_PROTOCOL_VERSION ||
        draft->account_count != 0U || draft->module_value_count != 0U ||
        !lxp_ct_is_zero(draft->genesis_state_root, 32U) ||
        !lxp_ct_is_zero(draft->genesis_receipt_state_root, 32U) ||
        !lxp_ct_is_zero(draft->signer_public_key, 32U) ||
        !lxp_ct_is_zero(draft->signature, 64U))
        return LXP_ERR_NON_CANONICAL;
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
