#ifndef LAYERX_LXP_BATCH_IDENTITY_H
#define LAYERX_LXP_BATCH_IDENTITY_H

#include "layerx/lxp_result.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_BATCH_IDENTITY_ACTIVITY_PREIMAGE_SIZE = 80,
    LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE = 88
};

lxp_result lxp_batch_identity_activity_preimage(
    const uint8_t previous_state_root[32], const uint8_t activity_id[32],
    uint64_t global_sequence, uint64_t batch_number,
    uint8_t preimage[LXP_BATCH_IDENTITY_ACTIVITY_PREIMAGE_SIZE]);

lxp_result lxp_batch_identity_committed_preimage(
    const uint8_t previous_state_root[32],
    const uint8_t activity_merkle_root[32], uint64_t first_sequence,
    uint64_t committed_last_sequence, uint64_t batch_number,
    uint8_t preimage[LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE]);

lxp_result lxp_batch_identity_activity(
    const uint8_t previous_state_root[32], const uint8_t activity_id[32],
    uint64_t global_sequence, uint64_t batch_number, uint8_t batch_id[32]);

lxp_result lxp_batch_identity_committed(
    const uint8_t previous_state_root[32],
    const uint8_t activity_merkle_root[32], uint64_t first_sequence,
    uint64_t committed_last_sequence, uint64_t batch_number,
    uint8_t batch_id[32]);

lxp_result lxp_batch_identity_committed_last_sequence(
    uint64_t first_sequence, uint64_t last_sequence,
    bool maintenance_published, uint64_t *committed_last_sequence);

#endif
