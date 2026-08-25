#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_paxeer.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
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

static int assemble(uint64_t batch_number, uint64_t last_sequence,
                    const uint8_t previous_root[32],
                    lxp_guarantor_ctx guarantors[3],
                    lxp_guarantor_cert *certificate,
                    lxp_arena *arena)
{
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation attestations[3];
    size_t i;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = LXP_PROTOCOL_VERSION;
    checkpoint.header.network_id = 42U;
    checkpoint.header.epoch = batch_number;
    checkpoint.header.batch_number = batch_number;
    checkpoint.header.first_sequence = 1U;
    checkpoint.header.last_sequence = last_sequence;
    (void)memcpy(checkpoint.header.previous_state_root, previous_root, 32U);
    checkpoint.header.resulting_state_root[0] = (uint8_t)batch_number;
    checkpoint.header.activity_merkle_root[0] = 1U;
    checkpoint.header.receipt_merkle_root[0] = 2U;
    checkpoint.header.event_merkle_root[0] = 3U;
    checkpoint.header.data_availability_root[0] = 4U;
    checkpoint.header.oracle_root[0] = 5U;
    checkpoint.header.timestamp_ms = batch_number * 1000U;
    checkpoint.header.sequencer_id[0] = 6U;
    for (i = 0U; i < 3U; ++i)
        if (lxp_guarantor_attest(&guarantors[i], &checkpoint, true, true,
                                 100U + i, arena, &attestations[i]) != LXP_OK)
            return 1;
    return lxp_guarantor_cert_assemble(&checkpoint, attestations, 3U, 2U,
                                       certificate) == LXP_OK ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[262144];
    uint8_t first_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t zero_root[32] = {0U};
    lxp_arena arena;
    lxp_paxeer_custody_abi abi;
    lxp_checkpoint_registry_state registry;
    lxp_checkpoint_registry_state before_second;
    lxp_checkpoint_registration registration;
    lxp_guarantor_ctx guarantors[3];
    lxp_guarantor_set guarantor_set;
    lxp_guarantor_cert certificate;
    lxp_finalisation_requirements requirements;
    size_t i;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_paxeer_custody_abi_init(&abi) != LXP_OK ||
        abi.input_count != LXP_PAXEER_CUSTODY_INPUT_COUNT)
        return 1;
    for (i = 0U; i < abi.input_count; ++i)
        if (abi.inputs[i] != (lxp_paxeer_custody_input_kind)(i + 1U)) return 1;
    (void)memset(&registry, 0, sizeof(registry));
    (void)memset(&guarantor_set, 0, sizeof(guarantor_set));
    guarantor_set.count = 3U;
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
        if (key_pair((uint8_t)(i + 1U), guarantors[i].paxeer_private_key,
                     guarantors[i].paxeer_public_key) != 0)
            return 1;
        guarantor_set.records[i].guarantor_id[0] = (uint8_t)(i + 1U);
        (void)memcpy(guarantor_set.records[i].public_key,
                     guarantors[i].paxeer_public_key, 33U);
        guarantor_set.records[i].bond_amount = (lxp_u128){0U, 1000U};
        guarantor_set.records[i].joined_epoch = 1U;
        guarantor_set.records[i].active = true;
    }
    (void)memset(&requirements, 0, sizeof(requirements));
    requirements.checkpoint_epoch = 1U;
    requirements.challenge_window_end_ms = 50U;
    requirements.checkpoint_deadline_ms = 1000U;
    requirements.now_ms = 60U;
    requirements.threshold = 2U;
    requirements.minimum_bond = (lxp_u128){0U, 500U};
    requirements.availability_challenges_answered = true;
    if (assemble(1U, 1000000U, zero_root, guarantors, &certificate,
                 &arena) != 0 ||
        lxp_checkpoint_register(&registry, &certificate, &guarantor_set,
                                &requirements, &arena, &registration) != LXP_OK ||
        registration.header_commitments.length !=
            LXP_BATCH_HEADER_ENCODED_SIZE ||
        registry.registered_header_length != LXP_BATCH_HEADER_ENCODED_SIZE ||
        registry.registration_count != 1U ||
        memcmp(registry.finalisation.settlement_anchor,
               certificate.checkpoint.header.resulting_state_root, 32U) != 0)
        return 1;
    (void)memcpy(first_header, registration.header_commitments.bytes,
                 sizeof(first_header));
    before_second = registry;
    requirements.checkpoint_epoch = 2U;
    requirements.now_ms = 70U;
    if (assemble(2U, 10000000U,
                 registry.finalisation.settlement_anchor, guarantors,
                 &certificate, &arena) != 0 ||
        lxp_checkpoint_register(&registry, &certificate, &guarantor_set,
                                &requirements, &arena, &registration) != LXP_OK ||
        registration.header_commitments.length != sizeof(first_header) ||
        registry.registration_count != 2U ||
        memcmp(first_header, registration.header_commitments.bytes,
               sizeof(first_header)) == 0)
        return 1;
    certificate.attestations[0].signature[0] ^= 1U;
    certificate.attestations[1].signature[0] ^= 1U;
    return lxp_checkpoint_register(&before_second, &certificate,
                                   &guarantor_set,
                                   &requirements, &arena, &registration) ==
           LXP_ERR_ATTESTATION_THRESHOLD ? 0 : 1;
}
