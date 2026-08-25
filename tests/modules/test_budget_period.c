#include "layerx/lx_budget.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

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
    if (run(false, immediate) != 0 || run(true, delayed) != 0 ||
        memcmp(immediate, delayed, 32U) != 0)
        return 1;
    return 0;
}
