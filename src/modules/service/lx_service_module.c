#include "lx_service_dispatch.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_SERVICE_OFFER_PUBLISH, LX_SERVICE_OFFER_WITHDRAW,
    LX_SERVICE_AGREEMENT_PROPOSE, LX_SERVICE_AGREEMENT_ACCEPT,
    LX_SERVICE_COMMIT_TASK, LX_SERVICE_COMMIT_ABANDON,
    LX_SERVICE_TOOL_EXEC_ATTEST, LX_SERVICE_PROGRESS_REPORT,
    LX_SERVICE_DELIVER, LX_SERVICE_ACCEPT, LX_SERVICE_REJECT,
    LX_SERVICE_DISPUTE_OPEN, LX_SERVICE_DISPUTE_RESOLVE
};

enum { LX_SERVICE_ORDINAL_COUNT = 13 };

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t length,
                                void **decoded)
{
    lx_service_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U ||
        ordinal > (uint16_t)LX_SERVICE_ORDINAL_COUNT || payload == NULL ||
        length == 0U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(lx_service_decoded), &memory);
    if (status != LXP_OK) return status;
    value = (lx_service_decoded *)memory;
    status = lx_service_payload_decode(ordinal, payload, length, value);
    if (status != LXP_OK) return status;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const lx_service_decoded *value = (const lx_service_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_activity_module_id(activity->activity_type) != LXP_MODULE_SERVICE ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_ct_is_zero(authority->principal, 32U))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result execute_offer_publish(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_offer_publish_payload *payload)
{
    lx_service_offer_request request;
    lx_service_offer offer;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.offer.offer_id, payload->offer_id, 32U);
    (void)memcpy(request.offer.activity_id, lxp_ctx_activity_id(ctx), 32U);
    (void)memcpy(request.offer.offering_agent, authority->principal, 32U);
    (void)memcpy(request.offer.asset_id, payload->asset_id, 32U);
    request.offer.price = payload->price;
    (void)memcpy(request.offer.terms_hash, payload->terms_hash, 32U);
    (void)memcpy(request.offer.deliverable_specification_hash,
                 payload->deliverable_specification_hash, 32U);
    request.offer.delivery_deadline = payload->delivery_deadline;
    request.offer.acceptance_window = payload->acceptance_window;
    request.offer.dispute_window = payload->dispute_window;
    request.offer.default_outcome = payload->default_outcome;
    request.offer.offer_expiry = payload->offer_expiry;
    status = lx_service_offer_publish_execute(ctx, &request, &offer);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_OFFER_PUBLISHED,
                           offer.offer_id, offer.offering_agent,
                           (uint8_t)offer.default_outcome,
                           offer.global_sequence);
}

static lxp_result execute_offer_withdraw(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_identifier_payload *payload)
{
    lx_service_offer_request request;
    lx_service_offer offer;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.offer.offer_id, payload->identifier, 32U);
    status = lx_service_offer_withdraw_execute(ctx, &request, &offer);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_OFFER_WITHDRAWN,
                           offer.offer_id, offer.offering_agent, 1U,
                           lxp_ctx_global_sequence(ctx));
}

static lxp_result execute_agreement_propose(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_agreement_propose_payload *payload)
{
    lx_service_agreement_request request;
    lx_service_agreement agreement;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.agreement_id, payload->agreement_id, 32U);
    (void)memcpy(request.offer_id, payload->offer_id, 32U);
    (void)memcpy(request.buyer, authority->principal, 32U);
    (void)memcpy(request.terms_hash, payload->terms_hash, 32U);
    (void)memcpy(request.escrow_id, payload->escrow_id, 32U);
    status = lx_service_agreement_propose_execute(ctx, &request, &agreement);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_AGREEMENT_PROPOSED,
                           agreement.agreement_id, agreement.provider,
                           (uint8_t)agreement.state,
                           agreement.accepted_sequence);
}

static lxp_result execute_agreement_accept(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_identifier_payload *payload)
{
    lx_service_agreement_request request;
    lx_service_agreement agreement;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.agreement_id, payload->identifier, 32U);
    status = lx_service_agreement_accept_execute(ctx, &request, &agreement);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_AGREEMENT_ACCEPTED,
                           agreement.agreement_id, agreement.buyer,
                           (uint8_t)agreement.state,
                           agreement.accepted_sequence);
}

static lxp_result execute_commit_task(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_commit_task_payload *payload)
{
    lx_service_commit_request request;
    lx_service_commitment commitment;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.commitment.commitment_id, payload->commitment_id,
                 32U);
    (void)memcpy(request.commitment.activity_id, lxp_ctx_activity_id(ctx),
                 32U);
    (void)memcpy(request.commitment.provider, authority->principal, 32U);
    (void)memcpy(request.commitment.agreement_id, payload->agreement_id, 32U);
    (void)memcpy(request.commitment.task_hash, payload->task_hash, 32U);
    (void)memcpy(request.commitment.escrow_id, payload->escrow_id, 32U);
    request.commitment.deadline = payload->deadline;
    request.commitment.resource_bound = payload->resource_bound;
    status = lx_service_commit_task_execute(ctx, &request, &commitment);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_TASK_COMMITTED,
                           commitment.commitment_id, commitment.agreement_id,
                           commitment.abandoned ? 1U : 0U,
                           commitment.global_sequence);
}

static lxp_result execute_commit_abandon(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_commit_abandon_payload *payload)
{
    lx_service_commit_request request;
    lx_service_commitment commitment;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    request.abandon_reason = payload->abandon_reason;
    (void)memcpy(request.commitment.commitment_id, payload->commitment_id,
                 32U);
    status = lx_service_commit_abandon_execute(ctx, &request, &commitment);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_COMMIT_ABANDONED,
                           commitment.commitment_id, commitment.agreement_id,
                           1U, lxp_ctx_global_sequence(ctx));
}

static lxp_result execute_tool_exec_attest(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_execution *payload)
{
    lx_service_attest_request request;
    lx_service_attestor_grant grant;
    lx_service_execution execution;
    lxp_result status;
    if (memcmp(payload->activity_id, lxp_ctx_activity_id(ctx), 32U) != 0)
        return LXP_ERR_INVALID_ATTESTATION;
    (void)memset(&grant, 0, sizeof(grant));
    (void)memcpy(grant.principal, authority->principal, 32U);
    (void)memcpy(grant.public_key, authority->verified_key, 32U);
    grant.module_id = LXP_MODULE_SERVICE;
    grant.activity_type = LX_SERVICE_TOOL_EXEC_ATTEST;
    (void)memset(&request, 0, sizeof(request));
    request.execution = *payload;
    request.grant = &grant;
    status = lx_service_tool_exec_attest_execute(ctx, &request, &execution);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_EXECUTION_ATTESTED,
                           execution.attestation_id, execution.commitment_id,
                           0U, execution.global_sequence);
}

static lxp_result execute_progress_report(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_progress_payload *payload)
{
    lx_service_progress_request request;
    lx_service_progress progress;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.progress.report_id, payload->report_id, 32U);
    (void)memcpy(request.progress.activity_id, lxp_ctx_activity_id(ctx), 32U);
    (void)memcpy(request.progress.commitment_id, payload->commitment_id, 32U);
    (void)memcpy(request.progress.provider, authority->principal, 32U);
    (void)memcpy(request.progress.note_hash, payload->note_hash, 32U);
    (void)memcpy(request.progress.availability_reference,
                 payload->availability_reference, 32U);
    request.progress.progress_bps = payload->progress_bps;
    status = lx_service_progress_report_execute(ctx, &request, &progress);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_PROGRESS_REPORTED,
                           progress.report_id, progress.commitment_id,
                           progress.progress_bps ==
                               LX_SERVICE_PROGRESS_COMPLETE_BPS ? 1U : 0U,
                           progress.global_sequence);
}

static lxp_result execute_deliver(lxp_module_ctx *ctx,
                                  const lxp_authority_resolved *authority,
                                  const lx_service_deliver_payload *payload)
{
    lx_service_delivery_request request;
    lx_service_delivery delivery;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.delivery.delivery_id, payload->delivery_id, 32U);
    (void)memcpy(request.delivery.activity_id, lxp_ctx_activity_id(ctx), 32U);
    (void)memcpy(request.delivery.agreement_id, payload->agreement_id, 32U);
    (void)memcpy(request.delivery.provider, authority->principal, 32U);
    (void)memcpy(request.delivery.deliverables, payload->deliverables,
                 payload->deliverable_count *
                     sizeof(payload->deliverables[0]));
    request.delivery.deliverable_count = payload->deliverable_count;
    status = lx_service_deliver_execute(ctx, &request, &delivery);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_DELIVERED,
                           delivery.delivery_id, delivery.agreement_id,
                           (uint8_t)delivery.deliverable_count,
                           delivery.global_sequence);
}

static lxp_result execute_accept(lxp_module_ctx *ctx,
                                 const lxp_authority_resolved *authority,
                                 const lx_service_identifier_payload *payload)
{
    lx_service_outcome_request request;
    lx_service_agreement agreement;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.agreement_id, payload->identifier, 32U);
    status = lx_service_accept_execute(ctx, &request, &agreement);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_OUTCOME_ACCEPTED,
                           agreement.agreement_id, agreement.provider,
                           (uint8_t)agreement.state,
                           agreement.outcome_sequence);
}

static lxp_result execute_reject(lxp_module_ctx *ctx,
                                 const lxp_authority_resolved *authority,
                                 const lx_service_reject_payload *payload)
{
    lx_service_outcome_request request;
    lx_service_agreement agreement;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.agreement_id, payload->agreement_id, 32U);
    request.rejection_reason = payload->rejection_reason;
    (void)memcpy(request.contested_hashes, payload->contested_hashes,
                 payload->contested_hash_count * 32U);
    request.contested_hash_count = payload->contested_hash_count;
    status = lx_service_reject_execute(ctx, &request, &agreement);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_OUTCOME_REJECTED,
                           agreement.agreement_id, agreement.provider,
                           (uint8_t)agreement.contested_hash_count,
                           agreement.outcome_sequence);
}

static lxp_result execute_dispute_open(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_dispute_open_payload *payload)
{
    lx_service_dispute_request request;
    lx_service_dispute dispute;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.dispute.dispute_id, payload->dispute_id, 32U);
    (void)memcpy(request.dispute.activity_id, lxp_ctx_activity_id(ctx), 32U);
    (void)memcpy(request.dispute.agreement_id, payload->agreement_id, 32U);
    (void)memcpy(request.dispute.raiser, authority->principal, 32U);
    (void)memcpy(request.dispute.evidence_hashes, payload->evidence_hashes,
                 payload->evidence_hash_count * 32U);
    request.dispute.evidence_hash_count = payload->evidence_hash_count;
    status = lx_service_dispute_open_execute(ctx, &request, &dispute);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_DISPUTE_OPENED,
                           dispute.dispute_id, dispute.agreement_id,
                           (uint8_t)dispute.evidence_hash_count,
                           dispute.global_sequence);
}

static lxp_result execute_dispute_resolve(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_service_dispute_resolve_payload *payload)
{
    lx_service_dispute_request request;
    lx_service_dispute dispute;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    request.authority = authority;
    (void)memcpy(request.dispute.dispute_id, payload->dispute_id, 32U);
    request.dispute.ruling = payload->ruling;
    request.dispute.provider_basis_points = payload->provider_basis_points;
    (void)memcpy(request.dispute.escrow_resolution_id,
                 payload->escrow_resolution_id, 32U);
    status = lx_service_dispute_resolve_execute(ctx, &request, &dispute);
    if (status != LXP_OK) return status;
    return lx_service_emit(ctx, (uint16_t)LX_SERVICE_EVENT_DISPUTE_RESOLVED,
                           dispute.dispute_id, dispute.agreement_id, 1U,
                           dispute.resolution_sequence);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const lx_service_decoded *value = (const lx_service_decoded *)decoded;
    const lx_service_payload *payload;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (effects == NULL || ctx->effects != effects)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
    if (status != LXP_OK) return status;
    payload = &value->payload;
    switch (value->ordinal) {
    case 1U:
        status = execute_offer_publish(ctx, authority,
                                       &payload->offer_publish);
        break;
    case 2U:
        status = execute_offer_withdraw(ctx, authority, &payload->identifier);
        break;
    case 3U:
        status = execute_agreement_propose(ctx, authority,
                                           &payload->agreement_propose);
        break;
    case 4U:
        status = execute_agreement_accept(ctx, authority,
                                          &payload->identifier);
        break;
    case 5U:
        status = execute_commit_task(ctx, authority, &payload->commit_task);
        break;
    case 6U:
        status = execute_commit_abandon(ctx, authority,
                                        &payload->commit_abandon);
        break;
    case 7U:
        status = execute_tool_exec_attest(ctx, authority,
                                          &payload->execution);
        break;
    case 8U:
        status = execute_progress_report(ctx, authority, &payload->progress);
        break;
    case 9U:
        status = execute_deliver(ctx, authority, &payload->deliver);
        break;
    case 10U:
        status = execute_accept(ctx, authority, &payload->identifier);
        break;
    case 11U:
        status = execute_reject(ctx, authority, &payload->reject);
        break;
    case 12U:
        status = execute_dispute_open(ctx, authority,
                                      &payload->dispute_open);
        break;
    case 13U:
        status = execute_dispute_resolve(ctx, authority,
                                         &payload->dispute_resolve);
        break;
    default:
        status = LXP_ERR_UNKNOWN_ACTIVITY;
        break;
    }
    if (status != LXP_OK) return status;
    return lx_service_effect_audit(activity->activity_type, effects);
}

static lxp_result module_epoch_end(lxp_module_ctx *ctx, uint64_t epoch,
                                   uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_SERVICE, root);
}

const lxp_module_iface *lx_service_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_SERVICE, 1U, "service", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        lx_service_epoch_begin, module_epoch_end, module_state_root, NULL
    };
    return &iface;
}
