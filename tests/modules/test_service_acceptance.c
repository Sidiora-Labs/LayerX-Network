#include "layerx/lx_service.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum { WORK_ARENA_BYTES = 16384, GAS_LIMIT = 100000 };

static lxp_state_store store_state;
static lxp_state_journal state_journal;
static lxp_kernel service_kernel;
static lxp_effect_buffer event_buffer;
static lxp_arena work_arena;
static uint8_t work_bytes[WORK_ARENA_BYTES];
static uint64_t parameter_set = 1U;

static void be16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8);
    bytes[1] = (uint8_t)value;
}

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
                            uint8_t default_outcome)
{
    size_t offset = 0U;
    (void)memset(out, 0, (size_t)LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES);
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    id32(out + offset, offer_marker); offset += 32U;
    id32(out + offset, 11U); offset += 32U;
    be64(out + offset + 8U, 25U); offset += 16U;
    id32(out + offset, 12U); offset += 32U;
    id32(out + offset, 13U); offset += 32U;
    be64(out + offset, 1000U); offset += 8U;
    be64(out + offset, 200U); offset += 8U;
    be64(out + offset, 300U); offset += 8U;
    out[offset++] = default_outcome;
    be64(out + offset, 900U); offset += 8U;
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
                              const uint8_t offer_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, offer_id, 32U); offset += 32U;
    id32(out + offset, 12U); offset += 32U;
    id32(out + offset, 41U); offset += 32U;
    return offset;
}

static size_t commit_payload(uint8_t *out, const uint8_t commitment_id[32],
                             const uint8_t agreement_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, commitment_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    id32(out + offset, 51U); offset += 32U;
    id32(out + offset, 41U); offset += 32U;
    be64(out + offset, 900U); offset += 8U;
    be64(out + offset, 100U); offset += 8U;
    return offset;
}

static size_t deliver_payload(uint8_t *out, const uint8_t delivery_id[32],
                              const uint8_t agreement_id[32],
                              const uint8_t *availability_markers,
                              size_t count)
{
    size_t offset = 0U;
    size_t i;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, delivery_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    out[offset++] = (uint8_t)count;
    for (i = 0U; i < count; ++i) {
        id32(out + offset, 13U); offset += 32U;
        be64(out + offset, 1000U + i); offset += 8U;
        id32(out + offset, availability_markers[i]); offset += 32U;
    }
    return offset;
}

static size_t reject_payload(uint8_t *out, const uint8_t agreement_id[32],
                             uint16_t reason, const uint8_t *hash_markers,
                             size_t count)
{
    size_t offset = 0U;
    size_t i;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    be16(out + offset, reason); offset += 2U;
    out[offset++] = (uint8_t)count;
    for (i = 0U; i < count; ++i) {
        id32(out + offset, hash_markers[i]);
        offset += 32U;
    }
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

static int lifecycle(uint8_t marker, uint8_t *agreement_id,
                     const uint8_t *availability_markers, size_t count,
                     const lxp_authority_resolved *provider,
                     const lxp_authority_resolved *buyer, uint8_t outcome,
                     uint64_t *sequence)
{
    uint8_t payload[LX_SERVICE_DELIVER_PAYLOAD_MAX_BYTES];
    uint8_t offer_id[32];
    uint8_t commitment_id[32];
    uint8_t delivery_id[32];
    lxp_result status = LXP_OK;
    size_t length;

    id32(offer_id, marker);
    id32(agreement_id, (uint8_t)(marker + 1U));
    id32(commitment_id, (uint8_t)(marker + 2U));
    id32(delivery_id, (uint8_t)(marker + 3U));
    length = offer_payload(payload, marker, outcome);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, provider, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    length = propose_payload(payload, agreement_id, offer_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, buyer, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    length = identifier_payload(payload, agreement_id);
    if (dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, provider, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    length = commit_payload(payload, commitment_id, agreement_id);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, provider, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    length = deliver_payload(payload, delivery_id, agreement_id,
                             availability_markers, count);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, provider, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    return 0;
}

int main(void)
{
    uint8_t payload[LX_SERVICE_DELIVER_PAYLOAD_MAX_BYTES];
    uint8_t accepted_id[32];
    uint8_t rejected_id[32];
    uint8_t swept_id[32];
    uint8_t markers[2];
    uint8_t contested[2];
    lx_service_agreement agreement;
    lx_service_outcome_request request;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_module_ctx ctx;
    lxp_result outcome = LXP_OK;
    uint64_t sequence = 0U;
    size_t length;

    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&outsider, 0, sizeof(outsider));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    outsider.principal[0] = 3U;
    markers[0] = 44U;
    markers[1] = 22U;
    contested[0] = 13U;
    contested[1] = 14U;

    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK)
        return 1;

    if (lifecycle(60U, accepted_id, markers, 2U, &provider, &buyer,
                  (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, &sequence) != 0 ||
        lifecycle(70U, rejected_id, markers, 2U, &provider, &buyer,
                  (uint8_t)LX_SERVICE_DEFAULT_REJECT, &sequence) != 0 ||
        lifecycle(80U, swept_id, markers, 1U, &provider, &buyer,
                  (uint8_t)LX_SERVICE_DEFAULT_ACCEPT, &sequence) != 0)
        return 1;

    length = identifier_payload(payload, accepted_id);
    if (dispatch(LX_SERVICE_ACCEPT, payload, length, &provider, 100U, 40U,
                 40U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_ACCEPT, payload, length, &outsider, 100U, 40U,
                 40U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_ACCEPT, payload, length, &buyer, 1201U, 40U, 40U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (dispatch(LX_SERVICE_ACCEPT, payload, length, &buyer, 1200U, 41U, 41U,
                 &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_OUTCOME_ACCEPTED, accepted_id,
                 provider.principal,
                 (uint8_t)LX_SERVICE_AGREEMENT_ACCEPTED, 41U) != 0 ||
        dispatch(LX_SERVICE_ACCEPT, payload, length, &buyer, 1200U, 42U, 42U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (open_ctx(&ctx, 1200U, 43U) != 0 ||
        lx_service_agreement_lookup(&ctx, accepted_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_ACCEPTED ||
        agreement.default_applied || agreement.outcome_sequence != 41U ||
        agreement.outcome_timestamp != 1200U ||
        agreement.acceptance_window_end != 1200U ||
        agreement.dispute_window_end != 1500U)
        return 1;

    length = reject_payload(payload, rejected_id, 0U, contested, 1U);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 44U, 44U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = reject_payload(payload, rejected_id, 9U, contested, 1U);
    payload[LX_SERVICE_REJECT_PAYLOAD_FIXED_BYTES - 1U] = 0U;
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 44U, 44U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    payload[LX_SERVICE_REJECT_PAYLOAD_FIXED_BYTES - 1U] =
        (uint8_t)(LX_SERVICE_MAX_DELIVERABLES + 1);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 44U, 44U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    length = reject_payload(payload, rejected_id, 9U, contested, 2U);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 44U, 44U,
                 &outcome) != LXP_OK ||
        outcome != LXP_ERR_DELIVERABLE_MISMATCH)
        return 1;
    contested[1] = 13U;
    length = reject_payload(payload, rejected_id, 9U, contested, 2U);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 44U, 44U,
                 &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = reject_payload(payload, rejected_id, 9U, contested, 1U);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &outsider, 100U, 44U,
                 44U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 1201U, 44U, 44U,
                 &outcome) != LXP_OK || outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 45U, 45U,
                 &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_OUTCOME_REJECTED, rejected_id,
                 provider.principal, 1U, 45U) != 0)
        return 1;
    if (open_ctx(&ctx, 100U, 46U) != 0 ||
        lx_service_agreement_lookup(&ctx, rejected_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_REJECTED ||
        agreement.rejection_reason != 9U ||
        agreement.contested_hash_count != 1U ||
        agreement.contested_hashes[0][0] != 13U ||
        agreement.default_applied || agreement.outcome_sequence != 45U)
        return 1;

    (void)memset(&request, 0, sizeof(request));
    request.authority = &buyer;
    (void)memcpy(request.agreement_id, swept_id, 32U);
    request.rejection_reason = 0U;
    request.contested_hash_count = 1U;
    id32(request.contested_hashes[0], 13U);
    if (open_ctx(&ctx, 100U, 47U) != 0 ||
        lx_service_reject_execute(&ctx, &request, &agreement) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    request.rejection_reason = 4U;
    request.contested_hash_count = 0U;
    if (lx_service_reject_execute(&ctx, &request, &agreement) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    request.contested_hash_count = (size_t)LX_SERVICE_MAX_DELIVERABLES + 1U;
    if (lx_service_reject_execute(&ctx, &request, &agreement) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    request.contested_hash_count = 1U;
    request.attempts_balance_mutation = true;
    if (lx_service_reject_execute(&ctx, &request, &agreement) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE ||
        lx_service_accept_execute(&ctx, &request, &agreement) !=
            LXP_ERR_MODULE_MAY_NOT_WRITE_BALANCE)
        return 1;
    request.attempts_balance_mutation = false;

    if (lx_service_epoch_begin(&ctx, 1U, 100U) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lx_service_epoch_begin(&ctx, 0U, 101U) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lx_service_epoch_begin(NULL, 0U, 100U) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lx_service_epoch_begin(&ctx, 0U, 100U) != LXP_OK)
        return 1;
    if (lx_service_agreement_lookup(&ctx, swept_id, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_DELIVERED ||
        agreement.default_applied)
        return 1;

    if (open_ctx(&ctx, 1300U, 48U) != 0 ||
        lx_service_epoch_begin(&ctx, 0U, 1300U) != LXP_OK ||
        lx_service_agreement_lookup(&ctx, swept_id, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_ACCEPTED ||
        !agreement.default_applied || agreement.outcome_sequence != 48U ||
        agreement.outcome_timestamp != 1300U ||
        lx_service_agreement_lookup(&ctx, accepted_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_ACCEPTED ||
        agreement.default_applied ||
        lx_service_agreement_lookup(&ctx, rejected_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_REJECTED ||
        agreement.default_applied ||
        lx_service_epoch_begin(&ctx, 0U, 1300U) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;

    if (open_ctx(&ctx, 1400U, 49U) != 0 ||
        lx_service_agreement_lookup(&ctx, swept_id, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_ACCEPTED ||
        !agreement.default_applied || agreement.outcome_sequence != 48U)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
