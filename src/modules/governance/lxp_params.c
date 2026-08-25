#include "layerx/lxp_governance.h"

#include <string.h>

static int key_compare(lxp_byte_span left, lxp_byte_span right)
{
    size_t shared = left.length < right.length ? left.length : right.length;
    int compared = shared == 0U ? 0 : memcmp(left.bytes, right.bytes, shared);
    if (compared != 0) return compared;
    if (left.length == right.length) return 0;
    return left.length < right.length ? -1 : 1;
}

static lxp_byte_span entry_key(const lxp_param_entry *entry)
{
    return (lxp_byte_span){entry->key, entry->key_length};
}

static lxp_param_entry *find_entry(lxp_param_table *table, lxp_byte_span key,
                                   size_t *position)
{
    size_t i;
    for (i = 0U; i < table->count; ++i) {
        int compared = key_compare(key, entry_key(&table->entries[i]));
        if (compared == 0) {
            if (position != NULL) *position = i;
            return &table->entries[i];
        }
        if (compared < 0) break;
    }
    if (position != NULL) *position = i;
    return NULL;
}

static const lxp_param_entry *find_const(const lxp_param_table *table,
                                         lxp_byte_span key)
{
    size_t i;
    for (i = 0U; i < table->count; ++i) {
        int compared = key_compare(key, entry_key(&table->entries[i]));
        if (compared == 0) return &table->entries[i];
        if (compared < 0) break;
    }
    return NULL;
}

static lxp_result next_version(lxp_param_table *table,
                               uint64_t activation_epoch,
                               uint32_t *version)
{
    lxp_param_version_record *record;
    if (table->version_count == LXP_MAX_PARAMETER_VERSIONS ||
        table->current_version == UINT32_MAX)
        return LXP_ERR_OVERFLOW;
    ++table->current_version;
    record = &table->versions[table->version_count++];
    record->parameter_version = table->current_version;
    record->activation_epoch = activation_epoch;
    *version = table->current_version;
    return LXP_OK;
}

lxp_result lxp_param_table_init(lxp_param_table *table)
{
    if (table == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(table, 0, sizeof(*table));
    return LXP_OK;
}

lxp_result lxp_param_table_validate(const lxp_param_table *table)
{
    size_t i;
    if (table == NULL || table->count > LXP_MAX_PARAMETERS ||
        table->version_count > LXP_MAX_PARAMETER_VERSIONS ||
        table->proposal_count > LXP_MAX_GOV_PROPOSALS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < table->count; ++i)
        if (table->entries[i].key_length == 0U ||
            table->entries[i].key_length > LXP_MAX_PARAMETER_KEY_BYTES ||
            table->entries[i].history_count == 0U ||
            table->entries[i].history_count > LXP_MAX_PARAMETER_HISTORY)
            return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < table->proposal_count; ++i)
        if (table->proposals[i].parameter_key_length == 0U ||
            table->proposals[i].parameter_key_length >
                LXP_MAX_PARAMETER_KEY_BYTES ||
            table->proposals[i].cohort_count >
                LXP_MAX_GOV_COHORT_MEMBERS)
            return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lxp_param_set_bounds(
    lxp_param_table *table, lxp_byte_span key, uint16_t target_module,
    uint64_t minimum_value, uint64_t maximum_value, uint64_t initial_value,
    uint64_t activation_epoch)
{
    lxp_param_entry *entry;
    size_t position;
    uint32_t version;
    lxp_result status;
    if (lxp_param_table_validate(table) != LXP_OK || key.bytes == NULL ||
        key.length == 0U ||
        key.length > LXP_MAX_PARAMETER_KEY_BYTES || target_module == 0U ||
        activation_epoch == 0U || minimum_value > maximum_value ||
        initial_value < minimum_value || initial_value > maximum_value ||
        table->count == LXP_MAX_PARAMETERS ||
        find_entry(table, key, &position) != NULL)
        return LXP_ERR_PARAMETER_BOUNDS;
    if (position < table->count)
        (void)memmove(&table->entries[position + 1U],
                      &table->entries[position],
                      (table->count - position) * sizeof(table->entries[0]));
    entry = &table->entries[position];
    (void)memset(entry, 0, sizeof(*entry));
    (void)memcpy(entry->key, key.bytes, key.length);
    entry->key_length = key.length;
    entry->target_module = target_module;
    entry->minimum_value = minimum_value;
    entry->maximum_value = maximum_value;
    status = next_version(table, activation_epoch, &version);
    if (status != LXP_OK) {
        if (position < table->count)
            (void)memmove(&table->entries[position],
                          &table->entries[position + 1U],
                          (table->count - position) *
                              sizeof(table->entries[0]));
        return status;
    }
    entry->history[0] = (lxp_param_value_record){
        initial_value, activation_epoch, version, {0U}
    };
    entry->history_count = 1U;
    ++table->count;
    return LXP_OK;
}

lxp_result lxp_param_apply_ordered(
    lxp_param_table *table, lxp_byte_span key, uint64_t value,
    uint64_t activation_epoch, const uint8_t proposal_id[32],
    bool ordered_governance_activity)
{
    lxp_param_entry *entry;
    lxp_param_value_record *record;
    uint32_t version;
    lxp_result status;
    if (lxp_param_table_validate(table) != LXP_OK || proposal_id == NULL ||
        key.bytes == NULL || key.length == 0U ||
        key.length > LXP_MAX_PARAMETER_KEY_BYTES ||
        !ordered_governance_activity)
        return LXP_ERR_AUTH_SCOPE;
    entry = find_entry(table, key, NULL);
    if (entry == NULL || value < entry->minimum_value ||
        value > entry->maximum_value || activation_epoch == 0U ||
        activation_epoch <= table->last_sealed_epoch ||
        entry->history_count == LXP_MAX_PARAMETER_HISTORY ||
        activation_epoch <=
            entry->history[entry->history_count - 1U].activation_epoch)
        return LXP_ERR_PARAMETER_BOUNDS;
    status = next_version(table, activation_epoch, &version);
    if (status != LXP_OK) return status;
    entry->proposed_value = value;
    (void)memcpy(entry->proposal_id, proposal_id, 32U);
    record = &entry->history[entry->history_count++];
    record->value = value;
    record->activation_epoch = activation_epoch;
    record->parameter_version = version;
    (void)memcpy(record->proposal_id, proposal_id, 32U);
    return LXP_OK;
}

lxp_result lxp_param_version(const lxp_param_table *table,
                             uint64_t execution_epoch,
                             uint32_t *parameter_version)
{
    size_t i;
    uint32_t selected = 0U;
    if (lxp_param_table_validate(table) != LXP_OK ||
        parameter_version == NULL || execution_epoch == 0U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < table->version_count; ++i)
        if (table->versions[i].activation_epoch <= execution_epoch &&
            table->versions[i].parameter_version > selected)
            selected = table->versions[i].parameter_version;
    if (selected == 0U) return LXP_ERR_NOT_YET_VALID;
    *parameter_version = selected;
    return LXP_OK;
}

lxp_result lxp_param_get(const lxp_param_table *table, lxp_byte_span key,
                         uint64_t execution_epoch, uint64_t *value,
                         uint32_t *parameter_version)
{
    const lxp_param_entry *entry;
    const lxp_param_value_record *selected = NULL;
    size_t i;
    lxp_result status;
    if (lxp_param_table_validate(table) != LXP_OK || value == NULL ||
        parameter_version == NULL || key.bytes == NULL || key.length == 0U ||
        key.length > LXP_MAX_PARAMETER_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    entry = find_const(table, key);
    if (entry == NULL) return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 0U; i < entry->history_count; ++i)
        if (entry->history[i].activation_epoch <= execution_epoch)
            selected = &entry->history[i];
    if (selected == NULL) return LXP_ERR_NOT_YET_VALID;
    status = lxp_param_version(table, execution_epoch, parameter_version);
    if (status != LXP_OK) return status;
    *value = selected->value;
    return LXP_OK;
}

lxp_result lxp_param_at(const lxp_param_table *table, size_t index,
                        const lxp_param_entry **entry)
{
    if (lxp_param_table_validate(table) != LXP_OK || entry == NULL ||
        index >= table->count)
        return LXP_ERR_NON_CANONICAL;
    *entry = &table->entries[index];
    return LXP_OK;
}

lxp_result lxp_param_mark_sealed(lxp_param_table *table, uint64_t epoch)
{
    if (lxp_param_table_validate(table) != LXP_OK ||
        epoch < table->last_sealed_epoch)
        return LXP_ERR_NON_MONOTONIC_TIME;
    table->last_sealed_epoch = epoch;
    return LXP_OK;
}
