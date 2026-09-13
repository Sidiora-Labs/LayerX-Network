#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_service_deliverable_check(
    lxp_module_ctx *ctx, const lx_service_agreement *agreement,
    const lx_service_delivery *delivery)
{
    lx_service_offer offer;
    size_t i;
    lxp_result status;
    if (ctx == NULL || agreement == NULL || delivery == NULL ||
        delivery->deliverable_count == 0U ||
        delivery->deliverable_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    status = lx_service_offer_lookup(ctx, agreement->offer_id, &offer);
    if (status != LXP_OK) return LXP_ERR_AGREEMENT_STATE;
    for (i = 0U; i < delivery->deliverable_count; ++i) {
        const lx_service_deliverable *item = &delivery->deliverables[i];
        if (memcmp(item->hash, offer.deliverable_specification_hash,
                   32U) != 0)
            return LXP_ERR_DELIVERABLE_MISMATCH;
        if (item->artifact_size == 0U ||
            lxp_ct_is_zero(item->availability_reference, 32U))
            return LXP_ERR_DA_MISSING;
    }
    return LXP_OK;
}

static int deliverable_compare(const lx_service_deliverable *left,
                               const lx_service_deliverable *right)
{
    int comparison = memcmp(left->hash, right->hash, 32U);
    if (comparison == 0)
        comparison = memcmp(left->availability_reference,
                            right->availability_reference, 32U);
    if (comparison == 0 && left->artifact_size != right->artifact_size)
        comparison = left->artifact_size < right->artifact_size ? -1 : 1;
    return comparison;
}

static void deliverables_sort(lx_service_delivery *delivery)
{
    size_t i;
    for (i = 1U; i < delivery->deliverable_count; ++i) {
        lx_service_deliverable current = delivery->deliverables[i];
        size_t position = i;
        while (position != 0U &&
               deliverable_compare(&current,
                                   &delivery->deliverables[position - 1U]) <
                   0) {
            delivery->deliverables[position] =
                delivery->deliverables[position - 1U];
            --position;
        }
        delivery->deliverables[position] = current;
    }
}

lxp_result lx_service_deliver_execute(
    lxp_module_ctx *ctx, const lx_service_delivery_request *request,
    lx_service_delivery *result)
{
    lx_service_agreement agreement;
    lx_service_delivery delivery;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL ||
        lxp_ct_is_zero(request->delivery.delivery_id, 32U) ||
        lxp_ct_is_zero(request->delivery.activity_id, 32U) ||
        lxp_ct_is_zero(request->delivery.provider, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(ctx, request->delivery.agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        (agreement.state != LX_SERVICE_AGREEMENT_COMMITTED &&
         agreement.state != LX_SERVICE_AGREEMENT_REJECTED) ||
        memcmp(request->delivery.provider, agreement.provider, 32U) != 0 ||
        memcmp(request->authority->principal, agreement.provider, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    if (lxp_ctx_batch_timestamp_ms(ctx) > agreement.delivery_deadline)
        return LXP_ERR_DELIVERY_DEADLINE_PASSED;
    status = lx_service_deliverable_check(ctx, &agreement,
                                          &request->delivery);
    if (status != LXP_OK) return status;
    if (lx_service_delivery_lookup(ctx, request->delivery.delivery_id,
                                   &delivery) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    delivery = request->delivery;
    deliverables_sort(&delivery);
    delivery.global_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_delivery_put(ctx, &delivery);
    if (status != LXP_OK) return status;
    if (agreement.state == LX_SERVICE_AGREEMENT_COMMITTED)
        agreement.state = LX_SERVICE_AGREEMENT_DELIVERED;
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = delivery;
    return LXP_OK;
}
