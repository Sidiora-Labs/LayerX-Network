#include "layerx/lxp_verify.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_merkle.h"

#include <string.h>

lxp_result lxp_receipt_verify_offline(
    const lxp_receipt *receipt,
    const uint8_t sequencer_public_key[32],
    lxp_arena *arena)
{
    if (receipt == NULL || sequencer_public_key == NULL || arena == NULL ||
        !lxp_protocol_version_supported(receipt->protocol_version) ||
        receipt->result_code != LXP_OK || receipt->operation == 0U ||
        lxp_ct_is_zero(receipt->activity_id, 32U) ||
        lxp_ct_is_zero(receipt->sequencer_signature, 64U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_receipt_verify(receipt, sequencer_public_key, arena);
}

lxp_result lxp_receipt_match_requirement(
    const lxp_receipt *receipt,
    const lxp_payment_requirement *requirement,
    uint32_t receipt_network_id)
{
    uint8_t receive_context_preimage[64];
    uint8_t expected_context[32];
    lxp_result status;
    if (receipt == NULL || requirement == NULL || receipt_network_id == 0U ||
        receipt_network_id != requirement->network_id)
        return LXP_ERR_WRONG_NETWORK;
    if (lxp_ct_memcmp(receipt->asset, requirement->asset, 32U) != 0 ||
        lxp_ct_memcmp(receipt->to, requirement->recipient, 32U) != 0 ||
        lxp_u128_cmp(receipt->amount, requirement->amount) != 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    if (receipt->operation == (uint8_t)LX_ASSET_SEND)
        (void)memcpy(expected_context, requirement->purpose_hash, 32U);
    else if (receipt->operation == (uint8_t)LX_ASSET_RECEIVE) {
        (void)memcpy(receive_context_preimage,
                     requirement->purpose_hash, 32U);
        (void)memcpy(receive_context_preimage + 32U,
                     requirement->invoice_id, 32U);
        status = lxp_hash_context_value(
            receive_context_preimage, sizeof(receive_context_preimage),
            expected_context);
        if (status != LXP_OK) return status;
    } else {
        return LXP_ERR_UNKNOWN_ACTIVITY;
    }
    return lxp_ct_memcmp(
        receipt->context_hash, expected_context, 32U) == 0 ?
        LXP_OK : LXP_ERR_CONTEXT_MISMATCH;
}

static lxp_result receipt_bytes_match(
    const lxp_receipt *receipt, lxp_byte_span expected, lxp_arena *arena)
{
    size_t mark = lxp_arena_mark(arena);
    lxp_byte_span encoded;
    lxp_result status = lxp_receipt_encode(receipt, true, arena, &encoded);
    if (status == LXP_OK &&
        (encoded.length != expected.length ||
         lxp_ct_memcmp(encoded.bytes, expected.bytes, encoded.length) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_receipt_verify_checkpointed(
    const lxp_receipt *receipt,
    const lxp_augmented_receipt *augmented,
    const lxp_guarantor_key_record *guarantor_keys,
    size_t guarantor_key_count,
    const uint8_t registered_checkpoint_id[32],
    lxp_byte_span registered_paxeer_reference,
    lxp_arena *arena)
{
    const lxp_guarantor_cert *certificate;
    uint8_t activity_id[32];
    uint8_t activity_leaf[32];
    uint8_t state_leaf[32];
    uint8_t checkpoint_id[32];
    size_t valid_signatures = 0U;
    lxp_result status;
    if (receipt == NULL || augmented == NULL ||
        augmented->guarantor_certificate == NULL ||
        guarantor_keys == NULL || guarantor_key_count == 0U ||
        registered_checkpoint_id == NULL ||
        registered_paxeer_reference.bytes == NULL ||
        registered_paxeer_reference.length == 0U || arena == NULL ||
        augmented->pre_checkpoint_receipt.bytes == NULL ||
        augmented->canonical_activity.bytes == NULL ||
        augmented->state_leaf.bytes == NULL ||
        augmented->paxeer_settlement_reference.bytes == NULL)
        return LXP_ERR_NON_CANONICAL;
    certificate = augmented->guarantor_certificate;
    status = receipt_bytes_match(
        receipt, augmented->pre_checkpoint_receipt, arena);
    if (status == LXP_OK)
        status = lxp_hash_activity_id(
            augmented->canonical_activity.bytes,
            augmented->canonical_activity.length, activity_id);
    if (status == LXP_OK &&
        lxp_ct_memcmp(activity_id, receipt->activity_id, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(
            augmented->canonical_activity.bytes,
            augmented->canonical_activity.length, activity_leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            activity_leaf, &augmented->activity_inclusion_proof,
            certificate->checkpoint.header.activity_merkle_root);
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(
            augmented->state_leaf.bytes,
            augmented->state_leaf.length, state_leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            state_leaf, &augmented->state_inclusion_proof,
            receipt->resulting_state_root);
    if (status == LXP_OK)
        status = lxp_guarantor_cert_verify(
            certificate, guarantor_keys, guarantor_key_count,
            arena, &valid_signatures);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(
            &certificate->checkpoint, arena, checkpoint_id);
    if (status == LXP_OK &&
        (valid_signatures < certificate->threshold ||
         lxp_ct_memcmp(checkpoint_id, augmented->checkpoint_id, 32U) != 0 ||
         lxp_ct_memcmp(checkpoint_id, registered_checkpoint_id, 32U) != 0 ||
         certificate->checkpoint.header.network_id == 0U))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK &&
        (augmented->paxeer_settlement_reference.length !=
             registered_paxeer_reference.length ||
         lxp_ct_memcmp(
             augmented->paxeer_settlement_reference.bytes,
             registered_paxeer_reference.bytes,
             registered_paxeer_reference.length) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    return status;
}
