#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_guarantor.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

static int key_pair(uint8_t value, uint8_t private_key[32],
                    uint8_t public_key[33])
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
        public_length = EC_POINT_point2oct(group, point,
            POINT_CONVERSION_COMPRESSED, public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return public_length == 33U ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[131072];
    uint8_t original_receipt[] = {1U, 4U, 9U, 16U};
    uint8_t original_copy[sizeof(original_receipt)];
    uint8_t settlement[] = {0xaaU, 0xbbU, 0xccU};
    uint8_t activity[] = {0x61U};
    uint8_t state_leaf[] = {0x73U};
    uint8_t validity[] = {0x50U, 0x52U, 0x4fU, 0x4fU, 0x46U};
    lxp_arena arena;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantors[3];
    lxp_guarantor_attestation attestations[3];
    lxp_guarantor_attestation shuffled[3];
    lxp_guarantor_key_record keys[3];
    lxp_guarantor_cert certificate;
    lxp_guarantor_cert proof_certificate;
    lxp_merkle_proof activity_proof;
    lxp_merkle_proof state_proof;
    lxp_augmented_receipt augmented;
    size_t valid = 0U;
    size_t i;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK)
        return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = 1U;
    checkpoint.header.network_id = 42U;
    checkpoint.header.epoch = 7U;
    checkpoint.header.batch_number = 8U;
    checkpoint.header.first_sequence = 11U;
    checkpoint.header.last_sequence = 19U;
    checkpoint.header.resulting_state_root[0] = 1U;
    checkpoint.header.data_availability_root[0] = 2U;
    for (i = 0U; i < 3U; ++i) {
        (void)memset(&guarantors[i], 0, sizeof(guarantors[i]));
        guarantors[i].guarantor_id[0] = (uint8_t)(3U - i);
        guarantors[i].ready_to_sign = true;
        guarantors[i].possesses_availability = true;
        guarantors[i].bond_view.bonded = true;
        if (key_pair((uint8_t)(i + 1U), guarantors[i].paxeer_private_key,
                     guarantors[i].paxeer_public_key) != 0)
            return 1;
        (void)memcpy(keys[i].guarantor_id, guarantors[i].guarantor_id, 32U);
        (void)memcpy(keys[i].public_key, guarantors[i].paxeer_public_key, 33U);
        keys[i].bonded = true;
        if (lxp_guarantor_attest(&guarantors[i], &checkpoint, true, true,
                                 1000U + i, &arena,
                                 &attestations[i]) != LXP_OK)
            return 1;
    }
    if (lxp_guarantor_attest(&guarantors[0], &checkpoint, false, true,
                             1000U, &arena, &shuffled[0]) !=
            LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;
    shuffled[0] = attestations[1];
    shuffled[1] = attestations[0];
    shuffled[2] = attestations[2];
    if (lxp_guarantor_cert_assemble(&checkpoint, shuffled, 3U, 2U,
                                    &certificate) != LXP_OK ||
        !certificate.bonded_economic_guarantee ||
        certificate.validity_proof_present ||
        certificate.attestations[0].guarantor_id[0] != 1U ||
        certificate.attestations[1].guarantor_id[0] != 2U ||
        certificate.attestations[2].guarantor_id[0] != 3U ||
        lxp_guarantor_cert_verify(&certificate, keys, 3U, &arena, &valid) !=
            LXP_OK || valid != 3U)
        return 1;
    keys[1].bonded = false;
    keys[2].bonded = false;
    if (lxp_guarantor_cert_verify(&certificate, keys, 3U, &arena, &valid) !=
            LXP_ERR_ATTESTATION_THRESHOLD || valid != 1U)
        return 1;
    certificate.attestation_count = LXP_MAX_GUARANTOR_ATTESTATIONS + 1U;
    if (lxp_guarantor_cert_verify(&certificate, keys, 3U, &arena, &valid) !=
            LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;
    certificate.attestation_count = 3U;
    keys[1].bonded = true;
    keys[2].bonded = true;
    shuffled[1] = shuffled[0];
    if (lxp_guarantor_cert_assemble(&checkpoint, shuffled, 3U, 2U,
                                    &proof_certificate) !=
            LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;
    checkpoint.validity_proof = (lxp_byte_span){validity, sizeof(validity)};
    for (i = 0U; i < 3U; ++i)
        if (lxp_guarantor_attest(&guarantors[i], &checkpoint, true, true,
                                 2000U + i, &arena,
                                 &attestations[i]) != LXP_OK)
            return 1;
    if (lxp_guarantor_cert_assemble(&checkpoint, attestations, 3U, 2U,
                                    &proof_certificate) != LXP_OK ||
        !proof_certificate.validity_proof_present ||
        lxp_guarantor_cert_verify(&proof_certificate, keys, 3U, &arena,
                                  &valid) != LXP_OK || valid != 3U)
        return 1;
    (void)memset(&activity_proof, 0, sizeof(activity_proof));
    (void)memset(&state_proof, 0, sizeof(state_proof));
    activity_proof.leaf_count = 1U;
    state_proof.leaf_count = 1U;
    (void)memcpy(original_copy, original_receipt, sizeof(original_receipt));
    if (lxp_receipt_augment(
            (lxp_byte_span){original_receipt, sizeof(original_receipt)},
            (lxp_byte_span){activity, sizeof(activity)},
            (lxp_byte_span){state_leaf, sizeof(state_leaf)},
            &activity_proof, &state_proof, &proof_certificate,
            (lxp_byte_span){settlement, sizeof(settlement)}, &augmented) !=
            LXP_OK ||
        augmented.pre_checkpoint_receipt.bytes != original_receipt ||
        augmented.pre_checkpoint_receipt.length != sizeof(original_receipt) ||
        memcmp(original_receipt, original_copy, sizeof(original_receipt)) != 0 ||
        memcmp(augmented.checkpoint_id,
               proof_certificate.attestations[0].checkpoint_id, 32U) != 0)
        return 1;
    certificate.attestations[0].signature[0] ^= 1U;
    certificate.attestations[1].signature[0] ^= 1U;
    return lxp_guarantor_cert_verify(&certificate, keys, 3U, &arena, &valid) ==
           LXP_ERR_ATTESTATION_THRESHOLD ? 0 : 1;
}
