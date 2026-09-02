#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_state.h"

#include <string.h>

typedef struct withdraw_fixture {
    lx_asset_registry assets;
    lx_asset_record asset_a;
    lx_asset_record asset_b;
    lx_account_registry accounts_a;
    lx_account_registry accounts_b;
    lx_account *agent_a;
    lx_account *withdrawals_a;
    lx_account *reserve_a;
    lx_account *agent_b;
    lx_account *withdrawals_b;
    lx_account *reserve_b;
    lxp_transfer_asset_state asset_states[2];
    lx_withdrawal_store store;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    lxp_receipt receipt;
    uint64_t global_sequence;
} withdraw_fixture;

static withdraw_fixture fixture;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

static int fresh_ctx(withdraw_fixture *f)
{
    ++f->global_sequence;
    if (lxp_arena_init(&f->arena, f->arena_bytes, sizeof(f->arena_bytes)) !=
            LXP_OK ||
        lxp_module_ctx_init(&f->ctx, &f->kernel, LXP_MODULE_ASSET, 10U, 0U,
                            f->global_sequence, 1000U, &f->arena, true) !=
            LXP_OK)
        return 1;
    return 0;
}

static int open_ledger(withdraw_fixture *f, const lx_asset_record *asset,
                       lx_account_registry *accounts, const char *agent_name,
                       lx_account **agent, lx_account **withdrawals,
                       lx_account **reserve)
{
    if (lx_account_registry_init(accounts) != LXP_OK ||
        lx_asset_account_open(&f->assets, accounts, asset->asset_id,
            (const uint8_t *)agent_name, strlen(agent_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, agent) != LXP_OK ||
        lx_asset_account_open(&f->assets, accounts, asset->asset_id,
            (const uint8_t *)"system:paxeer-withdrawals", 25U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, withdrawals) != LXP_OK ||
        lx_asset_account_open(&f->assets, accounts, asset->asset_id,
            (const uint8_t *)"system:paxeer-reserve", 21U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, reserve) != LXP_OK ||
        lxp_ledger_bootstrap_balance(*agent, asset->asset_id,
            (lxp_u128){ 0U, 100U }, 0U) != LXP_OK)
        return 1;
    return 0;
}

static int roots(const withdraw_fixture *f, uint8_t out[96])
{
    if (lxp_state_root(&f->kernel, out) != LXP_OK ||
        lx_asset_state_root(&f->assets, &f->accounts_a, out + 32) != LXP_OK ||
        lx_asset_state_root(&f->assets, &f->accounts_b, out + 64) != LXP_OK)
        return 1;
    return 0;
}

static void balances(const withdraw_fixture *f, lxp_u128 out[6])
{
    out[0] = f->agent_a->balance;
    out[1] = f->withdrawals_a->balance;
    out[2] = f->reserve_a->balance;
    out[3] = f->agent_b->balance;
    out[4] = f->withdrawals_b->balance;
    out[5] = f->reserve_b->balance;
}

static int same_balances(const lxp_u128 left[6], const lxp_u128 right[6])
{
    size_t i;
    for (i = 0U; i < 6U; ++i)
        if (lxp_u128_cmp(left[i], right[i]) != 0) return 1;
    return 0;
}

static int withdrawal_totals(const lx_withdrawal_store *store,
                             const uint8_t asset_id[32],
                             lxp_u128 *outstanding, lxp_u128 *settled)
{
    size_t i;
    *outstanding = (lxp_u128){ 0U, 0U };
    *settled = (lxp_u128){ 0U, 0U };
    for (i = 0U; i < store->count; ++i) {
        const lx_withdrawal_record *record = &store->records[i];
        lxp_u128 *bucket = record->settled ? settled : outstanding;
        if (memcmp(record->request.asset_id, asset_id, 32U) != 0) continue;
        if (lxp_u128_add(*bucket, record->request.amount, bucket) != LXP_OK)
            return 1;
    }
    return 0;
}

static int conserved(withdraw_fixture *f, const lx_account_registry *accounts,
                     const lx_account *agent, const uint8_t asset_id[32],
                     uint64_t outstanding_expected, uint64_t settled_expected)
{
    lx_asset_custody_attestation attestation;
    lx_asset_reserve_report_record report;
    lxp_u128 outstanding;
    lxp_u128 settled;
    lxp_u128 total;
    lxp_u128 custody;
    if (withdrawal_totals(&f->store, asset_id, &outstanding, &settled) != 0 ||
        outstanding.hi != 0U || outstanding.lo != outstanding_expected ||
        settled.hi != 0U || settled.lo != settled_expected)
        return 1;
    (void)memset(&attestation, 0, sizeof(attestation));
    (void)memcpy(attestation.asset_id, asset_id, 32U);
    attestation.custody_amount = (lxp_u128){ 0U, 100U };
    attestation.settled_out = settled;
    attestation.checkpoint_id[0] = 3U;
    attestation.state_root[0] = 5U;
    attestation.finalized = true;
    if (lx_asset_reserve_reconcile(accounts, &attestation, &report) != LXP_OK ||
        lx_asset_total_units(&f->assets, accounts, asset_id, &total) != LXP_OK ||
        total.hi != 0U || total.lo != 100U ||
        lxp_u128_cmp(report.raw_total, total) != 0 ||
        lxp_u128_cmp(report.withdrawals, outstanding) != 0 ||
        lxp_u128_cmp(report.reserve, settled) != 0 ||
        lxp_u128_add(agent->balance, outstanding, &custody) != LXP_OK ||
        lxp_u128_add(custody, settled, &custody) != LXP_OK ||
        lxp_u128_cmp(custody, total) != 0)
        return 1;
    return 0;
}

static int conserved_both(withdraw_fixture *f, uint64_t outstanding_a,
                          uint64_t settled_a, uint64_t outstanding_b,
                          uint64_t settled_b)
{
    if (conserved(f, &f->accounts_a, f->agent_a, f->asset_a.asset_id,
                  outstanding_a, settled_a) != 0 ||
        conserved(f, &f->accounts_b, f->agent_b, f->asset_b.asset_id,
                  outstanding_b, settled_b) != 0)
        return 1;
    return 0;
}

static void request_context(withdraw_fixture *f, lx_account *agent,
                            lx_account *withdrawals, const lx_asset_record *asset,
                            uint64_t amount,
                            lxp_transfer_source_authority *authority,
                            lx_asset_transfer_request *transfer)
{
    (void)memset(authority, 0, sizeof(*authority));
    (void)memcpy(authority->authorized_from, agent->id, 32U);
    authority->debit_authority_kind = LXP_AUTH_OWNER;
    (void)memset(transfer, 0, sizeof(*transfer));
    transfer->from = agent;
    transfer->to = withdrawals;
    transfer->asset = asset;
    transfer->amount = (lxp_u128){ 0U, amount };
    transfer->context.assets = f->asset_states;
    transfer->context.asset_count = 2U;
    transfer->context.sequence_account = agent;
    transfer->context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(transfer->context.authorized_from, agent->id, 32U);
    transfer->context.source_authorities = authority;
    transfer->context.source_authority_count = 1U;
}

static void settlement_context(const withdraw_fixture *f,
                               const lx_account *withdrawals,
                               lxp_transfer_source_authority *authority,
                               lxp_transfer_context *context)
{
    (void)memset(authority, 0, sizeof(*authority));
    (void)memcpy(authority->authorized_from, withdrawals->id, 32U);
    authority->debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    authority->protocol_system_capability = true;
    (void)memset(context, 0, sizeof(*context));
    context->assets = f->asset_states;
    context->asset_count = 2U;
    context->protocol_system_capability = true;
    context->debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    context->source_authorities = authority;
    context->source_authority_count = 1U;
}

static int refused_settle(withdraw_fixture *f, lx_account *withdrawals,
                          lx_account *reserve, const lx_asset_record *asset,
                          const lx_finalized_checkpoint *checkpoint,
                          const uint8_t nullifier[32], size_t record_index,
                          uint64_t outstanding_a, uint64_t settled_a,
                          uint64_t outstanding_b, uint64_t settled_b)
{
    uint8_t before[96];
    uint8_t after[96];
    lxp_u128 balances_before[6];
    lxp_u128 balances_after[6];
    lxp_transfer_source_authority authority;
    lxp_transfer_context context;
    if (fresh_ctx(f) != 0 || roots(f, before) != 0) return 1;
    balances(f, balances_before);
    settlement_context(f, withdrawals, &authority, &context);
    if (lx_asset_withdraw_settle(&f->ctx, withdrawals, reserve, asset,
                                 checkpoint, nullifier, &f->store, context,
                                 &f->receipt) != LXP_ERR_WITHDRAWAL_ASSET_MISMATCH ||
        f->ctx.transfer_applied ||
        f->store.records[record_index].settled ||
        memcmp(f->store.records[record_index].nullifier, nullifier, 32U) != 0 ||
        !lx_asset_nullifier_seen(&f->store, nullifier) ||
        roots(f, after) != 0 || memcmp(before, after, sizeof(before)) != 0)
        return 1;
    balances(f, balances_after);
    if (same_balances(balances_before, balances_after) != 0 ||
        conserved_both(f, outstanding_a, settled_a, outstanding_b, settled_b) != 0)
        return 1;
    return 0;
}

static int matched_settle(withdraw_fixture *f, lx_account *withdrawals,
                          lx_account *reserve, const lx_asset_record *asset,
                          const lx_finalized_checkpoint *checkpoint,
                          const uint8_t nullifier[32], size_t record_index,
                          uint64_t amount, uint64_t outstanding_a,
                          uint64_t settled_a, uint64_t outstanding_b,
                          uint64_t settled_b)
{
    uint8_t before[96];
    uint8_t after[96];
    lxp_transfer_source_authority authority;
    lxp_transfer_context context;
    if (fresh_ctx(f) != 0 || roots(f, before) != 0) return 1;
    settlement_context(f, withdrawals, &authority, &context);
    if (lx_asset_withdraw_settle(&f->ctx, withdrawals, reserve, asset,
                                 checkpoint, nullifier, &f->store, context,
                                 &f->receipt) != LXP_OK ||
        !f->ctx.transfer_applied ||
        !f->store.records[record_index].settled ||
        withdrawals->balance.hi != 0U || withdrawals->balance.lo != 0U ||
        reserve->balance.hi != 0U || reserve->balance.lo != amount ||
        roots(f, after) != 0 || memcmp(before, after, sizeof(before)) == 0 ||
        conserved_both(f, outstanding_a, settled_a, outstanding_b, settled_b) != 0)
        return 1;
    if (fresh_ctx(f) != 0) return 1;
    settlement_context(f, withdrawals, &authority, &context);
    if (lx_asset_withdraw_settle(&f->ctx, withdrawals, reserve, asset,
                                 checkpoint, nullifier, &f->store, context,
                                 &f->receipt) != LXP_ERR_WITHDRAWAL_ALREADY_SETTLED ||
        f->ctx.transfer_applied || reserve->balance.lo != amount ||
        conserved_both(f, outstanding_a, settled_a, outstanding_b, settled_b) != 0)
        return 1;
    return 0;
}

int main(void)
{
    withdraw_fixture *f = &fixture;
    lx_asset_transfer_request transfer;
    lxp_transfer_source_authority authority;
    lx_withdrawal_request withdrawal_a;
    lx_withdrawal_request withdrawal_b;
    lx_withdrawal_request crossed;
    lx_finalized_checkpoint checkpoint;
    uint8_t nullifier_a[32];
    uint8_t nullifier_b[32];
    uint8_t before[96];
    uint8_t after[96];
    uint64_t parameters = 1U;
    lxp_u128 total;

    (void)memset(f, 0, sizeof(*f));
    f->asset_a.asset_id[0] = 1U;
    (void)memcpy(f->asset_a.symbol, "A", 2U); f->asset_a.symbol_length = 1U;
    f->asset_a.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    f->asset_a.custody_reference[0] = 1U; f->asset_a.custody_reference_length = 1U;
    f->asset_b = f->asset_a;
    f->asset_b.asset_id[0] = 2U;
    (void)memcpy(f->asset_b.symbol, "B", 2U);
    f->asset_b.custody_reference[0] = 2U;
    if (lx_asset_registry_init(&f->assets, 0U) != LXP_OK ||
        lx_asset_register(&f->assets, &f->asset_a, 0U, (lxp_u128){ 0U, 0U }) !=
            LXP_OK ||
        lx_asset_register(&f->assets, &f->asset_b, 1U, (lxp_u128){ 0U, 0U }) !=
            LXP_OK ||
        open_ledger(f, &f->asset_a, &f->accounts_a, "agent:did:key:a:main",
                    &f->agent_a, &f->withdrawals_a, &f->reserve_a) != 0 ||
        open_ledger(f, &f->asset_b, &f->accounts_b, "agent:did:key:b:main",
                    &f->agent_b, &f->withdrawals_b, &f->reserve_b) != 0 ||
        lx_asset_transfer_state(&f->asset_a, &f->asset_states[0]) != LXP_OK ||
        lx_asset_transfer_state(&f->asset_b, &f->asset_states[1]) != LXP_OK ||
        lxp_state_store_init(&f->state, 0U) != LXP_OK ||
        lxp_state_store_bind_accounts(&f->state, &f->accounts_a) != LXP_OK ||
        lxp_state_store_require_account_root(&f->state) != LXP_OK ||
        lxp_kernel_create(&f->kernel, &f->state, &f->journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&f->kernel, lx_asset_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&f->kernel, NULL, apply_capability) != LXP_OK ||
        fresh_ctx(f) != 0 || roots(f, before) != 0 ||
        conserved_both(f, 0U, 0U, 0U, 0U) != 0) return 1;

    request_context(f, f->agent_a, f->withdrawals_a, &f->asset_a, 40U,
                    &authority, &transfer);
    (void)memset(&withdrawal_a, 0, sizeof(withdrawal_a));
    withdrawal_a.network_id = 7U; withdrawal_a.withdrawal_id[0] = 2U;
    (void)memcpy(withdrawal_a.account_id, f->agent_a->id, 32U);
    (void)memcpy(withdrawal_a.asset_id, f->asset_a.asset_id, 32U);
    withdrawal_a.amount = transfer.amount; withdrawal_a.checkpoint_id[0] = 3U;
    if (lx_withdrawal_nullifier(&withdrawal_a, nullifier_a) != LXP_OK ||
        lx_asset_withdraw_request(&f->ctx, &transfer, &withdrawal_a, &f->store,
                                  &f->receipt) != LXP_OK ||
        f->agent_a->balance.lo != 60U || f->withdrawals_a->balance.lo != 40U ||
        f->reserve_a->balance.lo != 0U || f->store.count != 1U ||
        !lx_asset_nullifier_seen(&f->store, nullifier_a) ||
        lx_asset_total_units(&f->assets, &f->accounts_a, f->asset_a.asset_id,
                             &total) != LXP_OK || total.lo != 100U ||
        roots(f, after) != 0 || memcmp(before, after, 32U) == 0 ||
        memcmp(before + 32, after + 32, 32U) == 0 ||
        memcmp(before + 64, after + 64, 32U) != 0 ||
        conserved_both(f, 40U, 0U, 0U, 0U) != 0) return 1;

    crossed = withdrawal_a;
    crossed.withdrawal_id[0] = 9U;
    (void)memcpy(crossed.asset_id, f->asset_b.asset_id, 32U);
    if (fresh_ctx(f) != 0 || roots(f, before) != 0 ||
        lx_asset_withdraw_request(&f->ctx, &transfer, &crossed, &f->store,
                                  &f->receipt) != LXP_ERR_GRANT_SCOPE_VIOLATION ||
        f->ctx.transfer_applied || f->store.count != 1U ||
        f->agent_a->balance.lo != 60U || f->withdrawals_a->balance.lo != 40U ||
        roots(f, after) != 0 || memcmp(before, after, sizeof(before)) != 0 ||
        conserved_both(f, 40U, 0U, 0U, 0U) != 0) return 1;

    if (fresh_ctx(f) != 0 ||
        lx_asset_withdraw_request(&f->ctx, &transfer, &withdrawal_a, &f->store,
                                  &f->receipt) !=
            LXP_ERR_WITHDRAWAL_ALREADY_SETTLED || f->agent_a->balance.lo != 60U)
        return 1;
    withdrawal_a.withdrawal_id[0] = 4U;
    transfer.amount = (lxp_u128){ 0U, 70U };
    withdrawal_a.amount = transfer.amount;
    transfer.context.actor_sequence = 1U;
    if (fresh_ctx(f) != 0 ||
        lx_asset_withdraw_request(&f->ctx, &transfer, &withdrawal_a, &f->store,
                                  &f->receipt) != LXP_ERR_INSUFFICIENT_BALANCE ||
        f->agent_a->balance.lo != 60U || f->withdrawals_a->balance.lo != 40U ||
        f->store.count != 1U || conserved_both(f, 40U, 0U, 0U, 0U) != 0)
        return 1;

    {
        lx_asset_transfer_request transfer_b;
        lxp_transfer_source_authority authority_b;
        request_context(f, f->agent_b, f->withdrawals_b, &f->asset_b, 30U,
                        &authority_b, &transfer_b);
        (void)memset(&withdrawal_b, 0, sizeof(withdrawal_b));
        withdrawal_b.network_id = 7U; withdrawal_b.withdrawal_id[0] = 5U;
        (void)memcpy(withdrawal_b.account_id, f->agent_b->id, 32U);
        (void)memcpy(withdrawal_b.asset_id, f->asset_b.asset_id, 32U);
        withdrawal_b.amount = transfer_b.amount;
        withdrawal_b.checkpoint_id[0] = 3U;
        if (lx_withdrawal_nullifier(&withdrawal_b, nullifier_b) != LXP_OK ||
            fresh_ctx(f) != 0 || roots(f, before) != 0 ||
            lx_asset_withdraw_request(&f->ctx, &transfer_b, &withdrawal_b,
                                      &f->store, &f->receipt) != LXP_OK ||
            f->agent_b->balance.lo != 70U || f->withdrawals_b->balance.lo != 30U ||
            f->reserve_b->balance.lo != 0U || f->store.count != 2U ||
            f->agent_a->balance.lo != 60U || f->withdrawals_a->balance.lo != 40U ||
            !lx_asset_nullifier_seen(&f->store, nullifier_b) ||
            memcmp(f->store.records[1].nullifier, nullifier_b, 32U) != 0 ||
            roots(f, after) != 0 || memcmp(before, after, 32U) != 0 ||
            memcmp(before + 32, after + 32, 32U) != 0 ||
            memcmp(before + 64, after + 64, 32U) == 0 ||
            conserved_both(f, 40U, 0U, 30U, 0U) != 0) return 1;
    }

    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.checkpoint_id[0] = 3U; checkpoint.state_root[0] = 5U;
    checkpoint.finalized = true;
    if (refused_settle(f, f->withdrawals_b, f->reserve_b, &f->asset_b,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_a, f->reserve_a, &f->asset_b,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_b, f->reserve_b, &f->asset_a,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_b, f->reserve_a, &f->asset_a,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_a, f->reserve_b, &f->asset_a,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_a, f->reserve_a, &f->asset_a,
                       &checkpoint, nullifier_b, 1U, 40U, 0U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_a, f->reserve_a, &f->asset_b,
                       &checkpoint, nullifier_b, 1U, 40U, 0U, 30U, 0U) != 0)
        return 1;

    if (matched_settle(f, f->withdrawals_a, f->reserve_a, &f->asset_a,
                       &checkpoint, nullifier_a, 0U, 40U, 0U, 40U, 30U, 0U) != 0 ||
        f->withdrawals_b->balance.lo != 30U || f->reserve_b->balance.lo != 0U ||
        refused_settle(f, f->withdrawals_a, f->reserve_a, &f->asset_a,
                       &checkpoint, nullifier_b, 1U, 0U, 40U, 30U, 0U) != 0 ||
        refused_settle(f, f->withdrawals_b, f->reserve_b, &f->asset_a,
                       &checkpoint, nullifier_b, 1U, 0U, 40U, 30U, 0U) != 0 ||
        matched_settle(f, f->withdrawals_b, f->reserve_b, &f->asset_b,
                       &checkpoint, nullifier_b, 1U, 30U, 0U, 40U, 0U, 30U) != 0 ||
        f->reserve_a->balance.lo != 40U || f->withdrawals_a->balance.lo != 0U ||
        lx_asset_total_units(&f->assets, &f->accounts_a, f->asset_a.asset_id,
                             &total) != LXP_OK || total.lo != 100U ||
        lx_asset_total_units(&f->assets, &f->accounts_b, f->asset_b.asset_id,
                             &total) != LXP_OK || total.lo != 100U)
        return 1;

    f->store.count = LX_DEPOSIT_NULLIFIER_CAPACITY + 1U;
    if (fresh_ctx(f) != 0 ||
        !lx_asset_nullifier_seen(&f->store, nullifier_a) ||
        lx_asset_withdraw_request(&f->ctx, &transfer, &withdrawal_a, &f->store,
                                  &f->receipt) != LXP_ERR_NON_CANONICAL)
        return 1;
    {
        lxp_transfer_context settlement = { 0 };
        if (fresh_ctx(f) != 0 ||
            lx_asset_withdraw_settle(&f->ctx, f->withdrawals_a, f->reserve_a,
                                     &f->asset_a, &checkpoint, nullifier_a,
                                     &f->store, settlement, &f->receipt) !=
            LXP_ERR_NON_CANONICAL)
            return 1;
    }
    if (lxp_state_store_destroy(&f->state) != LXP_OK) return 1;
    return 0;
}
