#include "lxp_test_epoch_modules.h"
#include "layerx/lx_service.h"

#include <stdio.h>
#include <string.h>

/* A node-shaped kernel whose genesis manifest enables the service module:
 * the kernel epoch transition drives lx_service_epoch_begin so that every
 * DELIVERED agreement whose acceptance window has closed receives its
 * declared default outcome, stamped with the sequence the transition
 * consumed, while agreements still inside their window or already decided
 * are left alone. A sweep the module cannot complete rolls the whole
 * transition back. */

enum { BACKLOG = LXP_MODULE_MAX_STAGED_WRITES, BACKLOG_MARKER = 0x20 };

static int fail(const char *what)
{
    (void)fprintf(stderr, "epoch service acceptance: %s\n", what);
    return 1;
}

static void agreement_init(lx_service_agreement *agreement, uint8_t marker,
                           uint64_t acceptance_window_end,
                           lx_service_default_outcome outcome,
                           lx_service_agreement_state state)
{
    (void)memset(agreement, 0, sizeof(*agreement));
    agreement->agreement_id[0] = marker;
    agreement->offer_id[0] = marker;
    agreement->offer_id[1] = 1U;
    agreement->provider[0] = 0x51U;
    agreement->buyer[0] = 0x52U;
    agreement->terms_hash[0] = 0x53U;
    agreement->delivery_deadline = 900U;
    agreement->acceptance_window_end = acceptance_window_end;
    agreement->dispute_window_end = acceptance_window_end + 500U;
    agreement->default_outcome = outcome;
    agreement->state = state;
}

static int expect(epoch_fixture *fixture, lxp_effect_buffer *effects,
                  uint64_t timestamp_ms, uint8_t marker,
                  lx_service_agreement_state state, bool default_applied,
                  uint64_t outcome_sequence, uint64_t outcome_timestamp)
{
    lxp_module_ctx ctx;
    lx_service_agreement agreement;
    uint8_t agreement_id[32];
    lxp_result status;
    (void)memset(agreement_id, 0, sizeof(agreement_id));
    agreement_id[0] = marker;
    status = epoch_fixture_ctx(fixture, &ctx, effects, LXP_MODULE_SERVICE,
                               timestamp_ms);
    if (status != LXP_OK) return 1;
    status = lx_service_agreement_lookup(&ctx, agreement_id, &agreement);
    lxp_module_ctx_rollback(&ctx);
    if (status != LXP_OK || agreement.state != state ||
        agreement.default_applied != default_applied ||
        agreement.outcome_sequence != outcome_sequence ||
        agreement.outcome_timestamp != outcome_timestamp)
        return 1;
    return 0;
}

int main(void)
{
    static epoch_fixture fixture;
    static lxp_effect_buffer effects;
    static const uint16_t enabled[] = { LXP_MODULE_SERVICE };
    const lxp_module_registration *registration;
    lx_account *buyer;
    lx_service_agreement agreement;
    lxp_module_ctx ctx;
    uint8_t root_before[32];
    uint8_t root[32];
    uint64_t sequence;
    size_t i;

    if (epoch_fixture_open(&fixture, enabled, 1U) != LXP_OK)
        return fail("fixture open");
    if (epoch_fixture_account(&fixture, "agent:did:key:buyer:main", 1U, 0U,
                              &buyer) != LXP_OK)
        return fail("account");
    if (epoch_fixture_bind(&fixture) != LXP_OK) return fail("bind");

    /* The manifest enabled service alone. Service declares no host runtime,
     * so its registration is its complete binding; escrow stays disabled in
     * the kernel and unbound. */
    if (!fixture.runtimes.enabled[LXP_MODULE_SERVICE] ||
        fixture.kernel.module_runtime[LXP_MODULE_SERVICE] != NULL ||
        lxp_kernel_module_by_id(&fixture.kernel, LXP_MODULE_SERVICE,
                                fixture.kernel.epoch,
                                &registration) != LXP_OK ||
        fixture.runtimes.enabled[LXP_MODULE_ESCROW] ||
        fixture.kernel.module_runtime[LXP_MODULE_ESCROW] != NULL ||
        lxp_kernel_module_by_id(&fixture.kernel, LXP_MODULE_ESCROW,
                                fixture.kernel.epoch,
                                &registration) != LXP_ERR_MODULE_DISABLED ||
        fixture.runtimes.enabled[LXP_MODULE_BUDGET] ||
        fixture.runtimes.enabled[LXP_MODULE_STREAM] ||
        fixture.runtimes.enabled[LXP_MODULE_PERPS])
        return fail("genesis gate");

    /* Four agreements: 0x11 and 0x12 are DELIVERED with windows closing at
     * 1300 and opposite defaults, 0x13 is DELIVERED with a window open until
     * 5000, 0x14 was already ACCEPTED by its buyer. */
    if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                          1000U) != LXP_OK)
        return fail("seed context");
    agreement_init(&agreement, 0x11U, 1300U, LX_SERVICE_DEFAULT_ACCEPT,
                   LX_SERVICE_AGREEMENT_DELIVERED);
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK)
        return fail("seed 0x11");
    agreement_init(&agreement, 0x12U, 1300U, LX_SERVICE_DEFAULT_REJECT,
                   LX_SERVICE_AGREEMENT_DELIVERED);
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK)
        return fail("seed 0x12");
    agreement_init(&agreement, 0x13U, 5000U, LX_SERVICE_DEFAULT_ACCEPT,
                   LX_SERVICE_AGREEMENT_DELIVERED);
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK)
        return fail("seed 0x13");
    agreement_init(&agreement, 0x14U, 1300U, LX_SERVICE_DEFAULT_ACCEPT,
                   LX_SERVICE_AGREEMENT_ACCEPTED);
    agreement.accepted_sequence = 7U;
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return fail("seed 0x14");
    sequence = fixture.state.next_sequence;
    if (lxp_state_root(&fixture.kernel, root_before) != LXP_OK)
        return fail("root before");

    /* Refused transitions touch nothing. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 1U, 1300U,
                                    &fixture.arena) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        lxp_kernel_epoch_transition(&fixture.kernel, 0U, 1300U,
                                    &fixture.arena) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        fixture.kernel.epoch != 1U ||
        fixture.state.next_sequence != sequence || fixture.journal.open)
        return fail("refusals");

    /* Epoch 2 opens at 1200, before any window closes: nothing is decided
     * and the transition still consumes its sequence. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 2U, 1200U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 2U, sequence + 1U) ||
        expect(&fixture, &effects, 1200U, 0x11U,
               LX_SERVICE_AGREEMENT_DELIVERED, false, 0U, 0U) != 0 ||
        expect(&fixture, &effects, 1200U, 0x12U,
               LX_SERVICE_AGREEMENT_DELIVERED, false, 0U, 0U) != 0 ||
        expect(&fixture, &effects, 1200U, 0x13U,
               LX_SERVICE_AGREEMENT_DELIVERED, false, 0U, 0U) != 0 ||
        expect(&fixture, &effects, 1200U, 0x14U,
               LX_SERVICE_AGREEMENT_ACCEPTED, false, 0U, 0U) != 0)
        return fail("transition before the windows close");

    /* Epoch 3 opens at 1300: the two closed windows receive their declared
     * defaults, stamped with the sequence this transition consumed. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 3U, 1300U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 3U, sequence + 2U) ||
        expect(&fixture, &effects, 1300U, 0x11U,
               LX_SERVICE_AGREEMENT_ACCEPTED, true, sequence + 1U,
               1300U) != 0 ||
        expect(&fixture, &effects, 1300U, 0x12U,
               LX_SERVICE_AGREEMENT_REJECTED, true, sequence + 1U,
               1300U) != 0 ||
        expect(&fixture, &effects, 1300U, 0x13U,
               LX_SERVICE_AGREEMENT_DELIVERED, false, 0U, 0U) != 0 ||
        expect(&fixture, &effects, 1300U, 0x14U,
               LX_SERVICE_AGREEMENT_ACCEPTED, false, 0U, 0U) != 0 ||
        lxp_state_root(&fixture.kernel, root) != LXP_OK ||
        memcmp(root, root_before, 32U) == 0)
        return fail("default acceptance");

    /* Replaying or regressing the epoch is refused and changes nothing. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 3U, 1300U,
                                    &fixture.arena) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        lxp_kernel_epoch_transition(&fixture.kernel, 2U, 1300U,
                                    &fixture.arena) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        !epoch_fixture_consistent(&fixture, 3U, sequence + 2U))
        return fail("replay");

    /* A backlog of LXP_MODULE_MAX_STAGED_WRITES agreements all due at 1400
     * is more than one sweep can decide: the hook refuses with
     * LXP_ERR_ARENA_EXHAUSTED and the transition rolls back the epoch, the
     * sequence and every staged decision. */
    for (i = 0U; i < (size_t)BACKLOG; ++i) {
        if ((i % 32U) == 0U &&
            epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                              1300U) != LXP_OK)
            return fail("backlog context");
        agreement_init(&agreement, (uint8_t)(BACKLOG_MARKER + i), 1400U,
                       LX_SERVICE_DEFAULT_ACCEPT,
                       LX_SERVICE_AGREEMENT_DELIVERED);
        if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK)
            return fail("backlog put");
        if ((i % 32U) == 31U && lxp_module_ctx_commit(&ctx) != LXP_OK)
            return fail("backlog commit");
    }
    if (lxp_state_root(&fixture.kernel, root_before) != LXP_OK)
        return fail("root before the backlog sweep");
    if (lxp_kernel_epoch_transition(&fixture.kernel, 4U, 1400U,
                                    &fixture.arena) !=
            LXP_ERR_ARENA_EXHAUSTED ||
        !epoch_fixture_consistent(&fixture, 3U, sequence + 2U) ||
        lxp_state_root(&fixture.kernel, root) != LXP_OK ||
        memcmp(root, root_before, 32U) != 0)
        return fail("backlog rollback");
    for (i = 0U; i < (size_t)BACKLOG; ++i)
        if (expect(&fixture, &effects, 1400U, (uint8_t)(BACKLOG_MARKER + i),
                   LX_SERVICE_AGREEMENT_DELIVERED, false, 0U, 0U) != 0)
            return fail("backlog left DELIVERED");

    /* One buyer accepts explicitly; the remaining backlog fits one sweep
     * and the transition lands, leaving the explicit acceptance untouched. */
    if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_SERVICE,
                          1350U) != LXP_OK)
        return fail("acceptance context");
    agreement_init(&agreement, (uint8_t)BACKLOG_MARKER, 1400U,
                   LX_SERVICE_DEFAULT_ACCEPT, LX_SERVICE_AGREEMENT_ACCEPTED);
    agreement.accepted_sequence = fixture.state.next_sequence;
    if (lx_service_agreement_put(&ctx, &agreement) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return fail("explicit acceptance");
    if (lxp_kernel_epoch_transition(&fixture.kernel, 4U, 1400U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 4U, sequence + 3U) ||
        expect(&fixture, &effects, 1400U, (uint8_t)BACKLOG_MARKER,
               LX_SERVICE_AGREEMENT_ACCEPTED, false, 0U, 0U) != 0)
        return fail("recovery");
    for (i = 1U; i < (size_t)BACKLOG; ++i)
        if (expect(&fixture, &effects, 1400U, (uint8_t)(BACKLOG_MARKER + i),
                   LX_SERVICE_AGREEMENT_ACCEPTED, true, sequence + 2U,
                   1400U) != 0)
            return fail("backlog defaults");
    return epoch_fixture_close(&fixture);
}
