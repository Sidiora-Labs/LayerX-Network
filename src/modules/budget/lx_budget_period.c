#include "layerx/lx_budget.h"

#include <stddef.h>

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
