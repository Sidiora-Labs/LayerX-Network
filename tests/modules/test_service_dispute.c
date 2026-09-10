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
static lxp_effect_buffer audit_buffer;
static uint64_t parameter_set = 1U;
static size_t transfer_calls;
static size_t event_effects;

static lxp_result counting_applier(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    (void)kernel;
    (void)set;
    (void)receipt;
    ++transfer_calls;
    return LXP_FATAL_INVARIANT;
}

static lxp_result counting_reader(const void *set, uint32_t parameter_id,
                                  uint64_t *value)
{
    (void)set;
    (void)parameter_id;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    *value = 0U;
    return LXP_OK;
}

static void be16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8);
    bytes[1] = (uint8_t)value;
}

static void be32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24);
    bytes[1] = (uint8_t)(value >> 16);
    bytes[2] = (uint8_t)(value >> 8);
    bytes[3] = (uint8_t)value;
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

static size_t offer_payload(uint8_t *out, uint8_t offer_marker)
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
    out[offset++] = (uint8_t)LX_SERVICE_DEFAULT_ACCEPT;
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
                              const uint8_t agreement_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, delivery_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    out[offset++] = 1U;
    id32(out + offset, 13U); offset += 32U;
    be64(out + offset, 4096U); offset += 8U;
    id32(out + offset, 21U); offset += 32U;
    return offset;
}

static size_t reject_payload(uint8_t *out, const uint8_t agreement_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    be16(out + offset, 5U); offset += 2U;
    out[offset++] = 1U;
    id32(out + offset, 13U); offset += 32U;
    return offset;
}

static size_t dispute_open_payload(uint8_t *out, const uint8_t dispute_id[32],
                                   const uint8_t agreement_id[32],
                                   const uint8_t *markers, size_t count)
{
    size_t offset = 0U;
    size_t i;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, dispute_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    out[offset++] = (uint8_t)count;
    for (i = 0U; i < count; ++i) {
        id32(out + offset, markers[i]);
        offset += 32U;
    }
    return offset;
}

static size_t dispute_resolve_payload(uint8_t *out,
                                      const uint8_t dispute_id[32],
                                      uint16_t ruling, uint32_t basis_points,
                                      uint8_t resolution_marker)
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, dispute_id, 32U); offset += 32U;
    be16(out + offset, ruling); offset += 2U;
    be32(out + offset, basis_points); offset += 4U;
    id32(out + offset, resolution_marker); offset += 32U;
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

static int effects_are_events(void)
{
    size_t i;
    for (i = 0U; i < event_buffer.count; ++i) {
        const lxp_effect *effect = &event_buffer.effects[i];
        if (effect->kind != LXP_EFFECT_EVENT || effect->monetary ||
            effect->module_id != LXP_MODULE_SERVICE)
            return 1;
        ++event_effects;
    }
    return 0;
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
    if (effects_are_events() != 0) return LXP_FATAL_INVARIANT;
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

static int rejected_agreement(uint8_t marker, uint8_t *agreement_id,
                              const lxp_authority_resolved *provider,
                              const lxp_authority_resolved *buyer,
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
    length = offer_payload(payload, marker);
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
    length = deliver_payload(payload, delivery_id, agreement_id);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, provider, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    length = reject_payload(payload, agreement_id);
    if (dispatch(LX_SERVICE_REJECT, payload, length, buyer, 100U,
                 ++(*sequence), marker, &status) != LXP_OK || status != LXP_OK)
        return 1;
    return 0;
}

static int iface_shape(void)
{
    const lxp_module_iface *iface = lx_service_module_iface();
    size_t i;
    if (iface == NULL || iface->module_id != LXP_MODULE_SERVICE ||
        iface->abi_version != 1U || iface->activity_type_count != 13U ||
        strcmp(iface->name, "service") != 0 || iface->genesis == NULL ||
        iface->decode == NULL || iface->validate == NULL ||
        iface->execute == NULL || iface->epoch_begin == NULL ||
        iface->epoch_end == NULL || iface->state_root == NULL)
        return 1;
    for (i = 0U; i < iface->activity_type_count; ++i) {
        if (lxp_activity_module_id(iface->activity_types[i]) !=
                LXP_MODULE_SERVICE ||
            lxp_activity_type_ordinal(iface->activity_types[i]) !=
                (uint16_t)(i + 1U))
            return 1;
        if (i != 0U && iface->activity_types[i] <= iface->activity_types[i - 1U])
            return 1;
    }
    return 0;
}

static int audit_refusals(void)
{
    lxp_effect_buffer *buffer = &audit_buffer;
    if (lx_service_effect_audit(LX_SERVICE_DISPUTE_OPEN, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lx_service_effect_audit(0x00060001U, &event_buffer) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        lx_service_effect_audit(LX_SERVICE_DISPUTE_OPEN, &event_buffer) !=
            LXP_OK)
        return 1;
    if (lxp_effect_buffer_init(buffer) != LXP_OK) return 1;
    buffer->count = 1U;
    buffer->effects[0].kind = LXP_EFFECT_TRANSFER;
    if (lx_service_effect_audit(LX_SERVICE_DISPUTE_OPEN, buffer) !=
        LXP_FATAL_INVARIANT)
        return 1;
    buffer->effects[0].kind = LXP_EFFECT_EVENT;
    buffer->effects[0].monetary = true;
    if (lx_service_effect_audit(LX_SERVICE_DISPUTE_OPEN, buffer) !=
        LXP_FATAL_INVARIANT)
        return 1;
    buffer->effects[0].monetary = false;
    buffer->count = (size_t)LXP_MAX_EFFECTS + 1U;
    return lx_service_effect_audit(LX_SERVICE_DISPUTE_OPEN, buffer) ==
           LXP_ERR_NON_CANONICAL ? 0 : 1;
}

int main(void)
{
    uint8_t payload[LX_SERVICE_DELIVER_PAYLOAD_MAX_BYTES];
    uint8_t disputed_id[32];
    uint8_t second_id[32];
    uint8_t open_id[32];
    uint8_t evidence[3];
    uint8_t root_before[32];
    uint8_t root_after[32];
    lx_service_dispute dispute;
    lx_service_agreement agreement;
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
    id32(open_id, 99U);
    evidence[0] = 55U;
    evidence[1] = 33U;
    evidence[2] = 44U;

    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_set_capabilities(&service_kernel, counting_reader,
                                    counting_applier) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK ||
        iface_shape() != 0)
        return 1;

    if (rejected_agreement(60U, disputed_id, &provider, &buyer,
                           &sequence) != 0 ||
        rejected_agreement(70U, second_id, &provider, &buyer, &sequence) != 0)
        return 1;

    if (open_ctx(&ctx, 100U, 40U) != 0 ||
        lx_service_module_iface()->state_root(&ctx, root_before) != LXP_OK)
        return 1;

    length = dispute_open_payload(payload, open_id, second_id, evidence, 3U);
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &outsider, 100U,
                 40U, 40U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DISPUTANT ||
        dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 1501U,
                 40U, 40U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_DISPUTE_WINDOW_CLOSED)
        return 1;
    payload[LX_SERVICE_DISPUTE_OPEN_PAYLOAD_FIXED_BYTES - 1U] = 0U;
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 100U, 40U,
                 40U, &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    payload[LX_SERVICE_DISPUTE_OPEN_PAYLOAD_FIXED_BYTES - 1U] =
        (uint8_t)(LX_SERVICE_MAX_DELIVERABLES + 1);
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 100U, 40U,
                 40U, &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    evidence[2] = 33U;
    length = dispute_open_payload(payload, open_id, second_id, evidence, 3U);
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 100U, 40U,
                 40U, &outcome) != LXP_OK || outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    evidence[2] = 44U;

    length = dispute_open_payload(payload, open_id, disputed_id, evidence,
                                  3U);
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 1500U,
                 41U, 41U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_DISPUTE_OPENED, open_id,
                 disputed_id, 3U, 41U) != 0)
        return 1;
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &buyer, 100U, 42U,
                 42U, &outcome) != LXP_OK || outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    length = dispute_open_payload(payload, open_id, second_id, evidence, 3U);
    if (dispatch(LX_SERVICE_DISPUTE_OPEN, payload, length, &provider, 100U,
                 43U, 43U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;

    if (open_ctx(&ctx, 100U, 44U) != 0 ||
        lx_service_dispute_lookup(&ctx, open_id, &dispute) != LXP_OK ||
        dispute.evidence_hash_count != 3U || dispute.resolved ||
        dispute.ruling != 0U || dispute.provider_basis_points != 0U ||
        dispute.resolution_sequence != 0U ||
        dispute.global_sequence != 41U || dispute.activity_id[0] != 41U ||
        memcmp(dispute.raiser, buyer.principal, 32U) != 0 ||
        dispute.evidence_hashes[0][0] != 33U ||
        dispute.evidence_hashes[1][0] != 44U ||
        dispute.evidence_hashes[2][0] != 55U ||
        lx_service_agreement_lookup(&ctx, disputed_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_DISPUTED ||
        lx_service_dispute_lookup(&ctx, second_id, &dispute) !=
            LXP_ERR_UNKNOWN_FIELD)
        return 1;

    length = dispute_resolve_payload(payload, open_id, 0U, 4000U, 77U);
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &buyer, 100U,
                 45U, 45U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_PARAMETER_BOUNDS)
        return 1;
    length = dispute_resolve_payload(payload, open_id, 2U,
                                     (uint32_t)LX_SERVICE_PROGRESS_COMPLETE_BPS
                                         + 1U, 77U);
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &buyer, 100U,
                 45U, 45U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_PARAMETER_BOUNDS)
        return 1;
    length = dispute_resolve_payload(payload, open_id, 2U, 4000U, 0U);
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &buyer, 100U,
                 45U, 45U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_PARAMETER_BOUNDS)
        return 1;
    length = dispute_resolve_payload(payload, second_id, 2U, 4000U, 77U);
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &buyer, 100U,
                 45U, 45U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    length = dispute_resolve_payload(payload, open_id, 2U, 4000U, 77U);
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &outsider,
                 100U, 45U, 45U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_UNAUTHORIZED_DISPUTANT)
        return 1;
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &provider,
                 100U, 46U, 46U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_DISPUTE_RESOLVED, open_id,
                 disputed_id, 1U, 46U) != 0)
        return 1;
    if (dispatch(LX_SERVICE_DISPUTE_RESOLVE, payload, length, &provider,
                 100U, 47U, 47U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;

    if (open_ctx(&ctx, 100U, 48U) != 0 ||
        lx_service_dispute_lookup(&ctx, open_id, &dispute) != LXP_OK ||
        !dispute.resolved || dispute.ruling != 2U ||
        dispute.provider_basis_points != 4000U ||
        dispute.escrow_resolution_id[0] != 77U ||
        dispute.resolution_sequence != 46U ||
        dispute.global_sequence != 41U ||
        lx_service_agreement_lookup(&ctx, disputed_id, &agreement) !=
            LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_RESOLVED ||
        lx_service_module_iface()->state_root(&ctx, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) == 0 ||
        lx_service_module_iface()->state_root(&ctx, NULL) !=
            LXP_ERR_NON_CANONICAL ||
        lx_service_module_iface()->epoch_end(&ctx, 0U, 100U) != LXP_OK ||
        lx_service_module_iface()->epoch_end(NULL, 0U, 100U) !=
            LXP_ERR_NON_CANONICAL)
        return 1;

    if (audit_refusals() != 0) return 1;
    if (transfer_calls != 0U || event_effects == 0U ||
        service_kernel.apply_transfer_set != counting_applier)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
