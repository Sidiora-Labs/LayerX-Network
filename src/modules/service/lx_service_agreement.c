#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_service_agreement_propose_execute(
    lxp_module_ctx *ctx, const lx_service_agreement_request *request,
    lx_service_agreement *result)
{
    lx_service_offer offer;
    lx_service_agreement existing;
    lx_service_agreement agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL || lxp_ct_is_zero(request->agreement_id, 32U) ||
        lxp_ct_is_zero(request->offer_id, 32U) ||
        lxp_ct_is_zero(request->buyer, 32U) ||
        lxp_ct_is_zero(request->terms_hash, 32U) ||
        lxp_ct_is_zero(request->escrow_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    if (memcmp(request->authority->principal, request->buyer, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_service_offer_lookup(ctx, request->offer_id, &offer);
    if (status != LXP_OK || offer.withdrawn || offer.accepted ||
        lxp_ctx_batch_timestamp_ms(ctx) > offer.offer_expiry)
        return LXP_ERR_OFFER_UNAVAILABLE;
    if (memcmp(request->buyer, offer.offering_agent, 32U) == 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (memcmp(request->terms_hash, offer.terms_hash, 32U) != 0)
        return LXP_ERR_TERMS_MISMATCH;
    if (lx_service_agreement_lookup(ctx, request->agreement_id, &existing) ==
        LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memset(&agreement, 0, sizeof(agreement));
    (void)memcpy(agreement.agreement_id, request->agreement_id, 32U);
    (void)memcpy(agreement.offer_id, offer.offer_id, 32U);
    (void)memcpy(agreement.provider, offer.offering_agent, 32U);
    (void)memcpy(agreement.buyer, request->buyer, 32U);
    (void)memcpy(agreement.terms_hash, request->terms_hash, 32U);
    (void)memcpy(agreement.escrow_id, request->escrow_id, 32U);
    agreement.delivery_deadline = offer.delivery_deadline;
    agreement.acceptance_window_end = offer.delivery_deadline +
                                      offer.acceptance_window;
    if (agreement.acceptance_window_end < offer.delivery_deadline)
        return LXP_ERR_OVERFLOW;
    agreement.dispute_window_end = agreement.acceptance_window_end +
                                   offer.dispute_window;
    if (agreement.dispute_window_end < agreement.acceptance_window_end)
        return LXP_ERR_OVERFLOW;
    agreement.default_outcome = offer.default_outcome;
    agreement.state = LX_SERVICE_AGREEMENT_PROPOSED;
    agreement.accepted_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = agreement;
    return LXP_OK;
}

lxp_result lx_service_agreement_accept_execute(
    lxp_module_ctx *ctx, const lx_service_agreement_request *request,
    lx_service_agreement *result)
{
    lx_service_offer offer;
    lx_service_agreement agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL || lxp_ct_is_zero(request->agreement_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(ctx, request->agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_PROPOSED)
        return LXP_ERR_AGREEMENT_STATE;
    if (memcmp(request->authority->principal, agreement.provider, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_service_offer_lookup(ctx, agreement.offer_id, &offer);
    if (status != LXP_OK || offer.withdrawn || offer.accepted ||
        lxp_ctx_batch_timestamp_ms(ctx) > offer.offer_expiry)
        return LXP_ERR_OFFER_UNAVAILABLE;
    if (memcmp(agreement.terms_hash, offer.terms_hash, 32U) != 0)
        return LXP_ERR_TERMS_MISMATCH;
    agreement.state = LX_SERVICE_AGREEMENT_FORMED;
    agreement.accepted_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    offer.accepted = true;
    status = lx_service_offer_put(ctx, &offer);
    if (status != LXP_OK) return status;
    *result = agreement;
    return LXP_OK;
}
