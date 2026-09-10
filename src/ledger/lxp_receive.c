#include "layerx/lxp_ledger.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_transfer.h"

#include <string.h>

enum { LXP_RECEIVE_TAG = 0x5201, LXP_RECEIVE_FIELDS = 10 };

typedef struct wire_cursor {
    uint8_t *out;
    const uint8_t *in;
    size_t length;
    size_t offset;
} wire_cursor;

static lxp_result write_value(wire_cursor *wire, const void *value, size_t length)
{
    if (wire == NULL || value == NULL || wire->out == NULL ||
        wire->offset > wire->length || length > wire->length - wire->offset)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(wire->out + wire->offset, value, length);
    wire->offset += length;
    return LXP_OK;
}

static lxp_result read_value(wire_cursor *wire, void *value, size_t length)
{
    if (wire == NULL || value == NULL || wire->in == NULL ||
        wire->offset > wire->length || length > wire->length - wire->offset)
        return LXP_ERR_TRUNCATED;
    (void)memcpy(value, wire->in + wire->offset, length);
    wire->offset += length;
    return LXP_OK;
}

static lxp_result write_u16(wire_cursor *wire, uint16_t value)
{
    uint8_t out[2] = { (uint8_t)(value >> 8U), (uint8_t)value };
    return write_value(wire, out, sizeof(out));
}

static lxp_result write_u32(wire_cursor *wire, uint32_t value)
{
    uint8_t out[4] = { (uint8_t)(value >> 24U), (uint8_t)(value >> 16U),
                       (uint8_t)(value >> 8U), (uint8_t)value };
    return write_value(wire, out, sizeof(out));
}

static lxp_result write_u64(wire_cursor *wire, uint64_t value)
{
    uint8_t out[8];
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
    return write_value(wire, out, sizeof(out));
}

static lxp_result read_u16(wire_cursor *wire, uint16_t *value)
{
    uint8_t in[2];
    lxp_result status = read_value(wire, in, sizeof(in));
    if (status == LXP_OK)
        *value = (uint16_t)(((uint16_t)in[0] << 8U) | in[1]);
    return status;
}

static lxp_result read_u32(wire_cursor *wire, uint32_t *value)
{
    uint8_t in[4];
    lxp_result status = read_value(wire, in, sizeof(in));
    if (status == LXP_OK)
        *value = ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
                 ((uint32_t)in[2] << 8U) | in[3];
    return status;
}

static lxp_result read_u64(wire_cursor *wire, uint64_t *value)
{
    uint8_t in[8];
    size_t i;
    lxp_result status = read_value(wire, in, sizeof(in));
    if (status != LXP_OK) return status;
    *value = 0U;
    for (i = 0U; i < 8U; ++i) *value = (*value << 8U) | in[i];
    return LXP_OK;
}

static lxp_result write_amount(wire_cursor *wire, lxp_u128 amount)
{
    uint8_t encoded[16];
    lxp_result status = lxp_u128_to_be(amount, encoded);
    return status == LXP_OK ? write_value(wire, encoded, sizeof(encoded)) : status;
}

static lxp_result read_amount(wire_cursor *wire, lxp_u128 *amount)
{
    uint8_t encoded[16];
    lxp_result status = read_value(wire, encoded, sizeof(encoded));
    return status == LXP_OK ? lxp_u128_from_be(encoded, amount) : status;
}

static lxp_result write_authorization(wire_cursor *wire,
                                      const lxp_send_authorization *authorization,
                                      bool signature)
{
    lxp_result status = write_value(wire, &authorization->kind, 1U);
#define WRITE_AUTH(value, count) do { \
    if (status == LXP_OK) status = write_value(wire, value, count); \
} while (0)
    WRITE_AUTH(authorization->controller, 32U);
    if (signature) WRITE_AUTH(authorization->public_key, 32U);
    if (signature) WRITE_AUTH(authorization->signature, 64U);
    WRITE_AUTH(authorization->signed_context_hash, 32U);
    if (status == LXP_OK) status = write_u32(wire, authorization->network_id);
    if (status == LXP_OK)
        status = write_u16(wire, authorization->protocol_version);
#undef WRITE_AUTH
    return status;
}

static lxp_result read_authorization(wire_cursor *wire,
                                     lxp_send_authorization *authorization)
{
    lxp_result status = read_value(wire, &authorization->kind, 1U);
#define READ_AUTH(value, count) do { \
    if (status == LXP_OK) status = read_value(wire, value, count); \
} while (0)
    READ_AUTH(authorization->controller, 32U);
    READ_AUTH(authorization->public_key, 32U);
    READ_AUTH(authorization->signature, 64U);
    READ_AUTH(authorization->signed_context_hash, 32U);
    if (status == LXP_OK) status = read_u32(wire, &authorization->network_id);
    if (status == LXP_OK)
        status = read_u16(wire, &authorization->protocol_version);
#undef READ_AUTH
    return status;
}

static lxp_result write_grant(wire_cursor *wire, const lxp_payer_grant *grant)
{
    uint8_t recurring = grant->recurring ? 1U : 0U;
    uint8_t reference = grant->has_reference ? 1U : 0U;
    lxp_result status = write_value(wire, grant->grant_id, 32U);
#define WRITE_GRANT(value, count) do { \
    if (status == LXP_OK) status = write_value(wire, value, count); \
} while (0)
    WRITE_GRANT(grant->from, 32U);
    WRITE_GRANT(grant->recipient, 32U);
    WRITE_GRANT(grant->asset, 32U);
    if (status == LXP_OK) status = write_amount(wire, grant->per_draw_maximum);
    if (status == LXP_OK) status = write_amount(wire, grant->allowance);
    WRITE_GRANT(&recurring, 1U);
    if (status == LXP_OK) status = write_u64(wire, grant->window_length);
    if (status == LXP_OK) status = write_u64(wire, grant->expiration);
    WRITE_GRANT(grant->purpose_hash, 32U);
    WRITE_GRANT(&reference, 1U);
    WRITE_GRANT(grant->reference_hash, 32U);
    if (status == LXP_OK)
        status = write_u64(wire, grant->revocation_sequence);
    WRITE_GRANT(grant->public_key, 32U);
    WRITE_GRANT(grant->signature, 64U);
#undef WRITE_GRANT
    return status;
}

static lxp_result read_grant(wire_cursor *wire, lxp_payer_grant *grant)
{
    uint8_t recurring;
    uint8_t reference;
    lxp_result status = read_value(wire, grant->grant_id, 32U);
#define READ_GRANT(value, count) do { \
    if (status == LXP_OK) status = read_value(wire, value, count); \
} while (0)
    READ_GRANT(grant->from, 32U);
    READ_GRANT(grant->recipient, 32U);
    READ_GRANT(grant->asset, 32U);
    if (status == LXP_OK) status = read_amount(wire, &grant->per_draw_maximum);
    if (status == LXP_OK) status = read_amount(wire, &grant->allowance);
    READ_GRANT(&recurring, 1U);
    if (status == LXP_OK) status = read_u64(wire, &grant->window_length);
    if (status == LXP_OK) status = read_u64(wire, &grant->expiration);
    READ_GRANT(grant->purpose_hash, 32U);
    READ_GRANT(&reference, 1U);
    READ_GRANT(grant->reference_hash, 32U);
    if (status == LXP_OK)
        status = read_u64(wire, &grant->revocation_sequence);
    READ_GRANT(grant->public_key, 32U);
    READ_GRANT(grant->signature, 64U);
#undef READ_GRANT
    if (status != LXP_OK || recurring > 1U || reference > 1U)
        return LXP_ERR_MALFORMED_RECEIVE;
    grant->recurring = recurring != 0U;
    grant->has_reference = reference != 0U;
    return LXP_OK;
}

lxp_result lxp_payer_grant_encode(const lxp_payer_grant *grant,
                                  uint8_t *bytes, size_t capacity, size_t *length)
{
    wire_cursor wire = { bytes, NULL, capacity, 0U };
    lxp_result status;
    if (grant == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = write_grant(&wire, grant);
    if (status == LXP_OK) *length = wire.offset;
    return status;
}

lxp_result lxp_payer_grant_decode(const uint8_t *bytes, size_t length,
                                  lxp_payer_grant *grant)
{
    wire_cursor wire = { NULL, bytes, length, 0U };
    lxp_result status;
    if (bytes == NULL || grant == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(grant, 0, sizeof(*grant));
    status = read_grant(&wire, grant);
    return status == LXP_OK && wire.offset == length ? LXP_OK :
           LXP_ERR_NON_CANONICAL;
}

static lxp_result write_receive_core(wire_cursor *wire,
                                     const lxp_receive *receive)
{
    lxp_result status = write_value(wire, receive->from, 32U);
#define WRITE_CORE(value, count) do { \
    if (status == LXP_OK) status = write_value(wire, value, count); \
} while (0)
    WRITE_CORE(receive->to, 32U);
    WRITE_CORE(receive->asset, 32U);
    if (status == LXP_OK) status = write_amount(wire, receive->amount);
    WRITE_CORE(receive->grant_id, 32U);
    if (status == LXP_OK)
        status = write_u64(wire, receive->receiver_sequence);
    WRITE_CORE(receive->idempotency_key, 32U);
    WRITE_CORE(receive->context_hash, 32U);
#undef WRITE_CORE
    return status;
}

lxp_result lxp_receive_authorization_message(const lxp_receive *receive,
                                             uint8_t *bytes, size_t capacity,
                                             size_t *length)
{
    static const uint8_t tag[] = "LXP:RECEIVE:v1";
    wire_cursor wire = { bytes, NULL, capacity, 0U };
    lxp_result status;
    if (receive == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_MALFORMED_RECEIVE;
    status = write_value(&wire, tag, sizeof(tag) - 1U);
    if (status == LXP_OK) status = write_receive_core(&wire, receive);
    if (status == LXP_OK)
        status = write_authorization(&wire, &receive->receiver_authorization,
                                     false);
    if (status == LXP_OK) *length = wire.offset;
    return status;
}

lxp_result lxp_receive_encode(const lxp_receive *receive, uint8_t *bytes,
                              size_t capacity, size_t *length)
{
    wire_cursor wire = { bytes, NULL, capacity, 0U };
    lxp_result status;
    if (receive == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_MALFORMED_RECEIVE;
    status = write_u16(&wire, LXP_RECEIVE_TAG);
    if (status == LXP_OK) status = write_u16(&wire, LXP_RECEIVE_FIELDS);
    if (status == LXP_OK) status = write_receive_core(&wire, receive);
    if (status == LXP_OK)
        status = write_authorization(&wire, &receive->receiver_authorization,
                                     true);
    if (status == LXP_OK) status = write_grant(&wire, &receive->payer_grant);
    if (status == LXP_OK) *length = wire.offset;
    return status;
}

lxp_result lxp_receive_decode(const uint8_t *bytes, size_t length,
                              lxp_receive *receive)
{
    wire_cursor wire = { NULL, bytes, length, 0U };
    uint16_t tag;
    uint16_t fields;
    lxp_result status;
    if (receive == NULL) return LXP_ERR_MALFORMED_RECEIVE;
    (void)memset(receive, 0, sizeof(*receive));
    status = read_u16(&wire, &tag);
    if (status == LXP_OK) status = read_u16(&wire, &fields);
    if (status != LXP_OK || tag != LXP_RECEIVE_TAG ||
        fields != LXP_RECEIVE_FIELDS) return LXP_ERR_MALFORMED_RECEIVE;
#define READ_CORE(value, count) do { \
    if (status == LXP_OK) status = read_value(&wire, value, count); \
} while (0)
    READ_CORE(receive->from, 32U);
    READ_CORE(receive->to, 32U);
    READ_CORE(receive->asset, 32U);
    if (status == LXP_OK) status = read_amount(&wire, &receive->amount);
    READ_CORE(receive->grant_id, 32U);
    if (status == LXP_OK)
        status = read_u64(&wire, &receive->receiver_sequence);
    READ_CORE(receive->idempotency_key, 32U);
    READ_CORE(receive->context_hash, 32U);
#undef READ_CORE
    if (status == LXP_OK)
        status = read_authorization(&wire, &receive->receiver_authorization);
    if (status == LXP_OK) status = read_grant(&wire, &receive->payer_grant);
    return status == LXP_OK && wire.offset == length ? LXP_OK :
           LXP_ERR_MALFORMED_RECEIVE;
}

static lx_account *find_account(lx_account_registry *registry,
                                const uint8_t id[32])
{
    size_t i;
    for (i = 0U; registry != NULL && i < registry->count; ++i)
        if (memcmp(registry->accounts[i].id, id, 32U) == 0)
            return &registry->accounts[i];
    return NULL;
}

static lxp_grant_state *find_grant(lxp_grant_store *store,
                                   const uint8_t id[32])
{
    size_t i;
    for (i = 0U; store != NULL && i < store->count; ++i)
        if (memcmp(store->grants[i].grant.grant_id, id, 32U) == 0)
            return &store->grants[i];
    return NULL;
}

static lxp_result verify_receiver(const lxp_receive *receive,
                                  const lxp_receive_environment *environment,
                                  lx_account *recipient)
{
    uint8_t message[512];
    size_t message_length;
    lxp_result status;
    const lxp_send_authorization *authorization =
        &receive->receiver_authorization;
    if (authorization->kind < LXP_AUTH_OWNER ||
        authorization->kind > LXP_AUTH_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    if (recipient == NULL || !recipient->has_authority_key ||
        memcmp(authorization->controller, recipient->id, 32U) != 0 ||
        memcmp(authorization->public_key, recipient->authority_key, 32U) != 0 ||
        authorization->network_id != environment->network_id ||
        authorization->protocol_version != environment->protocol_version)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (recipient->next_sequence == UINT64_MAX)
        return LXP_ERR_SEQUENCE_EXHAUSTED;
    if (receive->receiver_sequence != recipient->next_sequence)
        return LXP_ERR_SEQUENCE_MISMATCH;
    if (memcmp(receive->context_hash, authorization->signed_context_hash, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_receive_authorization_message(receive, message, sizeof(message),
                                               &message_length);
    if (status == LXP_OK)
        status = lxp_ed25519_verify(authorization->public_key,
                                    authorization->signature,
                                    LXP_DOMAIN_SIGNATURE_PREIMAGE,
                                    message, message_length);
    return status == LXP_OK ? LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
}

static lxp_result purpose_check(const lxp_receive *receive,
                                const lxp_grant_state *state)
{
    uint8_t preimage[64];
    uint8_t expected[32];
    size_t length = 32U;
    (void)memcpy(preimage, state->grant.purpose_hash, 32U);
    if (state->grant.has_reference) {
        (void)memcpy(preimage + 32U, state->grant.reference_hash, 32U);
        length = 64U;
        if (state->invoice_settled) return LXP_ERR_INVOICE_ALREADY_SETTLED;
    }
    if (lxp_hash_context_value(preimage, length, expected) != LXP_OK ||
        memcmp(expected, receive->context_hash, 32U) != 0)
        return LXP_ERR_PURPOSE_MISMATCH;
    return LXP_OK;
}

lxp_result lxp_receive_execute(const lxp_receive *receive,
                               lxp_receive_environment *environment,
                               lxp_send_receipt_projection *receipt)
{
    uint8_t encoded[1024];
    uint8_t activity_hash[32];
    size_t encoded_length;
    lxp_send_store_record record;
    lx_account *from;
    lx_account *to;
    lxp_grant_state *state;
    lxp_grant_state original_state;
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_set_result set_result;
    lxp_result status;
    if (receive == NULL || environment == NULL || environment->accounts == NULL ||
        environment->grants == NULL || environment->idempotency == NULL ||
        receipt == NULL) return LXP_ERR_MALFORMED_RECEIVE;
    status = lxp_receive_encode(receive, encoded, sizeof(encoded), &encoded_length);
    if (status == LXP_OK)
        status = lxp_hash_activity_id(encoded, encoded_length, activity_hash);
    if (status != LXP_OK) return status;
    status = lxp_send_store_lookup(environment->idempotency,
                                   receive->idempotency_key, activity_hash,
                                   receipt);
    if (status != LXP_OK) return status;
    status = lxp_send_store_admit(environment->idempotency);
    if (status != LXP_OK) return status;
    from = find_account(environment->accounts, receive->from);
    to = find_account(environment->accounts, receive->to);
    state = find_grant(environment->grants, receive->grant_id);
    if (state == NULL) return LXP_ERR_NO_PAYER_GRANT;
    if (from == NULL || to == NULL) return LXP_ERR_GRANT_SCOPE_VIOLATION;
    status = lxp_verify_payer_grant(&state->grant, from);
    if (status != LXP_OK) return status;
    if (state->revoked && environment->global_sequence >=
                          state->revoked_at_sequence) return LXP_ERR_GRANT_REVOKED;
    if (environment->batch_timestamp > state->grant.expiration)
        return LXP_ERR_GRANT_EXPIRED;
    if (memcmp(receive->payer_grant.grant_id, state->grant.grant_id, 32U) != 0 ||
        memcmp(receive->from, state->grant.from, 32U) != 0 ||
        memcmp(receive->to, state->grant.recipient, 32U) != 0 ||
        memcmp(receive->asset, state->grant.asset, 32U) != 0 ||
        lxp_u128_cmp(receive->amount, state->grant.per_draw_maximum) > 0)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    status = purpose_check(receive, state);
    if (status != LXP_OK) return status;
    status = verify_receiver(receive, environment, to);
    if (status != LXP_OK) return status;
    original_state = *state;
    status = lxp_grant_draw_record(state, receive->amount,
                                   environment->batch_timestamp);
    if (status != LXP_OK) return status;
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = from;
    leg.to = to;
    (void)memcpy(leg.asset_id, receive->asset, 32U);
    leg.amount = receive->amount;
    leg.reason = LXP_REASON_PAYMENT;
    (void)memset(&context, 0, sizeof(context));
    context.assets = environment->assets;
    context.asset_count = environment->asset_count;
    (void)memcpy(context.authorized_from, receive->from, 32U);
    context.actor_sequence = receive->receiver_sequence;
    context.sequence_account = to;
    context.batch_timestamp = environment->batch_timestamp;
    context.debit_authority_kind =
        (lxp_authorization_kind)receive->receiver_authorization.kind;
    status = lxp_apply_transfer_set(&leg, 1U, &context, &set_result);
    if (status != LXP_OK) { *state = original_state; return status; }
    if (state->grant.has_reference) state->invoice_settled = true;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->from_before = set_result.legs[0].from_balance_before;
    receipt->from_after = set_result.legs[0].from_balance_after;
    receipt->to_before = set_result.legs[0].to_balance_before;
    receipt->to_after = set_result.legs[0].to_balance_after;
    (void)memcpy(receipt->transfer_set_root, set_result.transfer_set_root, 32U);
    (void)memset(&record, 0, sizeof(record));
    (void)memcpy(record.activity_hash, activity_hash, 32U);
    (void)memcpy(record.idempotency_key, receive->idempotency_key, 32U);
    record.receipt = *receipt;
    return lxp_send_store_append(environment->idempotency, &record);
}
