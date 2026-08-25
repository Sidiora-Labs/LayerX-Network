#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_da.h"
#include "layerx/lxp_guarantor.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

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
        public_length = EC_POINT_point2oct(
            group, point, POINT_CONVERSION_COMPRESSED, public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return public_length == 33U ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[262144];
    uint8_t activities[] = {1U, 2U, 3U};
    uint8_t receipts[] = {4U, 5U};
    uint8_t oracles[] = {6U, 7U, 8U, 9U};
    uint8_t state_diff[] = {10U, 11U, 12U};
    uint8_t recovery[] = {13U, 14U, 15U, 16U, 17U};
    lxp_arena arena;
    lxp_batch_body body;
    lxp_da_bundle bundle;
    lxp_da_store store;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_attestation attestation;
    lxp_guarantor_attestation changed;
    lxp_guarantor_cert certificate;
    lxp_guarantor_key_record key;
    size_t valid = 0U;
    char directory[] = "/tmp/lxp-da-possession-XXXXXX";
    char path[LXP_DA_STORE_PATH_BYTES];

    if (mkdtemp(directory) == NULL ||
        lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_da_store_init(&store, directory) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.batch_number = 23U;
    body.activities = (lxp_byte_span){activities, sizeof(activities)};
    body.receipts = (lxp_byte_span){receipts, sizeof(receipts)};
    body.oracle_inputs = (lxp_byte_span){oracles, sizeof(oracles)};
    body.state_diff = (lxp_byte_span){state_diff, sizeof(state_diff)};
    body.recovery_metadata = (lxp_byte_span){recovery, sizeof(recovery)};
    if (lxp_da_bundle_build(&body, 3U, &arena, &bundle) != LXP_OK)
        return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = 1U;
    checkpoint.header.network_id = 44U;
    checkpoint.header.epoch = 4U;
    checkpoint.header.batch_number = body.header.batch_number;
    checkpoint.header.first_sequence = 101U;
    checkpoint.header.last_sequence = 103U;
    if (lxp_da_bundle_root(&bundle, &arena,
                           checkpoint.header.data_availability_root) != LXP_OK)
        return 1;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 71U;
    guarantor.ready_to_sign = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = 1U;
    guarantor.network_id = 44U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    if (key_pair(7U, guarantor.paxeer_private_key,
                 guarantor.paxeer_public_key) != 0)
        return 1;

    if (lxp_da_possession_attest(&store, &guarantor, &checkpoint, 1000U,
                                 &arena, &attestation) != LXP_ERR_DA_MISSING ||
        lxp_guarantor_attest(&guarantor, &checkpoint, true, true, 1000U,
                             &arena, &attestation) !=
            LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;
    if (lxp_da_store_bundle(&store, &bundle, &arena) != LXP_OK ||
        lxp_da_possession_attest(&store, &guarantor, &checkpoint, 1001U,
                                 &arena, &attestation) != LXP_OK ||
        lxp_da_possession_verify(
            &store, &attestation,
            checkpoint.header.data_availability_root, &arena) != LXP_OK ||
        memcmp(attestation.data_availability_root,
               checkpoint.header.data_availability_root, 32U) != 0 ||
        attestation.availability_class_mask !=
            LXP_GUARANTOR_AVAILABILITY_ALL)
        return 1;

    (void)memset(&key, 0, sizeof(key));
    (void)memcpy(key.guarantor_id, guarantor.guarantor_id, 32U);
    (void)memcpy(key.public_key, guarantor.paxeer_public_key, 33U);
    key.bonded = true;
    if (lxp_guarantor_cert_assemble(&checkpoint, &attestation, 1U, 1U,
                                    &certificate) != LXP_OK ||
        lxp_guarantor_cert_verify(&certificate, &key, 1U, &arena, &valid) !=
            LXP_OK || valid != 1U)
        return 1;
    changed = attestation;
    changed.availability_class_mask = 0x0fU;
    if (lxp_da_possession_verify(
            &store, &changed, checkpoint.header.data_availability_root,
            &arena) != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    changed = attestation;
    changed.data_availability_root[0] ^= 1U;
    if (lxp_da_possession_verify(
            &store, &changed, checkpoint.header.data_availability_root,
            &arena) != LXP_ERR_INVALID_ATTESTATION)
        return 1;
    certificate.attestations[0] = changed;
    if (lxp_guarantor_cert_verify(&certificate, &key, 1U, &arena, &valid) !=
        LXP_ERR_ROOT_MISMATCH)
        return 1;

    if (snprintf(path, sizeof(path), "%s/%020llu.lxda", directory,
                 (unsigned long long)body.header.batch_number) < 0 ||
        unlink(path) != 0 ||
        lxp_da_possession_verify(
            &store, &attestation,
            checkpoint.header.data_availability_root, &arena) !=
            LXP_ERR_DA_MISSING ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}
