#include "layerx/lxp_paxeer.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

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
    bond = find_bond(state, guarantor_id);
    if (bond == NULL || bond->removed_epoch != 0U ||
        bond->ejected_at_version != 0U)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_u128_add(bond->bond_amount, amount, &updated);
    if (status == LXP_OK) bond->bond_amount = updated;
    return status;
}

lxp_result lxp_paxeer_membership_sync_status(
    const lxp_paxeer_bond_state *state,
    lxp_paxeer_membership_sync_availability *availability)
{
    if (state == NULL || availability == NULL)
        return LXP_ERR_NON_CANONICAL;
    *availability = LXP_PAXEER_MEMBERSHIP_SYNC_UNAVAILABLE;
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
