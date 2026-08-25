#include "layerx/lx_escrow.h"
#include "layerx/lx_service.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <openssl/evp.h>
#include <string.h>

static size_t transfer_calls;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK) {
        ++transfer_calls;
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    }
    return status;
}

static int sign_execution(lx_service_execution *execution,
                          lx_service_attestor_grant *grant,
                          const uint8_t seed[32])
{
    uint8_t message[384];
    uint8_t digest[32];
    size_t message_length;
    size_t public_length = 32U;
    size_t signature_length = 64U;
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    int failed = key == NULL || context == NULL ||
        EVP_PKEY_get_raw_public_key(key, execution->public_key,
                                    &public_length) != 1;
    if (!failed) {
        (void)memcpy(grant->public_key, execution->public_key, 32U);
        failed = lx_service_attestation_bytes(execution, message,
                                               sizeof(message),
                                               &message_length) != LXP_OK ||
            lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message,
                            message_length, digest) != LXP_OK ||
            EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
            EVP_DigestSign(context, execution->signature, &signature_length,
                           digest, sizeof(digest)) != 1 ||
            signature_length != 64U;
    }
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return failed;
}

static int dispute_checks(lxp_kernel *kernel, lxp_arena *arena)
{
    lx_service_store store;
    lx_service_dispute_request request;
    lx_service_dispute result;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_effect monetary;
    const lxp_module_iface *iface = lx_service_module_iface();
    size_t i;

    (void)memset(&store, 0, sizeof(store));
    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&outsider, 0, sizeof(outsider));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    outsider.principal[0] = 3U;
    store.agreement_count = 1U;
    store.agreements[0].agreement_id[0] = 4U;
    (void)memcpy(store.agreements[0].provider, provider.principal, 32U);
    (void)memcpy(store.agreements[0].buyer, buyer.principal, 32U);
    store.agreements[0].state = LX_SERVICE_AGREEMENT_REJECTED;
    store.agreements[0].dispute_window_end = 1000U;
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.authority = &outsider;
    request.dispute.dispute_id[0] = 5U;
    request.dispute.activity_id[0] = 6U;
    request.dispute.agreement_id[0] = 4U;
    request.dispute.raiser[0] = 3U;
    request.dispute.evidence_hash_count = 2U;
    request.dispute.evidence_hashes[0][0] = 8U;
    request.dispute.evidence_hashes[1][0] = 7U;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_SERVICE, 500U, 0U, 1U,
                            1000U, arena, true) != LXP_OK ||
        lx_service_dispute_open_execute(&ctx, &request, &result) !=
            LXP_ERR_UNAUTHORIZED_DISPUTANT)
        return 1;
    request.authority = &buyer;
    request.dispute.raiser[0] = 2U;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_SERVICE, 1001U, 0U, 2U,
                            1000U, arena, true) != LXP_OK ||
        lx_service_dispute_open_execute(&ctx, &request, &result) !=
            LXP_ERR_DISPUTE_WINDOW_CLOSED ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_SERVICE, 500U, 0U, 3U,
                            1000U, arena, true) != LXP_OK ||
        lx_service_dispute_open_execute(&ctx, &request, &result) != LXP_OK ||
        result.global_sequence != 3U || result.evidence_hashes[0][0] != 7U ||
        store.agreements[0].state != LX_SERVICE_AGREEMENT_DISPUTED)
        return 1;
    request.authority = &provider;
    request.dispute.ruling = 9U;
    request.dispute.provider_basis_points = 7000U;
    request.dispute.escrow_resolution_id[0] = 10U;
    if (lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_SERVICE, 600U, 0U, 4U,
                            1000U, arena, true) != LXP_OK ||
        lx_service_dispute_resolve_execute(&ctx, &request, &result) != LXP_OK ||
        !result.resolved || result.provider_basis_points != 7000U ||
        result.escrow_resolution_id[0] != 10U ||
        store.agreements[0].state != LX_SERVICE_AGREEMENT_RESOLVED)
        return 1;
    if (lxp_effect_buffer_init(&effects) != LXP_OK) return 1;
    for (i = 0U; i < iface->activity_type_count; ++i)
        if (lx_service_effect_audit(iface->activity_types[i], &effects) !=
            LXP_OK) return 1;
    (void)memset(&monetary, 0, sizeof(monetary));
    monetary.module_id = LXP_MODULE_SERVICE;
    monetary.kind = LXP_EFFECT_TRANSFER;
    monetary.monetary = true;
    effects.effects[0] = monetary;
    effects.count = 1U;
    if (lx_service_effect_audit(LX_SERVICE_DELIVER, &effects) !=
        LXP_FATAL_INVARIANT) return 1;
    effects.count = LXP_MAX_EFFECTS + 1U;
    if (lx_service_effect_audit(LX_SERVICE_DELIVER, &effects) !=
        LXP_ERR_NON_CANONICAL) return 1;
    return 0;
}

static int service_to_escrow(lxp_kernel *kernel, lxp_arena *arena)
{
    static const uint8_t seed[32] = { 11U };
    lx_service_store service;
    lx_escrow_store escrow;
    lx_service_offer_request offer_request;
    lx_service_agreement_request agreement_request;
    lx_service_commit_request commit_request;
    lx_service_commitment commitment;
    lx_service_attestor_grant grant;
    lx_service_attest_request attest_request;
    lx_service_execution execution;
    lx_service_delivery_request delivery_request;
    lx_service_delivery delivery;
    lx_service_outcome_request outcome_request;
    lx_escrow_record escrow_record;
    lx_escrow_capture_request capture_request;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lx_account escrow_account;
    lx_account provider_account;
    lx_asset_record asset;
    lxp_transfer_asset_state asset_state;
    lxp_module_ctx ctx;
    lxp_receipt receipt;

    (void)memset(&service, 0, sizeof(service));
    (void)memset(&escrow, 0, sizeof(escrow));
    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&escrow_account, 0, sizeof(escrow_account));
    (void)memset(&provider_account, 0, sizeof(provider_account));
    (void)memset(&asset, 0, sizeof(asset));
    provider.principal[0] = 21U;
    buyer.principal[0] = 22U;
    escrow_account.id[0] = 23U;
    escrow_account.kind = LX_ACCOUNT_AGENT_ESCROW;
    (void)memcpy(provider_account.id, provider.principal, 32U);
    provider_account.kind = LX_ACCOUNT_AGENT_MAIN;
    asset.asset_id[0] = 24U;
    if (lxp_ledger_bootstrap_balance(&escrow_account, asset.asset_id,
                                     (lxp_u128){ 0U, 50U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(&provider_account, asset.asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_SERVICE, 100U, 0U, 10U,
                            1000U, arena, true) != LXP_OK)
        return 1;

    (void)memset(&offer_request, 0, sizeof(offer_request));
    offer_request.store = &service;
    offer_request.authority = &provider;
    offer_request.offer.offer_id[0] = 25U;
    offer_request.offer.activity_id[0] = 26U;
    (void)memcpy(offer_request.offer.offering_agent,
                 provider.principal, 32U);
    (void)memcpy(offer_request.offer.asset_id, asset.asset_id, 32U);
    offer_request.offer.price = (lxp_u128){ 0U, 50U };
    offer_request.offer.terms_hash[0] = 27U;
    offer_request.offer.deliverable_specification_hash[0] = 28U;
    offer_request.offer.delivery_deadline = 1000U;
    offer_request.offer.acceptance_window = 200U;
    offer_request.offer.dispute_window = 300U;
    offer_request.offer.default_outcome = LX_SERVICE_DEFAULT_ACCEPT;
    offer_request.offer.offer_expiry = 500U;
    if (lx_service_offer_publish_execute(&ctx, &offer_request) != LXP_OK)
        return 1;
    (void)memset(&agreement_request, 0, sizeof(agreement_request));
    agreement_request.store = &service;
    agreement_request.offer_id = offer_request.offer.offer_id;
    agreement_request.agreement_id[0] = 29U;
    (void)memcpy(agreement_request.buyer, buyer.principal, 32U);
    (void)memcpy(agreement_request.terms_hash,
                 offer_request.offer.terms_hash, 32U);
    agreement_request.escrow_id[0] = 30U;
    agreement_request.authority = &buyer;
    if (lx_service_agreement_accept_execute(&ctx, &agreement_request) != LXP_OK)
        return 1;
    (void)memset(&commit_request, 0, sizeof(commit_request));
    commit_request.store = &service;
    commit_request.authority = &provider;
    commit_request.commitment.commitment_id[0] = 31U;
    commit_request.commitment.activity_id[0] = 32U;
    (void)memcpy(commit_request.commitment.provider,
                 provider.principal, 32U);
    (void)memcpy(commit_request.commitment.agreement_id,
                 agreement_request.agreement_id, 32U);
    commit_request.commitment.task_hash[0] = 33U;
    commit_request.commitment.deadline = 900U;
    commit_request.commitment.resource_bound = 100U;
    (void)memcpy(commit_request.commitment.escrow_id,
                 agreement_request.escrow_id, 32U);
    if (lx_service_commit_task_execute(&ctx, &commit_request,
                                       &commitment) != LXP_OK)
        return 1;

    (void)memset(&grant, 0, sizeof(grant));
    (void)memset(&attest_request, 0, sizeof(attest_request));
    grant.principal[0] = 21U;
    grant.module_id = LXP_MODULE_SERVICE;
    grant.activity_type = LX_SERVICE_TOOL_EXEC_ATTEST;
    grant.not_before = 1U; grant.not_after = 500U;
    attest_request.store = &service;
    attest_request.grant = &grant;
    attest_request.execution.attestation_id[0] = 34U;
    attest_request.execution.activity_id[0] = 35U;
    (void)memcpy(attest_request.execution.agreement_id,
                 agreement_request.agreement_id, 32U);
    (void)memcpy(attest_request.execution.commitment_id,
                 commitment.commitment_id, 32U);
    attest_request.execution.tool_id[0] = 36U;
    attest_request.execution.input_commitment_hash[0] = 37U;
    attest_request.execution.output_commitment_hash[0] = 38U;
    attest_request.execution.execution_start = 10U;
    attest_request.execution.execution_end = 90U;
    attest_request.execution.resource_units = 80U;
    (void)memcpy(attest_request.execution.attestor_identity,
                 provider.principal, 32U);
    attest_request.execution.availability_reference[0] = 39U;
    if (sign_execution(&attest_request.execution, &grant, seed) != 0 ||
        lx_service_tool_exec_attest_execute(&ctx, &attest_request,
                                            &execution) != LXP_OK)
        return 1;
    (void)memset(&delivery_request, 0, sizeof(delivery_request));
    delivery_request.store = &service;
    delivery_request.authority = &provider;
    delivery_request.delivery.delivery_id[0] = 40U;
    delivery_request.delivery.activity_id[0] = 41U;
    (void)memcpy(delivery_request.delivery.agreement_id,
                 agreement_request.agreement_id, 32U);
    (void)memcpy(delivery_request.delivery.provider,
                 provider.principal, 32U);
    delivery_request.delivery.deliverable_count = 1U;
    delivery_request.delivery.deliverables[0].hash[0] = 28U;
    delivery_request.delivery.deliverables[0].artifact_size = 1024U;
    delivery_request.delivery.deliverables[0].availability_reference[0] = 42U;
    if (lx_service_deliver_execute(&ctx, &delivery_request, &delivery) != LXP_OK)
        return 1;
    (void)memset(&outcome_request, 0, sizeof(outcome_request));
    outcome_request.store = &service;
    outcome_request.agreement_id = agreement_request.agreement_id;
    outcome_request.authority = &buyer;
    if (lx_service_accept_execute(&ctx, &outcome_request) != LXP_OK ||
        escrow_account.balance.lo != 50U || provider_account.balance.lo != 0U ||
        transfer_calls != 0U)
        return 1;

    (void)memset(&escrow_record, 0, sizeof(escrow_record));
    escrow_record.escrow_id[0] = 30U;
    (void)memcpy(escrow_record.owner, buyer.principal, 32U);
    (void)memcpy(escrow_record.escrow_account, escrow_account.id, 32U);
    (void)memcpy(escrow_record.beneficiary, provider_account.id, 32U);
    (void)memcpy(escrow_record.asset_id, asset.asset_id, 32U);
    escrow_record.locked_amount = (lxp_u128){ 0U, 50U };
    escrow_record.state = LX_ESCROW_STATE_OPEN;
    (void)memcpy(escrow_record.agreement_reference,
                 agreement_request.agreement_id, 32U);
    if (lx_escrow_state_put(&escrow, &escrow_record) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_ESCROW, 100U, 0U, 20U,
                            1000U, arena, true) != LXP_OK)
        return 1;
    (void)memset(&capture_request, 0, sizeof(capture_request));
    capture_request.store = &escrow;
    capture_request.escrow_id = escrow_record.escrow_id;
    capture_request.escrow_account = &escrow_account;
    capture_request.beneficiary_account = &provider_account;
    capture_request.asset = &asset;
    capture_request.authority = &provider;
    capture_request.idempotency_key[0] = 1U;
    capture_request.context.assets = &asset_state;
    capture_request.context.asset_count = 1U;
    capture_request.context.sequence_account = &escrow_account;
    (void)memcpy(capture_request.context.authorized_from,
                 escrow_account.id, 32U);
    if (lx_escrow_capture_execute(&ctx, &capture_request, &receipt) != LXP_OK ||
        transfer_calls != 1U || !lxp_u128_is_zero(escrow_account.balance) ||
        provider_account.balance.lo != 50U)
        return 1;
    return 0;
}

int main(void)
{
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;

    if (lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_service_module_iface()) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        dispute_checks(&kernel, &arena) != 0 ||
        service_to_escrow(&kernel, &arena) != 0 ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
