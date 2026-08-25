#include "layerx/lxp_governance.h"

#include <stdint.h>
#include <string.h>

int main(void)
{
    uint8_t arena_storage[3U * LXP_MAX_ACTIVITY_BYTES];
    uint8_t activity_id[32] = {1U};
    uint8_t previous_root[32] = {2U};
    uint8_t resulting_root[32] = {3U};
    uint8_t activity_root[32] = {4U};
    uint8_t batch_id[32] = {5U};
    uint8_t proposal_id[32] = {6U};
    uint8_t second_proposal[32] = {7U};
    uint8_t fee_key[] = "fee.base";
    uint8_t limit_key[] = "batch.limit";
    uint8_t unknown_key[] = "unknown";
    lxp_byte_span fee = {fee_key, sizeof(fee_key) - 1U};
    lxp_byte_span limit = {limit_key, sizeof(limit_key) - 1U};
    lxp_param_table table;
    const lxp_param_entry *first;
    const lxp_param_entry *second;
    lxp_effect_buffer effects;
    lxp_receipt historical;
    lxp_receipt current;
    lxp_arena arena;
    lxp_byte_span encoded_historical;
    lxp_byte_span encoded_current;
    uint64_t value;
    uint32_t historical_version;
    uint32_t current_version;

    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_param_table_init(&table) != LXP_OK ||
        lxp_param_set_bounds(&table, fee, 2U, 1U, 1000U, 10U, 1U) != LXP_OK ||
        lxp_param_set_bounds(&table, limit, 1U, 10U, 100000U, 1000U, 1U) !=
            LXP_OK ||
        lxp_param_at(&table, 0U, &first) != LXP_OK ||
        lxp_param_at(&table, 1U, &second) != LXP_OK ||
        memcmp(first->key, "batch.limit", first->key_length) != 0 ||
        memcmp(second->key, "fee.base", second->key_length) != 0)
        return 11;
    if (lxp_param_apply_ordered(&table, fee, 25U, 5U, proposal_id, true) !=
            LXP_OK ||
        lxp_param_get(&table, fee, 2U, &value, &historical_version) != LXP_OK ||
        value != 10U ||
        lxp_param_get(&table, fee, 6U, &value, &current_version) != LXP_OK ||
        value != 25U || historical_version == current_version)
        return 21;
    if (lxp_param_get(&table,
                      (lxp_byte_span){unknown_key, sizeof(unknown_key) - 1U},
                      6U, &value, &current_version) !=
            LXP_ERR_PARAMETER_BOUNDS ||
        lxp_param_apply_ordered(&table, fee, 1001U, 7U, second_proposal,
                                true) != LXP_ERR_PARAMETER_BOUNDS ||
        lxp_param_apply_ordered(&table, fee, 30U, 7U, second_proposal,
                                false) != LXP_ERR_AUTH_SCOPE ||
        lxp_param_mark_sealed(&table, 6U) != LXP_OK ||
        lxp_param_apply_ordered(&table, fee, 30U, 6U, second_proposal,
                                true) != LXP_ERR_PARAMETER_BOUNDS)
        return 31;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(&historical, activity_id, 1U, previous_root,
                          resulting_root, activity_root, LXP_OK, &effects,
                          (lxp_u128){0U, 10U}, batch_id, 1U, 1U,
                          historical_version) != LXP_OK ||
        lxp_receipt_build(&current, activity_id, 2U, previous_root,
                          resulting_root, activity_root, LXP_OK, &effects,
                          (lxp_u128){0U, 25U}, batch_id, 1U, 1U,
                          current_version) != LXP_OK ||
        lxp_receipt_encode(&historical, false, &arena,
                           &encoded_historical) != LXP_OK ||
        lxp_receipt_encode(&current, false, &arena, &encoded_current) != LXP_OK ||
        historical.parameter_version != historical_version ||
        current.parameter_version != current_version ||
        (encoded_historical.length == encoded_current.length &&
         memcmp(encoded_historical.bytes, encoded_current.bytes,
                encoded_current.length) == 0))
        return 41;
    if (lxp_param_get(&table, fee, 2U, &value, &historical_version) != LXP_OK ||
        value != 10U)
        return 51;
    {
        lxp_param_table malformed = table;
        malformed.count = LXP_MAX_PARAMETERS + 1U;
        if (lxp_param_get(&malformed, fee, 2U, &value,
                          &historical_version) != LXP_ERR_NON_CANONICAL)
            return 61;
        malformed = table;
        malformed.version_count = LXP_MAX_PARAMETER_VERSIONS + 1U;
        if (lxp_param_version(&malformed, 2U, &historical_version) !=
            LXP_ERR_NON_CANONICAL)
            return 62;
        malformed = table;
        malformed.entries[0].history_count =
            LXP_MAX_PARAMETER_HISTORY + 1U;
        if (lxp_param_get(&malformed, fee, 2U, &value,
                          &historical_version) != LXP_ERR_NON_CANONICAL)
            return 63;
    }
    return 0;
}
