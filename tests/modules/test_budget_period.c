#include "layerx/lx_budget.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static int maintenance_expiry(uint64_t timestamp,
                               lx_budget_rollover_policy policy)
{
    lx_budget_record expired = {0};
    lx_budget_record active;
    lx_budget_record decoded;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint8_t expired_key[LX_BUDGET_STATE_KEY_BYTES];
    uint8_t active_key[LX_BUDGET_STATE_KEY_BYTES];
    uint8_t expired_bytes[LX_BUDGET_RECORD_MAX_BYTES];
    uint8_t active_bytes[LX_BUDGET_RECORD_MAX_BYTES];
    const uint8_t *stored;
    size_t expired_length;
    size_t active_length;
    size_t stored_length;
    uint64_t parameters = 1U;
    bool complete;

    expired.budget_id[0] = 1U;
    expired.period_start = 100U;
    expired.period_length = 100U;
    expired.expiry = 400U;
    expired.per_period_limit = (lxp_u128){ 0U, 100U };
    expired.configured_period_limit = expired.per_period_limit;
    expired.spent_this_period = (lxp_u128){ 0U, 30U };
    expired.rollover_policy = policy;
    if (policy == LX_BUDGET_ROLLOVER_CAPPED)
        expired.carry_cap = (lxp_u128){ 0U, 40U };
    active = expired;
    active.budget_id[0] = 2U;
    active.expiry = timestamp + 1U;
    if (lx_budget_state_key(expired.budget_id, expired_key) != LXP_OK ||
        lx_budget_state_key(active.budget_id, active_key) != LXP_OK ||
        lx_budget_record_encode(&expired, expired_bytes,
            sizeof(expired_bytes), &expired_length) != LXP_OK ||
        lx_budget_record_encode(&active, active_bytes,
            sizeof(active_bytes), &active_length) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_budget_module_iface()) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, timestamp, 0U,
            1U, 1000U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&ctx, expired_key, sizeof(expired_key), expired_bytes,
            expired_length) != LXP_OK ||
        lxp_ctx_kv_put(&ctx, active_key, sizeof(active_key), active_bytes,
            active_length) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, timestamp, 0U,
            2U, 1000U, &arena, true) != LXP_OK ||
        lx_budget_batch_maintenance(&ctx, &complete) != LXP_OK || complete)
        return 1;
    lxp_module_ctx_rollback(&ctx);
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, timestamp, 0U,
            2U, 1000U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_get(&ctx, active_key, sizeof(active_key), &stored,
            &stored_length) != LXP_OK || stored_length != active_length ||
        memcmp(stored, active_bytes, active_length) != 0 ||
        lx_budget_batch_maintenance(&ctx, &complete) != LXP_OK || complete ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    if (lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, timestamp, 0U,
            3U, 1000U, &arena, true) != LXP_OK ||
        lx_budget_batch_maintenance(&ctx, &complete) != LXP_OK || !complete ||
        lxp_ctx_kv_get(&ctx, expired_key, sizeof(expired_key), &stored,
            &stored_length) != LXP_OK || stored_length != expired_length ||
        memcmp(stored, expired_bytes, expired_length) != 0 ||
        lx_budget_record_decode(stored, stored_length, &decoded) != LXP_OK ||
        lx_budget_spend_prepare(&decoded, timestamp,
            (lxp_u128){ 0U, 100U }, (lxp_u128){ 0U, 1U }) != LXP_ERR_EXPIRED ||
        lxp_ctx_kv_get(&ctx, active_key, sizeof(active_key), &stored,
            &stored_length) != LXP_OK ||
        lx_budget_record_decode(stored, stored_length, &decoded) != LXP_OK ||
        decoded.period_start != timestamp - timestamp % 100U ||
        !lxp_u128_is_zero(decoded.spent_this_period) ||
        decoded.per_period_limit.lo !=
            (policy == LX_BUDGET_ROLLOVER_CAPPED ? 140U : 100U) ||
        decoded.carried.lo !=
            (policy == LX_BUDGET_ROLLOVER_CAPPED ? 40U : 0U))
        return 1;
    (void)memset(expired_bytes + 258U, 0, 8U);
    if (lxp_ctx_kv_put(&ctx, expired_key, sizeof(expired_key), expired_bytes,
            expired_length) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, timestamp, 0U,
            4U, 1000U, &arena, true) != LXP_OK ||
        lx_budget_batch_maintenance(&ctx, &complete) != LXP_ERR_NON_CANONICAL)
        return 1;
    lxp_module_ctx_rollback(&ctx);
    return lxp_state_store_destroy(&state) == LXP_OK ? 0 : 1;
}

static int run(bool delayed, uint8_t root[32])
{
    lx_budget_store store;
    lx_budget_runtime runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    uint64_t periods;
    uint8_t input[48];
    volatile uint64_t elapsed = 0U;
    uint64_t i;

    (void)memset(&store, 0, sizeof(store));
    store.count = 2U;
    store.records[0].period_start = 100U;
    store.records[0].period_length = 100U;
    store.records[0].per_period_limit = (lxp_u128){ 0U, 100U };
    store.records[0].spent_this_period = (lxp_u128){ 0U, 30U };
    store.records[0].rollover_policy = LX_BUDGET_ROLLOVER_CAPPED;
    store.records[0].carry_cap = (lxp_u128){ 0U, 40U };
    store.records[1].period_start = 100U;
    store.records[1].period_length = 100U;
    store.records[1].per_period_limit = (lxp_u128){ 0U, 100U };
    store.records[1].spent_this_period = (lxp_u128){ 0U, 30U };
    store.records[1].rollover_policy = LX_BUDGET_ROLLOVER_NONE;
    runtime.store = &store;
    if (lx_budget_periods_elapsed(&store.records[0], 450U, &periods) != LXP_OK ||
        periods != 3U || lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_budget_module_iface()) != LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_BUDGET,
                                       &runtime) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_BUDGET, 450U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    if (delayed)
        for (i = 0U; i < UINT64_C(1000000); ++i) elapsed += i;
    store.count = LX_BUDGET_STORE_CAPACITY + 1U;
    if (lx_budget_module_iface()->epoch_begin(&ctx, 0U, 450U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    store.count = 2U;
    if (lx_budget_module_iface()->epoch_begin(&ctx, 0U, 450U) != LXP_OK ||
        store.records[0].period_start != 400U ||
        store.records[0].per_period_limit.lo != 140U ||
        store.records[0].carried.lo != 40U ||
        store.records[1].period_start != 400U ||
        store.records[1].per_period_limit.lo != 100U ||
        !lxp_u128_is_zero(store.records[1].carried))
        return 1;
    (void)memset(input, 0, sizeof(input));
    (void)memcpy(input, &store.records[0].period_start, 8U);
    (void)memcpy(input + 8U, &store.records[0].per_period_limit, 16U);
    (void)memcpy(input + 24U, &store.records[1].period_start, 8U);
    (void)memcpy(input + 32U, &store.records[1].per_period_limit, 16U);
    if (lxp_hash_sha256(input, sizeof(input), root) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    (void)elapsed;
    return 0;
}

int main(void)
{
    uint8_t immediate[32];
    uint8_t delayed[32];
    if (maintenance_expiry(400U, LX_BUDGET_ROLLOVER_NONE) != 0 ||
        maintenance_expiry(450U, LX_BUDGET_ROLLOVER_CAPPED) != 0 ||
        maintenance_expiry(10000450U, LX_BUDGET_ROLLOVER_NONE) != 0 ||
        maintenance_expiry(10000450U, LX_BUDGET_ROLLOVER_CAPPED) != 0 ||
        run(false, immediate) != 0 || run(true, delayed) != 0 ||
        memcmp(immediate, delayed, 32U) != 0)
        return 1;
    return 0;
}
