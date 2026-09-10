#include "layerx/lxp_batch.h"
#include "layerx/lxp_crypto.h"

#include <string.h>

lxp_result lxp_batch_eligibility_init(
    lxp_batch_eligibility_state *state, uint64_t batch_number,
    const uint8_t (*replica_ids)[32], size_t replica_count, size_t threshold)
{
    size_t i;
    size_t j;
    if (state == NULL || replica_ids == NULL || replica_count == 0U ||
        replica_count > LXP_MAX_BATCH_REPLICAS || threshold == 0U ||
        threshold > replica_count) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < replica_count; ++i) {
        if (lxp_ct_is_zero(replica_ids[i], 32U)) return LXP_ERR_NON_CANONICAL;
        for (j = 0U; j < i; ++j)
            if (lxp_ct_memcmp(replica_ids[i], replica_ids[j], 32U) == 0)
                return LXP_ERR_NON_CANONICAL;
    }
    (void)memset(state, 0, sizeof(*state));
    state->batch_number = batch_number;
    state->replica_count = replica_count;
    state->threshold = threshold;
    (void)memcpy(state->replica_ids, replica_ids, replica_count * 32U);
    return LXP_OK;
}

lxp_result lxp_batch_eligibility_reset(lxp_batch_eligibility_state *state,
                                       uint64_t batch_number)
{
    size_t i;
    if (state == NULL || state->replica_count == 0U ||
        state->replica_count > LXP_MAX_BATCH_REPLICAS ||
        state->threshold == 0U || state->threshold > state->replica_count ||
        state->acknowledgement_count > state->replica_count)
        return LXP_ERR_NON_CANONICAL;
    if (batch_number < state->batch_number) return LXP_ERR_BATCH_GAP;
    if (batch_number == state->batch_number) return LXP_OK;
    for (i = 0U; i < LXP_MAX_BATCH_REPLICAS; ++i) state->acknowledged[i] = 0U;
    state->acknowledgement_count = 0U;
    state->batch_number = batch_number;
    return LXP_OK;
}

lxp_result lxp_replica_ack(lxp_batch_eligibility_state *state,
                           const uint8_t replica_id[32], lxp_log *log)
{
    size_t i;
    lxp_result status;
    if (state == NULL || replica_id == NULL || log == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < state->replica_count; ++i)
        if (lxp_ct_memcmp(replica_id, state->replica_ids[i], 32U) == 0)
            break;
    if (i == state->replica_count) return LXP_ERR_AUTH_SCOPE;
    if (state->acknowledged[i] != 0U) return LXP_OK;
    status = lxp_log_append(log, LXP_LOG_REPLICA_ACK, state->batch_number,
                            replica_id, 32U, NULL);
    if (status == LXP_OK) status = lxp_log_write_boundary(log);
    if (status != LXP_OK) return status;
    state->acknowledged[i] = 1U;
    state->acknowledgement_count += 1U;
    return LXP_OK;
}

lxp_result lxp_batch_eligibility(const lxp_batch_eligibility_state *state,
                                 bool *eligible)
{
    if (state == NULL || eligible == NULL || state->threshold == 0U ||
        state->threshold > state->replica_count ||
        state->acknowledgement_count > state->replica_count)
        return LXP_ERR_NON_CANONICAL;
    *eligible = state->acknowledgement_count >= state->threshold;
    return *eligible ? LXP_OK : LXP_ERR_ATTESTATION_THRESHOLD;
}
