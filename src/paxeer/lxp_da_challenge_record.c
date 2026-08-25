#include "layerx/lxp_guarantor.h"

#include <string.h>

lxp_result lxp_da_challenge_registry_init(
    lxp_da_challenge_registry *registry,
    lxp_da_evidence_publish_fn publish_evidence, void *publish_context)
{
    if (registry == NULL || publish_evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(registry, 0, sizeof(*registry));
    registry->publish_evidence = publish_evidence;
    registry->publish_context = publish_context;
    return LXP_OK;
}

static lxp_result append_record(lxp_da_challenge_registry *registry,
                                const lxp_da_challenge *challenge,
                                lxp_result outcome, bool answered,
                                bool slashable)
{
    lxp_da_challenge_record *record;
    size_t i;
    if (registry == NULL || challenge == NULL ||
        registry->count == LXP_MAX_DA_CHALLENGE_RECORDS)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < registry->count; ++i)
        if (memcmp(registry->records[i].challenge_id,
                   challenge->challenge_id, 32U) == 0)
            return LXP_ERR_NON_CANONICAL;
    record = &registry->records[registry->count++];
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->challenge_id, challenge->challenge_id, 32U);
    (void)memcpy(record->guarantor_id,
                 challenge->signed_commitment.guarantor_id, 32U);
    record->batch_number = challenge->batch_number;
    record->outcome = outcome;
    record->answered = answered;
    record->slashable = slashable;
    return LXP_OK;
}

lxp_result lxp_da_challenge_record_success(
    lxp_da_challenge_registry *registry,
    const lxp_da_challenge *challenge)
{
    return append_record(registry, challenge, LXP_OK, true, false);
}

lxp_result lxp_da_challenge_record_failure(
    lxp_da_challenge_registry *registry,
    const lxp_da_failure_evidence *evidence, lxp_guarantor_set *set)
{
    size_t i;
    lxp_result status;
    if (registry == NULL || evidence == NULL || set == NULL ||
        registry->publish_evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (set->version == UINT64_MAX) return LXP_ERR_OVERFLOW;
    status = registry->publish_evidence(registry->publish_context, evidence);
    if (status != LXP_OK) return status;
    status = append_record(registry, &evidence->challenge,
                           evidence->failure_code,
                           evidence->served_bytes.bytes != NULL, true);
    if (status != LXP_OK) return status;
    for (i = 0U; i < set->count; ++i)
        if (memcmp(set->records[i].guarantor_id,
                   evidence->challenge.signed_commitment.guarantor_id,
                   32U) == 0) {
            set->records[i].jailed = true;
            set->records[i].unresolved_slashing = true;
            ++set->version;
            return LXP_OK;
        }
    return LXP_ERR_AUTH_SCOPE;
}
