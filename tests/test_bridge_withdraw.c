#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_bridge.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <stdint.h>
#include <string.h>

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

int main(void)
{
    uint8_t arena_storage[262144];
    lxp_arena arena;
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *agent;
    lx_account *withdrawals;
    lx_account *reserve;
    lxp_transfer_asset_state asset_state;
    lx_asset_transfer_request transfer;
    lx_withdrawal_request withdrawal;
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
    uint64_t parameters = 1U;
    uint8_t leaf_hash[32];
    uint8_t checkpoint_id[32];
    uint8_t nullifier[32];
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
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U, (lxp_u128){0U, 0U}) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"agent:did:key:a:main", 20U, 1U,
            LX_ACCOUNT_OPEN_CREDIT, NULL, &agent) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"system:paxeer-withdrawals", 25U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, &withdrawals) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
            (const uint8_t *)"system:paxeer-reserve", 21U, 1U,
            LX_ACCOUNT_OPEN_GENESIS, NULL, &reserve) != LXP_OK ||
        lxp_ledger_bootstrap_balance(agent, asset.asset_id,
            (lxp_u128){0U, 100U}, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_asset_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&module_arena, module_arena_bytes,
                       sizeof(module_arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&module_ctx, &kernel, LXP_MODULE_ASSET, 10U, 0U,
                            1U, 1000U, &module_arena, true) != LXP_OK)
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

    (void)memset(&transfer, 0, sizeof(transfer));
    transfer.from = agent;
    transfer.to = withdrawals;
    transfer.asset = &asset;
    transfer.amount = withdrawal.amount;
    transfer.context.assets = &asset_state;
    transfer.context.asset_count = 1U;
    transfer.context.sequence_account = agent;
    (void)memcpy(transfer.context.authorized_from, agent->id, 32U);
    (void)memset(&store, 0, sizeof(store));
    if (lxp_bridge_withdraw_request(
            &module_ctx, &transfer, &withdrawal, &store, &receipt) != LXP_OK ||
        agent->balance.lo != 60U || withdrawals->balance.lo != 40U ||
        store.count != 1U ||
        memcmp(store.records[0].nullifier, nullifier, 32U) != 0)
        return 1;

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
        lxp_transfer_context settlement = {0};
        settlement.assets = &asset_state;
        settlement.asset_count = 1U;
        settlement.protocol_system_capability = true;
        if (lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            lxp_paxeer_challenge_window(
                &window, 160U, LXP_CHALLENGE_PENDING, 3U) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN ||
            lxp_paxeer_challenge_window(
                &window, 180U, LXP_CHALLENGE_FAILED, 3U) !=
                LXP_ERR_CHALLENGE_WINDOW_OPEN)
            return 1;
        claim.now_ms = 201U;
        if (lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) != LXP_OK ||
            withdrawals->balance.lo != 0U || reserve->balance.lo != 40U ||
            lx_asset_total_units(
                &assets, &accounts, asset.asset_id, &total) != LXP_OK ||
            total.lo != 100U)
            return 1;
        claim.state_membership_proof.leaf_count = 2U;
        if (lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED)
            return 1;
        altered_withdrawal = withdrawal;
        altered_withdrawal.checkpoint_id[0] ^= 0xffU;
        if (lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset,
                &altered_withdrawal, &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED)
            return 1;
        checkpoint.finalized = false;
        if (lxp_bridge_withdraw_finalize(
                &module_ctx, withdrawals, reserve, &asset, &withdrawal,
                &store, &claim, settlement, &receipt) !=
                LXP_ERR_WITHDRAWAL_ALREADY_SETTLED)
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
