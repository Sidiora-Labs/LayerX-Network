#include "layerx/lxp_governance.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static int key_equal(const lxp_param_entry *entry, lxp_byte_span key)
{
    return entry->key_length == key.length &&
        (key.length == 0U || memcmp(entry->key, key.bytes, key.length) == 0);
}

static lxp_param_entry *entry_for(lxp_param_table *table, lxp_byte_span key)
{
    size_t i;
    for (i = 0U; i < table->count; ++i)
        if (key_equal(&table->entries[i], key)) return &table->entries[i];
    return NULL;
}

static const lxp_param_entry *entry_for_const(
    const lxp_param_table *table, lxp_byte_span key)
{
    size_t i;
    for (i = 0U; i < table->count; ++i)
        if (key_equal(&table->entries[i], key)) return &table->entries[i];
    return NULL;
}

static int id_nonzero(const uint8_t id[32])
{
    uint8_t combined = 0U;
    size_t i;
    for (i = 0U; i < 32U; ++i) combined |= id[i];
    return combined != 0U;
}

lxp_result lxp_gov_stage_cohort(
    lxp_gov_param_proposal *proposal, lxp_gov_rollout_scope rollout_scope,
    const uint8_t (*cohort)[32], size_t cohort_count)
{
    size_t i;
    if (proposal == NULL || rollout_scope < LXP_GOV_ROLLOUT_ALL ||
        rollout_scope > LXP_GOV_ROLLOUT_ACCOUNT_SET ||
        (rollout_scope == LXP_GOV_ROLLOUT_ALL && cohort_count != 0U) ||
        (rollout_scope != LXP_GOV_ROLLOUT_ALL &&
         (cohort == NULL || cohort_count == 0U)) ||
        cohort_count > LXP_MAX_GOV_COHORT_MEMBERS)
        return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 0U; i < cohort_count; ++i)
        if (!id_nonzero(cohort[i]) ||
            (i != 0U && memcmp(cohort[i - 1U], cohort[i], 32U) >= 0))
            return LXP_ERR_UNSORTED_SEQUENCE;
    proposal->rollout_scope = rollout_scope;
    proposal->cohort_count = cohort_count;
    if (cohort_count != 0U)
        (void)memcpy(proposal->cohort, cohort, cohort_count * 32U);
    return LXP_OK;
}

lxp_result lxp_gov_param_propose(
    lxp_param_table *table, const lxp_gov_param_proposal *proposal,
    uint64_t current_epoch, uint64_t minimum_activation_delay,
    bool governance_authorized, bool ordered_governance_activity)
{
    const lxp_param_entry *entry;
    lxp_byte_span key;
    size_t i;
    if (lxp_param_table_validate(table) != LXP_OK || proposal == NULL ||
        !governance_authorized ||
        !ordered_governance_activity)
        return LXP_ERR_AUTH_SCOPE;
    key = (lxp_byte_span){proposal->parameter_key,
                         proposal->parameter_key_length};
    entry = entry_for_const(table, key);
    if (current_epoch == 0U || minimum_activation_delay == 0U ||
        current_epoch > UINT64_MAX - minimum_activation_delay ||
        proposal->activation_epoch < current_epoch + minimum_activation_delay ||
        proposal->activation_epoch <= table->last_sealed_epoch ||
        proposal->ordered_sequence == 0U || !id_nonzero(proposal->proposal_id) ||
        entry == NULL || entry->target_module != proposal->target_module ||
        proposal->proposed_value < entry->minimum_value ||
        proposal->proposed_value > entry->maximum_value ||
        proposal->rollout_scope < LXP_GOV_ROLLOUT_ALL ||
        proposal->rollout_scope > LXP_GOV_ROLLOUT_ACCOUNT_SET ||
        (proposal->rollout_scope == LXP_GOV_ROLLOUT_ALL &&
         proposal->cohort_count != 0U) ||
        (proposal->rollout_scope != LXP_GOV_ROLLOUT_ALL &&
         proposal->cohort_count == 0U) ||
        table->proposal_count == LXP_MAX_GOV_PROPOSALS)
        return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 0U; i < table->proposal_count; ++i)
        if (memcmp(table->proposals[i].proposal_id,
                   proposal->proposal_id, 32U) == 0 ||
            table->proposals[i].ordered_sequence == proposal->ordered_sequence)
            return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 1U; i < proposal->cohort_count; ++i)
        if (memcmp(proposal->cohort[i - 1U], proposal->cohort[i], 32U) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
    table->proposals[table->proposal_count++] = *proposal;
    return LXP_OK;
}

static lxp_result stage_version(lxp_param_table *table, uint64_t epoch,
                                uint32_t *version)
{
    lxp_param_version_record *record;
    if (table->current_version == UINT32_MAX ||
        table->version_count == LXP_MAX_PARAMETER_VERSIONS)
        return LXP_ERR_OVERFLOW;
    ++table->current_version;
    record = &table->versions[table->version_count++];
    record->parameter_version = table->current_version;
    record->activation_epoch = epoch;
    *version = table->current_version;
    return LXP_OK;
}

lxp_result lxp_gov_activation_apply(lxp_param_table *table,
                                    uint64_t batch_epoch,
                                    bool first_batch_of_epoch)
{
    lxp_param_table before;
    size_t i;
    bool due = false;
    lxp_result status = LXP_OK;
    if (lxp_param_table_validate(table) != LXP_OK || batch_epoch == 0U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < table->proposal_count; ++i) {
        if (!table->proposals[i].enacted &&
            table->proposals[i].activation_epoch < batch_epoch)
            return LXP_ERR_PARAMETER_BOUNDS;
        if (!table->proposals[i].enacted &&
            table->proposals[i].activation_epoch == batch_epoch)
            due = true;
    }
    if (!due) return LXP_OK;
    if (!first_batch_of_epoch) return LXP_ERR_PARAMETER_BOUNDS;
    before = *table;
    for (i = 0U; status == LXP_OK && i < table->proposal_count; ++i) {
        lxp_gov_param_proposal *proposal = &table->proposals[i];
        if (proposal->enacted || proposal->activation_epoch != batch_epoch)
            continue;
        if (proposal->rollout_scope == LXP_GOV_ROLLOUT_ALL) {
            status = lxp_param_apply_ordered(
                table,
                (lxp_byte_span){proposal->parameter_key,
                                proposal->parameter_key_length},
                proposal->proposed_value, proposal->activation_epoch,
                proposal->proposal_id, true);
            if (status == LXP_OK)
                proposal->parameter_version = table->current_version;
        } else {
            status = stage_version(table, proposal->activation_epoch,
                                   &proposal->parameter_version);
            if (status == LXP_OK) {
                lxp_param_entry *entry = entry_for(
                    table, (lxp_byte_span){proposal->parameter_key,
                                           proposal->parameter_key_length});
                if (entry == NULL) status = LXP_ERR_PARAMETER_BOUNDS;
                else {
                    entry->proposed_value = proposal->proposed_value;
                    (void)memcpy(entry->proposal_id,
                                 proposal->proposal_id, 32U);
                }
            }
        }
        if (status == LXP_OK) proposal->enacted = true;
    }
    if (status != LXP_OK) *table = before;
    return status;
}

static int cohort_contains(const lxp_gov_param_proposal *proposal,
                           const uint8_t cohort_id[32])
{
    size_t i;
    if (proposal->rollout_scope == LXP_GOV_ROLLOUT_ALL) return 1;
    if (cohort_id == NULL) return 0;
    for (i = 0U; i < proposal->cohort_count; ++i)
        if (memcmp(proposal->cohort[i], cohort_id, 32U) == 0) return 1;
    return 0;
}

lxp_result lxp_gov_param_enact(
    const lxp_param_table *table, lxp_byte_span key, uint64_t execution_epoch,
    const uint8_t cohort_id[32], uint64_t *value,
    uint32_t *parameter_version)
{
    const lxp_param_entry *entry;
    uint64_t base_activation = 0U;
    uint64_t selected_activation;
    size_t i;
    lxp_result status;
    if (lxp_param_table_validate(table) != LXP_OK || value == NULL ||
        parameter_version == NULL || key.bytes == NULL || key.length == 0U ||
        key.length > LXP_MAX_PARAMETER_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_param_get(table, key, execution_epoch, value,
                           parameter_version);
    if (status != LXP_OK) return status;
    entry = entry_for_const(table, key);
    if (entry == NULL) return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 0U; i < entry->history_count; ++i)
        if (entry->history[i].activation_epoch <= execution_epoch)
            base_activation = entry->history[i].activation_epoch;
    selected_activation = base_activation;
    for (i = 0U; i < table->proposal_count; ++i) {
        const lxp_gov_param_proposal *proposal = &table->proposals[i];
        if (proposal->enacted && proposal->activation_epoch <= execution_epoch &&
            proposal->activation_epoch >= selected_activation &&
            proposal->parameter_key_length == key.length &&
            memcmp(proposal->parameter_key, key.bytes, key.length) == 0 &&
            proposal->rollout_scope != LXP_GOV_ROLLOUT_ALL &&
            cohort_contains(proposal, cohort_id)) {
            *value = proposal->proposed_value;
            selected_activation = proposal->activation_epoch;
        }
    }
    return lxp_param_version(table, execution_epoch, parameter_version);
}

static void put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

lxp_result lxp_gov_parameter_state_root(
    const lxp_param_table *table, uint64_t execution_epoch,
    const uint8_t cohort_id[32], uint8_t root[32])
{
    lxp_hash_context hash;
    uint8_t encoded[4U + 2U + LXP_MAX_PARAMETER_KEY_BYTES + 8U];
    uint32_t version;
    size_t i;
    lxp_result status;
    if (lxp_param_table_validate(table) != LXP_OK || root == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_param_version(table, execution_epoch, &version);
    if (status != LXP_OK) return status;
    lxp_hash_init(&hash);
    encoded[0] = (uint8_t)(version >> 24U);
    encoded[1] = (uint8_t)(version >> 16U);
    encoded[2] = (uint8_t)(version >> 8U);
    encoded[3] = (uint8_t)version;
    status = lxp_hash_update(&hash, encoded, 4U);
    for (i = 0U; status == LXP_OK && i < table->count; ++i) {
        uint64_t value;
        uint32_t resolved_version;
        lxp_byte_span key = {table->entries[i].key,
                             table->entries[i].key_length};
        status = lxp_gov_param_enact(table, key, execution_epoch, cohort_id,
                                     &value, &resolved_version);
        if (status != LXP_OK) break;
        encoded[0] = (uint8_t)(table->entries[i].target_module >> 8U);
        encoded[1] = (uint8_t)table->entries[i].target_module;
        (void)memcpy(encoded + 2U, key.bytes, key.length);
        put_u64(encoded + 2U + key.length, value);
        status = lxp_hash_update(&hash, encoded, 2U + key.length + 8U);
    }
    return status == LXP_OK ? lxp_hash_final(&hash, root) : status;
}
