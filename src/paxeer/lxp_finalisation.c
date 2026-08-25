#include "layerx/lxp_guarantor.h"

#include <string.h>

lxp_result lxp_da_withhold_sim(const lxp_da_bundle *bundle,
                               lxp_da_class withheld_class,
                               lxp_arena *arena,
                               lxp_da_bundle *served_bundle,
                               uint8_t *available_class_mask)
{
    lxp_da_chunk *chunks;
    void *memory;
    size_t kept = 0U;
    size_t total = 0U;
    size_t i;
    lxp_result status;
    if (bundle == NULL || bundle->chunks == NULL || arena == NULL ||
        served_bundle == NULL || available_class_mask == NULL ||
        withheld_class < LXP_DA_ACTIVITIES ||
        withheld_class > LXP_DA_RECOVERY_METADATA)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_arena_alloc(arena, bundle->chunk_count * sizeof(*chunks),
                             _Alignof(lxp_da_chunk), &memory);
    if (status != LXP_OK) return status;
    chunks = (lxp_da_chunk *)memory;
    *available_class_mask = 0U;
    for (i = 0U; i < bundle->chunk_count; ++i) {
        if (bundle->chunks[i].availability_class == withheld_class) continue;
        chunks[kept] = bundle->chunks[i];
        chunks[kept].chunk_index = (uint32_t)kept;
        status = lxp_da_chunk_hash(&chunks[kept]);
        if (status != LXP_OK) return status;
        *available_class_mask |= (uint8_t)(1U <<
            ((uint8_t)chunks[kept].availability_class - 1U));
        if (chunks[kept].length > SIZE_MAX - total)
            return LXP_ERR_LENGTH_LIMIT;
        total += chunks[kept].length;
        ++kept;
    }
    served_bundle->chunks = chunks;
    served_bundle->chunk_count = kept;
    served_bundle->batch_number = bundle->batch_number;
    served_bundle->total_bytes = total;
    return LXP_OK;
}

lxp_result lxp_checkpoint_block_on_da(lxp_finalisation_state *state,
                                      uint64_t checkpoint_batch_number,
                                      bool data_available)
{
    if (state == NULL || checkpoint_batch_number == 0U ||
        (state->checkpoint_finalized &&
         checkpoint_batch_number <= state->finalized_batch_number))
        return LXP_ERR_NON_CANONICAL;
    if (data_available) {
        if (state->blocked_checkpoint_batch_number == checkpoint_batch_number) {
            state->blocked_checkpoint_batch_number = 0U;
            state->unfinalized_checkpoint_blocked = false;
        }
        return LXP_OK;
    }
    state->blocked_checkpoint_batch_number = checkpoint_batch_number;
    state->unfinalized_checkpoint_blocked = true;
    state->pending_withdrawal_settlement_enabled = false;
    state->pending_deposit_settlement_enabled = false;
    state->pending_dispute_settlement_enabled = false;
    return LXP_ERR_DA_MISSING;
}

lxp_result lxp_da_unavailable_mode(lxp_finalisation_state *state,
                                   uint64_t affected_batch_number,
                                   bool data_available,
                                   bool governance_reconstituted)
{
    if (state == NULL || affected_batch_number == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (data_available || governance_reconstituted) {
        state->emergency_data_mode = false;
        state->emergency_exit_enabled = false;
        state->finalisation_halted = false;
        state->availability_incident_batch_number = 0U;
        if (governance_reconstituted) {
            state->unfinalized_checkpoint_blocked = false;
            state->blocked_checkpoint_batch_number = 0U;
        }
        return LXP_OK;
    }
    if (state->checkpoint_finalized &&
        affected_batch_number <= state->finalized_batch_number) {
        state->availability_incident_batch_number = affected_batch_number;
        state->emergency_data_mode = true;
        state->emergency_exit_enabled = true;
        state->finalisation_halted = true;
        return LXP_ERR_DA_MISSING;
    }
    return lxp_checkpoint_block_on_da(state, affected_batch_number, false);
}

static const lxp_guarantor_bond_state *bond_for(
    const lxp_guarantor_set *set, const uint8_t guarantor_id[32])
{
    size_t i;
    for (i = 0U; i < set->count; ++i)
        if (memcmp(set->records[i].guarantor_id, guarantor_id, 32U) == 0)
            return &set->records[i];
    return NULL;
}

lxp_result lxp_checkpoint_finalisable(
    lxp_finalisation_state *state, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *set,
    const lxp_finalisation_requirements *requirements,
    lxp_arena *arena, bool *finalisable)
{
    lxp_guarantor_key_record keys[LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t valid_signatures = 0U;
    size_t eligible_count = 0U;
    size_t i;
    lxp_result status;
    if (state == NULL || certificate == NULL || set == NULL ||
        requirements == NULL || arena == NULL || finalisable == NULL ||
        requirements->threshold == 0U ||
        requirements->threshold > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_guarantor_set_validate(set) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    *finalisable = false;
    if (state->finalisation_halted ||
        (state->unfinalized_checkpoint_blocked &&
         state->blocked_checkpoint_batch_number ==
             certificate->checkpoint.header.batch_number))
        return LXP_ERR_DA_MISSING;
    if (requirements->equivocation_detected)
        return LXP_ERR_EQUIVOCATION;
    if (!requirements->availability_challenges_answered)
        return LXP_ERR_DA_MISSING;
    if (requirements->now_ms < requirements->challenge_window_end_ms)
        return LXP_ERR_NOT_YET_VALID;
    if (memcmp(certificate->checkpoint.header.previous_state_root,
               state->settlement_anchor, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    if (certificate->threshold != requirements->threshold)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    (void)memset(keys, 0, sizeof(keys));
    for (i = 0U; i < set->count; ++i) {
        bool eligible = false;
        status = lxp_guarantor_eligible(&set->records[i],
                                        requirements->checkpoint_epoch,
                                        requirements->minimum_bond,
                                        &eligible);
        if (status != LXP_OK) return status;
        (void)memcpy(keys[i].guarantor_id, set->records[i].guarantor_id, 32U);
        status = lxp_guarantor_signer_at_epoch(
            &set->records[i], requirements->checkpoint_epoch,
            keys[i].public_key);
        keys[i].bonded = eligible && status == LXP_OK;
        if (status != LXP_OK && status != LXP_ERR_AUTH_SCOPE) return status;
    }
    status = lxp_guarantor_cert_verify(certificate, keys, set->count, arena,
                                       &valid_signatures);
    if (status != LXP_OK) return status;
    for (i = 0U; i < certificate->attestation_count; ++i) {
        const lxp_guarantor_attestation *attestation =
            &certificate->attestations[i];
        const lxp_guarantor_bond_state *bond =
            bond_for(set, attestation->guarantor_id);
        bool eligible = false;
        if (attestation->attested_at_ms > requirements->checkpoint_deadline_ms)
            continue;
        if (bond != NULL)
            status = lxp_guarantor_eligible(
                bond, requirements->checkpoint_epoch,
                requirements->minimum_bond, &eligible);
        if (status != LXP_OK) return status;
        if (eligible) ++eligible_count;
    }
    if (eligible_count < requirements->threshold ||
        valid_signatures < requirements->threshold)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    (void)memcpy(state->settlement_anchor,
                 certificate->checkpoint.header.resulting_state_root, 32U);
    state->finalized_batch_number =
        certificate->checkpoint.header.batch_number;
    state->checkpoint_finalized = true;
    state->withdrawal_settlement_enabled = true;
    state->deposit_settlement_enabled = true;
    state->dispute_settlement_enabled = true;
    state->pending_withdrawal_settlement_enabled = true;
    state->pending_deposit_settlement_enabled = true;
    state->pending_dispute_settlement_enabled = true;
    *finalisable = true;
    return LXP_OK;
}
