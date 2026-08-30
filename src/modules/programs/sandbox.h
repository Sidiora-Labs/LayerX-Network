#ifndef LAYERX_PROGRAMS_SANDBOX_H
#define LAYERX_PROGRAMS_SANDBOX_H

#include "layerx/programs.h"

lxp_result lxp_programs_sandbox_destroy_decode(lxp_module_ctx *ctx,
    const uint8_t *payload, size_t length, void **decoded);
lxp_result lxp_programs_sandbox_destroy_validate(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const void *decoded);
lxp_result lxp_programs_sandbox_destroy_execute(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const void *decoded, lxp_effect_buffer *effects);
int32_t layerx_programs_sandbox_destroy_host(uint64_t token,
    const uint8_t lease_id[32], const uint8_t expected_root[32],
    const uint8_t activity_id[32], uint64_t expected_sequence,
    uint64_t boundary);
int32_t layerx_programs_sandbox_destroy_terminal(uint64_t token,
    uint8_t *output, uint32_t capacity);
lxp_result layerx_programs_sandbox_destroy_state_length(uint64_t token,uint16_t kind);
lxp_result layerx_programs_sandbox_destroy_state_byte(uint64_t token,uint16_t kind,uint32_t offset);
lxp_result layerx_programs_sandbox_destroy_archive(uint64_t token,uint16_t kind,
    const uint8_t *bytes,uint32_t length);
lxp_result layerx_programs_sandbox_destroy_charge(uint64_t token,
    const uint8_t from[32],const uint8_t to[32],const uint8_t asset[32],
    uint64_t amount_hi,uint64_t amount_lo,uint8_t root[32]);
lxp_result layerx_programs_sandbox_destroy_refund(uint64_t token,
    const uint8_t from[32],const uint8_t to[32],const uint8_t asset[32],
    uint64_t amount_hi,uint64_t amount_lo,uint8_t root[32]);
int32_t layerx_programs_sandbox_sweep_host(uint64_t token,uint64_t boundary,uint32_t limit);
lxp_result lxp_programs_sandbox_finalize_expiry_batch(lxp_module_ctx *ctx,uint64_t batch_number);

#endif
