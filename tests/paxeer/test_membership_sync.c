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

static void member(lxp_guarantor_bond_state *record, uint8_t identity,
                   const uint8_t public_key[33], lxp_u128 amount,
                   uint64_t joined_epoch, uint64_t authorization_version)
{
    (void)memset(record, 0, sizeof(*record));
    record->guarantor_id[31] = identity;
    (void)memcpy(record->public_key, public_key, 33U);
    record->bond_amount = amount;
    record->joined_epoch = joined_epoch;
    record->active = true;
    record->signer_authorization_count = 1U;
    (void)memcpy(record->signer_authorizations[0].public_key, public_key, 33U);
    record->signer_authorizations[0].active_from_epoch = joined_epoch;
    record->signer_authorizations[0].set_version = authorization_version;
}

int main(void)
{
    uint8_t first_private[32];
    uint8_t first_public[33];
    uint8_t second_private[32];
    uint8_t second_public[33];
    uint8_t contract[20] = {0xa1U};
    uint8_t first_id[32] = {0};
    uint8_t second_id[32] = {0};
    uint8_t commitment[32];
    uint8_t advanced_commitment[32];
    lxp_paxeer_bond_state state;
    lxp_paxeer_bond_state tampered;
    lxp_paxeer_membership_observation observation;
    lxp_paxeer_membership_observation foreign;
    lxp_paxeer_membership_observation advanced;
    lxp_paxeer_membership_sync_availability availability;
    lxp_guarantor_bond_state view;
    lxp_guarantor_bond_state jailed;
    bool eligible = false;
    first_id[31] = 1U;
    second_id[31] = 2U;
    if (key_pair(7U, first_private, first_public) != 0 ||
        key_pair(8U, second_private, second_public) != 0)
        return 1;
    if (lxp_paxeer_bond_init(&state, 2U, 42U, 31337U, contract,
                             (lxp_u128){0U, 10000U}, 100U) != LXP_OK ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE ||
        lxp_u128_cmp(state.minimum_bond, (lxp_u128){0U, 100U}) != 0)
        return 1;

    (void)memset(&observation, 0, sizeof(observation));
    observation.paxeer_chain_id = 31337U;
    (void)memcpy(observation.guarantor_bond_contract, contract, 20U);
    observation.membership_version = 5U;
    observation.observed_epoch = 4U;
    observation.minimum_bond = (lxp_u128){0U, 100U};
    observation.members.version = 5U;
    observation.members.count = 2U;
    member(&observation.members.records[0], 1U, first_public,
           (lxp_u128){0U, 1000U}, 1U, 2U);
    member(&observation.members.records[1], 2U, second_public,
           (lxp_u128){0U, 50U}, 1U, 4U);
    if (lxp_guarantor_set_validate(&observation.members) != LXP_OK)
        return 1;

    foreign = observation;
    foreign.paxeer_chain_id = 31338U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
            LXP_ERR_AUTH_SCOPE ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE)
        return 1;
    foreign = observation;
    foreign.guarantor_bond_contract[19] ^= 1U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
        LXP_ERR_AUTH_SCOPE)
        return 1;
    foreign = observation;
    foreign.minimum_bond = (lxp_u128){0U, 101U};
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
        LXP_ERR_CONTEXT_MISMATCH)
        return 1;
    foreign = observation;
    foreign.members.version = 6U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    foreign = observation;
    foreign.members.records[1].signer_authorizations[0].set_version = 0U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE)
        return 1;

    if (lxp_paxeer_membership_commitment(&observation, commitment) != LXP_OK ||
        lxp_paxeer_membership_sync(&state, &observation, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        state.guarantors.version != 5U || state.guarantors.count != 2U ||
        state.mirror_version != 5U ||
        state.membership.membership_version != 5U ||
        state.membership.observed_epoch != 4U ||
        state.membership.paxeer_chain_id != 31337U ||
        memcmp(state.membership.guarantor_bond_contract, contract, 20U) != 0 ||
        memcmp(state.membership.commitment, commitment, 32U) != 0)
        return 1;
    if (lxp_paxeer_bond_state_read(&state, first_id, &view, &eligible) !=
            LXP_OK ||
        !eligible || lxp_u128_cmp(view.bond_amount, (lxp_u128){0U, 1000U}) != 0 ||
        memcmp(view.public_key, first_public, 33U) != 0 ||
        lxp_paxeer_bond_state_read(&state, second_id, &view, &eligible) !=
            LXP_OK ||
        eligible || lxp_u128_cmp(view.bond_amount, (lxp_u128){0U, 50U}) != 0)
        return 1;
    if (lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return 1;

    foreign = observation;
    foreign.membership_version = 4U;
    foreign.members.version = 4U;
    foreign.members.records[0].signer_authorizations[0].set_version = 2U;
    foreign.members.records[1].signer_authorizations[0].set_version = 4U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
            LXP_ERR_SEQUENCE_MISMATCH ||
        state.membership.membership_version != 5U ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return 1;
    if (lxp_paxeer_membership_sync(&state, &observation, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        state.membership.membership_version != 5U)
        return 1;

    advanced = observation;
    advanced.membership_version = 6U;
    advanced.members.version = 6U;
    advanced.members.records[1].bond_amount = (lxp_u128){0U, 700U};
    if (lxp_paxeer_membership_commitment(&advanced, advanced_commitment) !=
            LXP_OK ||
        memcmp(advanced_commitment, commitment, 32U) == 0 ||
        lxp_paxeer_membership_sync(&state, &advanced, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        state.membership.membership_version != 6U ||
        memcmp(state.membership.commitment, advanced_commitment, 32U) != 0 ||
        lxp_paxeer_bond_state_read(&state, second_id, &view, &eligible) !=
            LXP_OK ||
        !eligible)
        return 1;

    if (lxp_paxeer_bond_deposit(&state, second_id, (lxp_u128){0U, 1U}) !=
            LXP_OK ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED)
        return 1;
    if (lxp_paxeer_membership_sync(&state, &advanced, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return 1;

    jailed = state.guarantors.records[1];
    jailed.active = false;
    jailed.jailed = true;
    if (lxp_guarantor_set_apply(&state.guarantors, 1U, true, &jailed) !=
            LXP_OK ||
        state.guarantors.version != 7U ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_STALE)
        return 1;
    if (lxp_paxeer_membership_sync(&state, &advanced, &availability) !=
        LXP_ERR_SEQUENCE_MISMATCH)
        return 1;

    tampered = state;
    tampered.guarantors = advanced.members;
    tampered.mirror_version = advanced.members.version;
    if (lxp_paxeer_membership_sync_status(&tampered, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND)
        return 1;
    tampered.membership.guarantor_bond_contract[0] ^= 1U;
    if (lxp_paxeer_membership_sync_status(&tampered, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED)
        return 1;
    tampered.membership.guarantor_bond_contract[0] ^= 1U;
    tampered.membership.commitment[31] ^= 1U;
    if (lxp_paxeer_membership_sync_status(&tampered, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED)
        return 1;
    tampered.membership.commitment[31] ^= 1U;
    tampered.membership.observed_epoch = 9U;
    if (lxp_paxeer_membership_sync_status(&tampered, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED)
        return 1;

    if (lxp_paxeer_membership_sync(NULL, &observation, &availability) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_sync(&state, NULL, &availability) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_sync(&state, &observation, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_commitment(NULL, commitment) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_commitment(&observation, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_sync_status(NULL, &availability) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_membership_sync_status(&state, NULL) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    (void)first_private;
    (void)second_private;
    return 0;
}
