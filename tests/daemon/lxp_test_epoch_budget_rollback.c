#include "lxp_test_epoch_modules.h"
#include "layerx/lx_service.h"

#include <stdio.h>
#include <string.h>

static int fail(const char *what)
{
    (void)fprintf(stderr, "epoch budget rollback: %s\n", what);
    return 1;
}

static void agreement_init(lx_service_agreement *agreement, uint8_t marker)
{
    (void)memset(agreement, 0, sizeof(*agreement));
    agreement->agreement_id[0] = marker;
    agreement->offer_id[0] = marker;
    agreement->offer_id[1] = 1U;
    agreement->provider[0] = 0x51U;
    agreement->buyer[0] = 0x52U;
    agreement->terms_hash[0] = 0x53U;
    agreement->delivery_deadline = 200U;
    agreement->acceptance_window_end = 400U;
    agreement->dispute_window_end = 900U;
    agreement->default_outcome = LX_SERVICE_DEFAULT_ACCEPT;
    agreement->state = LX_SERVICE_AGREEMENT_DELIVERED;
}

int main(void)
{
    static epoch_fixture fixture;
    static lxp_effect_buffer effects;
    static lx_budget_store before;
    static const uint16_t enabled[] = {
        LXP_MODULE_BUDGET, LXP_MODULE_SERVICE
    };
    lx_budget_store *store = &fixture.runtimes.budget_store;
    lx_account *buyer;
    lx_service_agreement agreement;
    lxp_module_ctx ctx;
    uint8_t root_before[32];
    uint8_t root[32];
    uint64_t sequence;
    size_t i;

    if (epoch_fixture_open(&fixture, enabled, 2U) != LXP_OK ||
        epoch_fixture_account(&fixture, "agent:did:key:buyer:main", 1U,
                              0U, &buyer) != LXP_OK ||
        epoch_fixture_bind(&fixture) != LXP_OK)
        return fail("fixture");
    store->count = 2U;
    for (i = 0U; i < store->count; ++i) {
        store->records[i].period_start = 100U;
        store->records[i].period_length = 100U;
        store->records[i].per_period_limit = (lxp_u128){ 0U, 100U };
        store->records[i].spent_this_period = (lxp_u128){ 0U, 30U };
    }
    store->records[0].rollover_policy = LX_BUDGET_ROLLOVER_CAPPED;
    store->records[0].carry_cap = (lxp_u128){ 0U, 40U };
    store->records[1].rollover_policy = LX_BUDGET_ROLLOVER_NONE;
    before = *store;
    for (i = 0U; i < (size_t)LXP_MODULE_MAX_STAGED_WRITES; ++i) {
        if ((i % 32U) == 0U &&
            epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                              300U) != LXP_OK)
            return fail("service context");
        agreement_init(&agreement, (uint8_t)(i + 1U));
        if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK)
            return fail("service agreement");
        if ((i % 32U) == 31U && lxp_module_ctx_commit(&ctx) != LXP_OK)
            return fail("service commit");
    }
    if (lxp_state_root(&fixture.kernel, root_before) != LXP_OK)
        return fail("root before");
    (void)memcpy(fixture.kernel.current_state_root, root_before, 32U);
    sequence = fixture.state.next_sequence;
    if (lxp_kernel_epoch_transition(&fixture.kernel, 2U, 450U,
                                    &fixture.arena) != LXP_ERR_ARENA_EXHAUSTED ||
        !epoch_fixture_consistent(&fixture, 1U, sequence) ||
        lxp_state_root(&fixture.kernel, root) != LXP_OK ||
        memcmp(root, root_before, 32U) != 0 ||
        memcmp(store, &before, sizeof(before)) != 0)
        return fail("later service hook must restore budget and state");
    for (i = 0U; i < (size_t)LXP_MODULE_MAX_STAGED_WRITES; ++i) {
        uint8_t identifier[32] = {0};
        identifier[0] = (uint8_t)(i + 1U);
        if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                              450U) != LXP_OK ||
            lx_service_agreement_lookup(&ctx, identifier, &agreement) != LXP_OK)
            return fail("service rollback lookup");
        lxp_module_ctx_rollback(&ctx);
        if (agreement.state != LX_SERVICE_AGREEMENT_DELIVERED ||
            agreement.default_applied || agreement.outcome_sequence != 0U ||
            agreement.outcome_timestamp != 0U)
            return fail("service rollback state");
    }
    if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                          350U) != LXP_OK)
        return fail("acceptance context");
    agreement_init(&agreement, 1U);
    agreement.state = LX_SERVICE_AGREEMENT_ACCEPTED;
    agreement.accepted_sequence = sequence;
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_state_root(&fixture.kernel, fixture.kernel.current_state_root) != LXP_OK)
        return fail("acceptance commit");
    if (lxp_kernel_epoch_transition(&fixture.kernel, 2U, 450U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 2U, sequence + 1U) ||
        store->records[0].period_start != 400U ||
        store->records[0].per_period_limit.hi != 0U ||
        store->records[0].per_period_limit.lo != 140U ||
        store->records[0].carried.hi != 0U ||
        store->records[0].carried.lo != 40U ||
        !lxp_u128_is_zero(store->records[0].spent_this_period) ||
        store->records[1].period_start != 400U ||
        store->records[1].per_period_limit.hi != 0U ||
        store->records[1].per_period_limit.lo != 100U ||
        !lxp_u128_is_zero(store->records[1].spent_this_period) ||
        !lxp_u128_is_zero(store->records[1].carried))
        return fail("recovered transition");
    before = *store;
    if (lxp_kernel_epoch_transition(&fixture.kernel, 2U, 450U,
                                    &fixture.arena) != LXP_ERR_IDEMPOTENT_REPLAY ||
        !epoch_fixture_consistent(&fixture, 2U, sequence + 1U) ||
        memcmp(store, &before, sizeof(before)) != 0)
        return fail("replay preserves budget");
    return epoch_fixture_close(&fixture);
}
