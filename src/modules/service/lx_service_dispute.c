#include "layerx/lx_service.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static lx_service_dispute *dispute_find(
    lx_service_store *store, const uint8_t dispute_id[32])
{
    size_t i;
    for (i = 0U; i < store->dispute_count; ++i)
        if (memcmp(store->disputes[i].dispute_id, dispute_id, 32U) == 0)
            return &store->disputes[i];
    return NULL;
}

static bool agreement_party(const lx_service_agreement *agreement,
                            const uint8_t identity[32])
{
    return memcmp(agreement->provider, identity, 32U) == 0 ||
           memcmp(agreement->buyer, identity, 32U) == 0;
}

static void evidence_sort(
    uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32], size_t count)
{
    size_t i;
    for (i = 1U; i < count; ++i) {
        uint8_t current[32];
        size_t position = i;
        (void)memcpy(current, hashes[i], 32U);
        while (position != 0U &&
               memcmp(current, hashes[position - 1U], 32U) < 0) {
            (void)memcpy(hashes[position], hashes[position - 1U], 32U);
            --position;
        }
        (void)memcpy(hashes[position], current, 32U);
    }
}

lxp_result lx_service_dispute_open_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result)
{
    lx_service_agreement *agreement;
    lx_service_dispute dispute;
    size_t i;
    lxp_result status;
    if (ctx == NULL || request == NULL ||
        lx_service_store_validate(request->store) != LXP_OK ||
        request->authority == NULL || result == NULL ||
        lxp_ct_is_zero(request->dispute.dispute_id, 32U) ||
        lxp_ct_is_zero(request->dispute.activity_id, 32U) ||
        request->dispute.evidence_hash_count == 0U ||
        request->dispute.evidence_hash_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(request->store,
                                          request->dispute.agreement_id,
                                          &agreement);
    if (status != LXP_OK || agreement->state !=
        LX_SERVICE_AGREEMENT_REJECTED) return LXP_ERR_AGREEMENT_STATE;
    if (!agreement_party(agreement, request->authority->principal) ||
        memcmp(request->dispute.raiser,
               request->authority->principal, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DISPUTANT;
    if (lxp_ctx_batch_timestamp_ms(ctx) > agreement->dispute_window_end)
        return LXP_ERR_DISPUTE_WINDOW_CLOSED;
    if (dispute_find(request->store, request->dispute.dispute_id) != NULL)
        return LXP_ERR_SEQUENCE_REUSED;
    if (request->store->dispute_count == LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    for (i = 0U; i < request->dispute.evidence_hash_count; ++i)
        if (lxp_ct_is_zero(request->dispute.evidence_hashes[i], 32U))
            return LXP_ERR_NON_CANONICAL;
    dispute = request->dispute;
    evidence_sort(dispute.evidence_hashes, dispute.evidence_hash_count);
    dispute.global_sequence = lxp_ctx_global_sequence(ctx);
    dispute.resolved = false;
    dispute.ruling = 0U;
    dispute.provider_basis_points = 0U;
    (void)memset(dispute.escrow_resolution_id, 0, 32U);
    dispute.resolution_sequence = 0U;
    request->store->disputes[request->store->dispute_count++] = dispute;
    agreement->state = LX_SERVICE_AGREEMENT_DISPUTED;
    *result = dispute;
    return LXP_OK;
}

lxp_result lx_service_dispute_resolve_execute(
    lxp_module_ctx *ctx, const lx_service_dispute_request *request,
    lx_service_dispute *result)
{
    lx_service_dispute *dispute;
    lx_service_agreement *agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL ||
        lx_service_store_validate(request->store) != LXP_OK ||
        request->authority == NULL || result == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    dispute = dispute_find(request->store, request->dispute.dispute_id);
    if (dispute == NULL || dispute->resolved || request->dispute.ruling == 0U ||
        request->dispute.provider_basis_points > 10000U ||
        lxp_ct_is_zero(request->dispute.escrow_resolution_id, 32U))
        return LXP_ERR_AGREEMENT_STATE;
    status = lx_service_agreement_lookup(request->store,
                                          dispute->agreement_id, &agreement);
    if (status != LXP_OK || agreement->state !=
        LX_SERVICE_AGREEMENT_DISPUTED ||
        !agreement_party(agreement, request->authority->principal))
        return LXP_ERR_UNAUTHORIZED_DISPUTANT;
    dispute->resolved = true;
    dispute->ruling = request->dispute.ruling;
    dispute->provider_basis_points =
        request->dispute.provider_basis_points;
    (void)memcpy(dispute->escrow_resolution_id,
                 request->dispute.escrow_resolution_id, 32U);
    dispute->resolution_sequence = lxp_ctx_global_sequence(ctx);
    agreement->state = LX_SERVICE_AGREEMENT_RESOLVED;
    *result = *dispute;
    return LXP_OK;
}

lxp_result lx_service_effect_audit(uint32_t activity_type,
                                   const lxp_effect_buffer *effects)
{
    const lxp_module_iface *iface = lx_service_module_iface();
    size_t i;
    bool declared = false;
    if (effects == NULL) return LXP_ERR_NON_CANONICAL;
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
