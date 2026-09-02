#include "layerx/lx_asset.h"
#include "layerx/lxp_bridge.h"
#include "lx_asset_internal.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_kernel.h"

#include <stdlib.h>
#include <string.h>

static void store_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

lxp_result lx_paxeer_deposit_root_message(
    const lx_paxeer_deposit_root_registration *registration,
    uint8_t *message, size_t capacity, size_t *message_length)
{
    static const uint8_t tag[] = "LX:PAXEER:DEPOSIT:ROOT:v1";
    const size_t required = sizeof(tag) - 1U + 32U * 4U + 4U + 2U;
    size_t cursor = 0U;
    if (registration == NULL || message == NULL || message_length == NULL ||
        capacity < required ||
        lxp_ct_is_zero(registration->checkpoint_id, 32U) ||
        lxp_ct_is_zero(registration->checkpoint_state_root, 32U) ||
        lxp_ct_is_zero(registration->deposit_root, 32U) ||
        lxp_ct_is_zero(registration->custody_reference, 32U) ||
        registration->network_id == 0U ||
        registration->protocol_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(message + cursor, tag, sizeof(tag) - 1U);
    cursor += sizeof(tag) - 1U;
#define COPY_REGISTRATION(field) do { \
    (void)memcpy(message + cursor, registration->field, 32U); cursor += 32U; \
} while (0)
    COPY_REGISTRATION(checkpoint_id);
    COPY_REGISTRATION(checkpoint_state_root);
    COPY_REGISTRATION(deposit_root);
    COPY_REGISTRATION(custody_reference);
#undef COPY_REGISTRATION
    store_u32(message + cursor, registration->network_id);
    cursor += 4U;
    message[cursor++] = (uint8_t)(registration->protocol_version >> 8U);
    message[cursor++] = (uint8_t)registration->protocol_version;
    *message_length = cursor;
    return LXP_OK;
}

lxp_result lx_checkpoint_registry_create(
    const uint8_t paxeer_checkpoint_authority[32],
    uint32_t network_id, uint16_t protocol_version,
    lx_checkpoint_registry **registry)
{
    lx_checkpoint_registry *created;
    if (registry == NULL || *registry != NULL ||
        paxeer_checkpoint_authority == NULL ||
        !lxp_ed25519_pubkey_is_canonical(paxeer_checkpoint_authority) ||
        network_id == 0U || protocol_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    created = (lx_checkpoint_registry *)calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(created->paxeer_checkpoint_authority,
                 paxeer_checkpoint_authority, 32U);
    created->network_id = network_id;
    created->protocol_version = protocol_version;
    *registry = created;
    return LXP_OK;
}

lxp_result lx_checkpoint_registry_destroy(lx_checkpoint_registry **registry)
{
    if (registry == NULL || *registry == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(*registry, 0, sizeof(**registry));
    free(*registry);
    *registry = NULL;
    return LXP_OK;
}

lxp_result lx_checkpoint_registry_register_deposit_root(
    lx_checkpoint_registry *registry,
    const lx_paxeer_deposit_root_registration *registration)
{
    uint8_t message[sizeof("LX:PAXEER:DEPOSIT:ROOT:v1") - 1U +
                    32U * 4U + 4U + 2U];
    size_t message_length;
    size_t i;
    lxp_result status;
    if (registry == NULL || registration == NULL ||
        registry->count > LX_CHECKPOINT_CAPACITY ||
        registry->network_id != registration->network_id ||
        registry->protocol_version != registration->protocol_version)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    for (i = 0U; i < registry->count; ++i)
        if (lxp_ct_memcmp(registry->checkpoints[i].checkpoint_id,
                          registration->checkpoint_id, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
    if (registry->count == LX_CHECKPOINT_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = lx_paxeer_deposit_root_message(
        registration, message, sizeof(message), &message_length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(
            registry->paxeer_checkpoint_authority, registration->signature,
            message, message_length);
    if (status != LXP_OK) return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    (void)memset(&registry->checkpoints[registry->count], 0,
                 sizeof(registry->checkpoints[0]));
    (void)memcpy(registry->checkpoints[registry->count].checkpoint_id,
                 registration->checkpoint_id, 32U);
    (void)memcpy(registry->checkpoints[registry->count].state_root,
                 registration->checkpoint_state_root, 32U);
    (void)memcpy(registry->checkpoints[registry->count].deposit_root,
                 registration->deposit_root, 32U);
    (void)memcpy(registry->checkpoints[registry->count].custody_reference,
                 registration->custody_reference, 32U);
    registry->checkpoints[registry->count].network_id =
        registration->network_id;
    registry->checkpoints[registry->count].protocol_version =
        registration->protocol_version;
    registry->checkpoints[registry->count].finalized = true;
    ++registry->count;
    return LXP_OK;
}

lxp_result lx_paxeer_deposit_leaf_hash(const lx_deposit_proof *proof,
                                       uint8_t leaf_hash[32])
{
    static const uint8_t tag[] = "LX:PAXEER:DEPOSIT:LEAF:v1";
    uint8_t input[sizeof(tag) - 1U + 32U * 4U + 16U + 4U + 2U];
    uint8_t amount[16];
    size_t cursor = 0U;
    lxp_result status;
    if (proof == NULL || leaf_hash == NULL ||
        lxp_ct_is_zero(proof->deposit_id, 32U) ||
        lxp_ct_is_zero(proof->custody_reference, 32U) ||
        lxp_ct_is_zero(proof->asset_id, 32U) ||
        lxp_ct_is_zero(proof->checkpoint_id, 32U) ||
        proof->network_id == 0U || proof->protocol_version == 0U ||
        lxp_u128_is_zero(proof->amount))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_to_be(proof->amount, amount);
    if (status != LXP_OK) return status;
    (void)memcpy(input + cursor, tag, sizeof(tag) - 1U);
    cursor += sizeof(tag) - 1U;
#define COPY_FIELD(field) do { \
    (void)memcpy(input + cursor, proof->field, 32U); cursor += 32U; \
} while (0)
    COPY_FIELD(deposit_id);
    COPY_FIELD(custody_reference);
    COPY_FIELD(asset_id);
    (void)memcpy(input + cursor, amount, sizeof(amount)); cursor += sizeof(amount);
    COPY_FIELD(checkpoint_id);
#undef COPY_FIELD
    store_u32(input + cursor, proof->network_id); cursor += 4U;
    input[cursor++] = (uint8_t)(proof->protocol_version >> 8U);
    input[cursor++] = (uint8_t)proof->protocol_version;
    return lxp_merkle_leaf_hash(input, cursor, leaf_hash);
}

lxp_result lx_bridge_verify_deposit(const lx_deposit_proof *proof,
                                    const lx_checkpoint_registry *checkpoints,
                                    uint32_t network_id,
                                    uint16_t protocol_version)
{
    uint8_t leaf_hash[32];
    const lx_finalized_checkpoint *checkpoint = NULL;
    size_t i;
    if (proof == NULL || checkpoints == NULL ||
        checkpoints->count > LX_CHECKPOINT_CAPACITY ||
        proof->network_id != network_id ||
        proof->protocol_version != protocol_version ||
        lx_paxeer_deposit_leaf_hash(proof, leaf_hash) != LXP_OK)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    for (i = 0U; i < checkpoints->count; ++i) {
        const lx_finalized_checkpoint *candidate = &checkpoints->checkpoints[i];
        if (lxp_ct_memcmp(candidate->checkpoint_id,
                          proof->checkpoint_id, 32U) == 0) {
            if (checkpoint != NULL) return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
            checkpoint = candidate;
        }
    }
    if (checkpoint == NULL || !checkpoint->finalized ||
        checkpoint->network_id != network_id ||
        checkpoint->protocol_version != protocol_version ||
        lxp_ct_is_zero(checkpoint->state_root, 32U) ||
        lxp_ct_is_zero(checkpoint->deposit_root, 32U) ||
        lxp_ct_memcmp(checkpoint->custody_reference,
                      proof->custody_reference, 32U) != 0 ||
        lxp_merkle_proof_verify(
            leaf_hash, &proof->inclusion_proof,
            checkpoint->deposit_root) != LXP_OK)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    return LXP_OK;
}

lxp_result lx_asset_deposit_credit(lxp_module_ctx *ctx,
                                   const lx_asset_transfer_request *request,
                                   const lx_deposit_proof *proof,
                                   const lx_checkpoint_registry *checkpoints,
                                   uint32_t network_id,
                                   uint16_t protocol_version,
                                   lxp_receipt *receipt)
{
    static const uint8_t nullifier_prefix[] = "deposit-nullifier:";
    lxp_transfer_set set;
    uint8_t nullifier_key[sizeof(nullifier_prefix) - 1U + 32U];
    uint8_t nullifier[32];
    const uint8_t *existing;
    size_t existing_length;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->from == NULL ||
        request->to == NULL || request->asset == NULL)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    status = lx_bridge_verify_deposit(proof, checkpoints, network_id,
                                      protocol_version);
    if (status != LXP_OK) return status;
    status = lxp_deposit_nullifier(proof, nullifier);
    if (status != LXP_OK) return status;
    (void)memcpy(nullifier_key, nullifier_prefix,
                 sizeof(nullifier_prefix) - 1U);
    (void)memcpy(nullifier_key + sizeof(nullifier_prefix) - 1U,
                 nullifier, 32U);
    status = lxp_ctx_kv_get(ctx, nullifier_key, sizeof(nullifier_key),
                            &existing, &existing_length);
    if (status == LXP_OK) return LXP_ERR_DEPOSIT_ALREADY_CREDITED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    if (request->from->kind != LX_ACCOUNT_SYSTEM_PAXEER_RESERVE ||
        request->to->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->asset->custody_kind != LX_ASSET_CUSTODY_PAXEER ||
        request->asset->custody_reference_length != 32U ||
        lxp_ct_memcmp(request->asset->custody_reference,
                      proof->custody_reference, 32U) != 0 ||
        memcmp(request->asset->asset_id, proof->asset_id, 32U) != 0 ||
        lxp_u128_cmp(request->amount, proof->amount) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->from;
    set.legs[0].to = request->to;
    (void)memcpy(set.legs[0].asset_id, proof->asset_id, 32U);
    set.legs[0].amount = proof->amount;
    set.legs[0].reason = LXP_REASON_DEPOSIT;
    set.context = request->context;
    set.context.protocol_system_capability = true;
    status = lxp_ctx_kv_put(ctx, nullifier_key, sizeof(nullifier_key),
                            nullifier, sizeof(nullifier));
    if (status != LXP_OK) return status;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) lxp_module_ctx_rollback(ctx);
    return status;
}

lxp_result lx_withdrawal_nullifier(const lx_withdrawal_request *request,
                                   uint8_t nullifier[32])
{
    return lxp_withdrawal_nullifier(request, nullifier);
}

bool lx_asset_nullifier_seen(const lx_withdrawal_store *store,
                             const uint8_t nullifier[32])
{
    size_t i;
    if (store == NULL || nullifier == NULL) return false;
    if (store->count > LX_DEPOSIT_NULLIFIER_CAPACITY) return true;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->records[i].nullifier, nullifier, 32U) == 0)
            return true;
    return false;
}

lxp_result lx_asset_withdraw_request(lxp_module_ctx *ctx,
                                     const lx_asset_transfer_request *transfer,
                                     const lx_withdrawal_request *withdrawal,
                                     lx_withdrawal_store *store,
                                     lxp_receipt *receipt)
{
    lxp_transfer_set set;
    uint8_t nullifier[32];
    lxp_result status;
    if (ctx == NULL || transfer == NULL || withdrawal == NULL || store == NULL ||
        transfer->from == NULL || transfer->to == NULL ||
        transfer->asset == NULL ||
        store->count > LX_DEPOSIT_NULLIFIER_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lx_withdrawal_nullifier(withdrawal, nullifier);
    if (status != LXP_OK) return status;
    if (lx_asset_nullifier_seen(store, nullifier))
        return LXP_ERR_WITHDRAWAL_ALREADY_SETTLED;
    if (store->count == LX_DEPOSIT_NULLIFIER_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (transfer->from->kind != LX_ACCOUNT_AGENT_MAIN ||
        transfer->to->kind != LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS ||
        memcmp(withdrawal->account_id, transfer->from->id, 32U) != 0 ||
        memcmp(withdrawal->asset_id, transfer->asset->asset_id, 32U) != 0 ||
        lxp_u128_cmp(withdrawal->amount, transfer->amount) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = transfer->from;
    set.legs[0].to = transfer->to;
    (void)memcpy(set.legs[0].asset_id, withdrawal->asset_id, 32U);
    set.legs[0].amount = withdrawal->amount;
    set.legs[0].reason = LXP_REASON_WITHDRAWAL;
    set.context = transfer->context;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    (void)memcpy(store->records[store->count].nullifier, nullifier, 32U);
    store->records[store->count].request = *withdrawal;
    store->records[store->count].settled = false;
    ++store->count;
    return LXP_OK;
}

lxp_result lx_asset_withdraw_settle(lxp_module_ctx *ctx,
                                    lx_account *withdrawals,
                                    lx_account *reserve,
                                    const lx_asset_record *asset,
                                    const lx_finalized_checkpoint *checkpoint,
                                    const uint8_t nullifier[32],
                                    lx_withdrawal_store *store,
                                    lxp_transfer_context context,
                                    lxp_receipt *receipt)
{
    lxp_transfer_set set;
    size_t i;
    lxp_result status;
    if (ctx == NULL || withdrawals == NULL || reserve == NULL || asset == NULL ||
        checkpoint == NULL || nullifier == NULL || store == NULL ||
        !checkpoint->finalized) return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    if (store->count > LX_DEPOSIT_NULLIFIER_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->records[i].nullifier, nullifier, 32U) == 0) break;
    if (i == store->count || store->records[i].settled)
        return LXP_ERR_WITHDRAWAL_ALREADY_SETTLED;
    if (memcmp(asset->asset_id, store->records[i].request.asset_id, 32U) != 0 ||
        !withdrawals->has_asset ||
        memcmp(withdrawals->asset_id,
               store->records[i].request.asset_id, 32U) != 0 ||
        (reserve->has_asset &&
         memcmp(reserve->asset_id,
                store->records[i].request.asset_id, 32U) != 0))
        return LXP_ERR_WITHDRAWAL_ASSET_MISMATCH;
    if (memcmp(store->records[i].request.checkpoint_id,
               checkpoint->checkpoint_id, 32U) != 0 ||
        withdrawals->kind != LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS ||
        reserve->kind != LX_ACCOUNT_SYSTEM_PAXEER_RESERVE)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = withdrawals;
    set.legs[0].to = reserve;
    (void)memcpy(set.legs[0].asset_id,
                 store->records[i].request.asset_id, 32U);
    set.legs[0].amount = store->records[i].request.amount;
    set.legs[0].reason = LXP_REASON_WITHDRAWAL;
    set.context = context;
    set.context.protocol_system_capability = true;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status == LXP_OK) store->records[i].settled = true;
    return status;
}
