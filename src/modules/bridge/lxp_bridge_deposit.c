#include "layerx/lxp_bridge.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

lxp_result lxp_deposit_nullifier(const lx_deposit_proof *proof,
                                 uint8_t nullifier[32])
{
    static const uint8_t tag[] = "LX:DEPOSIT:NULLIFIER:v1";
    uint8_t input[sizeof(tag) - 1U + 96U];
    size_t cursor = 0U;
    if (proof == NULL || nullifier == NULL ||
        lxp_ct_is_zero(proof->deposit_id, 32U) ||
        lxp_ct_is_zero(proof->custody_reference, 32U) ||
        lxp_ct_is_zero(proof->asset_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(input + cursor, tag, sizeof(tag) - 1U);
    cursor += sizeof(tag) - 1U;
    (void)memcpy(input + cursor, proof->deposit_id, 32U);
    cursor += 32U;
    (void)memcpy(input + cursor, proof->custody_reference, 32U);
    cursor += 32U;
    (void)memcpy(input + cursor, proof->asset_id, 32U);
    cursor += 32U;
    return lxp_hash_sha256(input, cursor, nullifier);
}

lxp_result lxp_deposit_proof_verify(const lx_deposit_proof *proof,
                                    const lx_checkpoint_registry *checkpoints,
                                    uint32_t network_id,
                                    uint16_t protocol_version)
{
    if (proof == NULL || checkpoints == NULL ||
        lxp_ct_is_zero(proof->asset_id, 32U) ||
        lxp_ct_is_zero(proof->checkpoint_id, 32U))
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    return lx_bridge_verify_deposit(proof, checkpoints, network_id,
                                    protocol_version);
}

lxp_result lxp_bridge_deposit_credit(
    const lxp_bridge_deposit_context *bridge,
    const lx_asset_transfer_request *transfer,
    const lx_deposit_proof *proof,
    lxp_receipt *receipt)
{
    lxp_u128 total_before;
    lxp_u128 total_after;
    lxp_result status;
    if (bridge == NULL || bridge->module_ctx == NULL ||
        bridge->assets == NULL || bridge->accounts == NULL ||
        bridge->checkpoints == NULL || bridge->nullifiers == NULL ||
        transfer == NULL || transfer->asset == NULL || proof == NULL ||
        receipt == NULL)
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    status = lxp_deposit_proof_verify(proof, bridge->checkpoints,
                                      bridge->network_id,
                                      bridge->protocol_version);
    if (status != LXP_OK) return status;
    status = lx_asset_total_units(bridge->assets, bridge->accounts,
                                  proof->asset_id, &total_before);
    if (status != LXP_OK) return LXP_FATAL_SUPPLY_MISMATCH;
    status = lx_asset_deposit_credit(
        bridge->module_ctx, transfer, proof, bridge->checkpoints,
        bridge->nullifiers, bridge->network_id, bridge->protocol_version,
        receipt);
    if (status != LXP_OK) return status;
    status = lx_asset_total_units(bridge->assets, bridge->accounts,
                                  proof->asset_id, &total_after);
    if (status != LXP_OK || lxp_u128_cmp(total_before, total_after) != 0)
        return LXP_FATAL_SUPPLY_MISMATCH;
    return LXP_OK;
}
