#include "layerx/lx_service.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static lxp_result commitment_lookup(lx_service_store *store,
                                    const uint8_t commitment_id[32],
                                    lx_service_commitment **commitment)
{
    size_t i;
    if (lx_service_store_validate(store) != LXP_OK ||
        commitment_id == NULL || commitment == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->commitment_count; ++i)
        if (memcmp(store->commitments[i].commitment_id,
                   commitment_id, 32U) == 0) {
            *commitment = &store->commitments[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_service_commitment_put(lx_service_store *store,
                                     const lx_service_commitment *commitment)
{
    lx_service_commitment *existing;
    if (store == NULL || commitment == NULL ||
        lxp_ct_is_zero(commitment->commitment_id, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (commitment_lookup(store, commitment->commitment_id, &existing) ==
        LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (store->commitment_count == LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    store->commitments[store->commitment_count++] = *commitment;
    return LXP_OK;
}

static lxp_result request_check(const lx_service_commit_request *request)
{
    const lx_service_commitment *commitment;
    if (request == NULL || request->store == NULL ||
        request->authority == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    commitment = &request->commitment;
    if (lxp_ct_is_zero(commitment->commitment_id, 32U) ||
        lxp_ct_is_zero(commitment->activity_id, 32U) ||
        lxp_ct_is_zero(commitment->provider, 32U) ||
        lxp_ct_is_zero(commitment->agreement_id, 32U) ||
        lxp_ct_is_zero(commitment->task_hash, 32U) ||
        commitment->deadline == 0U || commitment->resource_bound == 0U)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_commit_task_execute(
    lxp_module_ctx *ctx, const lx_service_commit_request *request,
    lx_service_commitment *result)
{
    lx_service_commitment *existing;
    lx_service_commitment commitment;
    lx_service_agreement *agreement;
    lxp_result status;
    if (ctx == NULL || result == NULL) return LXP_ERR_NON_CANONICAL;
    status = request_check(request);
    if (status != LXP_OK) return status;
    if (commitment_lookup(request->store,
                          request->commitment.commitment_id,
                          &existing) == LXP_OK) {
        *result = *existing;
        return LXP_OK;
    }
    status = lx_service_agreement_lookup(request->store,
                                          request->commitment.agreement_id,
                                          &agreement);
    if (status != LXP_OK || agreement->state != LX_SERVICE_AGREEMENT_FORMED ||
        memcmp(request->commitment.provider,
               agreement->provider, 32U) != 0 ||
        memcmp(request->authority->principal,
               agreement->provider, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    commitment = request->commitment;
    commitment.global_sequence = lxp_ctx_global_sequence(ctx);
    commitment.abandoned = false;
    commitment.abandon_reason = 0U;
    status = lx_service_commitment_put(request->store, &commitment);
    if (status != LXP_OK) return status;
    agreement->state = LX_SERVICE_AGREEMENT_COMMITTED;
    *result = commitment;
    return LXP_OK;
}

lxp_result lx_service_commit_abandon_execute(
    lxp_module_ctx *ctx, const lx_service_commit_request *request,
    lx_service_commitment *result)
{
    lx_service_commitment *commitment;
    lx_service_agreement *agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->authority == NULL || result == NULL ||
        request->attempts_balance_mutation)
        return request != NULL && request->attempts_balance_mutation ?
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE : LXP_ERR_NON_CANONICAL;
    status = commitment_lookup(request->store,
                               request->commitment.commitment_id,
                               &commitment);
    if (status != LXP_OK) return LXP_ERR_AGREEMENT_STATE;
    if (commitment->abandoned) {
        *result = *commitment;
        return LXP_OK;
    }
    if (memcmp(request->authority->principal,
               commitment->provider, 32U) != 0 ||
        request->abandon_reason == 0U)
        return LXP_ERR_AGREEMENT_STATE;
    status = lx_service_agreement_lookup(request->store,
                                          commitment->agreement_id,
                                          &agreement);
    if (status != LXP_OK || agreement->state !=
        LX_SERVICE_AGREEMENT_COMMITTED) return LXP_ERR_AGREEMENT_STATE;
    commitment->abandoned = true;
    commitment->abandon_reason = request->abandon_reason;
    agreement->state = LX_SERVICE_AGREEMENT_FORMED;
    *result = *commitment;
    return LXP_OK;
}
