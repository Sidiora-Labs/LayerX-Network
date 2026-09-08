#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_tools.h"
#include "layerx/lxp_hash.h"
#include "support/lxp_real_replay.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

typedef struct ctl_state {
    uint64_t next_sequence;
    uint8_t root[32];
    size_t ordered_log_count;
} ctl_state;

static lxp_result ctl_submit(
    void *context, const uint8_t *activity, size_t activity_length,
    uint64_t *global_sequence, uint8_t state_root[32])
{
    ctl_state *state = (ctl_state *)context;
    uint8_t preimage[32U + 64U];
    if (state == NULL || activity == NULL || activity_length == 0U ||
        activity_length > 64U || global_sequence == NULL ||
        state_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(preimage, state->root, 32U);
    (void)memcpy(preimage + 32U, activity, activity_length);
    if (lxp_hash_sha256(
            preimage, 32U + activity_length, state->root) != LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    *global_sequence = state->next_sequence++;
    ++state->ordered_log_count;
    (void)memcpy(state_root, state->root, 32U);
    return LXP_OK;
}

static lxp_result ctl_read(
    void *context, uint64_t *global_sequence, uint8_t state_root[32])
{
    ctl_state *state = (ctl_state *)context;
    if (state == NULL || global_sequence == NULL || state_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    *global_sequence = state->next_sequence;
    (void)memcpy(state_root, state->root, 32U);
    return LXP_OK;
}

static lxp_result genesis_action(
    void *context, lxp_genesis_cli_action action,
    lxp_byte_span canonical_input, uint8_t manifest_root[32])
{
    (void)context;
    if (action != LXP_GENESIS_BUILD && action != LXP_GENESIS_RECONCILE)
        return LXP_ERR_NON_CANONICAL;
    return lxp_hash_sha256(
        canonical_input.bytes, canonical_input.length, manifest_root);
}

static int key_pair(
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
    static uint8_t build_storage[16U * 1024U * 1024U];
    static uint8_t verify_storage[16U * 1024U * 1024U];
    uint8_t genesis_root[32] = {0U};
    uint8_t activity[] = {1U, 3U, 5U, 7U};
    uint8_t oracle[] = {0x90U, 0x91U};
    uint8_t manifest[] = {0x47U, 0x45U, 0x4eU, 0x31U};
    lxp_byte_span activities[1] = {{activity, sizeof(activity)}};
    lxp_byte_span oracles[1] = {{oracle, sizeof(oracle)}};
    lxp_arena build_arena;
    lxp_arena verify_arena;
    static lxp_real_replay_fixture builder;
    static lxp_real_replay_fixture verifier;


    lxp_batch_body body;
    lxp_da_bundle bundle;
    uint8_t da_root[32];
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantors[2];
    lxp_guarantor_attestation attestations[2];
    lxp_guarantor_key_record keys[2];
    lxp_guarantor_cert certificate;
    lxp_verify_run run;
    uint8_t verify_output[LXP_VERIFY_OUTPUT_BYTES];
    uint8_t ctl_output[LXP_CTL_OUTPUT_BYTES];
    uint8_t genesis_output[LXP_GENESIS_OUTPUT_BYTES];
    ctl_state state;
    lxp_ctl_context ctl;
    size_t i;

    (void)memset(&state, 0, sizeof(state));
    ctl = (lxp_ctl_context){ctl_submit, ctl_read, &state};
    if (lxp_ctl_main(
            LXP_CTL_SUBMIT, &ctl, activity, sizeof(activity),
            ctl_output) != LXP_OK ||
        memcmp(ctl_output, "LXCT\1\1", 6U) != 0 ||
        state.ordered_log_count != 1U || state.next_sequence != 1U ||
        lxp_ctl_main(
            LXP_CTL_READ_STATE, &ctl, NULL, 0U, ctl_output) != LXP_OK ||
        ctl_output[5] != (uint8_t)LXP_CTL_READ_STATE ||
        lxp_genesis_cli_main(
            LXP_GENESIS_BUILD,
            (lxp_byte_span){manifest, sizeof(manifest)},
            genesis_action, NULL, genesis_output) != LXP_OK ||
        memcmp(genesis_output, "LXGN\1\1", 6U) != 0)
        return 1;

    if (lxp_arena_init(
            &build_arena, build_storage,
            sizeof(build_storage)) != LXP_OK ||
        lxp_arena_init(
            &verify_arena, verify_storage,
            sizeof(verify_storage)) != LXP_OK ||
        lxp_real_replay_init(&builder) != 0 ||
        lxp_real_replay_init(&verifier) != 0)
        return 1;
    (void)memcpy(genesis_root, builder.kernel.current_state_root, 32U);
    for (i = 0U; i < 1U; ++i)
        if (lxp_real_replay_activity(&builder, i, &build_arena, &activities[i]) != 0)
            return 1;
    if (lxp_real_replay_build(&builder, 8U, activities, 1U, oracles, 1U,
                              &build_arena, &body) != 0)
        return 1;
    verifier.execution.batch_number = 8U;
    if (lxp_da_bundle_build(
            &body, LXP_DA_CANONICAL_CHUNK_BYTES, &build_arena, &bundle) != LXP_OK ||
        lxp_batch_availability_root(
            &body, &build_arena, da_root) != LXP_OK)
        return 1;
    (void)memcpy(body.header.data_availability_root, da_root, 32U);
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header = body.header;
    for (i = 0U; i < 2U; ++i) {
        (void)memset(&guarantors[i], 0, sizeof(guarantors[i]));
        guarantors[i].guarantor_id[0] = (uint8_t)(i + 1U);
        guarantors[i].ready_to_sign = true;
        guarantors[i].possesses_availability = true;
        guarantors[i].bond_view.bonded = true;
        guarantors[i].protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
        guarantors[i].network_id = body.header.network_id;
        guarantors[i].paxeer_chain_id = 31337U;
        guarantors[i].paxeer_settlement_contract[0] = 0xa1U;
        if (key_pair(
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
                2000U + i, &build_arena,
                &attestations[i]) != LXP_OK)
            return 1;
    }
    if (lxp_guarantor_cert_assemble(
            &checkpoint, attestations, 2U, 2U,
            &certificate) != LXP_OK)
        return 1;
    run = (lxp_verify_run){
        &bundle, &body.header, &certificate, keys, 2U,
        &verifier.engine, genesis_root, &verify_arena
    };
    if (lxp_verify_main(&run, verify_output) != LXP_OK ||
        memcmp(verify_output, "LXVF\1", 5U) != 0 ||
        memcmp(verify_output + 29U,
               body.header.resulting_state_root, 32U) != 0 ||
        memcmp(verify_output + 61U,
               body.header.activity_merkle_root, 32U) != 0 ||
        memcmp(verify_output + 189U,
               body.header.data_availability_root, 32U) != 0)
        return 1;
    ((uint8_t *)bundle.chunks[0].bytes.bytes)[0] ^= 1U;
    return lxp_verify_main(&run, verify_output) == LXP_OK ? 1 : 0;
}
