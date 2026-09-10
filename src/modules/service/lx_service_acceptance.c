#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result outcome_agreement(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request,
    lx_service_agreement *agreement)
{
    lxp_result status;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        agreement == NULL || lxp_ct_is_zero(request->agreement_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_agreement_lookup(ctx, request->agreement_id,
                                         agreement);
    if (status != LXP_OK ||
        agreement->state != LX_SERVICE_AGREEMENT_DELIVERED ||
        memcmp(request->authority->principal, agreement->buyer, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    if (lxp_ctx_batch_timestamp_ms(ctx) > agreement->acceptance_window_end)
        return LXP_ERR_AGREEMENT_STATE;
    return LXP_OK;
}

lxp_result lx_service_accept_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request,
    lx_service_agreement *result)
{
    lx_service_agreement agreement;
    lxp_result status;
    if (result == NULL) return LXP_ERR_NON_CANONICAL;
    status = outcome_agreement(ctx, request, &agreement);
    if (status != LXP_OK) return status;
    agreement.state = LX_SERVICE_AGREEMENT_ACCEPTED;
    agreement.outcome_sequence = lxp_ctx_global_sequence(ctx);
    agreement.outcome_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = agreement;
    return LXP_OK;
}

static bool delivered_hash(const lx_service_delivery *delivery,
                           const uint8_t hash[32])
{
    size_t i;
    for (i = 0U; i < delivery->deliverable_count; ++i)
        if (memcmp(delivery->deliverables[i].hash, hash, 32U) == 0)
            return true;
    return false;
}

lxp_result lx_service_reject_execute(
    lxp_module_ctx *ctx, const lx_service_outcome_request *request,
    lx_service_agreement *result)
{
    lx_service_agreement agreement;
    lx_service_delivery delivery;
    size_t i;
    lxp_result status;
    if (result == NULL) return LXP_ERR_NON_CANONICAL;
    status = outcome_agreement(ctx, request, &agreement);
    if (status != LXP_OK) return status;
    if (request->rejection_reason == 0U ||
        request->contested_hash_count == 0U ||
        request->contested_hash_count > LX_SERVICE_MAX_DELIVERABLES)
        return LXP_ERR_NON_CANONICAL;
    status = lx_service_delivery_latest(ctx, agreement.agreement_id,
                                        &delivery);
    if (status != LXP_OK) return LXP_ERR_DELIVERABLE_MISMATCH;
    for (i = 0U; i < request->contested_hash_count; ++i)
        if (!delivered_hash(&delivery, request->contested_hashes[i]))
            return LXP_ERR_DELIVERABLE_MISMATCH;
    agreement.rejection_reason = request->rejection_reason;
    agreement.contested_hash_count = request->contested_hash_count;
    (void)memcpy(agreement.contested_hashes, request->contested_hashes,
                 request->contested_hash_count * 32U);
    lx_service_hashes_sort(agreement.contested_hashes,
                           agreement.contested_hash_count);
    agreement.state = LX_SERVICE_AGREEMENT_REJECTED;
    agreement.outcome_sequence = lxp_ctx_global_sequence(ctx);
    agreement.outcome_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = agreement;
    return LXP_OK;
}

typedef struct default_scan {
    uint64_t batch_timestamp;
    uint8_t agreement_id[32];
    bool found;
} default_scan;

static lxp_result visit_agreement(const uint8_t *key, size_t key_length,
                                  const uint8_t *value, size_t value_length,
                                  void *user)
{
    default_scan *scan = (default_scan *)user;
    lx_service_agreement agreement;
    lxp_result status;
    if (key == NULL || key_length != LX_SERVICE_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    if (scan->found) return LXP_OK;
    status = lx_service_agreement_decode(value, value_length, &agreement);
    if (status != LXP_OK) return status;
    if (agreement.state != LX_SERVICE_AGREEMENT_DELIVERED ||
        scan->batch_timestamp < agreement.acceptance_window_end)
        return LXP_OK;
    (void)memcpy(scan->agreement_id, agreement.agreement_id, 32U);
    scan->found = true;
    return LXP_OK;
}

lxp_result lx_service_acceptance_default(lxp_module_ctx *ctx,
                                         uint64_t batch_timestamp,
                                         uint64_t global_sequence)
{
    size_t guard;
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    for (guard = 0U; guard < (size_t)LXP_MODULE_MAX_STAGED_WRITES; ++guard) {
        default_scan scan;
        lx_service_agreement agreement;
        lxp_result status;
        (void)memset(&scan, 0, sizeof(scan));
        scan.batch_timestamp = batch_timestamp;
        status = lxp_ctx_kv_iter(ctx, lx_service_agreement_prefix,
                                 LX_SERVICE_KEY_PREFIX_BYTES, visit_agreement,
                                 &scan);
        if (status != LXP_OK) return status;
        if (!scan.found) return LXP_OK;
        status = lx_service_agreement_lookup(ctx, scan.agreement_id,
                                             &agreement);
        if (status != LXP_OK) return status;
        agreement.state = agreement.default_outcome ==
            LX_SERVICE_DEFAULT_ACCEPT ? LX_SERVICE_AGREEMENT_ACCEPTED :
                                        LX_SERVICE_AGREEMENT_REJECTED;
        agreement.default_applied = true;
        agreement.outcome_sequence = global_sequence;
        agreement.outcome_timestamp = batch_timestamp;
        status = lx_service_agreement_put(ctx, &agreement);
        if (status != LXP_OK) return status;
    }
    return LXP_ERR_ARENA_EXHAUSTED;
}

lxp_result lx_service_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                  uint64_t timestamp)
{
    if (ctx == NULL || epoch != lxp_ctx_epoch(ctx) ||
        timestamp != lxp_ctx_batch_timestamp_ms(ctx))
        return LXP_ERR_TIMESTAMP_REGRESSION;
    return lx_service_acceptance_default(ctx, timestamp,
                                         lxp_ctx_global_sequence(ctx));
}
