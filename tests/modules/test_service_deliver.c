#include "test_service_helpers.h"

#include <string.h>

static const uint8_t delivery_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'd', 'e', 'l', 'i', 'v', 'r', ':', '1'
};
static const uint8_t deliverable_prefix[LX_SERVICE_KEY_PREFIX_BYTES] = {
    'd', 'e', 'l', 'i', 't', 'm', ':', '1'
};

typedef struct item_spec {
    uint8_t hash_marker;
    uint64_t artifact_size;
    uint8_t availability_marker;
} item_spec;

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
                             const uint8_t escrow_id[32])
{
    size_t offset = 0U;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, commitment_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    (void)memcpy(out + offset, task_hash, 32U); offset += 32U;
    (void)memcpy(out + offset, escrow_id, 32U); offset += 32U;
    be64(out + offset, 900U); offset += 8U;
    be64(out + offset, 100U); offset += 8U;
    return offset;
}

static size_t deliver_payload(uint8_t *out, const uint8_t delivery_id[32],
                              const uint8_t agreement_id[32],
                              const item_spec *items, size_t count)
{
    size_t offset = 0U;
    size_t i;
    out[offset++] = 0U;
    out[offset++] = (uint8_t)LX_SERVICE_RECORD_VERSION;
    (void)memcpy(out + offset, delivery_id, 32U); offset += 32U;
    (void)memcpy(out + offset, agreement_id, 32U); offset += 32U;
    out[offset++] = (uint8_t)count;
    for (i = 0U; i < count; ++i) {
        id32(out + offset, items[i].hash_marker); offset += 32U;
        be64(out + offset, items[i].artifact_size); offset += 8U;
        id32(out + offset, items[i].availability_marker); offset += 32U;
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

static int item_records_present(const uint8_t delivery_id[32], size_t count)
{
    uint8_t key[LX_SERVICE_ITEM_KEY_BYTES];
    const uint8_t *value = NULL;
    size_t length = 0U;
    size_t i;
    lxp_module_ctx ctx;
    if (open_ctx(&ctx, 1U, 1U) != 0) return 1;
    (void)memcpy(key, delivery_prefix, (size_t)LX_SERVICE_KEY_PREFIX_BYTES);
    (void)memcpy(key + LX_SERVICE_KEY_PREFIX_BYTES, delivery_id, 32U);
    if (lxp_ctx_kv_get(&ctx, key, (size_t)LX_SERVICE_KEY_BYTES, &value,
                       &length) != LXP_OK ||
        length != (size_t)LX_SERVICE_DELIVERY_RECORD_BYTES)
        return 1;
    (void)memcpy(key, deliverable_prefix,
                 (size_t)LX_SERVICE_KEY_PREFIX_BYTES);
    for (i = 0U; i < count; ++i) {
        key[LX_SERVICE_KEY_BYTES] = (uint8_t)i;
        if (lxp_ctx_kv_get(&ctx, key, sizeof(key), &value, &length) !=
                LXP_OK ||
            length != (size_t)LX_SERVICE_DELIVERABLE_RECORD_BYTES)
            return 1;
    }
    key[LX_SERVICE_KEY_BYTES] = (uint8_t)count;
    return lxp_ctx_kv_get(&ctx, key, sizeof(key), &value, &length) ==
           LXP_ERR_UNKNOWN_FIELD ? 0 : 1;
}

static int availability_check(lxp_module_ctx *ctx,
                              const uint8_t agreement_id[32])
{
    lx_service_agreement agreement;
    lx_service_delivery delivery;
    if (lx_service_agreement_lookup(ctx, agreement_id, &agreement) != LXP_OK)
        return 1;
    (void)memset(&delivery, 0, sizeof(delivery));
    id32(delivery.delivery_id, 99U);
    id32(delivery.activity_id, 98U);
    (void)memcpy(delivery.agreement_id, agreement_id, 32U);
    (void)memcpy(delivery.provider, agreement.provider, 32U);
    delivery.deliverable_count = 1U;
    id32(delivery.deliverables[0].hash, 13U);
    delivery.deliverables[0].artifact_size = 0U;
    id32(delivery.deliverables[0].availability_reference, 20U);
    if (lx_service_deliverable_check(ctx, &agreement, &delivery) !=
        LXP_ERR_DA_MISSING)
        return 1;
    delivery.deliverables[0].artifact_size = 10U;
    (void)memset(delivery.deliverables[0].availability_reference, 0, 32U);
    if (lx_service_deliverable_check(ctx, &agreement, &delivery) !=
        LXP_ERR_DA_MISSING)
        return 1;
    id32(delivery.deliverables[0].availability_reference, 20U);
    delivery.deliverable_count = 0U;
    if (lx_service_deliverable_check(ctx, &agreement, &delivery) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    delivery.deliverable_count = 1U;
    return lx_service_deliverable_check(ctx, &agreement, &delivery) ==
           LXP_OK ? 0 : 1;
}

int main(void)
{
    uint8_t payload[LX_SERVICE_DELIVER_PAYLOAD_MAX_BYTES];
    uint8_t offer_a[32];
    uint8_t offer_b[32];
    uint8_t formed[32];
    uint8_t proposed[32];
    uint8_t escrow_id[32];
    uint8_t terms_hash[32];
    uint8_t commitment_id[32];
    uint8_t task_hash[32];
    uint8_t delivery_one[32];
    uint8_t delivery_two[32];
    uint8_t contested[1];
    item_spec items[3];
    lx_service_delivery delivery;
    lx_service_agreement agreement;
    lxp_authority_resolved provider;
    lxp_authority_resolved buyer;
    lxp_authority_resolved outsider;
    lxp_module_ctx ctx;
    lxp_result outcome = LXP_OK;
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
    id32(delivery_one, 90U);
    id32(delivery_two, 91U);
    contested[0] = 13U;
    items[0].hash_marker = 13U;
    items[0].artifact_size = 3000U;
    items[0].availability_marker = 33U;
    items[1].hash_marker = 13U;
    items[1].artifact_size = 2000U;
    items[1].availability_marker = 22U;
    items[2].hash_marker = 13U;
    items[2].artifact_size = 1000U;
    items[2].availability_marker = 11U;

    if (lxp_state_store_init(&store_state, 0U) != LXP_OK ||
        lxp_kernel_create(&service_kernel, &store_state, &state_journal,
                          &parameter_set, 0U) != LXP_OK ||
        lxp_kernel_register_module(&service_kernel,
                                   lx_service_module_iface()) != LXP_OK)
        return 1;

    length = offer_payload(payload, 10U);
    if (dispatch(LX_SERVICE_OFFER_PUBLISH, payload, length, &provider, 100U,
                 1U, 4U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = offer_payload(payload, 20U);
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
    length = commit_payload(payload, commitment_id, formed, task_hash,
                            escrow_id);
    if (dispatch(LX_SERVICE_COMMIT_TASK, payload, length, &provider, 100U,
                 6U, 9U, &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;

    length = deliver_payload(payload, delivery_one, proposed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    length = deliver_payload(payload, delivery_one, formed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &outsider, 100U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE ||
        dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 1001U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_DELIVERY_DEADLINE_PASSED)
        return 1;
    payload[LX_SERVICE_DELIVER_PAYLOAD_FIXED_BYTES - 1U] = 0U;
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    payload[LX_SERVICE_DELIVER_PAYLOAD_FIXED_BYTES - 1U] =
        (uint8_t)(LX_SERVICE_MAX_DELIVERABLES + 1);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK || outcome != LXP_ERR_LENGTH_LIMIT)
        return 1;
    payload[LX_SERVICE_DELIVER_PAYLOAD_FIXED_BYTES - 1U] = 2U;
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    items[0].hash_marker = 14U;
    length = deliver_payload(payload, delivery_one, formed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_DELIVERABLE_MISMATCH)
        return 1;
    items[0].hash_marker = 13U;
    items[0].artifact_size = 0U;
    length = deliver_payload(payload, delivery_one, formed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 7U,
                 10U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_NON_CANONICAL)
        return 1;
    items[0].artifact_size = 3000U;

    length = deliver_payload(payload, delivery_one, formed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 8U,
                 11U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_DELIVERED, delivery_one, formed,
                 3U, 8U) != 0 ||
        item_records_present(delivery_one, 3U) != 0 ||
        dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 9U,
                 12U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (open_ctx(&ctx, 100U, 10U) != 0 ||
        lx_service_delivery_lookup(&ctx, delivery_one, &delivery) != LXP_OK ||
        delivery.deliverable_count != 3U || delivery.global_sequence != 8U ||
        delivery.activity_id[0] != 11U ||
        delivery.deliverables[0].availability_reference[0] != 11U ||
        delivery.deliverables[1].availability_reference[0] != 22U ||
        delivery.deliverables[2].availability_reference[0] != 33U ||
        delivery.deliverables[0].artifact_size != 1000U ||
        delivery.deliverables[2].artifact_size != 3000U ||
        memcmp(delivery.provider, provider.principal, 32U) != 0 ||
        lx_service_agreement_lookup(&ctx, formed, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_DELIVERED ||
        lx_service_delivery_latest(&ctx, formed, &delivery) != LXP_OK ||
        memcmp(delivery.delivery_id, delivery_one, 32U) != 0 ||
        lx_service_delivery_latest(&ctx, proposed, &delivery) !=
            LXP_ERR_UNKNOWN_FIELD ||
        availability_check(&ctx, formed) != 0)
        return 1;

    length = reject_payload(payload, formed, 7U, contested, 1U);
    if (dispatch(LX_SERVICE_REJECT, payload, length, &buyer, 100U, 11U, 13U,
                 &outcome) != LXP_OK || outcome != LXP_OK)
        return 1;
    length = deliver_payload(payload, delivery_one, formed, items, 3U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 12U,
                 14U, &outcome) != LXP_OK ||
        outcome != LXP_ERR_SEQUENCE_REUSED)
        return 1;
    length = deliver_payload(payload, delivery_two, formed, items, 1U);
    if (dispatch(LX_SERVICE_DELIVER, payload, length, &provider, 100U, 13U,
                 15U, &outcome) != LXP_OK || outcome != LXP_OK ||
        event_is((uint16_t)LX_SERVICE_EVENT_DELIVERED, delivery_two, formed,
                 1U, 13U) != 0 ||
        item_records_present(delivery_two, 1U) != 0)
        return 1;
    if (open_ctx(&ctx, 100U, 14U) != 0 ||
        lx_service_delivery_latest(&ctx, formed, &delivery) != LXP_OK ||
        memcmp(delivery.delivery_id, delivery_two, 32U) != 0 ||
        delivery.deliverable_count != 1U || delivery.global_sequence != 13U ||
        lx_service_agreement_lookup(&ctx, formed, &agreement) != LXP_OK ||
        agreement.state != LX_SERVICE_AGREEMENT_DELIVERED)
        return 1;

    if (lxp_state_store_destroy(&store_state) != LXP_OK) return 1;
    return 0;
}
