#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_guarantor.h"

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

static int refused(const lxp_finalisation_state *initial,
                   const lxp_guarantor_cert *certificate,
                   const lxp_guarantor_set *set,
                   const lxp_finalisation_requirements *requirements,
                   lxp_arena *arena)
{
    lxp_finalisation_state state = *initial;
    bool finalisable = true;
    lxp_result status = lxp_checkpoint_finalisable(
        &state, certificate, set, requirements, arena, &finalisable);
    return status != LXP_OK && !finalisable &&
           memcmp(&state, initial, sizeof(state)) == 0 ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[131072];
    lxp_arena arena;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_ctx signers[3];
    uint8_t rotated_private_key[32];
    uint8_t rotated_public_key[33];
    lxp_guarantor_attestation attestations[3];
    lxp_guarantor_cert certificate;
    lxp_guarantor_set set;
    lxp_guarantor_set changed_set;
    lxp_guarantor_bond_state bond;
    lxp_finalisation_state initial;
    lxp_finalisation_state finalized;
    lxp_finalisation_requirements requirements;
    bool finalisable = false;
    size_t i;
    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_guarantor_set_init(&set) != LXP_OK)
        return 1;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header.protocol_version = 1U;
    checkpoint.header.network_id = 5U;
    checkpoint.header.epoch = 7U;
    checkpoint.header.batch_number = 9U;
    checkpoint.header.previous_state_root[0] = 0x11U;
    checkpoint.header.resulting_state_root[0] = 0x22U;
    checkpoint.header.data_availability_root[0] = 0x33U;
    for (i = 0U; i < 3U; ++i) {
        (void)memset(&signers[i], 0, sizeof(signers[i]));
        signers[i].guarantor_id[0] = (uint8_t)(i + 1U);
        signers[i].ready_to_sign = true;
        signers[i].possesses_availability = true;
        signers[i].bond_view.bonded = true;
        signers[i].protocol_version = 1U;
        signers[i].network_id = 5U;
        signers[i].paxeer_chain_id = 31337U;
        signers[i].paxeer_settlement_contract[0] = 0xa1U;
        if (key_pair((uint8_t)(i + 1U), signers[i].paxeer_private_key,
                     signers[i].paxeer_public_key) != 0 ||
            lxp_guarantor_attest(&signers[i], &checkpoint, true, true,
                                 1000U + i, &arena,
                                 &attestations[i]) != LXP_OK)
            return 1;
        (void)memset(&bond, 0, sizeof(bond));
        (void)memcpy(bond.guarantor_id, signers[i].guarantor_id, 32U);
        (void)memcpy(bond.public_key, signers[i].paxeer_public_key, 33U);
        bond.bond_amount = (lxp_u128){0U, 100U};
        bond.joined_epoch = 1U;
        bond.active = true;
        if (lxp_guarantor_set_apply(&set, i + 1U, true, &bond) != LXP_OK)
            return 1;
    }
    if (set.version != 3U ||
        lxp_guarantor_set_apply(&set, 4U, false, &bond) != LXP_ERR_AUTH_SCOPE ||
        set.version != 3U ||
        lxp_guarantor_cert_assemble(&checkpoint, attestations, 3U, 2U,
                                    &certificate) != LXP_OK)
        return 1;
    if (key_pair(9U, rotated_private_key, rotated_public_key) != 0 ||
        lxp_guarantor_set_rotate_signer(
            &set, 4U, true, signers[0].guarantor_id,
            rotated_public_key, 8U) != LXP_OK ||
        set.version != 4U || set.last_governance_sequence != 4U ||
        set.records[0].signer_authorization_count != 2U ||
        set.records[0].signer_authorizations[0].active_until_epoch != 8U ||
        set.records[0].signer_authorizations[1].active_from_epoch != 8U ||
        memcmp(set.records[0].public_key, rotated_public_key, 33U) != 0)
        return 1;
    bond = set.records[0];
    (void)memcpy(bond.public_key, signers[0].paxeer_public_key, 33U);
    if (lxp_guarantor_set_apply(&set, 5U, true, &bond) !=
            LXP_ERR_NON_CANONICAL || set.version != 4U)
        return 1;
    changed_set = set;
    changed_set.count = (size_t)LXP_MAX_GUARANTOR_ATTESTATIONS + 1U;
    if (lxp_guarantor_set_validate(&changed_set) != LXP_ERR_NON_CANONICAL)
        return 1;
    changed_set = set;
    changed_set.records[0].signer_authorizations[0].active_until_epoch = 1U;
    if (lxp_guarantor_set_validate(&changed_set) != LXP_ERR_NON_CANONICAL)
        return 1;
    (void)memset(&bond, 0, sizeof(bond));
    bond.guarantor_id[0] = 9U;
    (void)memcpy(bond.public_key, signers[1].paxeer_public_key, 33U);
    bond.bond_amount = (lxp_u128){0U, 100U};
    bond.joined_epoch = 1U;
    bond.active = true;
    if (lxp_guarantor_set_apply(&set, 5U, true, &bond) !=
            LXP_ERR_NON_CANONICAL || set.version != 4U)
        return 1;
    (void)memset(&initial, 0, sizeof(initial));
    initial.settlement_anchor[0] = 0x11U;
    requirements.checkpoint_epoch = 7U;
    requirements.challenge_window_end_ms = 1100U;
    requirements.checkpoint_deadline_ms = 1050U;
    requirements.now_ms = 1200U;
    requirements.threshold = 2U;
    requirements.minimum_bond = (lxp_u128){0U, 50U};
    requirements.availability_challenges_answered = true;
    requirements.equivocation_detected = false;
    finalized = initial;
    if (lxp_checkpoint_finalisable(&finalized, &certificate, &set,
                                   &requirements, &arena, &finalisable) !=
            LXP_OK || !finalisable || !finalized.checkpoint_finalized ||
        !finalized.withdrawal_settlement_enabled ||
        !finalized.deposit_settlement_enabled ||
        !finalized.dispute_settlement_enabled ||
        finalized.finalized_batch_number != 9U ||
        memcmp(finalized.settlement_anchor,
               checkpoint.header.resulting_state_root, 32U) != 0)
        return 1;

    requirements.checkpoint_epoch = 8U;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    requirements.checkpoint_epoch = 7U;
    requirements.challenge_window_end_ms = 1300U;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    requirements.challenge_window_end_ms = 1100U;
    requirements.availability_challenges_answered = false;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    requirements.availability_challenges_answered = true;
    requirements.equivocation_detected = true;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    requirements.equivocation_detected = false;
    requirements.checkpoint_deadline_ms = 1000U;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    requirements.checkpoint_deadline_ms = 1050U;

    changed_set = set;
    changed_set.records[0].bond_amount = (lxp_u128){0U, 1U};
    changed_set.records[1].bond_amount = (lxp_u128){0U, 1U};
    if (refused(&initial, &certificate, &changed_set, &requirements,
                &arena) != 0) return 1;
    changed_set = set;
    changed_set.records[0].jailed = true;
    changed_set.records[1].jailed = true;
    if (refused(&initial, &certificate, &changed_set, &requirements,
                &arena) != 0) return 1;
    changed_set = set;
    changed_set.records[0].unresolved_slashing = true;
    changed_set.records[1].unresolved_slashing = true;
    if (refused(&initial, &certificate, &changed_set, &requirements,
                &arena) != 0) return 1;
    changed_set = set;
    changed_set.records[0].removed_epoch = 7U;
    changed_set.records[1].removed_epoch = 7U;
    if (refused(&initial, &certificate, &changed_set, &requirements,
                &arena) != 0) return 1;

    checkpoint.header.previous_state_root[0] = 0x44U;
    certificate.checkpoint = checkpoint;
    if (refused(&initial, &certificate, &set, &requirements, &arena) != 0)
        return 1;
    certificate.checkpoint.header.previous_state_root[0] = 0x11U;
    certificate.attestations[0].availability_class_mask = 0x0fU;
    certificate.attestations[1].availability_class_mask = 0x0fU;
    return refused(&initial, &certificate, &set, &requirements, &arena);
}
