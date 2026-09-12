#ifndef LXP_DAEMON_MODULES_H
#define LXP_DAEMON_MODULES_H

#include "layerx/lx_asset.h"
#include "layerx/lx_budget.h"
#include "layerx/lx_escrow.h"
#include "layerx/lx_stream.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>
#include <string.h>

/* Host runtimes of the genesis-gated modules. layerxd and the guarantor bind
 * them beside the ASSET and PROGRAMS runtimes, over the same process account
 * registry and asset registry, and only for the modules the genesis manifest
 * registered in the kernel: a module the manifest left disabled keeps no
 * runtime, and a module it enabled never runs without one. The escrow
 * runtime resolves accounts and assets through the process registries, the
 * budget runtime owns the process budget store, the stream runtime sees the
 * transfer state of every process asset; service and perps declare no host
 * runtime, so their kernel registration is their complete binding. */
typedef struct lxp_daemon_module_runtimes {
    lx_escrow_runtime escrow;
    lx_budget_store budget_store;
    lx_budget_runtime budget;
    lx_stream_runtime stream;
    bool enabled[LXP_MODULE_RESERVED_COUNT + 1U];
} lxp_daemon_module_runtimes;

static inline lxp_result lxp_daemon_module_enabled(const lxp_kernel *kernel,
                                                   uint16_t module_id,
                                                   bool *enabled)
{
    const lxp_module_registration *registration;
    lxp_result status;
    if (kernel == NULL || enabled == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_kernel_module_by_id(kernel, module_id, kernel->epoch,
                                     &registration);
    if (status == LXP_OK) {
        *enabled = true;
        return LXP_OK;
    }
    if (status == LXP_ERR_MODULE_DISABLED) {
        *enabled = false;
        return LXP_OK;
    }
    return status;
}

static inline lxp_result lxp_daemon_module_runtimes_bind(
    lxp_kernel *kernel, lxp_daemon_module_runtimes *runtimes,
    lx_account_registry *accounts, lx_asset_registry *assets,
    const lxp_transfer_asset_state *transfer_assets,
    size_t transfer_asset_count)
{
    static const uint16_t gated[] = {
        LXP_MODULE_ESCROW, LXP_MODULE_BUDGET, LXP_MODULE_STREAM,
        LXP_MODULE_SERVICE, LXP_MODULE_PERPS
    };
    size_t i;
    if (kernel == NULL || runtimes == NULL || accounts == NULL ||
        assets == NULL || transfer_assets == NULL ||
        accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        assets->count == 0U || assets->count > LX_ASSET_REGISTRY_CAPACITY ||
        transfer_asset_count == 0U ||
        transfer_asset_count > LX_ASSET_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(runtimes, 0, sizeof(*runtimes));
    runtimes->escrow.accounts = accounts;
    runtimes->escrow.assets = assets;
    runtimes->budget.store = &runtimes->budget_store;
    runtimes->stream.assets = transfer_assets;
    runtimes->stream.asset_count = transfer_asset_count;
    for (i = 0U; i < sizeof(gated) / sizeof(gated[0]); ++i) {
        bool enabled = false;
        lxp_result status = lxp_daemon_module_enabled(kernel, gated[i],
                                                      &enabled);
        if (status != LXP_OK) return status;
        if (!enabled) continue;
        switch (gated[i]) {
        case LXP_MODULE_ESCROW:
            status = lxp_kernel_bind_module_runtime(kernel, LXP_MODULE_ESCROW,
                                                    &runtimes->escrow);
            break;
        case LXP_MODULE_BUDGET:
            status = lxp_kernel_bind_module_runtime(kernel, LXP_MODULE_BUDGET,
                                                    &runtimes->budget);
            break;
        case LXP_MODULE_STREAM:
            status = lxp_kernel_bind_module_runtime(kernel, LXP_MODULE_STREAM,
                                                    &runtimes->stream);
            break;
        default:
            /* LXP_MODULE_SERVICE and LXP_MODULE_PERPS define no host
             * runtime type; their hooks and dispatch read none. */
            break;
        }
        if (status != LXP_OK) return status;
        runtimes->enabled[gated[i]] = true;
    }
    return LXP_OK;
}

#endif
