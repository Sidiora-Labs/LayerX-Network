#include "layerx/lx_budget.h"

#include <stddef.h>
#include <string.h>

lxp_result lx_budget_periods_elapsed(const lx_budget_record *record,
                                     uint64_t batch_timestamp,
                                     uint64_t *periods)
{
    if (record == NULL || periods == NULL || record->period_length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (batch_timestamp < record->period_start)
        return LXP_ERR_TIMESTAMP_REGRESSION;
    *periods = (batch_timestamp - record->period_start) /
               record->period_length;
    return LXP_OK;
}

lxp_result lx_budget_rollover(lx_budget_record *record,
                              uint64_t batch_timestamp)
{
    uint64_t periods;
    uint64_t advance;
    lxp_u128 unspent;
    lxp_u128 carry;
    lxp_u128 next_limit;
    lxp_u128 configured;
    lxp_result status;
    if (record == NULL) return LXP_ERR_NON_CANONICAL;
    configured = lxp_u128_is_zero(record->configured_period_limit) ?
        record->per_period_limit : record->configured_period_limit;
    status = lx_budget_periods_elapsed(record, batch_timestamp, &periods);
    if (status != LXP_OK || periods == 0U) return status;
    if (periods > UINT64_MAX / record->period_length)
        return LXP_ERR_OVERFLOW;
    advance = periods * record->period_length;
    if (record->period_start > UINT64_MAX - advance)
        return LXP_ERR_OVERFLOW;
    status = lxp_u128_sub(record->per_period_limit,
                          record->spent_this_period, &unspent);
    if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    carry = (lxp_u128){ 0U, 0U };
    if (record->rollover_policy == LX_BUDGET_ROLLOVER_CAPPED) {
        carry = unspent;
        if (lxp_u128_cmp(carry, record->carry_cap) > 0)
            carry = record->carry_cap;
    }
    status = lxp_u128_add(configured, carry, &next_limit);
    if (status != LXP_OK) return status;
    record->period_start += advance;
    record->spent_this_period = (lxp_u128){ 0U, 0U };
    record->carried = carry;
    record->per_period_limit = next_limit;
    return LXP_OK;
}

lxp_result lx_budget_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp)
{
    lx_budget_runtime *runtime;
    size_t i;
    if (ctx == NULL || epoch != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    runtime = (lx_budget_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL) return lxp_ctx_charge_gas(ctx, 1U);
    if (runtime->store == NULL ||
        runtime->store->count > LX_BUDGET_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < runtime->store->count; ++i) {
        lxp_result status = lx_budget_rollover(&runtime->store->records[i],
                                               timestamp);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

typedef struct budget_maintenance_scan {
    uint64_t timestamp;
    lx_budget_record record;
    bool found;
} budget_maintenance_scan;

static lxp_result budget_maintenance_visit(const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length, void *opaque)
{
    budget_maintenance_scan *scan = opaque;
    lx_budget_record record;
    uint64_t elapsed;
    lxp_result status;
    if (key == NULL || key_length != LX_BUDGET_STATE_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    if (scan->found) return LXP_OK;
    status = lx_budget_record_decode(value, value_length, &record);
    if (status != LXP_OK) return status;
    if (memcmp(key + 7U, record.budget_id, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (record.closed || scan->timestamp < record.period_start ||
        record.expiry <= scan->timestamp)
        return LXP_OK;
    status = lx_budget_periods_elapsed(&record, scan->timestamp, &elapsed);
    if (status != LXP_OK) return status;
    if (elapsed != 0U) {
        scan->record = record;
        scan->found = true;
    }
    return LXP_OK;
}

lxp_result lx_budget_batch_maintenance(lxp_module_ctx *ctx, bool *complete)
{
    static const uint8_t prefix[] = "budget:";
    budget_maintenance_scan scan = {0};
    uint8_t key[LX_BUDGET_STATE_KEY_BYTES];
    uint8_t bytes[LX_BUDGET_RECORD_MAX_BYTES];
    size_t length;
    lxp_result status;
    if (ctx == NULL || complete == NULL) return LXP_ERR_NON_CANONICAL;
    *complete = false;
    scan.timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    status = lxp_ctx_kv_iter(ctx, prefix, sizeof(prefix) - 1U,
        budget_maintenance_visit, &scan);
    if (status != LXP_OK) return status;
    if (!scan.found) {
        *complete = true;
        return LXP_OK;
    }
    status = lx_budget_rollover(&scan.record, scan.timestamp);
    if (status == LXP_OK) status = lx_budget_state_key(scan.record.budget_id, key);
    if (status == LXP_OK)
        status = lx_budget_record_encode(&scan.record, bytes, sizeof(bytes), &length);
    if (status == LXP_OK) status = lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length);
    return status;
}
