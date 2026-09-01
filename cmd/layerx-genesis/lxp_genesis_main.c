#include "layerx/lxp_genesis.h"

lxp_result lxp_genesis_main(
    const uint8_t *manifest_bytes, size_t manifest_length,
    const lxp_genesis_bootstrap_registration *registration,
    bool storage_empty, lxp_arena *arena, bool *activities_enabled)
{
    lxp_genesis_manifest manifest;
    lxp_result status = lxp_genesis_parse(
        manifest_bytes, manifest_length, LXP_GENESIS_INPUT_MANIFEST,
        &manifest);
    if (status == LXP_OK)
        status = lxp_genesis_accept(
            &manifest, registration, storage_empty,
            arena, activities_enabled);
    return status;
}
