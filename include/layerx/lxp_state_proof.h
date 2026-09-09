#ifndef LAYERX_LXP_STATE_PROOF_H
#define LAYERX_LXP_STATE_PROOF_H

#include "layerx/lxp_kernel.h"

enum {
    LXP_STATE_WITNESS_VERSION = 2,
    LXP_STATE_WITNESS_MAX_KEY = LXP_MODULE_MAX_KEY_BYTES + 1,
    LXP_STATE_WITNESS_MAX_VALUE = LXP_KERNEL_MAX_BLOB_BYTES,
    LXP_STATE_WITNESS_MAX_BYTES = 35 + LXP_STATE_WITNESS_MAX_KEY +
        LXP_STATE_WITNESS_MAX_VALUE + 96 * LXP_STATE_PROOF_MAX_DEPTH
};

typedef struct lxp_state_witness {
    uint16_t version;
    uint16_t module_id;
    uint32_t key_length;
    uint32_t value_length;
    uint8_t key[LXP_STATE_WITNESS_MAX_KEY];
    uint8_t value[LXP_STATE_WITNESS_MAX_VALUE];
    lxp_state_proof account_path;
    lxp_state_proof layer_a;
    lxp_state_proof layer_b;
} lxp_state_witness;

lxp_result lxp_state_proof_build(const lxp_kernel *state, uint16_t module_id,
                                 lxp_byte_span key, lxp_state_witness *proof);
lxp_result lxp_state_proof_verify(const lxp_state_witness *proof,
                                  const uint8_t state_root[32]);
lxp_result lxp_state_proof_encode(const lxp_state_witness *proof,
                                  uint8_t *bytes, size_t capacity, size_t *length);
lxp_result lxp_state_proof_decode(const uint8_t *bytes, size_t length,
                                  lxp_state_witness *proof);

#endif
