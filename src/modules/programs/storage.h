#ifndef LAYERX_PROGRAMS_STORAGE_H
#define LAYERX_PROGRAMS_STORAGE_H

#include "layerx/lxp_module.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LX_PROGRAMS_STORAGE_MAX_KEY_BYTES = 256,
    LX_PROGRAMS_STORAGE_MAX_VALUE_BYTES = 1048576
};

typedef struct lxp_programs_storage_cell {
    const uint8_t *key;
    uint16_t key_length;
    const uint8_t *value;
    uint32_t value_length;
} lxp_programs_storage_cell;

typedef lxp_result (*lxp_programs_storage_import_fn)(
    void *user, const uint8_t *key, uint16_t key_length,
    const uint8_t *value, uint32_t value_length);

/* Imports one exact program/principal or program-shared namespace in canonical
 * key order from the active module journal. */
lxp_result lxp_programs_storage_import(
    lxp_module_ctx *ctx, const uint8_t *namespace_bytes,
    uint16_t namespace_length, lxp_programs_storage_import_fn import_cell,
    void *user);

/* Replaces exactly one namespace with an already sorted final snapshot. Blob
 * and manifest writes remain staged in ctx and therefore commit or roll back
 * with the enclosing activity. Other namespace keys are never addressed. */
lxp_result lxp_programs_storage_stage_final(
    lxp_module_ctx *ctx, const uint8_t *namespace_bytes,
    uint16_t namespace_length, const lxp_programs_storage_cell *cells,
    uint32_t cell_count);

/* C-internal indexed view used by scalar FFI callbacks. Returned bytes remain
 * owned by the active module context and never cross the scalar boundary. */
lxp_result lxp_programs_storage_cell_at(
    lxp_module_ctx *ctx, const uint8_t *namespace_bytes,
    uint16_t namespace_length, uint32_t index, const uint8_t **key,
    uint16_t *key_length, const uint8_t **value, uint32_t *value_length,
    uint32_t *cell_count);

#endif
