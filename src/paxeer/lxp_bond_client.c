#include "layerx/lxp_paxeer.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

#define LXP_PAXEER_MEMBERSHIP_TAG "LXP/Paxeer/membership-mirror/v1"

static void put64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> (56U - 8U * i));
}

static void put32(uint8_t out[4], uint32_t value)
{
    size_t i;
    for (i = 0U; i < 4U; ++i)
        out[i] = (uint8_t)(value >> (24U - 8U * i));
}

static lxp_result absorb(lxp_hash_context *context, const void *data,
                         size_t length, lxp_result status)
{
    if (status != LXP_OK) return status;
    return lxp_hash_update(context, data, length);
}

static lxp_result absorb64(lxp_hash_context *context, uint64_t value,
                           lxp_result status)
{
    uint8_t encoded[8];
    put64(encoded, value);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result absorb32(lxp_hash_context *context, uint32_t value,
                           lxp_result status)
{
    uint8_t encoded[4];
    put32(encoded, value);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result absorb128(lxp_hash_context *context, lxp_u128 value,
                            lxp_result status)
{
    uint8_t encoded[16];
    if (status != LXP_OK) return status;
    status = lxp_u128_to_be(value, encoded);
    return absorb(context, encoded, sizeof(encoded), status);
}

static lxp_result membership_commitment(
    uint64_t paxeer_chain_id, const uint8_t contract[20],
    uint64_t membership_version, uint64_t observed_epoch,
    lxp_u128 minimum_bond, const lxp_guarantor_set *members,
    uint8_t commitment[32])
{
    lxp_hash_context context;
    size_t i;
    size_t j;
    lxp_result status;
    if (members->count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_LENGTH_LIMIT;
    lxp_hash_init(&context);
    status = absorb(&context, LXP_PAXEER_MEMBERSHIP_TAG,
                    sizeof(LXP_PAXEER_MEMBERSHIP_TAG), LXP_OK);
    status = absorb64(&context, paxeer_chain_id, status);
    status = absorb(&context, contract, 20U, status);
    status = absorb64(&context, membership_version, status);
    status = absorb64(&context, observed_epoch, status);
    status = absorb128(&context, minimum_bond, status);
    status = absorb64(&context, members->last_governance_sequence, status);
    status = absorb32(&context, (uint32_t)members->count, status);
    for (i = 0U; i < members->count; ++i) {
        const lxp_guarantor_bond_state *record = &members->records[i];
        uint8_t flags = (uint8_t)((record->active ? 1U : 0U) |
                                  (record->jailed ? 2U : 0U) |
                                  (record->unresolved_slashing ? 4U : 0U));
        if (record->signer_authorization_count >
            LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS) {
            lxp_secure_zero(&context, sizeof(context));
            return LXP_ERR_LENGTH_LIMIT;
        }
        status = absorb(&context, record->guarantor_id, 32U, status);
        status = absorb(&context, record->public_key, 33U, status);
        status = absorb128(&context, record->bond_amount, status);
        status = absorb64(&context, record->joined_epoch, status);
        status = absorb64(&context, record->removed_epoch, status);
        status = absorb64(&context, record->ejected_at_version, status);
        status = absorb(&context, &flags, 1U, status);
        status = absorb32(&context,
                          (uint32_t)record->signer_authorization_count,
                          status);
        for (j = 0U; j < record->signer_authorization_count; ++j) {
            const lxp_guarantor_signer_authorization *authorization =
                &record->signer_authorizations[j];
            status = absorb(&context, authorization->public_key, 33U, status);
            status = absorb64(&context, authorization->active_from_epoch,
                              status);
            status = absorb64(&context, authorization->active_until_epoch,
                              status);
            status = absorb64(&context, authorization->set_version, status);
        }
    }
    if (status != LXP_OK) {
        lxp_secure_zero(&context, sizeof(context));
        return status;
    }
    return lxp_hash_final(&context, commitment);
}

static lxp_guarantor_bond_state *find_bond(
    lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32])
{
    size_t i;
    for (i = 0U; i < state->guarantors.count; ++i)
        if (memcmp(state->guarantors.records[i].guarantor_id,
                   guarantor_id, 32U) == 0)
            return &state->guarantors.records[i];
    return NULL;
}

lxp_result lxp_paxeer_bond_init(lxp_paxeer_bond_state *state,
                                 uint16_t protocol_version,
                                 uint32_t network_id,
                                 uint64_t paxeer_chain_id,
                                 const uint8_t paxeer_contract[20],
                                 lxp_u128 custodied_value,
                                 uint32_t minimum_bond_bps)
{
    lxp_result status;
    if (state == NULL || paxeer_contract == NULL ||
        !lxp_protocol_version_supported(protocol_version) || network_id == 0U ||
        paxeer_chain_id == 0U || lxp_ct_is_zero(paxeer_contract, 20U) ||
        minimum_bond_bps == 0U ||
        minimum_bond_bps > LXP_BASIS_POINTS_ONE)
        return LXP_ERR_PARAMETER_BOUNDS;
    (void)memset(state, 0, sizeof(*state));
    status = lxp_guarantor_set_init(&state->guarantors);
    if (status == LXP_OK)
        status = lxp_u128_mul_bps_ceil(custodied_value, minimum_bond_bps,
                                       &state->minimum_bond);
    if (status != LXP_OK) return status;
    state->protocol_version = protocol_version;
    state->network_id = network_id;
    state->paxeer_chain_id = paxeer_chain_id;
    (void)memcpy(state->paxeer_settlement_contract, paxeer_contract, 20U);
    state->custodied_value = custodied_value;
    state->minimum_bond_bps = minimum_bond_bps;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_deposit(
    lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32],
    lxp_u128 amount)
{
    lxp_guarantor_bond_state *bond;
    lxp_u128 updated;
    lxp_result status;
    if (state == NULL || guarantor_id == NULL || lxp_u128_is_zero(amount))
        return LXP_ERR_NON_CANONICAL;
    if (lxp_guarantor_set_validate(&state->guarantors) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    bond = find_bond(state, guarantor_id);
    if (bond == NULL || bond->removed_epoch != 0U ||
        bond->ejected_at_version != 0U)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_u128_add(bond->bond_amount, amount, &updated);
    if (status == LXP_OK) bond->bond_amount = updated;
    return status;
}

lxp_result lxp_paxeer_membership_commitment(
    const lxp_paxeer_membership_observation *observation,
    uint8_t commitment[32])
{
    if (observation == NULL || commitment == NULL)
        return LXP_ERR_NON_CANONICAL;
    return membership_commitment(observation->paxeer_chain_id,
                                 observation->guarantor_bond_contract,
                                 observation->membership_version,
                                 observation->observed_epoch,
                                 observation->minimum_bond,
                                 &observation->members, commitment);
}

lxp_result lxp_paxeer_membership_sync(
    lxp_paxeer_bond_state *state,
    const lxp_paxeer_membership_observation *observation,
    lxp_paxeer_membership_sync_availability *availability)
{
    uint8_t commitment[32];
    lxp_result status;
    if (state == NULL || observation == NULL || availability == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_protocol_version_supported(state->protocol_version) ||
        state->network_id == 0U || state->paxeer_chain_id == 0U ||
        lxp_ct_is_zero(state->paxeer_settlement_contract, 20U))
        return LXP_ERR_NON_CANONICAL;
    if (observation->paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(observation->guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    if (observation->membership_version == 0U ||
        observation->observed_epoch == 0U ||
        observation->members.count == 0U ||
        observation->members.version != observation->membership_version)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_guarantor_set_validate(&observation->members);
    if (status != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_cmp(observation->minimum_bond, state->minimum_bond) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (observation->membership_version <
            state->membership.membership_version ||
        observation->membership_version < state->guarantors.version)
        return LXP_ERR_SEQUENCE_MISMATCH;
    status = lxp_paxeer_membership_commitment(observation, commitment);
    if (status != LXP_OK)
        return status;
    state->guarantors = observation->members;
    state->mirror_version = observation->members.version;
    state->membership.paxeer_chain_id = observation->paxeer_chain_id;
    (void)memcpy(state->membership.guarantor_bond_contract,
                 observation->guarantor_bond_contract, 20U);
    state->membership.membership_version = observation->membership_version;
    state->membership.observed_epoch = observation->observed_epoch;
    (void)memcpy(state->membership.commitment, commitment,
                 sizeof(state->membership.commitment));
    return lxp_paxeer_membership_sync_status(state, availability);
}

lxp_result lxp_paxeer_membership_sync_status(
    const lxp_paxeer_bond_state *state,
    lxp_paxeer_membership_sync_availability *availability)
{
    uint8_t commitment[32];
    lxp_result status;
    if (state == NULL || availability == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (state->membership.membership_version == 0U &&
        lxp_ct_is_zero(state->membership.commitment,
                       sizeof(state->membership.commitment))) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE;
        return LXP_OK;
    }
    if (state->membership.membership_version == 0U ||
        lxp_ct_is_zero(state->membership.commitment,
                       sizeof(state->membership.commitment)) ||
        state->membership.paxeer_chain_id != state->paxeer_chain_id ||
        memcmp(state->membership.guarantor_bond_contract,
               state->paxeer_settlement_contract, 20U) != 0) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED;
        return LXP_OK;
    }
    if (state->guarantors.version != state->membership.membership_version ||
        state->mirror_version != state->membership.membership_version) {
        *availability = LXP_PAXEER_MEMBERSHIP_SYNC_STALE;
        return LXP_OK;
    }
    status = membership_commitment(state->paxeer_chain_id,
                                   state->paxeer_settlement_contract,
                                   state->membership.membership_version,
                                   state->membership.observed_epoch,
                                   state->minimum_bond, &state->guarantors,
                                   commitment);
    if (status != LXP_OK)
        return status;
    *availability =
        lxp_ct_memcmp(commitment, state->membership.commitment,
                      sizeof(state->membership.commitment)) == 0
            ? LXP_PAXEER_MEMBERSHIP_SYNC_BOUND
            : LXP_PAXEER_MEMBERSHIP_SYNC_DIVERGED;
    return LXP_OK;
}

lxp_result lxp_paxeer_bond_state_read(
    const lxp_paxeer_bond_state *state, const uint8_t guarantor_id[32],
    lxp_guarantor_bond_state *bond, bool *threshold_eligible)
{
    size_t i;
    if (state == NULL || guarantor_id == NULL || bond == NULL ||
        threshold_eligible == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_guarantor_set_validate(&state->guarantors) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < state->guarantors.count; ++i)
        if (memcmp(state->guarantors.records[i].guarantor_id,
                   guarantor_id, 32U) == 0) {
            *bond = state->guarantors.records[i];
            *threshold_eligible = bond->active && !bond->jailed &&
                !bond->unresolved_slashing && bond->removed_epoch == 0U &&
                bond->ejected_at_version == 0U &&
                lxp_u128_cmp(bond->bond_amount, state->minimum_bond) >= 0;
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_paxeer_slash_submit(
    lxp_paxeer_bond_state *state, const uint8_t *evidence_bytes,
    size_t evidence_length, const lxp_equivocation_evidence *evidence,
    lxp_arena *arena)
{
    lxp_byte_span canonical;
    size_t mark;
    lxp_result status;
    if (state == NULL || evidence_bytes == NULL || evidence_length == 0U ||
        evidence == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (evidence->kind == LXP_EQUIVOCATION_GUARANTOR &&
        (evidence->guarantor_first.protocol_version !=
             state->protocol_version ||
         evidence->guarantor_first.network_id != state->network_id ||
         evidence->guarantor_first.paxeer_chain_id !=
             state->paxeer_chain_id ||
         memcmp(evidence->guarantor_first.paxeer_settlement_contract,
                state->paxeer_settlement_contract, 20U) != 0))
        return LXP_ERR_AUTH_SCOPE;
    mark = lxp_arena_mark(arena);
    status = lxp_equivocation_encode(evidence, arena, &canonical);
    if (status == LXP_OK &&
        (canonical.length != evidence_length ||
         lxp_ct_memcmp(canonical.bytes, evidence_bytes,
                       evidence_length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_slashing_submit(evidence, &state->guarantors, arena);
    (void)lxp_arena_reset(arena, mark);
    if (status == LXP_OK) state->mirror_version = state->guarantors.version;
    return status;
}
