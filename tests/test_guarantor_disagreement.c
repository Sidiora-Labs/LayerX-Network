#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_guarantor.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

typedef struct dissent_sink {
    lxp_guarantor_dissent_record records[3];
    size_t count;
} dissent_sink;

static int key_pair(uint8_t value, uint8_t private_key[32],
                    uint8_t public_key[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value = BN_new();
    const EC_GROUP *group = key == NULL ? NULL : EC_KEY_get0_group(key);
    EC_POINT *point = group == NULL ? NULL : EC_POINT_new(group);
    size_t length = 0U;
    (void)memset(private_key, 0, 32U);
    private_key[31] = value;
    if (key != NULL && private_value != NULL && point != NULL &&
        BN_bin2bn(private_key, 32, private_value) != NULL &&
        EC_POINT_mul(group, point, private_value, NULL, NULL, NULL) == 1 &&
        EC_KEY_set_private_key(key, private_value) == 1 &&
        EC_KEY_set_public_key(key, point) == 1)
        length = EC_POINT_point2oct(group, point, POINT_CONVERSION_COMPRESSED,
                                    public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return length == 33U ? 0 : 1;
}

static lxp_result publish_dissent(
    void *context, const lxp_guarantor_dissent_record *dissent)
{
    dissent_sink *sink = (dissent_sink *)context;
    if (sink->count == 3U) return LXP_ERR_LENGTH_LIMIT;
    sink->records[sink->count++] = *dissent;
    return LXP_OK;
}

int main(void)
{
    uint8_t arena_storage[262144];
    uint8_t expected_effect[] = {1U, 2U};
    uint8_t expected_balance[] = {3U, 4U};
    uint8_t expected_receipt[] = {5U, 6U};
    uint8_t expected_event[] = {7U, 8U};
    lxp_arena arena;
    lxp_guarantor_ctx guarantors[3];
    lxp_replay_activity_output published_output;
    lxp_replay_activity_output recomputed_output;
    lxp_replay_batch_result published;
    lxp_replay_batch_result recomputed;
    lxp_guarantor_divergence divergence;
    lxp_guarantor_dissent_record dissent;
    dissent_sink sink = {0};
    lxp_checkpoint_certificate checkpoint_one;
    lxp_checkpoint_certificate checkpoint_two;
    lxp_guarantor_attestation double_first;
    lxp_guarantor_attestation double_second;
    lxp_guarantor_cert below_threshold;
    lxp_equivocation_evidence evidence;
    lxp_equivocation_evidence transported;
    lxp_byte_span encoded;
    size_t i;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK)
        return 1;
    for (i = 0U; i < 3U; ++i) {
        (void)memset(&guarantors[i], 0, sizeof(guarantors[i]));
        guarantors[i].guarantor_id[0] = (uint8_t)(i + 1U);
        guarantors[i].bond_view.bonded = true;
        guarantors[i].ready_to_sign = true;
        guarantors[i].possesses_availability = true;
        guarantors[i].protocol_version = 1U;
        guarantors[i].network_id = 4U;
        guarantors[i].paxeer_chain_id = 31337U;
        guarantors[i].paxeer_settlement_contract[0] = 0xa1U;
        guarantors[i].publish_dissent = publish_dissent;
        guarantors[i].dissent_context = &sink;
        if (key_pair((uint8_t)(i + 1U), guarantors[i].paxeer_private_key,
                     guarantors[i].paxeer_public_key) != 0)
            return 1;
    }
    (void)memset(&published_output, 0, sizeof(published_output));
    published_output.result_code = LXP_OK;
    published_output.fee_charged = (lxp_u128){0U, 1U};
    published_output.effects = (lxp_byte_span){expected_effect,
                                               sizeof(expected_effect)};
    published_output.resulting_balance = (lxp_byte_span){
        expected_balance, sizeof(expected_balance)};
    published_output.canonical_receipt = (lxp_byte_span){
        expected_receipt, sizeof(expected_receipt)};
    published_output.canonical_events = (lxp_byte_span){
        expected_event, sizeof(expected_event)};
    published_output.resulting_state_root[0] = 0x11U;
    recomputed_output = published_output;
    recomputed_output.resulting_state_root[0] = 0x12U;
    (void)memset(&published, 0, sizeof(published));
    (void)memset(&recomputed, 0, sizeof(recomputed));
    published.outputs = &published_output;
    published.activity_count = 1U;
    recomputed.outputs = &recomputed_output;
    recomputed.activity_count = 1U;
    if (lxp_guarantor_first_divergence(8U, 20U, &published, &recomputed,
                                       &divergence) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        divergence.batch_number != 8U || divergence.global_sequence != 20U ||
        divergence.component != LXP_GUARANTOR_DIVERGENCE_STATE_ROOT ||
        lxp_guarantor_withhold(&guarantors[0], 3U, &divergence, &dissent) !=
            LXP_FATAL_REPLAY_DIVERGENCE || guarantors[0].ready_to_sign ||
        guarantors[0].attestation_halted_epoch != 3U || sink.count != 1U ||
        lxp_guarantor_dissent_verify(&sink.records[0],
                                     guarantors[0].paxeer_public_key) != LXP_OK)
        return 1;
    divergence.component = LXP_GUARANTOR_DIVERGENCE_SIGNATURE;
    divergence.global_sequence = 21U;
    if (lxp_guarantor_withhold(&guarantors[1], 3U, &divergence, &dissent) !=
            LXP_FATAL_REPLAY_DIVERGENCE || guarantors[1].ready_to_sign ||
        sink.count != 2U ||
        lxp_guarantor_dissent_verify(&sink.records[1],
                                     guarantors[1].paxeer_public_key) != LXP_OK)
        return 1;

    (void)memset(&checkpoint_one, 0, sizeof(checkpoint_one));
    checkpoint_one.header.protocol_version = 1U;
    checkpoint_one.header.network_id = 4U;
    checkpoint_one.header.epoch = 3U;
    checkpoint_one.header.batch_number = 8U;
    checkpoint_one.header.resulting_state_root[0] = 0x21U;
    checkpoint_two = checkpoint_one;
    checkpoint_two.header.resulting_state_root[0] = 0x22U;
    if (lxp_guarantor_attest(&guarantors[2], &checkpoint_one, true, true,
                             100U, &arena, &double_first) != LXP_OK ||
        lxp_guarantor_attest(&guarantors[2], &checkpoint_two, true, true,
                             101U, &arena, &double_second) != LXP_OK ||
        lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &double_first,
                                &double_second,
                                guarantors[2].paxeer_public_key, 33U,
                                &evidence) != LXP_OK ||
        lxp_equivocation_verify(&evidence, &arena) != LXP_OK ||
        lxp_equivocation_encode(&evidence, &arena, &encoded) != LXP_OK ||
        encoded.length == 0U)
        return 1;
    transported = evidence;
    if (lxp_equivocation_verify(&transported, &arena) != LXP_OK ||
        lxp_guarantor_cert_assemble(&checkpoint_one, &double_first, 1U, 2U,
                                    &below_threshold) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    evidence.guarantor_second.signature[0] ^= 1U;
    return lxp_equivocation_verify(&evidence, &arena) ==
           LXP_ERR_BAD_SIGNATURE ? 0 : 1;
}
