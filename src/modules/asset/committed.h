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
    size_t capacity = 0U;
    size_t count = 0U;
    lxp_result status;
    if (ctx == NULL || ctx->kernel == NULL || asset_id == NULL || record == NULL ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (ctx->kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < ctx->kernel->module_kv_count; ++i)
        if (ctx->kernel->module_kv[i].module_id == LXP_MODULE_ASSET) ++capacity;
    if (capacity == 0U) return LXP_ERR_ASSET_MISMATCH;
    if (capacity > LX_ASSET_REGISTRY_CAPACITY) capacity = LX_ASSET_REGISTRY_CAPACITY;
    status = lxp_ctx_arena_alloc(ctx, capacity * sizeof(*records),
        _Alignof(lx_asset_record), &memory);
    if (status != LXP_OK) return status;
    records = memory;
    status = lx_asset_committed_records(ctx->kernel, records, capacity, &count);
    if (status != LXP_OK) return status;
    for (size_t i = 0U; i < count; ++i) {
        if (memcmp(records[i].asset_id, asset_id, 32U) != 0) continue;
        *record = &records[i];
        return LXP_OK;
    }
    return LXP_ERR_ASSET_MISMATCH;
}

#endif
