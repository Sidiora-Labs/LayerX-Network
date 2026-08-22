#ifndef LAYERX_PROGRAMS_H
#define LAYERX_PROGRAMS_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <stdint.h>

enum {
    LX_PROGRAMS_DEPLOY = 0x00090001,
    LX_PROGRAMS_UPGRADE = 0x00090002,
    LX_PROGRAMS_CALL = 0x00090003,
    LX_PROGRAMS_REGISTRY = 0x00090004,
    LX_PROGRAMS_TRANSFER = 0x00090005,
    LX_PROGRAMS_ABI_VERSION = 1,
    LX_PROGRAMS_EVENT_DEPLOYED = 1,
    LX_PROGRAMS_EVENT_UPGRADED = 2,
    LX_PROGRAMS_EVENT_CALLED = 3,
    LX_PROGRAMS_EVENT_REGISTRY_READ = 4,
    LX_PROGRAMS_EVENT_TRANSFERRED = 5
};

const lxp_module_iface *programs_module_registration(void);
const lxp_module_iface *lx_programs_module_iface(void);

lxp_result lxp_programs_lifecycle_decode(lxp_module_ctx *ctx,
                                         uint16_t ordinal,
                                         const uint8_t *payload,
                                         size_t payload_length,
                                         void **decoded);
lxp_result lxp_programs_lifecycle_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_lifecycle_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);
void lxp_programs_lifecycle_release(lxp_module_ctx *ctx, void *decoded);

uint64_t layerx_programs_migration_begin(uint32_t wasm_length,
                                         uint16_t hook_length);
lxp_result layerx_programs_migration_wasm_byte(uint64_t handle, uint8_t byte);
lxp_result layerx_programs_migration_hook_byte(uint64_t handle, uint8_t byte);
lxp_result layerx_programs_migration_execute(uint64_t handle);
void layerx_programs_migration_abort(uint64_t handle);

typedef struct lx_programs_transfer_runtime {
    lx_account_registry *accounts;
    const lxp_transfer_asset_state *assets;
    size_t asset_count;
} lx_programs_transfer_runtime;

lxp_result lxp_programs_transfer_decode(lxp_module_ctx *ctx,
                                        const uint8_t *payload,
                                        size_t payload_length,
                                        void **decoded);
lxp_result lxp_programs_transfer_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
lxp_result lxp_programs_transfer_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);

lxp_result layerx_programs_authorize_402lxp_leg(
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t r0, uint64_t r1, uint64_t r2, uint64_t r3,
    uint64_t h0, uint64_t h1, uint64_t h2, uint64_t h3,
    uint64_t a0, uint64_t a1, uint64_t a2, uint64_t a3,
    uint64_t t0, uint64_t t1, uint64_t t2, uint64_t t3,
    uint64_t amount_hi, uint64_t amount_lo);

#endif
