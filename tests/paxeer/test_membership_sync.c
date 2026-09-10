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
    uint8_t binding_bytes[LXP_PAXEER_BOND_BINDING_MAX_SIZE];
    size_t binding_length = 0U;
    lxp_paxeer_bond_state state;
    lxp_paxeer_bond_state tampered;
    lxp_paxeer_bond_state restored;
    lxp_paxeer_bond_binding binding;
    lxp_paxeer_bond_deposit_evidence evidence;
    lxp_paxeer_bond_deposit_evidence rejected;
    lxp_paxeer_bond_deposit_record proof;
    lxp_paxeer_membership_observation observation;
    lxp_paxeer_membership_observation foreign;
    lxp_paxeer_membership_observation advanced;
    lxp_paxeer_membership_observation settled;
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
    observation.observed_block_number = 900U;
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
    foreign.observed_block_number = 0U;
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
        state.membership.observed_block_number != 900U ||
        state.membership.last_governance_sequence != 0U ||
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
    foreign.observed_block_number = 899U;
    if (lxp_paxeer_membership_sync(&state, &foreign, &availability) !=
        LXP_ERR_SEQUENCE_MISMATCH)
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
    advanced.observed_block_number = 940U;
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

    (void)memset(&evidence, 0, sizeof(evidence));
    evidence.paxeer_chain_id = 31337U;
    (void)memcpy(evidence.guarantor_bond_contract, contract, 20U);
    (void)memcpy(evidence.guarantor_id, second_id, 32U);
    evidence.transaction_id[31] = 0xd1U;
    evidence.observed_block_number = 940U;
    evidence.observed_at_ms = 1700000000000ULL;
    evidence.membership_version = 6U;
    evidence.amount = (lxp_u128){0U, 650U};
    evidence.total_bond = (lxp_u128){0U, 700U};
    if (lxp_paxeer_bond_deposit(NULL, &evidence) != LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_bond_deposit(&state, NULL) != LXP_ERR_NON_CANONICAL)
        return 1;
    rejected = evidence;
    rejected.amount = (lxp_u128){0U, 0U};
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_NON_CANONICAL)
        return 1;
    rejected = evidence;
    rejected.amount = (lxp_u128){0U, 701U};
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_NON_CANONICAL)
        return 1;
    rejected = evidence;
    (void)memset(rejected.transaction_id, 0, 32U);
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_NON_CANONICAL)
        return 1;
    rejected = evidence;
    rejected.observed_at_ms = 0U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_NON_CANONICAL)
        return 1;
    rejected = evidence;
    rejected.paxeer_chain_id = 31338U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_AUTH_SCOPE)
        return 1;
    rejected = evidence;
    rejected.guarantor_bond_contract[19] ^= 1U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_AUTH_SCOPE)
        return 1;
    rejected = evidence;
    rejected.membership_version = 7U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_SEQUENCE_MISMATCH)
        return 1;
    rejected = evidence;
    rejected.observed_block_number = 941U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_SEQUENCE_MISMATCH)
        return 1;
    rejected = evidence;
    rejected.guarantor_id[31] = 9U;
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_AUTH_SCOPE)
        return 1;
    rejected = evidence;
    rejected.total_bond = (lxp_u128){0U, 999U};
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_CONSERVATION)
        return 1;
    rejected = evidence;
    rejected.membership_version = 5U;
    rejected.observed_block_number = 900U;
    rejected.amount = (lxp_u128){0U, 900U};
    rejected.total_bond = (lxp_u128){0U, 900U};
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_ERR_CONSERVATION)
        return 1;
    if (lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        state.deposit_count != 0U)
        return 1;

    if (lxp_paxeer_bond_deposit(&state, &evidence) != LXP_OK ||
        state.deposit_count != 1U || state.mirror_version != 6U ||
        lxp_paxeer_bond_deposit(&state, &evidence) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        lxp_u128_cmp(state.guarantors.records[1].bond_amount,
                     (lxp_u128){0U, 700U}) != 0)
        return 1;
    if (lxp_paxeer_bond_deposit_proof(&state, evidence.transaction_id,
                                      &proof) != LXP_OK ||
        memcmp(proof.guarantor_id, second_id, 32U) != 0 ||
        proof.observed_block_number != 940U ||
        proof.observed_at_ms != 1700000000000ULL ||
        proof.membership_version != 6U ||
        lxp_u128_cmp(proof.amount, (lxp_u128){0U, 650U}) != 0 ||
        lxp_u128_cmp(proof.total_bond, (lxp_u128){0U, 700U}) != 0)
        return 1;
    rejected = evidence;
    rejected.transaction_id[31] = 0xd2U;
    if (lxp_paxeer_bond_deposit_proof(&state, rejected.transaction_id,
                                      &proof) != LXP_ERR_UNKNOWN_FIELD)
        return 1;
    rejected = evidence;
    rejected.transaction_id[31] = 0xd3U;
    rejected.membership_version = 5U;
    rejected.observed_block_number = 900U;
    rejected.amount = (lxp_u128){0U, 50U};
    rejected.total_bond = (lxp_u128){0U, 50U};
    if (lxp_paxeer_bond_deposit(&state, &rejected) != LXP_OK ||
        state.deposit_count != 2U ||
        lxp_paxeer_bond_deposit_proof(&state, rejected.transaction_id,
                                      &proof) != LXP_OK ||
        proof.membership_version != 5U ||
        lxp_u128_cmp(proof.total_bond, (lxp_u128){0U, 50U}) != 0)
        return 1;

    settled = advanced;
    settled.membership_version = 7U;
    settled.members.version = 7U;
    settled.observed_block_number = 941U;
    settled.members.records[1].bond_amount = (lxp_u128){0U, 701U};
    if (lxp_paxeer_membership_sync(&state, &settled, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        state.deposit_count != 2U)
        return 1;
    if (lxp_paxeer_membership_sync(&state, &advanced, &availability) !=
        LXP_ERR_SEQUENCE_MISMATCH)
        return 1;

    if (lxp_paxeer_bond_binding_encode(&state, binding_bytes,
                                       sizeof(binding_bytes),
                                       &binding_length) != LXP_OK ||
        binding_length != (size_t)LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
                              2U * (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE +
                              32U ||
        lxp_paxeer_bond_binding_encode(&state, binding_bytes,
                                       binding_length - 1U,
                                       &binding_length) != LXP_ERR_LENGTH_LIMIT)
        return 1;
    binding_length = (size_t)LXP_PAXEER_BOND_BINDING_PREFIX_SIZE +
                     2U * (size_t)LXP_PAXEER_BOND_DEPOSIT_ENCODED_SIZE + 32U;
    if (lxp_paxeer_bond_binding_decode(binding_bytes, binding_length,
                                       &binding) != LXP_OK ||
        binding.protocol_version != 2U || binding.network_id != 42U ||
        binding.minimum_bond_bps != 100U || binding.mirror_version != 7U ||
        binding.deposit_count != 2U ||
        binding.membership.paxeer_chain_id != 31337U ||
        binding.membership.membership_version != 7U ||
        binding.membership.observed_block_number != 941U ||
        binding.membership.observed_epoch != 4U ||
        memcmp(binding.membership.commitment, state.membership.commitment,
               32U) != 0 ||
        memcmp(binding.deposits[0].transaction_id, evidence.transaction_id,
               32U) != 0 ||
        lxp_u128_cmp(binding.custodied_value, (lxp_u128){0U, 10000U}) != 0)
        return 1;
    if (lxp_paxeer_bond_binding_decode(binding_bytes, binding_length - 1U,
                                       &binding) != LXP_ERR_LENGTH_LIMIT)
        return 1;
    binding_bytes[70] ^= 1U;
    if (lxp_paxeer_bond_binding_decode(binding_bytes, binding_length,
                                       &binding) != LXP_ERR_NON_CANONICAL)
        return 1;
    binding_bytes[70] ^= 1U;
    if (lxp_paxeer_bond_binding_decode(binding_bytes, binding_length,
                                       &binding) != LXP_OK)
        return 1;

    if (lxp_paxeer_bond_init(&restored, 2U, 42U, 31337U, contract,
                             (lxp_u128){0U, 10000U}, 100U) != LXP_OK ||
        lxp_paxeer_bond_binding_adopt(&restored, &binding) !=
            LXP_ERR_CONTEXT_MISMATCH ||
        lxp_paxeer_membership_sync(&restored, &advanced, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        lxp_paxeer_bond_binding_adopt(&restored, &binding) !=
            LXP_ERR_SEQUENCE_MISMATCH ||
        restored.deposit_count != 0U)
        return 1;
    if (lxp_paxeer_membership_sync(&restored, &settled, &availability) !=
            LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_BOUND ||
        lxp_paxeer_bond_binding_adopt(&restored, &binding) != LXP_OK ||
        restored.deposit_count != 2U ||
        memcmp(restored.deposits[0].transaction_id, evidence.transaction_id,
               32U) != 0 ||
        lxp_paxeer_bond_binding_adopt(&restored, &binding) != LXP_OK ||
        restored.deposit_count != 2U)
        return 1;
    binding.membership.guarantor_bond_contract[0] ^= 1U;
    if (lxp_paxeer_bond_binding_adopt(&restored, &binding) != LXP_ERR_AUTH_SCOPE)
        return 1;
    binding.membership.guarantor_bond_contract[0] ^= 1U;
    binding.minimum_bond_bps = 200U;
    if (lxp_paxeer_bond_binding_adopt(&restored, &binding) !=
        LXP_ERR_CONTEXT_MISMATCH)
        return 1;
    binding.minimum_bond_bps = 100U;
    if (lxp_paxeer_bond_binding_encode(NULL, binding_bytes,
                                       sizeof(binding_bytes),
                                       &binding_length) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_bond_binding_decode(NULL, binding_length, &binding) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_bond_binding_adopt(NULL, &binding) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_bond_binding_adopt(&restored, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_paxeer_bond_deposit_proof(&restored, NULL, &proof) !=
            LXP_ERR_NON_CANONICAL)
        return 1;

    jailed = state.guarantors.records[1];
    jailed.active = false;
    jailed.jailed = true;
    if (lxp_guarantor_set_apply(&state.guarantors, 1U, true, &jailed) !=
            LXP_OK ||
        state.guarantors.version != 8U ||
        lxp_paxeer_membership_sync_status(&state, &availability) != LXP_OK ||
        availability != LXP_PAXEER_MEMBERSHIP_SYNC_STALE)
        return 1;
    if (lxp_paxeer_membership_sync(&state, &advanced, &availability) !=
        LXP_ERR_SEQUENCE_MISMATCH)
        return 1;

    tampered = state;
    tampered.guarantors = settled.members;
    tampered.mirror_version = settled.members.version;
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
