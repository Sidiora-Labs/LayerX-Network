#ifndef LAYERX_ASSET_COMMITTED_H
#define LAYERX_ASSET_COMMITTED_H

#include "layerx/lx_asset.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static inline lxp_result lxp_module_committed_asset(lxp_module_ctx *ctx,
    const uint8_t asset_id[32], const lx_asset_record **record)
{
    lx_asset_record *records;
    void *memory;
    size_t count = 0U;
    lxp_result status;
    if (ctx == NULL || ctx->kernel == NULL || asset_id == NULL || record == NULL ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_ctx_arena_alloc(ctx, LX_ASSET_REGISTRY_CAPACITY * sizeof(*records),
        _Alignof(lx_asset_record), &memory);
    if (status != LXP_OK) return status;
    records = memory;
    status = lx_asset_committed_records(ctx->kernel, records, LX_ASSET_REGISTRY_CAPACITY, &count);
    if (status != LXP_OK) return status;
    for (size_t i = 0U; i < count; ++i) {
        if (memcmp(records[i].asset_id, asset_id, 32U) != 0) continue;
        *record = &records[i];
        return LXP_OK;
    }
    return LXP_ERR_ASSET_MISMATCH;
}

#endif
