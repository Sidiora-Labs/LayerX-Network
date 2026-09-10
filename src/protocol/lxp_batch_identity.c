#include "layerx/lxp_batch_identity.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

static void batch_identity_write_u64(uint8_t *out, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        out[index] = (uint8_t)(value >> (56U - 8U * index));
}

lxp_result lxp_batch_identity_activity_preimage(
    const uint8_t previous_state_root[32], const uint8_t activity_id[32],
    uint64_t global_sequence, uint64_t batch_number,
    uint8_t preimage[LXP_BATCH_IDENTITY_ACTIVITY_PREIMAGE_SIZE])
{
    if (previous_state_root == NULL || activity_id == NULL || preimage == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(preimage, previous_state_root, 32U);
    (void)memcpy(preimage + 32U, activity_id, 32U);
    batch_identity_write_u64(preimage + 64U, global_sequence);
    batch_identity_write_u64(preimage + 72U, batch_number);
    return LXP_OK;
}

lxp_result lxp_batch_identity_committed_preimage(
    const uint8_t previous_state_root[32],
    const uint8_t activity_merkle_root[32], uint64_t first_sequence,
    uint64_t committed_last_sequence, uint64_t batch_number,
    uint8_t preimage[LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE])
{
    if (previous_state_root == NULL || activity_merkle_root == NULL ||
        preimage == NULL || first_sequence == 0U ||
        committed_last_sequence < first_sequence)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(preimage, previous_state_root, 32U);
    (void)memcpy(preimage + 32U, activity_merkle_root, 32U);
    batch_identity_write_u64(preimage + 64U, first_sequence);
    batch_identity_write_u64(preimage + 72U, committed_last_sequence);
    batch_identity_write_u64(preimage + 80U, batch_number);
    return LXP_OK;
}

lxp_result lxp_batch_identity_activity(
    const uint8_t previous_state_root[32], const uint8_t activity_id[32],
    uint64_t global_sequence, uint64_t batch_number, uint8_t batch_id[32])
{
    uint8_t preimage[LXP_BATCH_IDENTITY_ACTIVITY_PREIMAGE_SIZE];
    lxp_result status;
    if (batch_id == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_identity_activity_preimage(
        previous_state_root, activity_id, global_sequence, batch_number,
        preimage);
    if (status == LXP_OK)
        status = lxp_hash_context_value(preimage, sizeof(preimage), batch_id);
    lxp_secure_zero(preimage, sizeof(preimage));
    return status;
}

lxp_result lxp_batch_identity_committed(
    const uint8_t previous_state_root[32],
    const uint8_t activity_merkle_root[32], uint64_t first_sequence,
    uint64_t committed_last_sequence, uint64_t batch_number,
    uint8_t batch_id[32])
{
    uint8_t preimage[LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE];
    lxp_result status;
    if (batch_id == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_identity_committed_preimage(
        previous_state_root, activity_merkle_root, first_sequence,
        committed_last_sequence, batch_number, preimage);
    if (status == LXP_OK)
        status = lxp_hash_context_value(preimage, sizeof(preimage), batch_id);
    lxp_secure_zero(preimage, sizeof(preimage));
    return status;
}

lxp_result lxp_batch_identity_committed_last_sequence(
    uint64_t first_sequence, uint64_t last_sequence,
    bool maintenance_published, uint64_t *committed_last_sequence)
{
    if (committed_last_sequence == NULL || first_sequence == 0U ||
        last_sequence < first_sequence ||
        (maintenance_published && last_sequence == first_sequence))
        return LXP_ERR_NON_CANONICAL;
    *committed_last_sequence =
        maintenance_published ? last_sequence - 1U : last_sequence;
    return LXP_OK;
}
