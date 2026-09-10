#include "test_service_helpers.h"

#include <string.h>

static const uint8_t commitment_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'c', 'o', 'm', 'm', 'i', 't', ':', '1'
};
static const uint8_t progress_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'p', 'r', 'o', 'g', 'r', 's', ':', '1'
};

static size_t offer_payload(uint8_t *out, uint8_t offer_marker,
                            uint8_t spec_marker, uint64_t expiry)
{
    size_t offset = 0U;
    (void)memset(out, 0, (size_t)LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES);
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    id32(out + offset, offer_marker); offset += 32U;
    id32(out + offset, 11U); offset += 32U;
    be64(out + offset + 8U, 25U); offset += 16U;
    id32(out + offset, 12U); offset += 32U;
    id32(out + offset, spec_marker); offset += 32U;
    be64(out + offset, 1000U); offset += 8U;
    be64(out + offset, 200U); offset += 8U;
    be64(out + offset, 300U); offset += 8U;
    out[offset++] = (uint8_t)LX_SERVICE_DEFAULT_ACCEPT;
    be64(out + offset, expiry); offset += 8U;
    return offset;
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

static size_t commit_payload(uint8_t *out, const uint8_t commitment_id[32],
                             const uint8_t agreement_id[32],
                             const uint8_t task_hash[32],
                             const uint8_t escrow_id[32], uint64_t deadline,
                             uint64_t resource_bound)
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, commitment_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, task_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, escrow_id, 32U); offset += 32U;
    be64(out + offset, deadline); offset += 8U;
    be64(out + offset, resource_bound); offset += 8U;
    return offset;
}

static size_t abandon_payload(uint8_t *out, const uint8_t commitment_id[32],
                              uint16_t reason)
{
    out[0] = 0U;
    out[1] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + LX_SERVICE_PAYLOAD_VERSION_BYTES, commitment_id, 32U);
    be16(out + LX_SERVICE_PAYLOAD_VERSION_BYTES + 32U, reason);
    return (size_t)LX_SERVICE_COMMIT_ABANDON_PAYLOAD_BYTES;
}

static size_t progress_payload(uint8_t *out, const uint8_t report_id[32],
                               const uint8_t commitment_id[32],
                               const uint8_t note_hash[32],
                               const uint8_t availability[32], uint32_t bps)
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, report_id, 32U); offset += 32U;
    (void)memcpy(out + offset, commitment_id, 32U); offset += 32U;
    (void)memcpy(out + offset, note_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, availability, 32U); offset += 32U;
    be32(out + offset, bps); offset += 4U;
    return offset;
}

int main(void)
{
    uint8_t payload[LX_SERVICE_OFFER_PUBLISH_PAYLOAD_BYTES];
    uint8_t record[LX_SERVICE_COMMITMENT_RECORD_BYTES];
    uint8_t progress_record[LX_SERVICE_PROGRESS_RECORD_BYTES];
    uint8_t offer_a[32];
    uint8_t offer_b[32];
    uint8_t formed[32];
    uint8_t proposed[32];
    uint8_t escrow_id[32];
    uint8_t terms_hash[32];
    uint8_t commitment_id[32];
    uint8_t task_hash[32];
    uint8_t report_one[32];
    uint8_t report_two[32];
    uint8_t report_three[32];
    uint8_t note_hash[32];
    uint8_t availability[32];
    lx_service_commitment commitment;
    lx_service_commitment decoded_commitment;
    lx_service_agreement agreement;
    lx_service_progress progress;
    lx_service_progress decoded_progress;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_module_ctx ctx;
    lxp_result outcome = LXP_OK;
    uint32_t high_water = 0U;
    size_t length;

    (void)memset(&provider, 0, sizeof(provider));
    (void)memset(&buyer, 0, sizeof(buyer));
    (void)memset(&outsider, 0, sizeof(outsider));
    provider.principal[0] = 1U;
    buyer.principal[0] = 2U;
    outsider.principal[0] = 3U;
    id32(offer_a, 10U);
    id32(offer_b, 20U);
    id32(formed, 40U);
    id32(proposed, 60U);
    id32(escrow_id, 41U);
    id32(terms_hash, 12U);
    id32(commitment_id, 50U);
    id32(task_hash, 51U);
    id32(report_one, 70U);
    id32(report_two, 71U);
    id32(report_three, 72U);
    id32(note_hash, 73U);
    id32(availability, 74U);

    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK)
        return 1;

    length = offer_payload(payload, 10U, 13U, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 1U, 4U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = offer_payload(payload, 20U, 13U, 900U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 2U, 5U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = propose_payload(payload, formed, offer_a, terms_hash, escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 3U, 6U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = propose_payload(payload, proposed, offer_b, terms_hash,
                             escrow_id);
    if (dispatch(LX_SERVICE_AGREEMENT_PROPOSE, payload, length, &buyer, 100U,
                 4U, 7U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = identifier_payload(payload, formed);
    if (dispatch(LX_SERVICE_AGREEMENT_ACCEPT, payload, length, &provider,
                 100U, 5U, 8U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;

    length = commit_payload(payload, commitment_id, proposed, task_hash,
                            escrow_id, 900U, 100U);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 6U, 9U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    length = commit_payload(payload, commitment_id, formed, task_hash,
                            escrow_id, 900U, 0U);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 6U, 9U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = commit_payload(payload, commitment_id, formed, task_hash,
                            escrow_id, 900U, 100U);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &outsider, 100U,
                 6U, 9U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 7U, 10U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_TASK_COMMITTED, commitment_id,
                 formed, 0U, 7U) != 0 ||
        record_present(commitment_prefix, commitment_id,
                       (size_t)LX_SERVICE_COMMITMENT_RECORD_BYTES) != 0 ||
        dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 8U, 11U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_TASK_COMMITTED, commitment_id,
                 formed, 0U, 7U) != 0)
        return 1;
    if (open_ctx(&ctx, 100U, 9U) != 0 ||
        lx_service_commitment_lookup(&ctx, commitment_id, &commitment) !=
            LXP_OK ||
        commitment.global_sequence != 7U || commitment.activity_id[0] != 10U ||
        commitment.deadline != 900U || commitment.resource_bound != 100U ||
        commitment.abandoned || commitment.abandon_reason != 0U ||
        memcmp(commitment.provider, provider.principal, 32U) != 0 ||
        memcmp(commitment.agreement_id, formed, 32U) != 0 ||
        lx_service_agreement_lookup(&ctx, formed, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_COMMITTED)
        return 1;
    if (lx_service_commitment_encode(&commitment, record) != LXP_OK ||
        lx_service_commitment_decode(record, sizeof(record),
                                     &decoded_commitment) != LXP_OK ||
        memcmp(&commitment, &decoded_commitment, sizeof(commitment)) != 0 ||
        lx_service_commitment_decode(record, sizeof(record) - 1U,
                                     &decoded_commitment) !=
            LXP_ERR_NON_CANONICAL)
        return 1;

    length = progress_payload(payload, report_one, commitment_id, note_hash,
                              availability, 0U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 10U, 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_PARAMETER_BOUNDS)
        return 1;
    length = progress_payload(payload, report_one, commitment_id, note_hash,
                              availability,
                              (uint32_t)LX_SERVICE_PROGRESS_COMPLETE_BPS +
                                  1U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 10U, 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_PARAMETER_BOUNDS)
        return 1;
    length = progress_payload(payload, report_one, commitment_id, note_hash,
                              availability, 2500U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &outsider,
                 100U, 10U, 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 11U, 13U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_PROGRESS_REPORTED, report_one,
                 commitment_id, 0U, 11U) != 0 ||
        record_present(progress_prefix, report_one,
                       (size_t)LX_SERVICE_PROGRESS_RECORD_BYTES) != 0 ||
        dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 12U, 14U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;
    length = progress_payload(payload, report_two, commitment_id, note_hash,
                              availability, 2500U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 13U, 15U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_METER_REGRESSION)
        return 1;
    length = progress_payload(payload, report_two, commitment_id, note_hash,
                              availability, 1000U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 14U, 16U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_METER_REGRESSION)
        return 1;
    length = progress_payload(payload, report_two, commitment_id, note_hash,
                              availability,
                              (uint32_t)LX_SERVICE_PROGRESS_COMPLETE_BPS);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 15U, 17U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_PROGRESS_REPORTED, report_two,
                 commitment_id, 1U, 15U) != 0)
        return 1;
    length = progress_payload(payload, report_three, commitment_id,
                              note_hash, availability, 5000U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 901U, 16U, 18U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_DELIVERY_DEADLINE_PASSED)
        return 1;
    if (open_ctx(&ctx, 100U, 17U) != 0 ||
        lx_service_progress_high_water(&ctx, commitment_id, &high_water) !=
            LXP_OK ||
        high_water != (uint32_t)LX_SERVICE_PROGRESS_COMPLETE_BPS ||
        lx_service_progress_lookup(&ctx, report_one, &progress) != LXP_OK ||
        progress.progress_bps != 2500U || progress.reported_at != 100U ||
        progress.global_sequence != 11U || progress.activity_id[0] != 13U ||
        memcmp(progress.agreement_id, formed, 32U) != 0 ||
        memcmp(progress.provider, provider.principal, 32U) != 0 ||
        lx_service_progress_encode(&progress, progress_record) != LXP_OK ||
        lx_service_progress_decode(progress_record, sizeof(progress_record),
                                   &decoded_progress) != LXP_OK ||
        memcmp(&progress, &decoded_progress, sizeof(progress)) != 0)
        return 1;

    length = abandon_payload(payload, commitment_id, 0U);
    if (dispatch(LX_SERVICE_COMMIT_ABANDON, payload, length, &provider, 100U,
                 18U, 19U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    length = abandon_payload(payload, commitment_id, 9U);
    if (dispatch(LX_SERVICE_COMMIT_ABANDON, payload, length, &outsider, 100U,
                 18U, 19U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_COMMIT_ABANDON, payload, length, &provider, 100U,
                 19U, 20U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_COMMIT_ABANDONED, commitment_id,
                 formed, 1U, 19U) != 0 ||
        dispatch(LX_SERVICE_COMMIT_ABANDON, payload, length, &provider, 100U,
                 20U, 21U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    if (open_ctx(&ctx, 100U, 21U) != 0 ||
        lx_service_commitment_lookup(&ctx, commitment_id, &commitment) !=
            LXP_OK ||
        !commitment.abandoned || commitment.abandon_reason != 9U ||
        lx_service_agreement_lookup(&ctx, formed, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_FORMED ||
        lx_service_commitment_encode(&commitment, record) != LXP_OK ||
        lx_service_commitment_decode(record, sizeof(record),
                                     &decoded_commitment) != LXP_OK ||
        memcmp(&commitment, &decoded_commitment, sizeof(commitment)) != 0)
        return 1;
    length = progress_payload(payload, report_three, commitment_id,
                              note_hash, availability, 5000U);
    if (dispatch(LX_SERVICE_PROGRESS_REPORT, payload, length, &provider,
                 100U, 22U, 23U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
