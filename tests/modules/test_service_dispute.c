#include "test_service_helpers.h"

#include "layerx/lx_escrow.h"

#include <string.h>

static lxp_effect_buffer audit_buffer;
static lxp_arena escrow_arena;
static uint8_t escrow_bytes[WORK_ARENA_BYTES];
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

static int open_escrow_ctx(lxp_module_ctx *ctx, uint64_t sequence)
{
    if (lxp_arena_init(&escrow_arena, escrow_bytes, sizeof(escrow_bytes)) !=
        LXP_OK)
        return 1;
    return lxp_module_ctx_init(ctx, &service_kernel, LXP_MODULE_ESCROW, 100U,
                               0U, sequence, (uint64_t)GAS_LIMIT,
                               &escrow_arena, true) == LXP_OK ? 0 : 1;
}

/* Seed one escrow hold, held for the agreement the service dispute is raised
 * over, through escrow's context-based key-value API. */
static int escrow_hold_seed(const uint8_t hold_id[32],
                            const uint8_t agreement_id[32],
                            lx_escrow_record *seeded, uint8_t root[32])
{
    lxp_module_ctx ctx;
    (void)memset(seeded, 0, sizeof(*seeded));
    (void)memcpy(seeded->escrow_id, hold_id, 32U);
    id32(seeded->owner, 2U);
    id32(seeded->escrow_account, 81U);
    id32(seeded->beneficiary, 1U);
    id32(seeded->arbiter, 3U);
    id32(seeded->asset_id, 82U);
    seeded->locked_amount.lo = 1000U;
    seeded->state = LX_ESCROW_STATE_OPEN;
    seeded->expiry = 5000U;
    seeded->dispute_window = 2000U;
    id32(seeded->terms_hash, 83U);
    (void)memcpy(seeded->agreement_reference, agreement_id, 32U);
    if (open_escrow_ctx(&ctx, 30U) != 0 ||
        lx_escrow_state_put(&ctx, seeded) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    /* lxp_module_ctx_commit leaves the staged writes on the committed context
     * (observation 6.11.3210), so the escrow subtree root is read from a
     * context opened after the commit. */
    return open_escrow_ctx(&ctx, 31U) == 0 &&
           lx_escrow_module_iface()->state_root(&ctx, root) == LXP_OK ? 0 : 1;
}

/* The service dispute settles inside the service module only: the escrow hold
 * it names keeps its locked and captured amounts, its state, its parties and
 * the escrow subtree root it had before the dispute was opened. */
static int escrow_hold_unchanged(const uint8_t hold_id[32],
                                 const uint8_t agreement_id[32],
                                 const lx_escrow_record *before,
                                 const uint8_t root_before[32],
                                 uint64_t sequence)
{
    lx_escrow_record after;
    uint8_t root_after[32];
    lxp_module_ctx ctx;
    if (open_escrow_ctx(&ctx, sequence) != 0 ||
        lx_escrow_module_iface()->state_root(&ctx, root_after) != LXP_OK ||
        memcmp(root_before, root_after, 32U) != 0 ||
        lx_escrow_lookup(&ctx, hold_id, &after) != LXP_OK ||
        memcmp(after.agreement_reference, agreement_id, 32U) != 0)
        return 1;
    return after.state == before->state &&
           after.locked_amount.hi == before->locked_amount.hi &&
           after.locked_amount.lo == before->locked_amount.lo &&
           after.captured_amount.hi == before->captured_amount.hi &&
           after.captured_amount.lo == before->captured_amount.lo &&
           memcmp(after.owner, before->owner, 32U) == 0 &&
           memcmp(after.escrow_account, before->escrow_account, 32U) == 0 &&
           memcmp(after.beneficiary, before->beneficiary, 32U) == 0 &&
           memcmp(after.asset_id, before->asset_id, 32U) == 0 ? 0 : 1;
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
    uint8_t hold_id[32];
    uint8_t escrow_root_before[32];
    uint8_t record[LX_SERVICE_DISPUTE_RECORD_BYTES];
    size_t record_length = 0U;
    lx_escrow_record hold_before;
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
    id32(hold_id, 80U);
    evidence[0] = 55U;
    evidence[1] = 33U;
    evidence[2] = 44U;

    effect_audit_hook = effects_are_events;
    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_set_capabilities(&service_kernel, counting_reader,
                                    counting_applier) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_escrow_module_iface()) != LXP_OK ||
        iface_shape() != 0)
        return 1;

    if (rejected_agreement(60U, disputed_id, &provider, &buyer,
                           &sequence) != 0 ||
        rejected_agreement(70U, second_id, &provider, &buyer, &sequence) != 0)
        return 1;

    if (escrow_hold_seed(hold_id, disputed_id, &hold_before,
                         escrow_root_before) != 0)
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
                 40U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_DUPLICATE_ENTRY)
        return 1;
    evidence[2] = 0U;
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

    if (lx_service_dispute_lookup(&ctx, open_id, &dispute) != LXP_OK ||
        dispute.evidence_hash_count != 3U)
        return 1;
    (void)memcpy(dispute.evidence_hashes[2], dispute.evidence_hashes[1], 32U);
    if (lx_service_dispute_encode(&dispute, record, &record_length) !=
        LXP_ERR_DUPLICATE_ENTRY)
        return 1;
    id32(dispute.evidence_hashes[2], 55U);
    if (lx_service_dispute_encode(&dispute, record, &record_length) !=
            LXP_OK ||
        record_length < 64U ||
        lx_service_dispute_decode(record, record_length, &dispute) != LXP_OK ||
        dispute.evidence_hash_count != 3U)
        return 1;
    (void)memcpy(record + record_length - 32U, record + record_length - 64U,
                 32U);
    if (lx_service_dispute_decode(record, record_length, &dispute) !=
        LXP_ERR_DUPLICATE_ENTRY)
        return 1;
    (void)memset(record + record_length - 32U, 0, 32U);
    if (lx_service_dispute_decode(record, record_length, &dispute) !=
        LXP_ERR_NON_CANONICAL)
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

    if (escrow_hold_unchanged(hold_id, disputed_id, &hold_before,
                              escrow_root_before, 49U) != 0)
        return 1;

    if (audit_refusals() != 0) return 1;
    if (transfer_calls != 0U || event_effects == 0U ||
        service_kernel.apply_transfer_set != counting_applier)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
