#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_guarantor_signer_authorization *current_authorization(
    lxp_guarantor_bond_state *bond_state)
{
    if (bond_state->signer_authorization_count == 0U) return NULL;
    return &bond_state->signer_authorizations[
        bond_state->signer_authorization_count - 1U];
}

static int valid_public_key(const uint8_t public_key[33])
{
    uint8_t address[20];
    return public_key != NULL &&
        lxp_secp256k1_address(public_key, 33U, address) == LXP_OK;
}

static int valid_signer_history(const lxp_guarantor_bond_state *bond_state)
{
    size_t i;
    if (bond_state->signer_authorization_count == 0U ||
        bond_state->signer_authorization_count >
            LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS ||
        bond_state->signer_authorizations[0].active_from_epoch !=
            bond_state->joined_epoch)
        return 0;
    for (i = 0U; i < bond_state->signer_authorization_count; ++i) {
        const lxp_guarantor_signer_authorization *authorization =
            &bond_state->signer_authorizations[i];
        if (!valid_public_key(authorization->public_key) ||
            authorization->active_from_epoch == 0U ||
            authorization->set_version == 0U ||
            (authorization->active_until_epoch != 0U &&
             authorization->active_until_epoch <=
                 authorization->active_from_epoch) ||
            (i != 0U &&
             (bond_state->signer_authorizations[i - 1U].active_until_epoch !=
                  authorization->active_from_epoch ||
              bond_state->signer_authorizations[i - 1U].set_version >=
                  authorization->set_version)))
            return 0;
    }
    if (memcmp(
            bond_state->public_key,
            bond_state->signer_authorizations[
                bond_state->signer_authorization_count - 1U].public_key,
            33U) != 0)
        return 0;
    if (bond_state->removed_epoch == 0U)
        return bond_state->signer_authorizations[
            bond_state->signer_authorization_count - 1U]
                   .active_until_epoch == 0U;
    return bond_state->signer_authorizations[
        bond_state->signer_authorization_count - 1U]
               .active_until_epoch == bond_state->removed_epoch;
}

lxp_result lxp_guarantor_set_validate(const lxp_guarantor_set *set)
{
    size_t i;
    size_t j;
    size_t left;
    size_t right;
    if (set == NULL || set->count > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        set->last_governance_sequence > set->version ||
        (set->count == 0U &&
         (set->version != 0U || set->last_governance_sequence != 0U)))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < set->count; ++i) {
        const lxp_guarantor_bond_state *record = &set->records[i];
        if (lxp_ct_is_zero(record->guarantor_id, 32U) ||
            record->joined_epoch == 0U || !valid_signer_history(record) ||
            record->ejected_at_version > set->version ||
            (record->ejected_at_version != 0U &&
             (record->active || !record->jailed)) ||
            (record->removed_epoch != 0U && record->active))
            return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < record->signer_authorization_count; ++j)
            if (record->signer_authorizations[j].set_version > set->version)
                return LXP_ERR_NON_CANONICAL;
    }
    for (i = 0U; i < set->count; ++i) {
        const lxp_guarantor_bond_state *record = &set->records[i];
        for (right = i + 1U; right < set->count; ++right) {
            if (memcmp(record->guarantor_id,
                       set->records[right].guarantor_id, 32U) == 0)
                return LXP_ERR_NON_CANONICAL;
            for (left = 0U; left < record->signer_authorization_count;
                 ++left)
                for (j = 0U;
                     j < set->records[right].signer_authorization_count;
                     ++j)
                    if (memcmp(
                            record->signer_authorizations[left].public_key,
                            set->records[right]
                                .signer_authorizations[j]
                                .public_key,
                            33U) == 0)
                        return LXP_ERR_NON_CANONICAL;
        }
    }
    return LXP_OK;
}

lxp_result lxp_guarantor_set_init(lxp_guarantor_set *set)
{
    if (set == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(set, 0, sizeof(*set));
    return LXP_OK;
}

lxp_result lxp_guarantor_set_apply(
    lxp_guarantor_set *set, uint64_t governance_sequence,
    bool ordered_governance_activity,
    const lxp_guarantor_bond_state *bond_state)
{
    size_t i;
    if (set == NULL || bond_state == NULL || !ordered_governance_activity)
        return LXP_ERR_AUTH_SCOPE;
    if (lxp_guarantor_set_validate(set) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (set->last_governance_sequence == UINT64_MAX ||
        governance_sequence != set->last_governance_sequence + 1U)
        return LXP_ERR_SEQUENCE_MISMATCH;
    if (set->version == UINT64_MAX) return LXP_ERR_OVERFLOW;
    for (i = 0U; i < set->count; ++i)
        if (memcmp(set->records[i].guarantor_id,
                   bond_state->guarantor_id, 32U) == 0) {
            lxp_guarantor_bond_state *current = &set->records[i];
            lxp_guarantor_signer_authorization *authorization =
                current_authorization(current);
            if (!valid_signer_history(current) ||
                bond_state->joined_epoch != current->joined_epoch ||
                memcmp(bond_state->public_key, current->public_key, 33U) != 0 ||
                bond_state->ejected_at_version != current->ejected_at_version ||
                (current->removed_epoch != 0U &&
                 bond_state->removed_epoch != current->removed_epoch) ||
                (current->removed_epoch == 0U &&
                 bond_state->removed_epoch != 0U &&
                 (authorization == NULL ||
                  bond_state->removed_epoch <= authorization->active_from_epoch)) ||
                (bond_state->removed_epoch != 0U && bond_state->active) ||
                (current->ejected_at_version != 0U && bond_state->active))
                return LXP_ERR_NON_CANONICAL;
            if (current->removed_epoch == 0U &&
                bond_state->removed_epoch != 0U) {
                authorization->active_until_epoch = bond_state->removed_epoch;
            }
            current->bond_amount = bond_state->bond_amount;
            current->removed_epoch = bond_state->removed_epoch;
            current->jailed = bond_state->jailed;
            current->unresolved_slashing = bond_state->unresolved_slashing;
            current->active = bond_state->active;
            set->last_governance_sequence = governance_sequence;
            ++set->version;
            return LXP_OK;
        }
    if (set->count == LXP_MAX_GUARANTOR_ATTESTATIONS ||
        !bond_state->active || bond_state->joined_epoch == 0U ||
        bond_state->removed_epoch != 0U || bond_state->ejected_at_version != 0U ||
        lxp_ct_is_zero(bond_state->guarantor_id, 32U) ||
        !valid_public_key(bond_state->public_key))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < set->count; ++i) {
        size_t authorization_index;
        for (authorization_index = 0U;
             authorization_index <
                 set->records[i].signer_authorization_count;
             ++authorization_index)
            if (memcmp(
                    set->records[i]
                        .signer_authorizations[authorization_index]
                        .public_key,
                    bond_state->public_key, 33U) == 0)
                return LXP_ERR_NON_CANONICAL;
    }
    set->records[set->count] = *bond_state;
    set->records[set->count].signer_authorization_count = 1U;
    (void)memset(set->records[set->count].signer_authorizations, 0,
                 sizeof(set->records[set->count].signer_authorizations));
    (void)memcpy(
        set->records[set->count].signer_authorizations[0].public_key,
        bond_state->public_key, 33U);
    set->records[set->count].signer_authorizations[0].active_from_epoch =
        bond_state->joined_epoch;
    set->records[set->count].signer_authorizations[0].set_version =
        set->version + 1U;
    ++set->count;
    set->last_governance_sequence = governance_sequence;
    ++set->version;
    return LXP_OK;
}

lxp_result lxp_guarantor_set_rotate_signer(
    lxp_guarantor_set *set, uint64_t governance_sequence,
    bool ordered_governance_activity, const uint8_t guarantor_id[32],
    const uint8_t public_key[33], uint64_t activation_epoch)
{
    size_t i;
    size_t j;
    if (set == NULL || guarantor_id == NULL ||
        !ordered_governance_activity || !valid_public_key(public_key))
        return LXP_ERR_AUTH_SCOPE;
    if (lxp_guarantor_set_validate(set) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    if (set->last_governance_sequence == UINT64_MAX ||
        governance_sequence != set->last_governance_sequence + 1U)
        return LXP_ERR_SEQUENCE_MISMATCH;
    if (set->version == UINT64_MAX) return LXP_ERR_OVERFLOW;
    for (i = 0U; i < set->count; ++i)
        for (j = 0U; j < set->records[i].signer_authorization_count; ++j)
            if (memcmp(set->records[i].signer_authorizations[j].public_key,
                       public_key, 33U) == 0)
                return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < set->count; ++i)
        if (memcmp(set->records[i].guarantor_id, guarantor_id, 32U) == 0) {
            lxp_guarantor_bond_state *record = &set->records[i];
            lxp_guarantor_signer_authorization *current =
                current_authorization(record);
            lxp_guarantor_signer_authorization *next;
            if (!record->active || record->jailed ||
                record->removed_epoch != 0U ||
                record->ejected_at_version != 0U || current == NULL ||
                current->active_until_epoch != 0U ||
                activation_epoch <= current->active_from_epoch ||
                record->signer_authorization_count ==
                    LXP_MAX_GUARANTOR_SIGNER_AUTHORIZATIONS)
                return LXP_ERR_NON_CANONICAL;
            current->active_until_epoch = activation_epoch;
            next = &record->signer_authorizations[
                record->signer_authorization_count++];
            (void)memset(next, 0, sizeof(*next));
            (void)memcpy(next->public_key, public_key, 33U);
            next->active_from_epoch = activation_epoch;
            next->set_version = set->version + 1U;
            (void)memcpy(record->public_key, public_key, 33U);
            set->last_governance_sequence = governance_sequence;
            ++set->version;
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lxp_guarantor_signer_authorized(
    const lxp_guarantor_bond_state *bond_state,
    const uint8_t public_key[33], uint64_t checkpoint_epoch,
    bool *authorized)
{
    size_t i;
    if (bond_state == NULL || public_key == NULL || authorized == NULL ||
        checkpoint_epoch == 0U || !valid_signer_history(bond_state))
        return LXP_ERR_NON_CANONICAL;
    *authorized = false;
    for (i = 0U; i < bond_state->signer_authorization_count; ++i) {
        const lxp_guarantor_signer_authorization *authorization =
            &bond_state->signer_authorizations[i];
        if (memcmp(authorization->public_key, public_key, 33U) == 0 &&
            authorization->active_from_epoch <= checkpoint_epoch &&
            (authorization->active_until_epoch == 0U ||
             checkpoint_epoch < authorization->active_until_epoch)) {
            *authorized = true;
            return LXP_OK;
        }
    }
    return LXP_OK;
}

lxp_result lxp_guarantor_signer_at_epoch(
    const lxp_guarantor_bond_state *bond_state, uint64_t checkpoint_epoch,
    uint8_t public_key[33])
{
    size_t i;
    if (bond_state == NULL || public_key == NULL || checkpoint_epoch == 0U ||
        !valid_signer_history(bond_state))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < bond_state->signer_authorization_count; ++i) {
        const lxp_guarantor_signer_authorization *authorization =
            &bond_state->signer_authorizations[i];
        if (authorization->active_from_epoch <= checkpoint_epoch &&
            (authorization->active_until_epoch == 0U ||
             checkpoint_epoch < authorization->active_until_epoch)) {
            (void)memcpy(public_key, authorization->public_key, 33U);
            return LXP_OK;
        }
    }
    return LXP_ERR_AUTH_SCOPE;
}

lxp_result lxp_guarantor_eligible(
    const lxp_guarantor_bond_state *bond_state, uint64_t checkpoint_epoch,
    lxp_u128 minimum_bond, bool *eligible)
{
    if (bond_state == NULL || eligible == NULL || checkpoint_epoch == 0U ||
        !valid_signer_history(bond_state))
        return LXP_ERR_NON_CANONICAL;
    *eligible = bond_state->active && !bond_state->jailed &&
        !bond_state->unresolved_slashing &&
        bond_state->ejected_at_version == 0U &&
        bond_state->joined_epoch <= checkpoint_epoch &&
        (bond_state->removed_epoch == 0U ||
         bond_state->removed_epoch > checkpoint_epoch) &&
        lxp_u128_cmp(bond_state->bond_amount, minimum_bond) >= 0;
    return LXP_OK;
}
