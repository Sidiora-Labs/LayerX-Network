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

static lxp_da_class class_from_name(const char *name)
{
    if (strcmp(name, "activities") == 0) return LXP_DA_ACTIVITIES;
    if (strcmp(name, "receipts") == 0) return LXP_DA_RECEIPTS;
    if (strcmp(name, "oracle") == 0) return LXP_DA_ORACLE_INPUTS;
    if (strcmp(name, "state-diff") == 0) return LXP_DA_STATE_DIFF;
    if (strcmp(name, "recovery") == 0) return LXP_DA_RECOVERY_METADATA;
    return (lxp_da_class)0;
}

int main(int argc, char **argv)
{
    uint8_t arena_storage[262144];
    uint8_t section[5][9];
    uint8_t anchor[32];
    lxp_arena arena;
    lxp_batch_body body;
    lxp_da_bundle complete;
    lxp_da_bundle withheld;
    lxp_da_store store;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_attestation attestation;
    lxp_finalisation_state state;
    uint8_t complete_root[32];
    uint8_t withheld_root[32];
    uint8_t available_mask = 0U;
    lxp_da_class withheld_class;
    char directory[] = "/tmp/lxp-da-unavailable-XXXXXX";
    size_t i;
    size_t j;

    if (argc != 2) return 2;
    withheld_class = class_from_name(argv[1]);
    if (withheld_class == 0) return 2;
    for (i = 0U; i < 5U; ++i)
        for (j = 0U; j < sizeof(section[i]); ++j)
            section[i][j] = (uint8_t)(1U + i * 16U + j);
    if (mkdtemp(directory) == NULL ||
        lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_da_store_init(&store, directory) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.batch_number = 51U;
    body.activities = (lxp_byte_span){section[0], sizeof(section[0])};
    body.receipts = (lxp_byte_span){section[1], sizeof(section[1])};
    body.oracle_inputs = (lxp_byte_span){section[2], sizeof(section[2])};
    body.state_diff = (lxp_byte_span){section[3], sizeof(section[3])};
    body.recovery_metadata = (lxp_byte_span){section[4], sizeof(section[4])};
    if (lxp_da_bundle_build(&body, 4U, &arena, &complete) != LXP_OK ||
        lxp_da_bundle_root(&complete, &arena, complete_root) != LXP_OK ||
        lxp_da_withhold_sim(&complete, withheld_class, &arena, &withheld,
                            &available_mask) != LXP_OK ||
        lxp_da_bundle_root(&withheld, &arena, withheld_root) != LXP_OK ||
        memcmp(complete_root, withheld_root, 32U) == 0 ||
        available_mask != (uint8_t)(LXP_GUARANTOR_AVAILABILITY_ALL &
            (uint8_t)~(uint8_t)(1U << ((uint8_t)withheld_class - 1U))))
        return 1;
    if (lxp_da_store_bundle(&store, &withheld, &arena) != LXP_ERR_DA_MISSING)
        return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = 1U;
    checkpoint.header.network_id = 44U;
    checkpoint.header.epoch = 10U;
    checkpoint.header.batch_number = body.header.batch_number;
    checkpoint.header.first_sequence = 801U;
    checkpoint.header.last_sequence = 805U;
    (void)memcpy(checkpoint.header.data_availability_root,
                 complete_root, 32U);
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 91U;
    guarantor.ready_to_sign = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = 1U;
    guarantor.network_id = 44U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    if (key_pair(11U, guarantor.paxeer_private_key,
                 guarantor.paxeer_public_key) != 0 ||
        lxp_da_possession_attest(&store, &guarantor, &checkpoint, 2000U,
                                 &arena, &attestation) != LXP_ERR_DA_MISSING)
        return 1;

    (void)memset(&state, 0, sizeof(state));
    (void)memset(anchor, 0x5a, sizeof(anchor));
    (void)memcpy(state.settlement_anchor, anchor, sizeof(anchor));
    state.checkpoint_finalized = true;
    state.finalized_batch_number = 50U;
    state.withdrawal_settlement_enabled = true;
    state.deposit_settlement_enabled = true;
    state.dispute_settlement_enabled = true;
    state.pending_withdrawal_settlement_enabled = true;
    state.pending_deposit_settlement_enabled = true;
    state.pending_dispute_settlement_enabled = true;
    if (lxp_checkpoint_block_on_da(&state, 51U, false) !=
            LXP_ERR_DA_MISSING ||
        !state.unfinalized_checkpoint_blocked ||
        state.blocked_checkpoint_batch_number != 51U ||
        state.pending_withdrawal_settlement_enabled ||
        state.pending_deposit_settlement_enabled ||
        state.pending_dispute_settlement_enabled ||
        memcmp(state.settlement_anchor, anchor, 32U) != 0 ||
        !state.withdrawal_settlement_enabled)
        return 1;
    if (lxp_da_unavailable_mode(&state, 50U, false, false) !=
            LXP_ERR_DA_MISSING ||
        !state.checkpoint_finalized || !state.emergency_data_mode ||
        !state.emergency_exit_enabled || !state.finalisation_halted ||
        state.finalized_batch_number != 50U ||
        memcmp(state.settlement_anchor, anchor, 32U) != 0)
        return 1;
    if (lxp_da_unavailable_mode(&state, 50U, true, false) != LXP_OK ||
        state.emergency_data_mode || state.finalisation_halted ||
        state.emergency_exit_enabled)
        return 1;
    if (lxp_da_unavailable_mode(&state, 50U, false, false) !=
            LXP_ERR_DA_MISSING ||
        lxp_da_unavailable_mode(&state, 50U, false, true) != LXP_OK ||
        state.emergency_data_mode || state.finalisation_halted ||
        state.unfinalized_checkpoint_blocked)
        return 1;
    if (rmdir(directory) != 0) return 1;
    return 0;
}
