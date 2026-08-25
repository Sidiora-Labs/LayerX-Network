#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_tools.h"
#include "layerx/lxp_hash.h"

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

static lxp_result parameter_version(
    void *context, uint64_t epoch, uint32_t *version)
{
    (void)context;
    if (epoch > UINT32_MAX - 30U) return LXP_ERR_OVERFLOW;
    *version = (uint32_t)epoch + 30U;
    return LXP_OK;
}

static lxp_result transition(
    void *context, uint16_t transition_version,
    uint32_t parameters, uint64_t timestamp, uint64_t sequence,
    lxp_byte_span activity, const uint8_t previous_root[32],
    lxp_arena *arena, lxp_replay_activity_output *output)
{
    uint8_t *material;
    void *memory;
    size_t length = 32U + 2U + 4U + 8U + 8U + activity.length;
    size_t offset = 0U;
    size_t i;
    lxp_result status = lxp_arena_alloc(arena, length, 1U, &memory);
    (void)context;
    if (status != LXP_OK) return status;
    material = (uint8_t *)memory;
    (void)memcpy(material, previous_root, 32U);
    offset += 32U;
    material[offset++] = (uint8_t)(transition_version >> 8U);
    material[offset++] = (uint8_t)transition_version;
    for (i = 0U; i < 4U; ++i)
        material[offset + 3U - i] = (uint8_t)(parameters >> (i * 8U));
    offset += 4U;
    for (i = 0U; i < 8U; ++i)
        material[offset + 7U - i] = (uint8_t)(timestamp >> (i * 8U));
    offset += 8U;
    for (i = 0U; i < 8U; ++i)
        material[offset + 7U - i] = (uint8_t)(sequence >> (i * 8U));
    offset += 8U;
    (void)memcpy(material + offset, activity.bytes, activity.length);
    status = lxp_hash_sha256(material, length, output->resulting_state_root);
    if (status != LXP_OK) return status;
    output->result_code = LXP_OK;
    output->fee_charged = (lxp_u128){0U, parameters + activity.length};
    output->effects = activity;
    output->resulting_balance = (lxp_byte_span){
        output->resulting_state_root, 16U
    };
    output->canonical_receipt = (lxp_byte_span){
        output->resulting_state_root, 32U
    };
    output->canonical_events = activity;
    return LXP_OK;
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
    static uint8_t build_storage[1048576U];
    static uint8_t verify_storage[1048576U];
    uint8_t genesis_root[32] = {0U};
    uint8_t activity[] = {1U, 3U, 5U, 7U};
    uint8_t oracle[] = {0x90U, 0x91U};
    uint8_t state_diff[] = {0xa0U, 0xa1U};
    uint8_t recovery[] = {0xb0U, 0xb1U};
    uint8_t manifest[] = {0x47U, 0x45U, 0x4eU, 0x31U};
    lxp_byte_span activities[1] = {{activity, sizeof(activity)}};
    lxp_byte_span oracles[1] = {{oracle, sizeof(oracle)}};
    lxp_arena build_arena;
    lxp_arena verify_arena;
    lxp_replay_engine build_engine;
    lxp_replay_engine verify_engine;
    lxp_replay_batch_result built;
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
        lxp_replay_engine_init(
            &build_engine, parameter_version, NULL) != LXP_OK ||
        lxp_replay_engine_register(
            &build_engine, 1U, transition) != LXP_OK ||
        lxp_replay_engine_init(
            &verify_engine, parameter_version, NULL) != LXP_OK ||
        lxp_replay_engine_register(
            &verify_engine, 1U, transition) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    body.header.network_id = 42U;
    body.header.epoch = 7U;
    body.header.batch_number = 8U;
    body.header.first_sequence = 11U;
    body.header.last_sequence = 11U;
    body.header.timestamp_ms = 1700000001000U;
    body.header.sequencer_id[0] = 9U;
    body.state_diff = (lxp_byte_span){state_diff, sizeof(state_diff)};
    body.recovery_metadata = (lxp_byte_span){recovery, sizeof(recovery)};
    if (lxp_replay_section_encode(
            activities, 1U, &build_arena,
            &body.activities) != LXP_OK ||
        lxp_replay_section_encode(
            oracles, 1U, &build_arena,
            &body.oracle_inputs) != LXP_OK ||
        lxp_replay_batch(
            &build_engine, &body, genesis_root,
            &build_arena, &built) != LXP_OK)
        return 1;
    body.receipts = built.canonical_receipt_section;
    body.events = built.canonical_event_section;
    (void)memcpy(body.header.resulting_state_root,
                 built.resulting_state_root, 32U);
    (void)memcpy(body.header.activity_merkle_root,
                 built.roots.activity_merkle_root, 32U);
    (void)memcpy(body.header.receipt_merkle_root,
                 built.roots.receipt_merkle_root, 32U);
    (void)memcpy(body.header.event_merkle_root,
                 built.roots.event_merkle_root, 32U);
    (void)memcpy(body.header.oracle_root,
                 built.roots.oracle_root, 32U);
    if (lxp_da_bundle_build(
            &body, 7U, &build_arena, &bundle) != LXP_OK ||
        lxp_da_bundle_root(
            &bundle, &build_arena, da_root) != LXP_OK)
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
        guarantors[i].network_id = 42U;
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
        &verify_engine, genesis_root, &verify_arena
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
