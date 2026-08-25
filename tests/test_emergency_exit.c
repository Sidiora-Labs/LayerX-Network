#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_bridge.h"

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

static int balance_leaf(const lxp_exit_balance_record *record,
                        uint8_t leaf_hash[32])
{
    uint8_t canonical[112];
    uint8_t amount[16];
    if (lxp_u128_to_be(record->balance, amount) != LXP_OK) return 1;
    (void)memcpy(canonical, record->account_id, 32U);
    (void)memcpy(canonical + 32U, record->asset_id, 32U);
    (void)memcpy(canonical + 64U, amount, 16U);
    (void)memcpy(canonical + 80U, record->payout_recipient, 32U);
    return lxp_merkle_leaf_hash(canonical, sizeof(canonical), leaf_hash) ==
        LXP_OK ? 0 : 1;
}

int main(void)
{
    uint8_t arena_storage[131072];
    lxp_arena arena;
    lxp_exit_state exit_state;
    lxp_exit_balance_record balance;
    lxp_checkpoint_certificate checkpoint_certificate;
    lxp_guarantor_ctx guarantor;
    lxp_guarantor_attestation attestation;
    lxp_guarantor_key_record recorded_key;
    lxp_guarantor_cert certificate;
    lx_finalized_checkpoint checkpoint;
    lxp_merkle_proof proof;
    lxp_exit_claim claim;
    lx_withdrawal_request ordinary_route;
    lx_withdrawal_store shared_store;
    uint8_t leaf_hash[32];
    uint8_t exit_nullifier[32];
    uint8_t ordinary_nullifier[32];
    bool eligible = false;

    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK)
        return 1;
    (void)memset(&exit_state, 0, sizeof(exit_state));
    exit_state.now_ms = 150U;
    exit_state.last_finalised_at_ms = 100U;
    exit_state.liveness_bound_ms = 100U;
    exit_state.last_finalised_sequence = 900U;
    if (lxp_exit_eligibility(&exit_state, &eligible) != LXP_OK || eligible ||
        lxp_exit_declare(&exit_state) != LXP_ERR_DEPOSIT_PROOF_NOT_FINAL)
        return 1;
    exit_state.now_ms = 200U;
    if (lxp_exit_declare(&exit_state) != LXP_OK || !exit_state.declared ||
        exit_state.discard_after_sequence != 900U) return 1;
    exit_state.declared = false;
    exit_state.now_ms = 101U;
    exit_state.governance_emergency = true;
    if (lxp_exit_declare(&exit_state) != LXP_OK) return 1;
    exit_state.governance_emergency = false;
    exit_state.latest_checkpoint_fraud_accepted = true;
    if (lxp_exit_eligibility(&exit_state, &eligible) != LXP_OK || !eligible)
        return 1;

    (void)memset(&balance, 0, sizeof(balance));
    balance.account_id[0] = 1U;
    balance.asset_id[0] = 2U;
    balance.balance = (lxp_u128){0U, 700000000U};
    balance.payout_recipient[31] = 0xaaU;
    if (balance_leaf(&balance, leaf_hash) != 0) return 1;
    (void)memset(&checkpoint_certificate, 0, sizeof(checkpoint_certificate));
    checkpoint_certificate.header.protocol_version = LXP_PROTOCOL_VERSION;
    checkpoint_certificate.header.network_id = 42U;
    checkpoint_certificate.header.epoch = 7U;
    checkpoint_certificate.header.batch_number = 8U;
    checkpoint_certificate.header.first_sequence = 1U;
    checkpoint_certificate.header.last_sequence = 900U;
    checkpoint_certificate.header.previous_state_root[0] = 9U;
    (void)memcpy(checkpoint_certificate.header.resulting_state_root,
                 leaf_hash, 32U);
    checkpoint_certificate.header.activity_merkle_root[0] = 1U;
    checkpoint_certificate.header.receipt_merkle_root[0] = 2U;
    checkpoint_certificate.header.event_merkle_root[0] = 3U;
    checkpoint_certificate.header.data_availability_root[0] = 4U;
    checkpoint_certificate.header.oracle_root[0] = 5U;
    checkpoint_certificate.header.timestamp_ms = 1000U;
    checkpoint_certificate.header.sequencer_id[0] = 6U;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 1U;
    guarantor.ready_to_sign = true;
    guarantor.possesses_availability = true;
    guarantor.bond_view.bonded = true;
    guarantor.protocol_version = LXP_PROTOCOL_VERSION;
    guarantor.network_id = 42U;
    guarantor.paxeer_chain_id = 31337U;
    guarantor.paxeer_settlement_contract[0] = 0xa1U;
    if (key_pair(1U, guarantor.paxeer_private_key,
                 guarantor.paxeer_public_key) != 0 ||
        lxp_guarantor_attest(
            &guarantor, &checkpoint_certificate, true, true, 1001U,
            &arena, &attestation) != LXP_OK ||
        lxp_guarantor_cert_assemble(
            &checkpoint_certificate, &attestation, 1U, 1U,
            &certificate) != LXP_OK)
        return 1;
    (void)memset(&recorded_key, 0, sizeof(recorded_key));
    recorded_key.guarantor_id[0] = 1U;
    (void)memcpy(recorded_key.public_key,
                 guarantor.paxeer_public_key, 33U);
    recorded_key.bonded = true;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    if (lxp_checkpoint_certificate_hash(
            &checkpoint_certificate, &arena,
            checkpoint.checkpoint_id) != LXP_OK)
        return 1;
    (void)memcpy(checkpoint.state_root, leaf_hash, 32U);
    checkpoint.finalized = true;
    (void)memset(&proof, 0, sizeof(proof));
    proof.leaf_count = 1U;
    if (lxp_exit_claim_build(
            &checkpoint, &certificate, &balance, &proof,
            &arena, &claim) != LXP_OK ||
        lxp_exit_verify_balance_proof(
            &claim, &recorded_key, 1U, &arena) != LXP_OK ||
        claim.withdrawal.network_id != 42U ||
        claim.withdrawal.amount.lo != 700000000U ||
        memcmp(claim.withdrawal.checkpoint_id,
               checkpoint.checkpoint_id, 32U) != 0)
        return 1;
    proof.leaf_count = 2U;
    claim.balance_proof = proof;
    if (lxp_exit_verify_balance_proof(
            &claim, &recorded_key, 1U, &arena) != LXP_ERR_ROOT_MISMATCH)
        return 1;
    claim.balance_proof.leaf_count = 1U;
    ordinary_route = claim.withdrawal;
    if (lxp_withdrawal_nullifier(&claim.withdrawal, exit_nullifier) != LXP_OK ||
        lxp_withdrawal_nullifier(&ordinary_route, ordinary_nullifier) != LXP_OK ||
        memcmp(exit_nullifier, ordinary_nullifier, 32U) != 0)
        return 1;
    (void)memset(&shared_store, 0, sizeof(shared_store));
    shared_store.count = 1U;
    (void)memcpy(shared_store.records[0].nullifier,
                 exit_nullifier, 32U);
    return lx_asset_nullifier_seen(&shared_store, ordinary_nullifier) ? 0 : 1;
}
