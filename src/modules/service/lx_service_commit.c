#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static lxp_result request_check(const lx_service_commit_request *request)
{
    const lx_service_commitment *commitment;
    if (request == NULL || request->authority == NULL)
        return LXP_ERR_NON_CANONICAL;
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
    lx_service_commitment commitment;
    lx_service_agreement agreement;
    lxp_result status;
    if (ctx == NULL || result == NULL) return LXP_ERR_NON_CANONICAL;
    status = request_check(request);
    if (status != LXP_OK) return status;
    if (lx_service_commitment_lookup(ctx, request->commitment.commitment_id,
                                     &commitment) == LXP_OK) {
        *result = commitment;
        return LXP_OK;
    }
    status = lx_service_agreement_lookup(ctx,
                                         request->commitment.agreement_id,
                                         &agreement);
    if (status != LXP_OK || agreement.state != LX_SERVICE_AGREEMENT_FORMED ||
        memcmp(request->commitment.provider, agreement.provider, 32U) != 0 ||
        memcmp(request->authority->principal, agreement.provider, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    commitment = request->commitment;
    commitment.global_sequence = lxp_ctx_global_sequence(ctx);
    commitment.abandoned = false;
    commitment.abandon_reason = 0U;
    status = lx_service_commitment_put(ctx, &commitment);
    if (status != LXP_OK) return status;
    agreement.state = LX_SERVICE_AGREEMENT_COMMITTED;
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = commitment;
    return LXP_OK;
}

lxp_result lx_service_commit_abandon_execute(
    lxp_module_ctx *ctx, const lx_service_commit_request *request,
    lx_service_commitment *result)
{
    lx_service_commitment commitment;
    lx_service_agreement agreement;
    lxp_result status;
    if (request != NULL && request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_service_commitment_lookup(ctx,
                                          request->commitment.commitment_id,
                                          &commitment);
    if (status != LXP_OK) return LXP_ERR_AGREEMENT_STATE;
    if (commitment.abandoned) {
        *result = commitment;
        return LXP_OK;
    }
    if (memcmp(request->authority->principal, commitment.provider, 32U) != 0 ||
        request->abandon_reason == 0U)
        return LXP_ERR_AGREEMENT_STATE;
    status = lx_service_agreement_lookup(ctx, commitment.agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_COMMITTED)
        return LXP_ERR_AGREEMENT_STATE;
    commitment.abandoned = true;
    commitment.abandon_reason = request->abandon_reason;
    status = lx_service_commitment_put(ctx, &commitment);
    if (status != LXP_OK) return status;
    agreement.state = LX_SERVICE_AGREEMENT_FORMED;
    status = lx_service_agreement_put(ctx, &agreement);
    if (status != LXP_OK) return status;
    *result = commitment;
    return LXP_OK;
}

lxp_result lx_service_progress_report_execute(
    lxp_module_ctx *ctx, const lx_service_progress_request *request,
    lx_service_progress *result)
{
    lx_service_commitment commitment;
    lx_service_agreement agreement;
    lx_service_progress progress;
    uint32_t high_water = 0U;
    lxp_result status;
    if (request != NULL && request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    if (ctx == NULL || request == NULL || request->authority == NULL ||
        result == NULL ||
        lxp_ct_is_zero(request->progress.report_id, 32U) ||
        lxp_ct_is_zero(request->progress.activity_id, 32U) ||
        lxp_ct_is_zero(request->progress.commitment_id, 32U) ||
        lxp_ct_is_zero(request->progress.provider, 32U) ||
        lxp_ct_is_zero(request->progress.note_hash, 32U) ||
        lxp_ct_is_zero(request->progress.availability_reference, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (request->progress.progress_bps == 0U ||
        request->progress.progress_bps > LX_SERVICE_PROGRESS_COMPLETE_BPS)
        return LXP_ERR_PARAMETER_BOUNDS;
    if (lx_service_progress_lookup(ctx, request->progress.report_id,
                                   &progress) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    status = lx_service_commitment_lookup(ctx,
                                          request->progress.commitment_id,
                                          &commitment);
    if (status != LXP_OK || commitment.abandoned ||
        memcmp(request->progress.provider, commitment.provider, 32U) != 0 ||
        memcmp(request->authority->principal, commitment.provider, 32U) != 0)
        return LXP_ERR_AGREEMENT_STATE;
    status = lx_service_agreement_lookup(ctx, commitment.agreement_id,
                                         &agreement);
    if (status != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_COMMITTED)
        return LXP_ERR_AGREEMENT_STATE;
    if (lxp_ctx_batch_timestamp_ms(ctx) > commitment.deadline)
        return LXP_ERR_DELIVERY_DEADLINE_PASSED;
    status = lx_service_progress_high_water(ctx, commitment.commitment_id,
                                            &high_water);
    if (status != LXP_OK) return status;
    if (request->progress.progress_bps <= high_water)
        return LXP_ERR_METER_REGRESSION;
    progress = request->progress;
    (void)memcpy(progress.agreement_id, commitment.agreement_id, 32U);
    progress.reported_at = lxp_ctx_batch_timestamp_ms(ctx);
    progress.global_sequence = lxp_ctx_global_sequence(ctx);
    status = lx_service_progress_put(ctx, &progress);
    if (status != LXP_OK) return status;
    *result = progress;
    return LXP_OK;
}
