#include "lxp_test_epoch_modules.h"
#include "layerx/lx_escrow.h"

#include <stdio.h>
#include <string.h>

/* A node-shaped kernel whose genesis manifest enables the escrow module:
 * the binder installs the process escrow runtime, and the kernel epoch
 * transition drives lx_escrow_epoch_begin so that an expired OPEN hold is
 * timed out and its locked amount returned to the owner, with the sequence,
 * the state root and the journal left exactly where a replayer expects them. */

static int fail(const char *what)
{
    (void)fprintf(stderr, "epoch escrow timeout: %s\n", what);
    return 1;
}

static int hold_state(epoch_fixture *fixture, lxp_effect_buffer *effects,
                      uint64_t timestamp_ms, const uint8_t escrow_id[32],
                      lx_escrow_record *stored)
{
    lxp_module_ctx ctx;
    lxp_result status = epoch_fixture_ctx(fixture, &ctx, effects,
                                          LXP_MODULE_ESCROW, timestamp_ms);
    if (status != LXP_OK) return 1;
    status = lx_escrow_lookup(&ctx, escrow_id, stored);
    lxp_module_ctx_rollback(&ctx);
    return status == LXP_OK ? 0 : 1;
}

int main(void)
{
    static epoch_fixture fixture;
    static lxp_effect_buffer effects;
    static const uint16_t enabled[] = { LXP_MODULE_ESCROW };
    lx_account *owner;
    lx_account *escrow_account;
    lx_escrow_record record;
    lx_escrow_record stored;
    lxp_module_ctx ctx;
    lxp_receipt receipt;
    uint8_t timeout_key[32];
    uint8_t root_before[32];
    uint8_t root_after[32];
    uint8_t root[32];
    uint64_t sequence;
    size_t saved_count;
    bool found = false;

    if (epoch_fixture_open(&fixture, enabled, 1U) != LXP_OK)
        return fail("fixture open");
    if (epoch_fixture_account(&fixture, "agent:did:key:owner:main", 1U, 0U,
                              &owner) != LXP_OK ||
        epoch_fixture_account(&fixture, "agent:did:key:owner:escrow:epoch",
                              2U, 50U, &escrow_account) != LXP_OK ||
        escrow_account->kind != LX_ACCOUNT_AGENT_ESCROW)
        return fail("accounts");
    if (epoch_fixture_bind(&fixture) != LXP_OK) return fail("bind");

    /* The manifest enabled escrow alone: it holds the process runtime over
     * the process registries, and the modules the manifest left disabled
     * hold none. */
    if (!fixture.runtimes.enabled[LXP_MODULE_ESCROW] ||
        fixture.kernel.module_runtime[LXP_MODULE_ESCROW] !=
            &fixture.runtimes.escrow ||
        fixture.runtimes.escrow.accounts != &fixture.accounts ||
        fixture.runtimes.escrow.assets != &fixture.assets ||
        fixture.runtimes.enabled[LXP_MODULE_BUDGET] ||
        fixture.runtimes.enabled[LXP_MODULE_STREAM] ||
        fixture.runtimes.enabled[LXP_MODULE_SERVICE] ||
        fixture.runtimes.enabled[LXP_MODULE_PERPS] ||
        fixture.kernel.module_runtime[LXP_MODULE_BUDGET] != NULL ||
        fixture.kernel.module_runtime[LXP_MODULE_STREAM] != NULL ||
        fixture.kernel.module_runtime[LXP_MODULE_SERVICE] != NULL ||
        fixture.kernel.module_runtime[LXP_MODULE_PERPS] != NULL)
        return fail("genesis gate");

    /* One OPEN hold of 50 units that expires at 1000. */
    if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_ESCROW,
                          500U) != LXP_OK)
        return fail("seed context");
    (void)memset(&record, 0, sizeof(record));
    record.escrow_id[0] = 1U;
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    (void)memcpy(record.beneficiary, owner->id, 32U);
    record.arbiter[0] = 9U;
    (void)memcpy(record.asset_id, fixture.asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 50U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    record.dispute_window = 900U;
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lx_escrow_timeout_key(&record, timeout_key) != LXP_OK)
        return fail("seed hold");
    sequence = fixture.state.next_sequence;
    if (lxp_state_root(&fixture.kernel, root_before) != LXP_OK)
        return fail("root before");

    /* Refused transitions touch nothing. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 1U, 1200U,
                                    &fixture.arena) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        lxp_kernel_epoch_transition(&fixture.kernel, 0U, 1200U,
                                    &fixture.arena) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lxp_kernel_epoch_transition(&fixture.kernel, 2U, 0U,
                                    &fixture.arena) != LXP_ERR_NON_CANONICAL ||
        lxp_kernel_epoch_transition(&fixture.kernel, 2U, 1200U, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        fixture.kernel.epoch != 1U ||
        fixture.state.next_sequence != sequence || fixture.journal.open ||
        escrow_account->balance.lo != 50U || owner->balance.lo != 0U)
        return fail("refusals");

    /* Epoch 2 opens before the expiry: the sweep runs and finds nothing
     * due, and the transition still consumes its sequence. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 2U, 999U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 2U, sequence + 1U) ||
        escrow_account->balance.lo != 50U || owner->balance.lo != 0U ||
        hold_state(&fixture, &effects, 999U, record.escrow_id, &stored) != 0 ||
        stored.state != LX_ESCROW_STATE_OPEN ||
        stored.locked_amount.lo != 50U)
        return fail("transition before expiry");

    /* Epoch 3 opens past the expiry: the escrow sweep times the hold out
     * and the production ledger applier returns the 50 units to the owner. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 3U, 1200U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 3U, sequence + 2U) ||
        escrow_account->balance.lo != 0U || owner->balance.lo != 50U)
        return fail("transition past expiry");
    if (epoch_fixture_ctx(&fixture, &ctx, &effects, LXP_MODULE_ESCROW,
                          1200U) != LXP_OK ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_TIMED_OUT ||
        !lxp_u128_is_zero(stored.locked_amount) ||
        lx_escrow_invariant_check(&stored, escrow_account) != LXP_OK ||
        lx_escrow_receipt_replay(&ctx, timeout_key, &receipt, &found) !=
            LXP_OK ||
        !found || receipt.operation != 5U || receipt.amount.lo != 50U)
        return fail("timed out hold");
    lxp_module_ctx_rollback(&ctx);
    if (lxp_state_root(&fixture.kernel, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) == 0)
        return fail("root unchanged by the sweep");

    /* Replaying the epoch is refused and changes nothing. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 3U, 1200U,
                                    &fixture.arena) !=
            LXP_ERR_IDEMPOTENT_REPLAY ||
        !epoch_fixture_consistent(&fixture, 3U, sequence + 2U))
        return fail("replay");

    /* A hook that refuses rolls the whole transition back: an account
     * registry reporting more accounts than its capacity makes
     * lx_escrow_require_runtime refuse the runtime. */
    saved_count = fixture.accounts.count;
    fixture.accounts.count = (size_t)LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
    if (lxp_kernel_epoch_transition(&fixture.kernel, 4U, 1300U,
                                    &fixture.arena) != LXP_ERR_NON_CANONICAL) {
        fixture.accounts.count = saved_count;
        return fail("failing hook accepted");
    }
    fixture.accounts.count = saved_count;
    if (!epoch_fixture_consistent(&fixture, 3U, sequence + 2U) ||
        lxp_state_root(&fixture.kernel, root) != LXP_OK ||
        memcmp(root, root_after, 32U) != 0 || owner->balance.lo != 50U)
        return fail("rollback");

    /* The transition lands once the hook can run. */
    if (lxp_kernel_epoch_transition(&fixture.kernel, 4U, 1300U,
                                    &fixture.arena) != LXP_OK ||
        !epoch_fixture_consistent(&fixture, 4U, sequence + 3U) ||
        owner->balance.lo != 50U)
        return fail("recovery");
    return epoch_fixture_close(&fixture);
}
