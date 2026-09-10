#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result offer_request_check(const lx_service_offer_request *request)
{
    const lx_service_offer *offer;
    if (request == NULL || request->authority == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    offer = &request->offer;
    if (lxp_ct_is_zero(offer->offer_id, 32U) ||
        lxp_ct_is_zero(offer->activity_id, 32U) ||
        lxp_ct_is_zero(offer->offering_agent, 32U) ||
        lxp_ct_is_zero(offer->asset_id, 32U) ||
        lxp_u128_is_zero(offer->price) ||
        lxp_ct_is_zero(offer->terms_hash, 32U) ||
        lxp_ct_is_zero(offer->deliverable_specification_hash, 32U) ||
        offer->delivery_deadline == 0U || offer->acceptance_window == 0U ||
        offer->dispute_window == 0U || offer->offer_expiry == 0U ||
        offer->default_outcome < LX_SERVICE_DEFAULT_ACCEPT ||
        offer->default_outcome > LX_SERVICE_DEFAULT_REJECT ||
        memcmp(request->authority->principal, offer->offering_agent,
               32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_offer_publish_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request,
    lx_service_offer *result)
{
    lx_service_offer existing;
    lx_service_offer offer;
    lxp_result status;
    if (ctx == NULL || result == NULL) return LXP_ERR_NON_CANONICAL;
    status = offer_request_check(request);
    if (status != LXP_OK) return status;
    if (lx_service_offer_lookup(ctx, request->offer.offer_id, &existing) ==
        LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    offer = request->offer;
    offer.global_sequence = lxp_ctx_global_sequence(ctx);
    offer.withdrawn = false;
    offer.accepted = false;
    status = lx_service_offer_put(ctx, &offer);
    if (status != LXP_OK) return status;
    *result = offer;
    return LXP_OK;
}

lxp_result lx_service_offer_withdraw_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request,
    lx_service_offer *result)
{
    lx_service_offer offer;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_offer_lookup(ctx, request->offer.offer_id, &offer);
    if (status != LXP_OK || offer.withdrawn || offer.accepted ||
        lxp_ctx_batch_timestamp_ms(ctx) > offer.offer_expiry)
        return LXP_ERR_OFFER_UNAVAILABLE;
    if (memcmp(request->authority->principal, offer.offering_agent, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    offer.withdrawn = true;
    status = lx_service_offer_put(ctx, &offer);
    if (status != LXP_OK) return status;
    *result = offer;
    return LXP_OK;
}
