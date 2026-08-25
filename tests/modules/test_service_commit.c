#include "layerx/lx_service.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

int main(void)
{
    lx_service_store store;
    lx_service_agreement *agreement;
    lx_service_commit_request request;
    lx_service_commitment first;
    lx_service_commitment replayed;
    lxp_authority_resolved provider;
    lxp_authority_resolved stranger;
    lxp_effect_buffer effects;
    lxp_receipt receipt;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint8_t activity_id[32] = { 1U };
    uint8_t previous_root[32] = { 2U };
    uint8_t resulting_root[32] = { 3U };
    uint8_t activity_root[32] = { 4U };
    uint8_t batch_id[32] = { 5U };
    uint64_t parameters = 1U;

    (void)memset(&store, 0, sizeof(store));
    store.delivery_count = LX_SERVICE_STORE_CAPACITY + 1U;
    if (lx_service_store_validate(&store) != LXP_ERR_NON_CANONICAL)
        return 1;
    store.delivery_count = 0U;
    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&stranger, 0, sizeof(stranger));
    provider.principal[0] = 10U;
    stranger.principal[0] = 11U;
    store.agreement_count = 1U;
    agreement = &store.agreements[0];
    agreement->agreement_id[0] = 20U;
    (void)memcpy(agreement->provider, provider.principal, 32U);
    agreement->buyer[0] = 21U;
    agreement->escrow_id[0] = 22U;
    agreement->state = LX_SERVICE_AGREEMENT_FORMED;
    if (lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_service_module_iface()) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SERVICE, 100U, 0U, 50U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    (void)memset(&request, 0, sizeof(request));
    request.store = &store;
    request.authority = &stranger;
    request.commitment.commitment_id[0] = 30U;
    request.commitment.activity_id[0] = 31U;
    (void)memcpy(request.commitment.provider, provider.principal, 32U);
    (void)memcpy(request.commitment.agreement_id,
                 agreement->agreement_id, 32U);
    request.commitment.task_hash[0] = 32U;
    request.commitment.deadline = 1000U;
    request.commitment.resource_bound = 500U;
    (void)memcpy(request.commitment.escrow_id, agreement->escrow_id, 32U);
    store.commitment_count = LX_SERVICE_STORE_CAPACITY + 1U;
    if (lx_service_commitment_put(&store, &request.commitment) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    store.commitment_count = 0U;
    if (lx_service_commit_task_execute(&ctx, &request, &first) !=
            LXP_ERR_AGREEMENT_STATE || store.commitment_count != 0U)
        return 1;
    request.authority = &provider;
    request.attempts_balance_mutation = true;
    if (lx_service_commit_task_execute(&ctx, &request, &first) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE)
        return 1;
    request.attempts_balance_mutation = false;
    if (lx_service_commit_task_execute(&ctx, &request, &first) != LXP_OK ||
        store.commitment_count != 1U || first.global_sequence != 50U ||
        agreement->state != LX_SERVICE_AGREEMENT_COMMITTED ||
        lx_service_commit_task_execute(&ctx, &request, &replayed) != LXP_OK ||
        memcmp(&first, &replayed, sizeof(first)) != 0 ||
        store.commitment_count != 1U)
        return 1;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(&receipt, activity_id, 50U, previous_root,
                          resulting_root, activity_root, LXP_OK, &effects,
                          (lxp_u128){ 0U, 1U }, batch_id,
                          LXP_MODULE_SERVICE, 1U, 1U) != LXP_OK ||
        receipt.effects.count != 0U)
        return 1;
    request.abandon_reason = 7U;
    if (lx_service_commit_abandon_execute(&ctx, &request, &replayed) !=
            LXP_OK || !replayed.abandoned || replayed.abandon_reason != 7U ||
        agreement->state != LX_SERVICE_AGREEMENT_FORMED ||
        lx_service_commit_abandon_execute(&ctx, &request, &first) != LXP_OK ||
        memcmp(&first, &replayed, sizeof(first)) != 0 ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
