#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_verify.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/obj_mac.h>
#include <string.h>

static int ed25519_public_key(
    const uint8_t private_key[32], uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int secp256k1_key_pair(
    uint8_t value, uint8_t private_key[32], uint8_t public_key[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value = BN_new();
    const EC_GROUP *group = key == NULL ? NULL : EC_KEY_get0_group(key);
    EC_POINT *point = group == NULL ? NULL : EC_POINT_new(group);
    size_t public_length = 0U;
    (void)memset(private_key, 0, 32U);
    private_key[31] = value;
    if (key != NULL && private_value != NULL && point != NULL &&
        BN_bin2bn(private_key, 32, private_value) != NULL &&
        EC_POINT_mul(group, point, private_value, NULL, NULL, NULL) == 1 &&
        EC_KEY_set_private_key(key, private_value) == 1 &&
        EC_KEY_set_public_key(key, point) == 1)
        public_length = EC_POINT_point2oct(
            group, point, POINT_CONVERSION_COMPRESSED,
            public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return public_length == 33U ? 0 : 1;
}

int main(void)
{
    static const uint8_t sequencer_private_key[32] = {3U};
    static const uint8_t canonical_activity[] = {1U, 2U, 3U, 4U};
    static const uint8_t other_activity[] = {5U, 6U};
    static const uint8_t state_leaf[] = {0xa1U, 0x20U, 0x19U};
    static const uint8_t other_state_leaf[] = {0xa2U, 0x20U, 0x07U};
    static const uint8_t paxeer_reference[] = {
        0x50U, 0x41U, 0x58U, 0x45U, 0x45U, 0x52U, 0x2dU, 0x31U
    };
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 65536U];
    static uint8_t receipt_bytes[LXP_MAX_ACTIVITY_BYTES];
    lxp_arena arena;
    uint8_t sequencer_public_key[32];
    uint8_t activity_hashes[2][32];
    uint8_t state_hashes[2][32];
    uint8_t activity_root[32];
    uint8_t state_root[32];
    lxp_merkle_proof activity_proof;
    lxp_merkle_proof state_proof;
    lxp_ledger_receipt_input input;
    lxp_receipt receipt;
    lxp_receipt original;
    lxp_byte_span encoded;
    lxp_payment_requirement requirement;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantors[3];
    lxp_guarantor_attestation attestations[3];
    lxp_guarantor_key_record keys[3];
    lxp_guarantor_cert certificate;
    lxp_augmented_receipt augmented;
    uint8_t checkpoint_id[32];
    uint8_t altered_reference[sizeof(paxeer_reference)];
    size_t receipt_length;
    size_t mark;
    size_t i;

    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        ed25519_public_key(
            sequencer_private_key, sequencer_public_key) != 0 ||
        lxp_merkle_leaf_hash(
            canonical_activity, sizeof(canonical_activity),
            activity_hashes[0]) != LXP_OK ||
        lxp_merkle_leaf_hash(
            other_activity, sizeof(other_activity),
            activity_hashes[1]) != LXP_OK ||
        lxp_merkle_proof_generate(
            (const uint8_t (*)[32])activity_hashes, 2U, 0U, &arena,
            &activity_proof, activity_root) != LXP_OK ||
        lxp_merkle_leaf_hash(
            state_leaf, sizeof(state_leaf), state_hashes[0]) != LXP_OK ||
        lxp_merkle_leaf_hash(
            other_state_leaf, sizeof(other_state_leaf),
            state_hashes[1]) != LXP_OK ||
        lxp_merkle_proof_generate(
            (const uint8_t (*)[32])state_hashes, 2U, 0U, &arena,
            &state_proof, state_root) != LXP_OK)
        return 1;

    (void)memset(&input, 0, sizeof(input));
    if (lxp_hash_activity_id(
            canonical_activity, sizeof(canonical_activity),
            input.transaction_id) != LXP_OK)
        return 1;
    input.operation = (uint8_t)LX_ASSET_SEND;
    input.global_sequence = 9U;
    input.asset[0] = 4U;
    input.amount = (lxp_u128){0U, 25U};
    input.from[0] = 5U;
    input.from_balance_before = (lxp_u128){0U, 100U};
    input.from_balance_after = (lxp_u128){0U, 75U};
    input.to[0] = 6U;
    input.to_balance_before = (lxp_u128){0U, 10U};
    input.to_balance_after = (lxp_u128){0U, 35U};
    input.transfer_set_root[0] = 7U;
    input.authorization_hash[0] = 8U;
    input.context_hash[0] = 9U;
    input.previous_state_root[0] = 10U;
    (void)memcpy(input.resulting_state_root, state_root, 32U);
    input.batch_id[0] = 11U;
    input.timestamp = 1000U;
    input.leg_count = 1U;
    if (lxp_ledger_receipt_build(&receipt, &input) != LXP_OK ||
        lxp_receipt_sign(
            &receipt, sequencer_private_key, &arena) != LXP_OK ||
        lxp_receipt_verify_offline(
            &receipt, sequencer_public_key, &arena) != LXP_OK)
        return 1;
    original = receipt;
    mark = lxp_arena_mark(&arena);
    if (lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(receipt_bytes))
        return 1;
    receipt_length = encoded.length;
    (void)memcpy(receipt_bytes, encoded.bytes, receipt_length);
    if (lxp_arena_reset(&arena, mark) != LXP_OK) return 1;

    (void)memset(&requirement, 0, sizeof(requirement));
    requirement.network_id = 42U;
    (void)memcpy(requirement.recipient, receipt.to, 32U);
    (void)memcpy(requirement.asset, receipt.asset, 32U);
    requirement.amount = receipt.amount;
    requirement.invoice_id[0] = 12U;
    (void)memcpy(requirement.purpose_hash, receipt.context_hash, 32U);
    requirement.expiry = 2000U;
    requirement.acceptable_conditions = 1U;
    if (lxp_verify_receipt_against_requirement(
            &receipt, &requirement, 42U, sequencer_public_key,
            &arena) != LXP_OK ||
        lxp_receipt_match_requirement(
            &receipt, &requirement, 43U) != LXP_ERR_WRONG_NETWORK)
        return 1;

    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = LXP_PROTOCOL_VERSION;
    checkpoint.header.network_id = 42U;
    checkpoint.header.epoch = 2U;
    checkpoint.header.batch_number = 3U;
    checkpoint.header.first_sequence = 9U;
    checkpoint.header.last_sequence = 9U;
    checkpoint.header.previous_state_root[0] = 10U;
    (void)memcpy(checkpoint.header.resulting_state_root, state_root, 32U);
    (void)memcpy(checkpoint.header.activity_merkle_root, activity_root, 32U);
    checkpoint.header.receipt_merkle_root[0] = 13U;
    checkpoint.header.event_merkle_root[0] = 14U;
    checkpoint.header.data_availability_root[0] = 15U;
    checkpoint.header.oracle_root[0] = 16U;
    checkpoint.header.timestamp_ms = 1000U;
    checkpoint.header.sequencer_id[0] = 17U;
    for (i = 0U; i < 3U; ++i) {
        (void)memset(&guarantors[i], 0, sizeof(guarantors[i]));
        guarantors[i].guarantor_id[0] = (uint8_t)(i + 1U);
        guarantors[i].ready_to_sign = true;
        guarantors[i].possesses_availability = true;
        guarantors[i].bond_view.bonded = true;
        guarantors[i].protocol_version = LXP_PROTOCOL_VERSION;
        guarantors[i].network_id = 42U;
        guarantors[i].paxeer_chain_id = 31337U;
        guarantors[i].paxeer_settlement_contract[0] = 0xa1U;
        if (secp256k1_key_pair(
                (uint8_t)(i + 1U), guarantors[i].paxeer_private_key,
                guarantors[i].paxeer_public_key) != 0)
            return 1;
        (void)memcpy(keys[i].guarantor_id,
                     guarantors[i].guarantor_id, 32U);
        (void)memcpy(keys[i].public_key,
                     guarantors[i].paxeer_public_key, 33U);
        keys[i].bonded = true;
        if (lxp_guarantor_attest(
                &guarantors[i], &checkpoint, true, true,
                2000U + i, &arena, &attestations[i]) != LXP_OK)
            return 1;
    }
    if (lxp_guarantor_cert_assemble(
            &checkpoint, attestations, 3U, 2U, &certificate) != LXP_OK ||
        lxp_checkpoint_certificate_hash(
            &checkpoint, &arena, checkpoint_id) != LXP_OK ||
        lxp_receipt_augment(
            (lxp_byte_span){receipt_bytes, receipt_length},
            (lxp_byte_span){canonical_activity, sizeof(canonical_activity)},
            (lxp_byte_span){state_leaf, sizeof(state_leaf)},
            &activity_proof, &state_proof, &certificate,
            (lxp_byte_span){paxeer_reference, sizeof(paxeer_reference)},
            &augmented) != LXP_OK ||
        lxp_receipt_verify_checkpointed(
            &receipt, &augmented, keys, 3U, checkpoint_id,
            (lxp_byte_span){paxeer_reference, sizeof(paxeer_reference)},
            &arena) != LXP_OK ||
        memcmp(&receipt, &original, sizeof(receipt)) != 0)
        return 1;
    (void)memcpy(altered_reference, paxeer_reference,
                 sizeof(paxeer_reference));
    altered_reference[0] ^= 1U;
    if (lxp_receipt_verify_checkpointed(
            &receipt, &augmented, keys, 3U, checkpoint_id,
            (lxp_byte_span){altered_reference, sizeof(altered_reference)},
            &arena) != LXP_ERR_ROOT_MISMATCH)
        return 1;
    receipt.amount.lo = 24U;
    return lxp_receipt_verify_offline(
        &receipt, sequencer_public_key, &arena) == LXP_ERR_BAD_SIGNATURE ?
        0 : 1;
}
