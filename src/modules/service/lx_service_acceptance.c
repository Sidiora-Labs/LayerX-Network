#include "layerx/lx_service.h"

#include <string.h>

static lxp_result outcome_agreement(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request,
    lx_service_agreement **agreement)
{
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->agreement_id == NULL || request->authority == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(request->store,
                                          request->agreement_id,
                                          agreement);
    if (status != LXP_OK ||
        (*agreement)->state != LX_SERVICE_AGREEMENT_DELIVERED ||
        memcmp(request->authority->principal,
               (*agreement)->buyer, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    if (lxp_ctx_batch_timestamp_ms(ctx) >
        (*agreement)->acceptance_window_end)
        return LXP_ERR_AGREEMENT_STATE;
    return LXP_OK;
}

lxp_result lx_service_accept_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request)
{
    lx_service_agreement *agreement;
    lxp_result status = outcome_agreement(ctx, request, &agreement);
    if (status != LXP_OK) return status;
    agreement->state = LX_SERVICE_AGREEMENT_ACCEPTED;
    agreement->outcome_sequence = lxp_ctx_global_sequence(ctx);
    agreement->outcome_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    return LXP_OK;
}

static bool delivered_hash(const lx_service_store *store,
                           const uint8_t agreement_id[32],
                           const uint8_t hash[32])
{
    size_t i;
    size_t j;
    for (i = store->delivery_count; i != 0U; --i) {
        const lx_service_delivery *delivery = &store->deliveries[i - 1U];
        if (memcmp(delivery->agreement_id, agreement_id, 32U) != 0) continue;
        for (j = 0U; j < delivery->deliverable_count; ++j)
            if (memcmp(delivery->deliverables[j].hash, hash, 32U) == 0)
                return true;
        return false;
    }
    return false;
}

static void hashes_sort(uint8_t hashes[LX_SERVICE_MAX_DELIVERABLES][32],
                        size_t count)
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

lxp_result lx_service_reject_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request)
{
    lx_service_agreement *agreement;
    size_t i;
    lxp_result status = outcome_agreement(ctx, request, &agreement);
    if (status != LXP_OK) return status;
    if (request->rejection_reason == 0U ||
        request->contested_hash_count == 0U ||
        request->contested_hash_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < request->contested_hash_count; ++i)
        if (!delivered_hash(request->store, agreement->agreement_id,
                            request->contested_hashes[i]))
            return LXP_ERR_DELIVERABLE_MISMATCH;
    agreement->rejection_reason = request->rejection_reason;
    agreement->contested_hash_count = request->contested_hash_count;
    (void)memcpy(agreement->contested_hashes, request->contested_hashes,
                 request->contested_hash_count * 32U);
    hashes_sort(agreement->contested_hashes,
                agreement->contested_hash_count);
    agreement->state = LX_SERVICE_AGREEMENT_REJECTED;
    agreement->outcome_sequence = lxp_ctx_global_sequence(ctx);
    agreement->outcome_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    return LXP_OK;
}

lxp_result lx_service_acceptance_default(lx_service_store *store,
                                         uint64_t batch_timestamp,
                                         uint64_t global_sequence)
{
    size_t i;
    if (lx_service_store_validate(store) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->agreement_count; ++i) {
        lx_service_agreement *agreement = &store->agreements[i];
        if (agreement->state != LX_SERVICE_AGREEMENT_DELIVERED ||
            batch_timestamp < agreement->acceptance_window_end)
            continue;
        agreement->state = agreement->default_outcome ==
            LX_SERVICE_DEFAULT_ACCEPT ? LX_SERVICE_AGREEMENT_ACCEPTED :
                                        LX_SERVICE_AGREEMENT_REJECTED;
        agreement->default_applied = true;
        agreement->outcome_sequence = global_sequence;
        agreement->outcome_timestamp = batch_timestamp;
    }
    return LXP_OK;
}

lxp_result lx_service_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                  uint64_t timestamp)
{
    lx_service_runtime *runtime;
    if (ctx == NULL || epoch != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    runtime = (lx_service_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL) return lxp_ctx_charge_gas(ctx, 1U);
    if (runtime->store == NULL) return LXP_ERR_NON_CANONICAL;
    return lx_service_acceptance_default(runtime->store, timestamp,
                                         lxp_ctx_global_sequence(ctx));
}
