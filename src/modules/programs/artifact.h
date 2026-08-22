#ifndef LAYERX_PROGRAMS_ARTIFACT_H
#define LAYERX_PROGRAMS_ARTIFACT_H

#include "layerx/lxp_module.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LX_PROGRAMS_ARTIFACT_MANIFEST_BYTES = 38
};

lxp_result lxp_programs_artifact_store(lxp_module_ctx *ctx,
                                       const uint8_t program_id[32],
                                       const uint8_t code_hash[32],
                                       const uint8_t *wasm,
                                       size_t wasm_length);

lxp_result lxp_programs_artifact_open(lxp_module_ctx *ctx,
                                      const uint8_t program_id[32],
                                      const uint8_t expected_hash[32],
                                      const uint8_t **wasm,
                                      size_t *wasm_length);

lxp_result lxp_programs_artifact_catalog_count(lxp_module_ctx *ctx,
                                               size_t *count);

lxp_result lxp_programs_artifact_catalog_open(
    lxp_module_ctx *ctx, size_t index, uint8_t program_id[32],
    uint8_t code_hash[32], const uint8_t **wasm, size_t *wasm_length);

#endif
