#include "layerx/lx_service.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static void offer_request_init(lx_service_offer_request *request,
                               lx_service_store *store,
                               const lxp_authority_resolved *authority,
                               uint8_t marker)
{
    (void)memset(request, 0, sizeof(*request));
    request->store = store;
    request->authority = authority;
    request->offer.offer_id[0] = marker;
    request->offer.activity_id[0] = (uint8_t)(marker + 1U);
    (void)memcpy(request->offer.offering_agent,
                 authority->principal, 32U);
    request->offer.asset_id[0] = 10U;
    request->offer.price = (lxp_u128){ 0U, 25U };
    request->offer.terms_hash[0] = 11U;
    request->offer.deliverable_specification_hash[0] = 12U;
    request->offer.delivery_deadline = 1000U;
    request->offer.acceptance_window = 200U;
    request->offer.dispute_window = 300U;
    request->offer.default_outcome = LX_SERVICE_DEFAULT_ACCEPT;
    request->offer.offer_expiry = 900U;
}

int main(void)
{
    lx_service_store store;
    lx_service_offer_request publish;
    lx_service_offer_request withdraw;
    lx_service_agreement_request accept;
    lx_service_offer *offer;
    lx_service_agreement *agreement;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    const lxp_module_iface *iface = lx_service_module_iface();

    (void)memset(&store, 0, sizeof(store));
    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    if (iface->module_id != LXP_MODULE_SERVICE ||
        iface->activity_type_count != 13U ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, iface) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SERVICE, 500U, 0U, 41U,
                            1000U, &arena, true) != LXP_OK)
        return 1;

    offer_request_init(&publish, &store, &provider, 3U);
    store.offer_count = LX_SERVICE_STORE_CAPACITY + 1U;
    if (lx_service_offer_publish_execute(&ctx, &publish) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    store.offer_count = 0U;
    publish.attempts_balance_mutation = true;
    if (lx_service_offer_publish_execute(&ctx, &publish) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE || store.offer_count != 0U)
        return 1;
    publish.attempts_balance_mutation = false;
    if (lx_service_offer_publish_execute(&ctx, &publish) != LXP_OK ||
        lx_service_offer_lookup(&store, publish.offer.offer_id, &offer) !=
            LXP_OK ||
        offer->global_sequence != 41U || offer->activity_id[0] != 4U ||
        offer->price.lo != 25U || offer->withdrawn || offer->accepted)
        return 1;

    (void)memset(&accept, 0, sizeof(accept));
    accept.store = &store;
    accept.offer_id = offer->offer_id;
    accept.agreement_id[0] = 5U;
    (void)memcpy(accept.buyer, buyer.principal, 32U);
    accept.terms_hash[0] = 99U;
    accept.escrow_id[0] = 6U;
    accept.authority = &buyer;
    if (lx_service_agreement_accept_execute(&ctx, &accept) !=
            LXP_ERR_TERMS_MISMATCH || store.agreement_count != 0U)
        return 1;
    (void)memcpy(accept.terms_hash, offer->terms_hash, 32U);
    if (lx_service_agreement_accept_execute(&ctx, &accept) != LXP_OK ||
        lx_service_agreement_lookup(&store, accept.agreement_id, &agreement) !=
            LXP_OK ||
        agreement->state != LX_SERVICE_AGREEMENT_FORMED ||
        agreement->accepted_sequence != 41U || agreement->escrow_id[0] != 6U ||
        !offer->accepted ||
        lx_service_agreement_accept_execute(&ctx, &accept) !=
            LXP_ERR_OFFER_UNAVAILABLE)
        return 1;

    offer_request_init(&withdraw, &store, &provider, 7U);
    if (lx_service_offer_publish_execute(&ctx, &withdraw) != LXP_OK ||
        lx_service_offer_withdraw_execute(&ctx, &withdraw) != LXP_OK ||
        lx_service_offer_lookup(&store, withdraw.offer.offer_id, &offer) !=
            LXP_OK || !offer->withdrawn)
        return 1;
    accept.offer_id = offer->offer_id;
    accept.agreement_id[0] = 8U;
    (void)memcpy(accept.terms_hash, offer->terms_hash, 32U);
    if (lx_service_agreement_accept_execute(&ctx, &accept) !=
            LXP_ERR_OFFER_UNAVAILABLE ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SERVICE, 1001U, 0U, 42U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    offer_request_init(&publish, &store, &provider, 9U);
    if (lx_service_offer_publish_execute(&ctx, &publish) != LXP_OK)
        return 1;
    accept.offer_id = publish.offer.offer_id;
    accept.agreement_id[0] = 10U;
    (void)memcpy(accept.terms_hash, publish.offer.terms_hash, 32U);
    if (lx_service_agreement_accept_execute(&ctx, &accept) !=
            LXP_ERR_OFFER_UNAVAILABLE ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
