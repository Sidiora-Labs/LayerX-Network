#include "layerx/lx_service.h"

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

typedef struct service_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
} service_decoded;

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
    service_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 13U ||
        (payload == NULL && length != 0U)) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(service_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (service_decoded *)memory;
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = length;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const service_decoded *value = (const service_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const service_decoded *value = (const service_decoded *)decoded;
    (void)activity;
    (void)authority;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
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

lxp_result lx_service_store_validate(const lx_service_store *store)
{
    if (store == NULL || store->offer_count > LX_SERVICE_STORE_CAPACITY ||
        store->agreement_count > LX_SERVICE_STORE_CAPACITY ||
        store->commitment_count > LX_SERVICE_STORE_CAPACITY ||
        store->execution_count > LX_SERVICE_STORE_CAPACITY ||
        store->delivery_count > LX_SERVICE_STORE_CAPACITY ||
        store->dispute_count > LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_offer_lookup(lx_service_store *store,
                                   const uint8_t offer_id[32],
                                   lx_service_offer **offer)
{
    size_t i;
    if (lx_service_store_validate(store) != LXP_OK || offer_id == NULL ||
        offer == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->offer_count; ++i)
        if (memcmp(store->offers[i].offer_id, offer_id, 32U) == 0) {
            *offer = &store->offers[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_service_agreement_lookup(lx_service_store *store,
                                       const uint8_t agreement_id[32],
                                       lx_service_agreement **agreement)
{
    size_t i;
    if (lx_service_store_validate(store) != LXP_OK || agreement_id == NULL ||
        agreement == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->agreement_count; ++i)
        if (memcmp(store->agreements[i].agreement_id,
                   agreement_id, 32U) == 0) {
            *agreement = &store->agreements[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

static lxp_result offer_validate(const lx_service_offer_request *request)
{
    const lx_service_offer *offer;
    if (request == NULL || request->store == NULL ||
        request->authority == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    offer = &request->offer;
    if (lxp_ct_is_zero(offer->offer_id, 32U) ||
        lxp_ct_is_zero(offer->activity_id, 32U) ||
        lxp_ct_is_zero(offer->offering_agent, 32U) ||
        lxp_ct_is_zero(offer->asset_id, 32U) || lxp_u128_is_zero(offer->price) ||
        lxp_ct_is_zero(offer->terms_hash, 32U) ||
        lxp_ct_is_zero(offer->deliverable_specification_hash, 32U) ||
        offer->delivery_deadline == 0U || offer->acceptance_window == 0U ||
        offer->dispute_window == 0U || offer->offer_expiry == 0U ||
        offer->default_outcome < LX_SERVICE_DEFAULT_ACCEPT ||
        offer->default_outcome > LX_SERVICE_DEFAULT_REJECT ||
        memcmp(request->authority->principal,
               offer->offering_agent, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lx_service_offer_publish_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request)
{
    lx_service_offer *existing;
    lx_service_offer offer;
    lxp_result status;
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    status = offer_validate(request);
    if (status != LXP_OK) return status;
    if (request->store->offer_count == LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (lx_service_offer_lookup(request->store, request->offer.offer_id,
                                &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    offer = request->offer;
    offer.global_sequence = lxp_ctx_global_sequence(ctx);
    offer.withdrawn = false;
    offer.accepted = false;
    request->store->offers[request->store->offer_count++] = offer;
    return LXP_OK;
}

lxp_result lx_service_offer_withdraw_execute(
    lxp_module_ctx *ctx, const lx_service_offer_request *request)
{
    lx_service_offer *offer;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->authority == NULL) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    status = lx_service_offer_lookup(request->store, request->offer.offer_id,
                                     &offer);
    if (status != LXP_OK || offer->withdrawn || offer->accepted ||
        lxp_ctx_batch_timestamp_ms(ctx) > offer->offer_expiry)
        return LXP_ERR_OFFER_UNAVAILABLE;
    if (memcmp(request->authority->principal,
               offer->offering_agent, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    offer->withdrawn = true;
    return LXP_OK;
}

lxp_result lx_service_agreement_accept_execute(
    lxp_module_ctx *ctx, const lx_service_agreement_request *request)
{
    lx_service_offer *offer;
    lx_service_agreement *existing;
    lx_service_agreement agreement;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->store == NULL ||
        request->offer_id == NULL || request->authority == NULL ||
        lxp_ct_is_zero(request->agreement_id, 32U) ||
        lxp_ct_is_zero(request->buyer, 32U)) return LXP_ERR_NON_CANONICAL;
    if (request->attempts_balance_mutation)
        return LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE;
    if (memcmp(request->authority->principal, request->buyer, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lx_service_offer_lookup(request->store, request->offer_id, &offer);
    if (status != LXP_OK || offer->withdrawn || offer->accepted ||
        lxp_ctx_batch_timestamp_ms(ctx) > offer->offer_expiry)
        return LXP_ERR_OFFER_UNAVAILABLE;
    if (memcmp(request->terms_hash, offer->terms_hash, 32U) != 0)
        return LXP_ERR_TERMS_MISMATCH;
    if (request->store->agreement_count == LX_SERVICE_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (lx_service_agreement_lookup(request->store, request->agreement_id,
                                    &existing) == LXP_OK)
        return LXP_ERR_SEQUENCE_REUSED;
    (void)memset(&agreement, 0, sizeof(agreement));
    (void)memcpy(agreement.agreement_id, request->agreement_id, 32U);
    (void)memcpy(agreement.offer_id, offer->offer_id, 32U);
    (void)memcpy(agreement.provider, offer->offering_agent, 32U);
    (void)memcpy(agreement.buyer, request->buyer, 32U);
    (void)memcpy(agreement.terms_hash, request->terms_hash, 32U);
    (void)memcpy(agreement.escrow_id, request->escrow_id, 32U);
    agreement.delivery_deadline = offer->delivery_deadline;
    agreement.acceptance_window_end = offer->delivery_deadline +
                                      offer->acceptance_window;
    if (agreement.acceptance_window_end < offer->delivery_deadline)
        return LXP_ERR_OVERFLOW;
    agreement.dispute_window_end = agreement.acceptance_window_end +
                                   offer->dispute_window;
    if (agreement.dispute_window_end < agreement.acceptance_window_end)
        return LXP_ERR_OVERFLOW;
    agreement.default_outcome = offer->default_outcome;
    agreement.state = LX_SERVICE_AGREEMENT_FORMED;
    agreement.accepted_sequence = lxp_ctx_global_sequence(ctx);
    request->store->agreements[request->store->agreement_count++] = agreement;
    offer->accepted = true;
    return LXP_OK;
}
