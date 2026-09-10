#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static bool agreement_party(const lx_service_agreement *agreement,
                            const uint8_t identity[32])
{
    return memcmp(agreement->provider, identity, 32U) == 0 ||
           memcmp(agreement->buyer, identity, 32U) == 0;
}

lxp_result lx_service_dispute_open_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result)
{
    lx_service_agreement agreement;
    lx_service_dispute dispute;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL ||
        lxp_ct_is_zero(request->dispute.dispute_id, 32U) ||
        lxp_ct_is_zero(request->dispute.activity_id, 32U) ||
        request->dispute.evidence_hash_count == 0U ||
        request->dispute.evidence_hash_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(ctx, request->dispute.agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_REJECTED)
        return LXP_ERR_AGREEMENT_STATE;
    if (!agreement_party(&agreement, request->authority->principal) ||
        memcmp(request->dispute.raiser, request->authority->principal,
               32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DISPUTANT;
    if (lxp_ctx_batch_timestamp_ms(ctx) > agreement.dispute_window_end)
        return LXP_ERR_DISPUTE_WINDOW_CLOSED;
    if (lx_service_dispute_lookup(ctx, request->dispute.dispute_id,
                                  &dispute) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    status = lx_service_hashes_check(request->dispute.evidence_hashes,
                                     request->dispute.evidence_hash_count);
    if (status != LXP_OK) return status;
    dispute = request->dispute;
    lx_service_hashes_sort(dispute.evidence_hashes,
                           dispute.evidence_hash_count);
    dispute.global_sequence = lxp_ctx_global_sequence(ctx);
    dispute.resolved = false;
    dispute.ruling = 0U;
    dispute.provider_basis_points = 0U;
    (void)memset(dispute.escrow_resolution_id, 0, 32U);
    dispute.resolution_sequence = 0U;
    status = lx_service_dispute_put(ctx, &dispute);
    if (status != LXP_OK) return status;
    agreement.state = LX_SERVICE_AGREEMENT_DISPUTED;
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = dispute;
    return LXP_OK;
}

lxp_result lx_service_dispute_resolve_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result)
{
    lx_service_dispute dispute;
    lx_service_agreement agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_dispute_lookup(ctx, request->dispute.dispute_id,
                                       &dispute);
    if (status != LXP_OK || dispute.resolved ||
        request->dispute.ruling == 0U ||
        request->dispute.provider_basis_points >
            LX_SERVICE_PROGRESS_COMPLETE_BPS ||
        lxp_ct_is_zero(request->dispute.escrow_resolution_id, 32U))
        return LXP_ERR_AGREEMENT_STATE;
    status = lx_service_agreement_lookup(ctx, dispute.agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_DISPUTED ||
        !agreement_party(&agreement, request->authority->principal))
        return LXP_ERR_UNAUTHORIZED_DISPUTANT;
    dispute.resolved = true;
    dispute.ruling = request->dispute.ruling;
    dispute.provider_basis_points = request->dispute.provider_basis_points;
    (void)memcpy(dispute.escrow_resolution_id,
                 request->dispute.escrow_resolution_id, 32U);
    dispute.resolution_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_dispute_put(ctx, &dispute);
    if (status != LXP_OK) return status;
    agreement.state = LX_SERVICE_AGREEMENT_RESOLVED;
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = dispute;
    return LXP_OK;
}

lxp_result lx_service_effect_audit(uint32_t activity_type,
                                   const lxp_effect_buffer *effects)
{
    const lxp_module_iface *iface = lx_service_module_iface();
    size_t i;
    bool declared = false;
    if (effects == NULL || effects->count > LXP_MAX_EFFECTS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < iface->activity_type_count; ++i)
        if (iface->activity_types[i] == activity_type) {
            declared = true;
            break;
        }
    if (!declared) return LXP_ERR_UNKNOWN_ACTIVITY;
    for (i = 0U; i < effects->count; ++i)
        if (effects->effects[i].kind == LXP_EFFECT_TRANSFER ||
            effects->effects[i].monetary)
            return LXP_FATAL_INVARIANT;
    return LXP_OK;
}
