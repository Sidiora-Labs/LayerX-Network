#include "layerx/lx_service.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum { WORK_ARENA_BYTES = 16384, GAS_LIMIT = 100000 };

static const uint8_t offer_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'o', 'f', 'f', 'e', 'r', ':', ':', '1'
};
static const uint8_t agreement_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'a', 'g', 'r', 'e', 'e', ':', ':', '1'
};

static lxp_state_store store_state;
static lxp_state_journal state_journal;
static lxp_kernel service_kernel;
static lxp_effect_buffer event_buffer;
static lxp_arena work_arena;
static uint8_t work_bytes[WORK_ARENA_BYTES];
static uint64_t parameter_set = 1U;

static void be64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static void id32(uint8_t out[32], uint8_t marker)
{
    (void)memset(out, 0, 32U);
    out[0] = marker;
}

static size_t offer_payload(uint8_t *out, uint8_t offer_marker,
                            uint8_t asset_marker, uint64_t price,
                            uint8_t terms_marker, uint8_t spec_marker,
                            uint64_t deadline, uint64_t acceptance,
                            uint64_t dispute, uint8_t outcome,
                            uint64_t expiry)
{
    size_t offset = 0U;
    (void)memset(out, 0, (size_t)LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES);
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    id32(out + offset, offer_marker); offset += 32U;
    id32(out + offset, asset_marker); offset += 32U;
    be64(out + offset + 8U, price); offset += 16U;
    id32(out + offset, terms_marker); offset += 32U;
    id32(out + offset, spec_marker); offset += 32U;
    be64(out + offset, deadline); offset += 8U;
    be64(out + offset, acceptance); offset += 8U;
    be64(out + offset, dispute); offset += 8U;
    out[offset++] = outcome;
    be64(out + offset, expiry); offset += 8U;
    return offset;
}

static size_t identifier_payload(uint8_t *out, const uint8_t identifier[32])
{
    out[0] = 0U;
    out[1] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + LX_SERVICE_PAYLOAD_VERSION_BYTES, identifier, 32U);
    return (size_t)LX_SERVICE_IDENTIFIER_PAYLOAD_BYTES;
}

static size_t propose_payload(uint8_t *out, const uint8_t agreement_id[32],
                              const uint8_t offer_id[32],
                              const uint8_t terms_hash[32],
                              const uint8_t escrow_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, offer_id, 32U); offset += 32U;
    (void)memcpy(out + offset, terms_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, escrow_id, 32U); offset += 32U;
    return offset;
}

static int open_ctx(lxp_module_ctx *ctx, uint64_t timestamp,
                    uint64_t sequence)
{
    if (lxp_arena_init(&work_arena, work_bytes, sizeof(work_bytes)) !=
            LXP_OK ||
        lxp_effect_buffer_init(&event_buffer) != LXP_OK ||
        lxp_module_ctx_init(ctx, &service_kernel, LXP_MODULE_SERVICE,
                            timestamp, 0U, sequence, (uint64_t)GAS_LIMIT,
                            &work_arena, true) != LXP_OK)
        return 1;
    return lxp_module_ctx_bind_effects(ctx, &event_buffer) == LXP_OK ? 0 : 1;
}

static lxp_result dispatch(uint32_t activity_type, const uint8_t *payload,
                           size_t length,
                           const lxp_authority_resolved *authority,
                           uint64_t timestamp, uint64_t sequence,
                           uint8_t activity_marker,
                           lxp_result *module_result)
{
    const lxp_module_registration *registration = NULL;
    lxp_activity activity;
    lxp_module_ctx ctx;
    lxp_result status;

    *module_result = LXP_FATAL_INVARIANT;
    status = lxp_kernel_module_for_activity(&service_kernel, activity_type,
                                            0U, &registration);
    if (status != LXP_OK) return status;
    if (open_ctx(&ctx, timestamp, sequence) != 0) return LXP_FATAL_INVARIANT;
    ctx.activity_id[0] = activity_marker;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = activity_type;
    activity.payload.bytes = payload;
    activity.payload.length = length;
    status = lxp_kernel_dispatch(registration, &ctx, &activity, authority,
                                 &event_buffer, module_result);
    if (status != LXP_OK) return status;
    if (*module_result == LXP_OK) return lxp_module_ctx_commit(&ctx);
    lxp_module_ctx_rollback(&ctx);
    return LXP_OK;
}

static int event_is(uint16_t event_type, const uint8_t primary[32],
                    const uint8_t secondary[32], uint8_t code,
                    uint64_t sequence)
{
    uint8_t body[LX_SERVICE_EVENT_BODY_BYTES];
    const lxp_effect *effect = &event_buffer.effects[0];
    if (event_buffer.count != 1U || effect->kind != LXP_EFFECT_EVENT ||
        effect->monetary || effect->module_id != LXP_MODULE_SERVICE ||
        effect->event_type != event_type ||
        effect->body_length != (uint16_t)LX_SERVICE_EVENT_BODY_BYTES)
        return 1;
    (void)memcpy(body, primary, 32U);
    (void)memcpy(body + 32U, secondary, 32U);
    body[64] = code;
    be64(body + 65U, sequence);
    return memcmp(effect->body, body, sizeof(body)) == 0 ? 0 : 1;
}

static int record_present(const uint8_t prefix[LX_SERVICE_KEY_PREFIX_BYTES],
                          const uint8_t identifier[32], size_t expected)
{
    uint8_t key[LX_SERVICE_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t length = 0U;
    lxp_module_ctx ctx;
    if (open_ctx(&ctx, 1U, 1U) != 0) return 1;
    (void)memcpy(key, prefix, (size_t)LX_SERVICE_KEY_PREFIX_BYTES);
    (void)memcpy(key + LX_SERVICE_KEY_PREFIX_BYTES, identifier, 32U);
    if (lxp_ctx_kv_get(&ctx, key, sizeof(key), &value, &length) != LXP_OK)
        return 1;
    return length == expected ? 0 : 1;
}

static int decode_refusals(const lxp_authority_resolved *provider)
{
    uint8_t payload[LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES + 1U];
    lxp_result outcome = LXP_OK;
    size_t length = offer_payload(payload, 10U, 11U, 25U, 12U, 13U, 1000U,
                                  200U, 300U,
                                  (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, 900U);
    if (length != (size_t)LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES) return 1;
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length - 1U, provider,
                 100U, 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_TRUNCATED)
        return 1;
    payload[1] = (uint8_t)(LX_SERVICE_RECORD_VERSION + 1);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, provider, 100U,
                 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_VERSION_UNSUPPORTED)
        return 1;
    payload[1] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    payload[length] = 0U;
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length + 1U, provider,
                 100U, 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_TRAILING_BYTES)
        return 1;
    (void)memset(payload + LX_SERVICE_PAYLOAD_VERSION_BYTES, 0, 32U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, provider, 100U,
                 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = offer_payload(payload, 10U, 11U, 25U, 12U, 13U, 1000U, 200U,
                           300U, 3U, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, provider, 100U,
                 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = offer_payload(payload, 10U, 11U, 0U, 12U, 13U, 1000U, 200U,
                           300U, (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, provider, 100U,
                 1U, 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE + 1U, payload, length, provider,
                 100U, 1U, 4U, &outcome) != LXP_ERR_UNKNOWN_ACTIVITY)
        return 1;
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, 0U, provider, 100U, 1U,
                 4U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNKNOWN_ACTIVITY)
        return 1;
    return 0;
}

static int balance_mutation_refusals(const lxp_authority_resolved *provider)
{
    lx_service_offer_request offer_request;
    lx_service_agreement_request agreement_request;
    lx_service_offer offer;
    lx_service_agreement agreement;
    lxp_module_ctx ctx;
    if (open_ctx(&ctx, 100U, 1U) != 0) return 1;
    (void)memset(&offer_request, 0, sizeof(offer_request));
    offer_request.authority = provider;
    offer_request.attempts_balance_mutation = true;
    id32(offer_request.offer.offer_id, 90U);
    id32(offer_request.offer.activity_id, 91U);
    (void)memcpy(offer_request.offer.offering_agent, provider->principal,
                 32U);
    id32(offer_request.offer.asset_id, 11U);
    offer_request.offer.price = (lxp_u128){ 0U, 25U };
    id32(offer_request.offer.terms_hash, 12U);
    id32(offer_request.offer.deliverable_specification_hash, 13U);
    offer_request.offer.delivery_deadline = 1000U;
    offer_request.offer.acceptance_window = 200U;
    offer_request.offer.dispute_window = 300U;
    offer_request.offer.default_outcome = LX_SERVICE_DEFAULT_ACCEPT;
    offer_request.offer.offer_expiry = 900U;
    if (lx_service_offer_publish_execute(&ctx, &offer_request, &offer) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE ||
        lx_service_offer_withdraw_execute(&ctx, &offer_request, &offer) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE)
        return 1;
    (void)memset(&agreement_request, 0, sizeof(agreement_request));
    agreement_request.authority = provider;
    agreement_request.attempts_balance_mutation = true;
    id32(agreement_request.agreement_id, 92U);
    id32(agreement_request.offer_id, 90U);
    (void)memcpy(agreement_request.buyer, provider->principal, 32U);
    id32(agreement_request.terms_hash, 12U);
    id32(agreement_request.escrow_id, 93U);
    if (lx_service_agreement_propose_execute(&ctx, &agreement_request,
                                             &agreement) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE ||
        lx_service_agreement_accept_execute(&ctx, &agreement_request,
                                            &agreement) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE)
        return 1;
    offer_request.attempts_balance_mutation = false;
    (void)memcpy(offer_request.offer.offering_agent, agreement_request.buyer,
                 32U);
    offer_request.offer.offering_agent[0] = 99U;
    if (lx_service_offer_publish_execute(&ctx, &offer_request, &offer) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}

int main(void)
{
    uint8_t payload[LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES + 1U];
    uint8_t offer_a[32];
    uint8_t offer_b[32];
    uint8_t offer_c[32];
    uint8_t agreement_id[32];
    uint8_t escrow_id[32];
    uint8_t terms_hash[32];
    uint8_t encoded[LX_SERVICE_AGREEMENT_RECORD_BYTES];
    lx_service_offer offer;
    lx_service_offer decoded_offer;
    lx_service_agreement agreement;
    lx_service_agreement decoded_agreement;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_module_ctx ctx;
    const lxp_module_iface *iface = lx_service_module_iface();
    lxp_result outcome = LXP_OK;
    size_t length;
    size_t encoded_length = 0U;

    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&outsider, 0, sizeof(outsider));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    outsider.principal[0] = 3U;
    id32(offer_a, 10U);
    id32(offer_b, 20U);
    id32(offer_c, 30U);
    id32(agreement_id, 40U);
    id32(escrow_id, 41U);
    id32(terms_hash, 12U);

    if (iface == NULL || iface->module_id != LXP_MODULE_SERVICE ||
        iface->activity_type_count != 13U ||
        iface->activity_types[0] != LX_SERVICE_OFFER_PUBLISH ||
        iface->activity_types[12] != LX_SERVICE_DISPUTE_RESOLVE ||
        iface->epoch_begin == NULL || iface->epoch_end == NULL ||
        iface->state_root == NULL ||
        lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel, iface) != LXP_OK)
        return 1;

    if (decode_refusals(&provider) != 0) return 1;
    if (balance_mutation_refusals(&provider) != 0) return 1;

    length = offer_payload(payload, 10U, 11U, 25U, 12U, 13U, 1000U, 200U,
                           300U, (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 41U, 4U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_OFFER_PUBLISHED, offer_a,
                 provider.principal,
                 (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, 41U) != 0 ||
        record_present(offer_prefix, offer_a,
                       (size_t)LX_SERVICE_OFFER_RECORD_BYTES) != 0)
        return 1;
    if (open_ctx(&ctx, 100U, 42U) != 0 ||
        lx_service_offer_lookup(&ctx, offer_a, &offer) != LXP_OK ||
        offer.activity_id[0] != 4U || offer.global_sequence != 41U ||
        offer.price.hi != 0U || offer.price.lo != 25U || offer.withdrawn ||
        offer.accepted || offer.delivery_deadline != 1000U ||
        offer.acceptance_window != 200U || offer.dispute_window != 300U ||
        offer.offer_expiry != 900U ||
        offer.default_outcome != LX_SERVICE_DEFAULT_ACCEPT ||
        memcmp(offer.offering_agent, provider.principal, 32U) != 0)
        return 1;
    if (lx_service_offer_encode(&offer, encoded) != LXP_OK ||
        lx_service_offer_decode(encoded,
                                (size_t)LX_SERVICE_OFFER_RECORD_BYTES,
                                &decoded_offer) != LXP_OK ||
        memcmp(&offer, &decoded_offer, sizeof(offer)) != 0 ||
        lx_service_offer_decode(encoded,
                                (size_t)LX_SERVICE_OFFER_RECORD_BYTES - 1U,
                                &decoded_offer) != LXP_ERR_NON_CANONICAL)
        return 1;
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 43U, 5U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;

    length = offer_payload(payload, 20U, 11U, 25U, 12U, 13U, 1000U, 200U,
                           300U, (uint8_t)LX_SERVICE_DEFAULT_REJECT, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 44U, 6U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = identifier_payload(payload, offer_b);
    if (dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &outsider, 100U,
                 45U, 7U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DEBIT ||
        dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &provider, 901U,
                 45U, 7U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE ||
        dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &provider, 100U,
                 45U, 7U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_OFFER_WITHDRAWN, offer_b,
                 provider.principal, 1U, 45U) != 0 ||
        dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &provider, 100U,
                 46U, 8U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE)
        return 1;
    length = identifier_payload(payload, offer_c);
    if (dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &provider, 100U,
                 47U, 9U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE)
        return 1;

    length = propose_payload(payload, agreement_id, offer_b, terms_hash,
                             escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 48U, 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE)
        return 1;
    length = propose_payload(payload, agreement_id, offer_a, escrow_id,
                             escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 49U, 11U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_TERMS_MISMATCH)
        return 1;
    length = propose_payload(payload, agreement_id, offer_a, terms_hash,
                             escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &provider,
                 100U, 50U, 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DEBIT ||
        dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 901U,
                 50U, 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE ||
        dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 51U, 13U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_AGREEMENT_PROPOSED, agreement_id,
                 provider.principal,
                 (uint8_t)LX_SERVICE_AGREEMENT_PROPOSED, 51U) != 0 ||
        record_present(agreement_prefix, agreement_id,
                       (size_t)LX_SERVICE_AGREEMENT_FIXED_BYTES) != 0 ||
        dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 52U, 14U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;
    if (open_ctx(&ctx, 100U, 53U) != 0 ||
        lx_service_agreement_lookup(&ctx, agreement_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_PROPOSED ||
        agreement.accepted_sequence != 51U ||
        agreement.delivery_deadline != 1000U ||
        agreement.acceptance_window_end != 1200U ||
        agreement.dispute_window_end != 1500U ||
        agreement.default_outcome != LX_SERVICE_DEFAULT_ACCEPT ||
        agreement.contested_hash_count != 0U || agreement.default_applied ||
        memcmp(agreement.provider, provider.principal, 32U) != 0 ||
        memcmp(agreement.buyer, buyer.principal, 32U) != 0 ||
        memcmp(agreement.offer_id, offer_a, 32U) != 0 ||
        memcmp(agreement.escrow_id, escrow_id, 32U) != 0)
        return 1;
    if (lx_service_agreement_encode(&agreement, encoded,
                                    &encoded_length) != LXP_OK ||
        encoded_length != (size_t)LX_SERVICE_AGREEMENT_FIXED_BYTES ||
        lx_service_agreement_decode(encoded, encoded_length,
                                    &decoded_agreement) != LXP_OK ||
        memcmp(&agreement, &decoded_agreement, sizeof(agreement)) != 0 ||
        lx_service_agreement_decode(encoded, encoded_length - 1U,
                                    &decoded_agreement) !=
            LXP_ERR_NON_CANONICAL)
        return 1;

    length = identifier_payload(payload, agreement_id);
    if (dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &buyer, 100U,
                 54U, 15U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DEBIT ||
        dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &outsider,
                 100U, 54U, 15U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DEBIT ||
        dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &provider,
                 100U, 55U, 16U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_AGREEMENT_ACCEPTED, agreement_id,
                 buyer.principal, (uint8_t)LX_SERVICE_AGREEMENT_FORMED,
                 55U) != 0 ||
        dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &provider,
                 100U, 56U, 17U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    length = identifier_payload(payload, offer_c);
    if (dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &provider,
                 100U, 57U, 18U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (open_ctx(&ctx, 100U, 58U) != 0 ||
        lx_service_agreement_lookup(&ctx, agreement_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_FORMED ||
        agreement.accepted_sequence != 55U ||
        lx_service_offer_lookup(&ctx, offer_a, &offer) != LXP_OK ||
        !offer.accepted || offer.withdrawn)
        return 1;
    length = identifier_payload(payload, offer_a);
    if (dispatch(LX_SERVICE_OFFER_WITHDRAW, payload, length, &provider, 100U,
                 59U, 19U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_OFFER_UNAVAILABLE)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
