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

typedef struct published_evidence {
    uint8_t signature[64];
    uint8_t served_first_byte;
    size_t served_length;
    size_t count;
} published_evidence;

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

static lxp_result publish(void *context,
                          const lxp_da_failure_evidence *evidence)
{
    published_evidence *published = (published_evidence *)context;
    if (evidence == NULL || evidence->failure_code == LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(published->signature,
                 evidence->challenge.signed_commitment.signature, 64U);
    published->served_length = evidence->served_bytes.length;
    if (evidence->served_bytes.length != 0U)
        published->served_first_byte = evidence->served_bytes.bytes[0];
    ++published->count;
    return LXP_OK;
}

int main(void)
{
    uint8_t build_storage[262144];
    uint8_t response_storage[262144];
    uint8_t sections[5][11];
    lxp_arena build_arena;
    lxp_arena response_arena;
    lxp_batch_body body;
    lxp_da_bundle bundle;
    lxp_da_store store;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_attestation attestation;
    lxp_da_challenge challenge;
    lxp_da_challenge late_challenge;
    lxp_da_challenge_response response;
    lxp_da_challenge_response late_response;
    lxp_da_failure_evidence evidence;
    lxp_da_challenge_registry registry;
    lxp_guarantor_set set;
    lxp_guarantor_bond_state bond;
    lxp_guarantor_bond_state restored;
    published_evidence published;
    uint8_t checkpoint_hash[32];
    uint32_t indices[8];
    uint32_t repeated[8];
    bool satisfied = false;
    bool eligible = false;
    char directory[] = "/tmp/lxp-da-challenge-XXXXXX";
    char path[LXP_DA_STORE_PATH_BYTES];
    size_t i;
    size_t j;

    for (i = 0U; i < 5U; ++i)
        for (j = 0U; j < sizeof(sections[i]); ++j)
            sections[i][j] = (uint8_t)(i * 16U + j + 1U);
    if (mkdtemp(directory) == NULL ||
        lxp_arena_init(&build_arena, build_storage,
                       sizeof(build_storage)) != LXP_OK ||
        lxp_arena_init(&response_arena, response_storage,
                       sizeof(response_storage)) != LXP_OK ||
        lxp_da_store_init(&store, directory) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.batch_number = 41U;
    body.activities = (lxp_byte_span){sections[0], sizeof(sections[0])};
    body.receipts = (lxp_byte_span){sections[1], sizeof(sections[1])};
    body.oracle_inputs = (lxp_byte_span){sections[2], sizeof(sections[2])};
    body.state_diff = (lxp_byte_span){sections[3], sizeof(sections[3])};
    body.recovery_metadata = (lxp_byte_span){sections[4], sizeof(sections[4])};
    if (lxp_da_bundle_build(&body, 4U, &build_arena, &bundle) != LXP_OK ||
        lxp_da_store_bundle(&store, &bundle, &build_arena) != LXP_OK)
        return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = 1U;
    checkpoint.header.network_id = 44U;
    checkpoint.header.epoch = 9U;
    checkpoint.header.batch_number = body.header.batch_number;
    checkpoint.header.first_sequence = 700U;
    checkpoint.header.last_sequence = 704U;
    if (lxp_da_bundle_root(&bundle, &build_arena,
                           checkpoint.header.data_availability_root) != LXP_OK ||
        lxp_checkpoint_certificate_hash(&checkpoint, &build_arena,
                                        checkpoint_hash) != LXP_OK)
        return 1;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 81U;
    guarantor.ready_to_sign = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = 1U;
    guarantor.network_id = 44U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    if (key_pair(9U, guarantor.paxeer_private_key,
                 guarantor.paxeer_public_key) != 0 ||
        lxp_da_possession_attest(&store, &guarantor, &checkpoint, 1000U,
                                 &build_arena, &attestation) != LXP_OK)
        return 1;
    if (lxp_da_challenge_indices(checkpoint_hash,
                                 (uint32_t)bundle.chunk_count, 8U,
                                 indices) != LXP_OK ||
        lxp_da_challenge_indices(checkpoint_hash,
                                 (uint32_t)bundle.chunk_count, 8U,
                                 repeated) != LXP_OK ||
        memcmp(indices, repeated, sizeof(indices)) != 0)
        return 1;
    for (i = 0U; i < 8U; ++i) {
        if (indices[i] >= bundle.chunk_count) return 1;
        for (j = i + 1U; j < 8U; ++j)
            if (indices[i] == indices[j]) return 1;
    }

    if (lxp_da_challenge_issue(
            &attestation, checkpoint_hash, indices[0],
            (uint32_t)bundle.chunk_count, 1100U, 100U, &challenge) != LXP_OK ||
        lxp_da_challenge_respond(&store, &challenge, 1150U, &response_arena,
                                 &response) != LXP_OK ||
        lxp_da_challenge_judge(&challenge, &response, 1151U, &satisfied,
                               &evidence) != LXP_OK || !satisfied)
        return 1;
    (void)memset(&published, 0, sizeof(published));
    if (lxp_da_challenge_registry_init(&registry, publish, &published) !=
            LXP_OK ||
        lxp_da_challenge_record_success(&registry, &challenge) != LXP_OK ||
        registry.count != 1U || registry.records[0].slashable)
        return 1;

    if (lxp_da_challenge_issue(
            &attestation, checkpoint_hash, indices[1],
            (uint32_t)bundle.chunk_count, 1200U, 100U,
            &late_challenge) != LXP_OK ||
        lxp_da_challenge_judge(&late_challenge, NULL, 1250U, &satisfied,
                               &evidence) != LXP_ERR_NOT_YET_VALID ||
        lxp_da_challenge_respond(&store, &late_challenge, 1301U,
                                 &response_arena, &late_response) != LXP_OK ||
        lxp_da_challenge_judge(&late_challenge, &late_response, 1301U,
                               &satisfied, &evidence) != LXP_ERR_DA_MISSING ||
        satisfied)
        return 1;

    if (lxp_arena_reset(&response_arena, 0U) != LXP_OK ||
        lxp_da_challenge_respond(&store, &late_challenge, 1250U,
                                 &response_arena, &response) != LXP_OK ||
        response.chunk.length == 0U)
        return 1;
    ((uint8_t *)response.chunk.bytes.bytes)[0] ^= 1U;
    if (lxp_da_challenge_judge(&late_challenge, &response, 1251U,
                               &satisfied, &evidence) != LXP_ERR_DA_MISSING ||
        satisfied || evidence.served_bytes.length != response.chunk.length ||
        evidence.served_bytes.bytes[0] != response.chunk.bytes.bytes[0] ||
        memcmp(evidence.challenge.signed_commitment.signature,
               attestation.signature, 64U) != 0)
        return 1;

    if (lxp_guarantor_set_init(&set) != LXP_OK)
        return 1;
    (void)memset(&bond, 0, sizeof(bond));
    (void)memcpy(bond.guarantor_id, guarantor.guarantor_id, 32U);
    (void)memcpy(bond.public_key, guarantor.paxeer_public_key, 33U);
    bond.bond_amount = (lxp_u128){0U, 1000U};
    bond.joined_epoch = 1U;
    bond.active = true;
    if (lxp_guarantor_set_apply(&set, 1U, true, &bond) != LXP_OK ||
        lxp_da_challenge_record_failure(&registry, &evidence, &set) != LXP_OK ||
        registry.count != 2U || !registry.records[1].slashable ||
        published.count != 1U ||
        published.served_length != response.chunk.length ||
        published.served_first_byte != response.chunk.bytes.bytes[0] ||
        memcmp(published.signature, attestation.signature, 64U) != 0 ||
        lxp_guarantor_eligible(&set.records[0], checkpoint.header.epoch,
                               (lxp_u128){0U, 1U}, &eligible) != LXP_OK ||
        eligible)
        return 1;
    restored = set.records[0];
    restored.jailed = false;
    restored.unresolved_slashing = false;
    if (lxp_guarantor_set_apply(&set, 2U, true, &restored) != LXP_OK ||
        lxp_guarantor_eligible(&set.records[0], checkpoint.header.epoch,
                               (lxp_u128){0U, 1U}, &eligible) != LXP_OK ||
        !eligible)
        return 1;

    if (snprintf(path, sizeof(path), "%s/%020llu.lxda", directory,
                 (unsigned long long)body.header.batch_number) < 0 ||
        unlink(path) != 0 || rmdir(directory) != 0)
        return 1;
    return 0;
}
