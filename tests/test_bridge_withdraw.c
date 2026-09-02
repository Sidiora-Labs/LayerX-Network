#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_bridge.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_state.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

typedef struct bridge_fixture {
    lx_asset_registry *assets;
    lx_account_registry *accounts_a;
    lx_account_registry *accounts_b;
    lx_account *ledger[6];
    lxp_transfer_asset_state asset_states[2];
    lx_withdrawal_store *store;
    lxp_kernel *kernel;
    lxp_module_ctx *ctx;
    lxp_arena *arena;
    uint8_t *arena_bytes;
    size_t arena_capacity;
    lxp_receipt *receipt;
    uint64_t global_sequence;
} bridge_fixture;

static int key_pair(uint8_t value, uint8_t private_key[32],
                    uint8_t public_key[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value = BN_new();
    const EC_GROUP *group = key == NULL ? NULL : EC_KEY_get0_group(key);
    EC_POINT *point = group == NULL ? NULL : EC_POINT_new(group);
    size_t public_length = 0U;
    (void)memset(private_key, 0, 32U);
    private_key[31] = value;
    if (key != NULL && private_value != NULL && point != NULL &&
        BN_bin2bn(private_key, 32, private_value) != NULL &&
        EC_POINT_mul(group, point, private_value, NULL, NULL, NULL) == 1 &&
        EC_KEY_set_private_key(key, private_value) == 1 &&
        EC_KEY_set_public_key(key, point) == 1)
        public_length = EC_POINT_point2oct(
            group, point, POINT_CONVERSION_COMPRESSED,
            public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return public_length == 33U ? 0 : 1;
}

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

static int fresh_ctx(bridge_fixture *f)
{
    ++f->global_sequence;
    if (lxp_arena_init(f->arena, f->arena_bytes, f->arena_capacity) != LXP_OK ||
        lxp_module_ctx_init(f->ctx, f->kernel, LXP_MODULE_ASSET, 10U, 0U,
                            f->global_sequence, 1000U, f->arena, true) !=
            LXP_OK)
        return 1;
    return 0;
}

static int open_ledger(lx_asset_registry *assets, const lx_asset_record *asset,
                       lx_account_registry *accounts, const char *agent_name,
                       lx_account **agent, lx_account **withdrawals,
                       lx_account **reserve)
{
    if (lx_account_registry_init(accounts) != LXP_OK ||
        lx_asset_account_open(assets, accounts, asset->asset_id,
            (const uint8_t *)agent_name, strlen(agent_name), 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, agent) != LXP_OK ||
        lx_asset_account_open(assets, accounts, asset->asset_id,
            (const uint8_t *)"system:paxeer-withdrawals", 25U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, withdrawals) != LXP_OK ||
        lx_asset_account_open(assets, accounts, asset->asset_id,
            (const uint8_t *)"system:paxeer-reserve", 21U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, reserve) != LXP_OK ||
        lxp_ledger_bootstrap_balance(*agent, asset->asset_id,
            (lxp_u128){0U, 100U}, 0U) != LXP_OK)
        return 1;
    return 0;
}

static int roots(const bridge_fixture *f, uint8_t out[96])
{
    if (lxp_state_root(f->kernel, out) != LXP_OK ||
        lx_asset_state_root(f->assets, f->accounts_a, out + 32) != LXP_OK ||
        lx_asset_state_root(f->assets, f->accounts_b, out + 64) != LXP_OK)
        return 1;
    return 0;
}

static void balances(const bridge_fixture *f, lxp_u128 out[6])
{
    size_t i;
    for (i = 0U; i < 6U; ++i) out[i] = f->ledger[i]->balance;
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
    *outstanding = (lxp_u128){0U, 0U};
    *settled = (lxp_u128){0U, 0U};
    for (i = 0U; i < store->count; ++i) {
        const lx_withdrawal_record *record = &store->records[i];
        lxp_u128 *bucket = record->settled ? settled : outstanding;
        if (memcmp(record->request.asset_id, asset_id, 32U) != 0) continue;
        if (lxp_u128_add(*bucket, record->request.amount, bucket) != LXP_OK)
            return 1;
    }
    return 0;
}

static int conserved(const bridge_fixture *f,
                     const lx_account_registry *accounts,
                     const lx_account *agent, const uint8_t asset_id[32],
                     uint64_t outstanding_expected, uint64_t settled_expected)
{
    lx_asset_custody_attestation attestation;
    lx_asset_reserve_report_record report;
    lxp_u128 outstanding;
    lxp_u128 settled;
    lxp_u128 total;
    lxp_u128 custody;
    if (withdrawal_totals(f->store, asset_id, &outstanding, &settled) != 0 ||
        outstanding.hi != 0U || outstanding.lo != outstanding_expected ||
        settled.hi != 0U || settled.lo != settled_expected)
        return 1;
    (void)memset(&attestation, 0, sizeof(attestation));
    (void)memcpy(attestation.asset_id, asset_id, 32U);
    attestation.custody_amount = (lxp_u128){0U, 100U};
    attestation.settled_out = settled;
    attestation.checkpoint_id[0] = 3U;
    attestation.state_root[0] = 5U;
    attestation.finalized = true;
    if (lx_asset_reserve_reconcile(accounts, &attestation, &report) != LXP_OK ||
        lx_asset_total_units(f->assets, accounts, asset_id, &total) != LXP_OK ||
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

static int conserved_both(const bridge_fixture *f,
                          const uint8_t asset_a[32], const uint8_t asset_b[32],
                          uint64_t outstanding_a, uint64_t settled_a,
                          uint64_t outstanding_b, uint64_t settled_b)
{
    if (conserved(f, f->accounts_a, f->ledger[0], asset_a,
                  outstanding_a, settled_a) != 0 ||
        conserved(f, f->accounts_b, f->ledger[3], asset_b,
                  outstanding_b, settled_b) != 0)
        return 1;
    return 0;
}

static void request_context(const bridge_fixture *f, lx_account *agent,
                            lx_account *withdrawals,
                            const lx_asset_record *asset, lxp_u128 amount,
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
    transfer->amount = amount;
    transfer->context.assets = f->asset_states;
    transfer->context.asset_count = 2U;
    transfer->context.sequence_account = agent;
    transfer->context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(transfer->context.authorized_from, agent->id, 32U);
    transfer->context.source_authorities = authority;
    transfer->context.source_authority_count = 1U;
}

static void settlement_context(const bridge_fixture *f,
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

static int refused_finalize(bridge_fixture *f, lx_account *withdrawals,
                            lx_account *reserve, const lx_asset_record *asset,
                            const lx_withdrawal_request *withdrawal,
                            const lxp_withdrawal_claim *claim,
                            lxp_result expected, size_t record_index,
                            const uint8_t asset_a[32],
                            const uint8_t asset_b[32],
                            uint64_t outstanding_a, uint64_t settled_a,
                            uint64_t outstanding_b, uint64_t settled_b)
{
    uint8_t before[96];
    uint8_t after[96];
    uint8_t nullifier[32];
    lxp_u128 balances_before[6];
    lxp_u128 balances_after[6];
    lxp_transfer_source_authority authority;
    lxp_transfer_context settlement;
    if (lxp_withdrawal_nullifier(withdrawal, nullifier) != LXP_OK ||
        fresh_ctx(f) != 0 || roots(f, before) != 0) return 1;
    balances(f, balances_before);
    settlement_context(f, withdrawals, &authority, &settlement);
    if (lxp_bridge_withdraw_finalize(
            f->ctx, withdrawals, reserve, asset, withdrawal, f->store, claim,
            settlement, f->receipt) != expected ||
        f->ctx->transfer_applied ||
        f->store->records[record_index].settled ||
        memcmp(f->store->records[record_index].nullifier, nullifier, 32U) != 0 ||
        !lx_asset_nullifier_seen(f->store, nullifier) ||
        roots(f, after) != 0 || memcmp(before, after, sizeof(before)) != 0)
        return 1;
    balances(f, balances_after);
    if (same_balances(balances_before, balances_after) != 0 ||
        conserved_both(f, asset_a, asset_b, outstanding_a, settled_a,
                       outstanding_b, settled_b) != 0)
        return 1;
    return 0;
}

int main(void)
{
    static uint8_t arena_storage[262144];
    static lx_asset_registry assets;
    static lx_account_registry accounts;
    static lx_account_registry accounts_b;
    lxp_arena arena;
    lx_asset_record asset;
    lx_asset_record asset_b;
    lx_account *agent;
    lx_account *withdrawals;
    lx_account *reserve;
    lx_account *agent_b;
    lx_account *withdrawals_b;
    lx_account *reserve_b;
    lx_asset_transfer_request transfer;
    lxp_transfer_source_authority authority;
    lx_withdrawal_request withdrawal;
    lx_withdrawal_request withdrawal_b;
    lx_withdrawal_request altered_withdrawal;
    lx_withdrawal_store store;
    lxp_checkpoint_certificate checkpoint_certificate;
    lxp_guarantor_ctx guarantors[3];
    lxp_guarantor_attestation attestations[3];
    lxp_guarantor_key_record keys[3];
    lxp_guarantor_cert certificate;
    lx_finalized_checkpoint checkpoint;
    lxp_challenge_window_state window;
    lxp_challenge_window_state challenged;
    lxp_withdrawal_claim claim;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx module_ctx;
    lxp_receipt receipt;
    uint8_t module_arena_bytes[4096];
    lxp_arena module_arena;
    bridge_fixture fixture;
    uint64_t parameters = 1U;
    uint8_t leaf_hash[32];
    uint8_t checkpoint_id[32];
    uint8_t nullifier[32];
    uint8_t nullifier_b[32];
    uint8_t before[96];
    uint8_t after[96];
    lxp_u128 total;
    size_t i;

    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK)
        return 1;
    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    (void)memcpy(asset.symbol, "A", 2U);
    asset.symbol_length = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    asset_b = asset;
    asset_b.asset_id[0] = 2U;
    (void)memcpy(asset_b.symbol, "B", 2U);
    asset_b.custody_reference[0] = 2U;
    (void)memset(&fixture, 0, sizeof(fixture));
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U, (lxp_u128){0U, 0U}) != LXP_OK ||
        lx_asset_register(&assets, &asset_b, 1U, (lxp_u128){0U, 0U}) != LXP_OK ||
        open_ledger(&assets, &asset, &accounts, "agent:did:key:a:main",
                    &agent, &withdrawals, &reserve) != 0 ||
        open_ledger(&assets, &asset_b, &accounts_b, "agent:did:key:b:main",
                    &agent_b, &withdrawals_b, &reserve_b) != 0 ||
        lx_asset_transfer_state(&asset, &fixture.asset_states[0]) != LXP_OK ||
        lx_asset_transfer_state(&asset_b, &fixture.asset_states[1]) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_state_store_bind_accounts(&state, &accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&state) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK)
        return 1;
    fixture.assets = &assets;
    fixture.accounts_a = &accounts;
    fixture.accounts_b = &accounts_b;
    fixture.ledger[0] = agent;
    fixture.ledger[1] = withdrawals;
    fixture.ledger[2] = reserve;
    fixture.ledger[3] = agent_b;
    fixture.ledger[4] = withdrawals_b;
    fixture.ledger[5] = reserve_b;
    fixture.store = &store;
    fixture.kernel = &kernel;
    fixture.ctx = &module_ctx;
    fixture.arena = &module_arena;
    fixture.arena_bytes = module_arena_bytes;
    fixture.arena_capacity = sizeof(module_arena_bytes);
    fixture.receipt = &receipt;
    (void)memset(&store, 0, sizeof(store));
    if (fresh_ctx(&fixture) != 0 ||
        conserved_both(&fixture, asset.asset_id, asset_b.asset_id,
                       0U, 0U, 0U, 0U) != 0)
        return 1;

    (void)memset(&withdrawal, 0, sizeof(withdrawal));
    withdrawal.network_id = 7U;
    withdrawal.withdrawal_id[0] = 2U;
    (void)memcpy(withdrawal.account_id, agent->id, 32U);
    (void)memcpy(withdrawal.asset_id, asset.asset_id, 32U);
    withdrawal.amount = (lxp_u128){0U, 40U};
    withdrawal.payout_recipient[31] = 0xaaU;
    if (lxp_withdrawal_leaf(&withdrawal, leaf_hash) != LXP_OK) return 1;

    (void)memset(&checkpoint_certificate, 0, sizeof(checkpoint_certificate));
    checkpoint_certificate.header.protocol_version = LXP_PROTOCOL_VERSION;
    checkpoint_certificate.header.network_id = 7U;
    checkpoint_certificate.header.epoch = 1U;
    checkpoint_certificate.header.batch_number = 1U;
    checkpoint_certificate.header.first_sequence = 1U;
    checkpoint_certificate.header.last_sequence = 1U;
    checkpoint_certificate.header.previous_state_root[0] = 9U;
    (void)memcpy(checkpoint_certificate.header.resulting_state_root,
                 leaf_hash, 32U);
    checkpoint_certificate.header.activity_merkle_root[0] = 1U;
    checkpoint_certificate.header.receipt_merkle_root[0] = 2U;
    checkpoint_certificate.header.event_merkle_root[0] = 3U;
    checkpoint_certificate.header.data_availability_root[0] = 4U;
    checkpoint_certificate.header.oracle_root[0] = 5U;
    checkpoint_certificate.header.timestamp_ms = 100U;
    checkpoint_certificate.header.sequencer_id[0] = 6U;
    for (i = 0U; i < 3U; ++i) {
        (void)memset(&guarantors[i], 0, sizeof(guarantors[i]));
        guarantors[i].guarantor_id[0] = (uint8_t)(i + 1U);
        guarantors[i].ready_to_sign = true;
        guarantors[i].possesses_availability = true;
        guarantors[i].bond_view.bonded = true;
        guarantors[i].protocol_version = LXP_PROTOCOL_VERSION;
        guarantors[i].network_id = 7U;
        guarantors[i].paxeer_chain_id = 31337U;
        guarantors[i].paxeer_settlement_contract[0] = 0xa1U;
        if (key_pair((uint8_t)(i + 1U), guarantors[i].paxeer_private_key,
                     guarantors[i].paxeer_public_key) != 0) return 1;
        (void)memcpy(keys[i].guarantor_id, guarantors[i].guarantor_id, 32U);
        (void)memcpy(keys[i].public_key,
                     guarantors[i].paxeer_public_key, 33U);
        keys[i].bonded = true;
        if (lxp_guarantor_attest(
                &guarantors[i], &checkpoint_certificate, true, true,
                101U + i, &arena, &attestations[i]) != LXP_OK) return 1;
    }
    if (lxp_guarantor_cert_assemble(
            &checkpoint_certificate, attestations, 3U, 2U,
            &certificate) != LXP_OK ||
        lxp_checkpoint_certificate_hash(
            &checkpoint_certificate, &arena, checkpoint_id) != LXP_OK)
        return 1;
    (void)memcpy(withdrawal.checkpoint_id, checkpoint_id, 32U);
    if (lxp_withdrawal_nullifier(&withdrawal, nullifier) != LXP_OK) return 1;

    request_context(&fixture, agent, withdrawals, &asset, withdrawal.amount,
                    &authority, &transfer);
    if (roots(&fixture, before) != 0 ||
        lxp_bridge_withdraw_request(
            &module_ctx, &transfer, &withdrawal, &store, &receipt) != LXP_OK ||
        agent->balance.lo != 60U || withdrawals->balance.lo != 40U ||
        store.count != 1U ||
        memcmp(store.records[0].nullifier, nullifier, 32U) != 0 ||
        roots(&fixture, after) != 0 || memcmp(before, after, 32U) == 0 ||
        memcmp(before + 32, after + 32, 32U) == 0 ||
        memcmp(before + 64, after + 64, 32U) != 0 ||
        conserved_both(&fixture, asset.asset_id, asset_b.asset_id,
                       40U, 0U, 0U, 0U) != 0)
        return 1;

    (void)memset(&withdrawal_b, 0, sizeof(withdrawal_b));
    withdrawal_b.network_id = 7U;
    withdrawal_b.withdrawal_id[0] = 3U;
    (void)memcpy(withdrawal_b.account_id, agent_b->id, 32U);
    (void)memcpy(withdrawal_b.asset_id, asset_b.asset_id, 32U);
    withdrawal_b.amount = (lxp_u128){0U, 30U};
    withdrawal_b.payout_recipient[31] = 0xbbU;
    (void)memcpy(withdrawal_b.checkpoint_id, checkpoint_id, 32U);
    {
        lx_asset_transfer_request transfer_b;
        lxp_transfer_source_authority authority_b;
        request_context(&fixture, agent_b, withdrawals_b, &asset_b,
                        withdrawal_b.amount, &authority_b, &transfer_b);
        if (lxp_withdrawal_nullifier(&withdrawal_b, nullifier_b) != LXP_OK ||
            fresh_ctx(&fixture) != 0 || roots(&fixture, before) != 0 ||
            lxp_bridge_withdraw_request(
                &module_ctx, &transfer_b, &withdrawal_b, &store, &receipt) !=
                LXP_OK ||
            agent_b->balance.lo != 70U || withdrawals_b->balance.lo != 30U ||
            agent->balance.lo != 60U || withdrawals->balance.lo != 40U ||
            store.count != 2U ||
            memcmp(store.records[1].nullifier, nullifier_b, 32U) != 0 ||
            roots(&fixture, after) != 0 || memcmp(before, after, 64U) != 0 ||
            memcmp(before + 64, after + 64, 32U) == 0 ||
            conserved_both(&fixture, asset.asset_id, asset_b.asset_id,
                           40U, 0U, 30U, 0U) != 0)
            return 1;
    }

    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    (void)memcpy(checkpoint.checkpoint_id, checkpoint_id, 32U);
    (void)memcpy(checkpoint.state_root, leaf_hash, 32U);
    checkpoint.finalized = true;
    (void)memset(&window, 0, sizeof(window));
    (void)memcpy(window.checkpoint_id, checkpoint_id, 32U);
    window.opened_at_ms = 100U;
    window.closes_at_ms = 200U;
    (void)memset(&claim, 0, sizeof(claim));
    claim.checkpoint = &checkpoint;
    claim.certificate = &certificate;
    claim.guarantor_keys = keys;
    claim.guarantor_key_count = 3U;
    claim.state_membership_proof.leaf_count = 1U;
    claim.challenge_window = &window;
    claim.now_ms = 150U;
    claim.arena = &arena;
    {
        lxp_transfer_source_authority settlement_authority;
        lxp_transfer_context settlement;
        settlement_context(&fixture, withdrawals, &settlement_authority,
                           &settlement);
        if (fresh_ctx(&fixture) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            lxp_paxeer_challenge_window(
                &window, 160U, LXP_CHALLENGE_PENDING, 3U) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            fresh_ctx(&fixture) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            lxp_paxeer_challenge_window(
                &window, 180U, LXP_CHALLENGE_FAILED, 3U) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN)
            return 1;
        claim.now_ms = 201U;
        if (refused_finalize(&fixture, withdrawals_b, reserve_b, &asset_b,
                             &withdrawal, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 0U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals, reserve, &asset_b,
                             &withdrawal, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 0U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals_b, reserve_b, &asset,
                             &withdrawal, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 0U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals_b, reserve, &asset,
                             &withdrawal, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 0U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals, reserve_b, &asset,
                             &withdrawal, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 0U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals, reserve, &asset,
                             &withdrawal_b, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 1U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals_b, reserve_b, &asset,
                             &withdrawal_b, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 1U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals_b, reserve_b, &asset_b,
                             &withdrawal_b, &claim, LXP_ERR_ROOT_MISMATCH, 1U,
                             asset.asset_id, asset_b.asset_id,
                             40U, 0U, 30U, 0U) != 0)
            return 1;
        if (fresh_ctx(&fixture) != 0 || roots(&fixture, before) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) != LXP_OK ||
            !module_ctx.transfer_applied || !store.records[0].settled ||
            withdrawals->balance.lo != 0U || reserve->balance.lo != 40U ||
            withdrawals_b->balance.lo != 30U || reserve_b->balance.lo != 0U ||
            lx_asset_total_units(
                &assets, &accounts, asset.asset_id, &total) != LXP_OK ||
            total.lo != 100U ||
            lx_asset_total_units(
                &assets, &accounts_b, asset_b.asset_id, &total) != LXP_OK ||
            total.lo != 100U ||
            roots(&fixture, after) != 0 || memcmp(before, after, 32U) == 0 ||
            memcmp(before + 32, after + 32, 32U) == 0 ||
            memcmp(before + 64, after + 64, 32U) != 0 ||
            conserved_both(&fixture, asset.asset_id, asset_b.asset_id,
                           0U, 40U, 30U, 0U) != 0)
            return 1;
        if (refused_finalize(&fixture, withdrawals, reserve, &asset,
                             &withdrawal_b, &claim,
                             LXP_ERR_WITHDRAWAL_ASSET_MISMATCH, 1U,
                             asset.asset_id, asset_b.asset_id,
                             0U, 40U, 30U, 0U) != 0 ||
            refused_finalize(&fixture, withdrawals_b, reserve_b, &asset_b,
                             &withdrawal_b, &claim, LXP_ERR_ROOT_MISMATCH, 1U,
                             asset.asset_id, asset_b.asset_id,
                             0U, 40U, 30U, 0U) != 0)
            return 1;
        claim.state_membership_proof.leaf_count = 2U;
        if (fresh_ctx(&fixture) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED)
            return 1;
        altered_withdrawal = withdrawal;
        altered_withdrawal.checkpoint_id[0] ^= 0xffU;
        if (fresh_ctx(&fixture) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset,
                &altered_withdrawal, &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED)
            return 1;
        checkpoint.finalized = false;
        if (fresh_ctx(&fixture) != 0 ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED ||
            reserve->balance.lo != 40U || withdrawals_b->balance.lo != 30U ||
            conserved_both(&fixture, asset.asset_id, asset_b.asset_id,
                           0U, 40U, 30U, 0U) != 0)
            return 1;
    }

    (void)memset(&challenged, 0, sizeof(challenged));
    challenged.checkpoint_id[0] = 0x44U;
    challenged.opened_at_ms = 100U;
    challenged.closes_at_ms = 200U;
    if (lxp_paxeer_challenge_window(
            &challenged, 120U, LXP_CHALLENGE_PENDING, 3U) !=
            LXP_ERR_CHALLENGE_WINDOW_OPEN ||
        lxp_paxeer_challenge_window(
            &challenged, 220U, LXP_CHALLENGE_SUCCEEDED, 3U) !=
            LXP_ERR_WITHDRAWAL_CANCELLED ||
        !challenged.payouts_cancelled ||
        challenged.slashed_attester_count != 3U ||
        lxp_paxeer_challenge_window(
            &challenged, 230U, LXP_CHALLENGE_NONE, 3U) !=
            LXP_ERR_WITHDRAWAL_CANCELLED)
        return 1;
    return lxp_state_store_destroy(&state) == LXP_OK ? 0 : 1;
}
