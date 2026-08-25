#ifndef LAYERX_LXP_BRIDGE_H
#define LAYERX_LXP_BRIDGE_H

#include "layerx/lx_asset.h"
#include "layerx/lxp_guarantor.h"

#include <stdint.h>

typedef struct lxp_bridge_deposit_context {
    lxp_module_ctx *module_ctx;
    lx_asset_registry *assets;
    const lx_account_registry *accounts;
    const lx_checkpoint_registry *checkpoints;
    uint32_t network_id;
    uint16_t protocol_version;
} lxp_bridge_deposit_context;

typedef enum lxp_challenge_outcome {
    LXP_CHALLENGE_NONE = 0,
    LXP_CHALLENGE_PENDING = 1,
    LXP_CHALLENGE_SUCCEEDED = 2,
    LXP_CHALLENGE_FAILED = 3
} lxp_challenge_outcome;

typedef struct lxp_challenge_window_state {
    uint8_t checkpoint_id[32];
    uint64_t opened_at_ms;
    uint64_t closes_at_ms;
    lxp_challenge_outcome outcome;
    size_t slashed_attester_count;
    bool payouts_cancelled;
} lxp_challenge_window_state;

typedef struct lxp_withdrawal_claim {
    const lx_finalized_checkpoint *checkpoint;
    const lxp_guarantor_cert *certificate;
    const lxp_guarantor_key_record *guarantor_keys;
    size_t guarantor_key_count;
    lxp_merkle_proof state_membership_proof;
    lxp_challenge_window_state *challenge_window;
    uint64_t now_ms;
    lxp_arena *arena;
} lxp_withdrawal_claim;

typedef struct lxp_exit_state {
    uint64_t now_ms;
    uint64_t last_finalised_at_ms;
    uint64_t liveness_bound_ms;
    uint64_t last_finalised_sequence;
    uint64_t discard_after_sequence;
    bool governance_emergency;
    bool latest_checkpoint_fraud_accepted;
    bool declared;
} lxp_exit_state;

typedef struct lxp_exit_balance_record {
    uint8_t account_id[32];
    uint8_t asset_id[32];
    lxp_u128 balance;
    uint8_t payout_recipient[32];
} lxp_exit_balance_record;

typedef struct lxp_exit_claim {
    const lx_finalized_checkpoint *checkpoint;
    const lxp_guarantor_cert *certificate;
    lxp_exit_balance_record balance_record;
    lxp_merkle_proof balance_proof;
    lx_withdrawal_request withdrawal;
} lxp_exit_claim;

lxp_result lxp_deposit_nullifier(const lx_deposit_proof *proof,
                                 uint8_t nullifier[32]);
lxp_result lxp_deposit_proof_verify(const lx_deposit_proof *proof,
                                    const lx_checkpoint_registry *checkpoints,
                                    uint32_t network_id,
                                    uint16_t protocol_version);
lxp_result lxp_bridge_deposit_credit(
    const lxp_bridge_deposit_context *bridge,
    const lx_asset_transfer_request *transfer,
    const lx_deposit_proof *proof,
    lxp_receipt *receipt);
lxp_result lxp_withdrawal_nullifier(const lx_withdrawal_request *request,
                                    uint8_t nullifier[32]);
lxp_result lxp_withdrawal_leaf(const lx_withdrawal_request *request,
                               uint8_t leaf_hash[32]);
lxp_result lxp_bridge_withdraw_request(
    lxp_module_ctx *ctx,
    const lx_asset_transfer_request *transfer,
    const lx_withdrawal_request *withdrawal,
    lx_withdrawal_store *store,
    lxp_receipt *receipt);
lxp_result lxp_paxeer_challenge_window(
    lxp_challenge_window_state *window,
    uint64_t now_ms,
    lxp_challenge_outcome resolution,
    size_t attesting_guarantor_count);
lxp_result lxp_bridge_withdraw_finalize(
    lxp_module_ctx *ctx,
    lx_account *withdrawals,
    lx_account *reserve,
    const lx_asset_record *asset,
    const lx_withdrawal_request *withdrawal,
    lx_withdrawal_store *store,
    const lxp_withdrawal_claim *claim,
    lxp_transfer_context transfer_context,
    lxp_receipt *receipt);
lxp_result lxp_exit_eligibility(const lxp_exit_state *state, bool *eligible);
lxp_result lxp_exit_declare(lxp_exit_state *state);
lxp_result lxp_exit_claim_build(
    const lx_finalized_checkpoint *checkpoint,
    const lxp_guarantor_cert *certificate,
    const lxp_exit_balance_record *balance_record,
    const lxp_merkle_proof *balance_proof,
    lxp_arena *arena,
    lxp_exit_claim *claim);
lxp_result lxp_exit_verify_balance_proof(
    const lxp_exit_claim *claim,
    const lxp_guarantor_key_record *guarantor_keys,
    size_t guarantor_key_count,
    lxp_arena *arena);

#endif
