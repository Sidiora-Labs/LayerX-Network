#include "layerx/lxp_ledger.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_transfer.h"

#include <string.h>

enum { LXP_SEND_WIRE_TAG = 0x5301, LXP_SEND_FIELD_COUNT = 10 };

static lxp_result put(uint8_t *bytes, size_t capacity, size_t *cursor,
                      const void *value, size_t length)
{
    if (bytes == NULL || cursor == NULL || value == NULL ||
        *cursor > capacity || length > capacity - *cursor)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(bytes + *cursor, value, length);
    *cursor += length;
    return LXP_OK;
}

static lxp_result take(const uint8_t *bytes, size_t length, size_t *cursor,
                       void *value, size_t count)
{
    if (bytes == NULL || cursor == NULL || value == NULL ||
        *cursor > length || count > length - *cursor) return LXP_ERR_TRUNCATED;
    (void)memcpy(value, bytes + *cursor, count);
    *cursor += count;
    return LXP_OK;
}

static lxp_result put_u16(uint8_t *bytes, size_t capacity, size_t *cursor,
                          uint16_t value)
{
    uint8_t encoded[2] = { (uint8_t)(value >> 8U), (uint8_t)value };
    return put(bytes, capacity, cursor, encoded, sizeof(encoded));
}

static lxp_result put_u32(uint8_t *bytes, size_t capacity, size_t *cursor,
                          uint32_t value)
{
    uint8_t encoded[4] = { (uint8_t)(value >> 24U), (uint8_t)(value >> 16U),
                           (uint8_t)(value >> 8U), (uint8_t)value };
    return put(bytes, capacity, cursor, encoded, sizeof(encoded));
}

static lxp_result put_u64(uint8_t *bytes, size_t capacity, size_t *cursor,
                          uint64_t value)
{
    uint8_t encoded[8];
    size_t i;
    for (i = 0U; i < 8U; ++i) encoded[7U - i] = (uint8_t)(value >> (i * 8U));
    return put(bytes, capacity, cursor, encoded, sizeof(encoded));
}

static lxp_result take_u16(const uint8_t *bytes, size_t length, size_t *cursor,
                           uint16_t *value)
{
    uint8_t encoded[2];
    lxp_result status = take(bytes, length, cursor, encoded, sizeof(encoded));
    if (status == LXP_OK)
        *value = (uint16_t)(((uint16_t)encoded[0] << 8U) | encoded[1]);
    return status;
}

static lxp_result take_u32(const uint8_t *bytes, size_t length, size_t *cursor,
                           uint32_t *value)
{
    uint8_t encoded[4];
    lxp_result status = take(bytes, length, cursor, encoded, sizeof(encoded));
    if (status == LXP_OK)
        *value = ((uint32_t)encoded[0] << 24U) |
                 ((uint32_t)encoded[1] << 16U) |
                 ((uint32_t)encoded[2] << 8U) | encoded[3];
    return status;
}

static lxp_result take_u64(const uint8_t *bytes, size_t length, size_t *cursor,
                           uint64_t *value)
{
    uint8_t encoded[8];
    size_t i;
    lxp_result status = take(bytes, length, cursor, encoded, sizeof(encoded));
    if (status != LXP_OK) return status;
    *value = 0U;
    for (i = 0U; i < 8U; ++i) *value = (*value << 8U) | encoded[i];
    return LXP_OK;
}

static lxp_result encode_common(const lxp_send *send, uint8_t *bytes,
                                size_t capacity, size_t *cursor)
{
    uint8_t amount[16];
    size_t i;
    lxp_result status = lxp_u128_to_be(send->amount, amount);
#define PUT_VALUE(value, length) do { \
    if (status == LXP_OK) status = put(bytes, capacity, cursor, value, length); \
} while (0)
    PUT_VALUE(send->from, 32U);
    PUT_VALUE(send->to, 32U);
    PUT_VALUE(send->asset, 32U);
    PUT_VALUE(amount, sizeof(amount));
    if (status == LXP_OK) status = put_u64(bytes, capacity, cursor, send->sequence);
    PUT_VALUE(send->idempotency_key, 32U);
    if (status == LXP_OK) status = put_u64(bytes, capacity, cursor, send->expires_at);
    PUT_VALUE(send->context_hash, 32U);
    if (send->condition_count > LXP_SEND_MAX_CONDITIONS) return LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) {
        uint8_t count = (uint8_t)send->condition_count;
        status = put(bytes, capacity, cursor, &count, 1U);
    }
    for (i = 0U; status == LXP_OK && i < send->condition_count; ++i) {
        status = put(bytes, capacity, cursor, &send->conditions[i].kind, 1U);
        if (status == LXP_OK)
            status = put_u64(bytes, capacity, cursor,
                             send->conditions[i].timestamp);
    }
#undef PUT_VALUE
    return status;
}

lxp_result lxp_send_encode(const lxp_send *send, uint8_t *bytes,
                           size_t capacity, size_t *length)
{
    size_t cursor = 0U;
    lxp_result status;
    if (send == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = put_u16(bytes, capacity, &cursor, LXP_SEND_WIRE_TAG);
    if (status == LXP_OK)
        status = put_u16(bytes, capacity, &cursor, LXP_SEND_FIELD_COUNT);
    if (status == LXP_OK) status = encode_common(send, bytes, capacity, &cursor);
#define PUT_AUTH(value, count) do { \
    if (status == LXP_OK) status = put(bytes, capacity, &cursor, value, count); \
} while (0)
    PUT_AUTH(&send->authorization.kind, 1U);
    PUT_AUTH(send->authorization.controller, 32U);
    PUT_AUTH(send->authorization.public_key, 32U);
    PUT_AUTH(send->authorization.signature, 64U);
    PUT_AUTH(send->authorization.signed_context_hash, 32U);
    if (status == LXP_OK)
        status = put_u32(bytes, capacity, &cursor,
                         send->authorization.network_id);
    if (status == LXP_OK)
        status = put_u16(bytes, capacity, &cursor,
                         send->authorization.protocol_version);
#undef PUT_AUTH
    if (status == LXP_OK) *length = cursor;
    return status;
}

lxp_result lxp_send_authorization_message(const lxp_send *send, uint8_t *bytes,
                                          size_t capacity, size_t *length)
{
    size_t cursor = 0U;
    lxp_result status;
    if (send == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = put_u16(bytes, capacity, &cursor, LXP_SEND_WIRE_TAG);
    if (status == LXP_OK) status = encode_common(send, bytes, capacity, &cursor);
    if (status == LXP_OK)
        status = put(bytes, capacity, &cursor, &send->authorization.kind, 1U);
    if (status == LXP_OK)
        status = put(bytes, capacity, &cursor,
                     send->authorization.controller, 32U);
    if (status == LXP_OK)
        status = put(bytes, capacity, &cursor,
                     send->authorization.signed_context_hash, 32U);
    if (status == LXP_OK)
        status = put_u32(bytes, capacity, &cursor,
                         send->authorization.network_id);
    if (status == LXP_OK)
        status = put_u16(bytes, capacity, &cursor,
                         send->authorization.protocol_version);
    if (status == LXP_OK) *length = cursor;
    return status;
}

lxp_result lxp_send_decode(const uint8_t *bytes, size_t length, lxp_send *send)
{
    uint16_t tag;
    uint16_t fields;
    uint8_t amount[16];
    uint8_t count;
    size_t cursor = 0U;
    size_t i;
    lxp_result status;
    if (send == NULL) return LXP_ERR_MALFORMED_SEND;
    (void)memset(send, 0, sizeof(*send));
    status = take_u16(bytes, length, &cursor, &tag);
    if (status == LXP_OK) status = take_u16(bytes, length, &cursor, &fields);
    if (status != LXP_OK || tag != LXP_SEND_WIRE_TAG ||
        fields != LXP_SEND_FIELD_COUNT) return LXP_ERR_MALFORMED_SEND;
#define TAKE_VALUE(value, count_value) do { \
    if (status == LXP_OK) status = take(bytes, length, &cursor, value, count_value); \
} while (0)
    TAKE_VALUE(send->from, 32U);
    TAKE_VALUE(send->to, 32U);
    TAKE_VALUE(send->asset, 32U);
    TAKE_VALUE(amount, sizeof(amount));
    if (status == LXP_OK) status = lxp_u128_from_be(amount, &send->amount);
    if (status == LXP_OK) status = take_u64(bytes, length, &cursor, &send->sequence);
    TAKE_VALUE(send->idempotency_key, 32U);
    if (status == LXP_OK) status = take_u64(bytes, length, &cursor, &send->expires_at);
    TAKE_VALUE(send->context_hash, 32U);
    TAKE_VALUE(&count, 1U);
    if (status != LXP_OK || count > LXP_SEND_MAX_CONDITIONS)
        return LXP_ERR_MALFORMED_SEND;
    send->condition_count = count;
    for (i = 0U; status == LXP_OK && i < send->condition_count; ++i) {
        TAKE_VALUE(&send->conditions[i].kind, 1U);
        if (status == LXP_OK)
            status = take_u64(bytes, length, &cursor,
                              &send->conditions[i].timestamp);
    }
    TAKE_VALUE(&send->authorization.kind, 1U);
    TAKE_VALUE(send->authorization.controller, 32U);
    TAKE_VALUE(send->authorization.public_key, 32U);
    TAKE_VALUE(send->authorization.signature, 64U);
    TAKE_VALUE(send->authorization.signed_context_hash, 32U);
    if (status == LXP_OK)
        status = take_u32(bytes, length, &cursor,
                          &send->authorization.network_id);
    if (status == LXP_OK)
        status = take_u16(bytes, length, &cursor,
                          &send->authorization.protocol_version);
#undef TAKE_VALUE
    return status == LXP_OK && cursor == length ? LXP_OK : LXP_ERR_MALFORMED_SEND;
}

static lx_account *account_by_id(lx_account_registry *registry,
                                 const uint8_t id[32])
{
    size_t i;
    if (registry == NULL) return NULL;
    for (i = 0U; i < registry->count; ++i)
        if (memcmp(registry->accounts[i].id, id, 32U) == 0)
            return &registry->accounts[i];
    return NULL;
}

lxp_result lxp_send_validate(const lxp_send *send,
                             const lxp_send_environment *environment)
{
    lx_account *from;
    uint8_t message[512];
    size_t message_length;
    size_t i;
    lxp_result status;
    if (send == NULL || environment == NULL || environment->accounts == NULL)
        return LXP_ERR_MALFORMED_SEND;
    if (send->authorization.kind < LXP_AUTH_OWNER ||
        send->authorization.kind > LXP_AUTH_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    from = account_by_id(environment->accounts, send->from);
    if (from == NULL || memcmp(send->authorization.controller, send->from, 32U) != 0 ||
        !from->has_authority_key ||
        memcmp(from->authority_key, send->authorization.public_key, 32U) != 0 ||
        send->authorization.network_id != environment->network_id ||
        send->authorization.protocol_version != environment->protocol_version)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (from->next_sequence == UINT64_MAX) return LXP_ERR_SEQUENCE_EXHAUSTED;
    if (send->sequence != from->next_sequence) return LXP_ERR_SEQUENCE_MISMATCH;
    if (environment->batch_timestamp > send->expires_at) return LXP_ERR_EXPIRED;
    for (i = 0U; i < send->condition_count; ++i) {
        if ((send->conditions[i].kind == LXP_CONDITION_NOT_BEFORE &&
             environment->batch_timestamp < send->conditions[i].timestamp) ||
            (send->conditions[i].kind == LXP_CONDITION_NOT_AFTER &&
             environment->batch_timestamp > send->conditions[i].timestamp))
            return LXP_ERR_CONDITION_UNMET;
        if (send->conditions[i].kind != LXP_CONDITION_NOT_BEFORE &&
            send->conditions[i].kind != LXP_CONDITION_NOT_AFTER)
            return LXP_ERR_CONDITION_UNMET;
    }
    if (memcmp(send->context_hash, send->authorization.signed_context_hash,
               32U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_send_authorization_message(send, message, sizeof(message),
                                            &message_length);
    if (status != LXP_OK) return status;
    status = lxp_ed25519_verify(send->authorization.public_key,
                                send->authorization.signature,
                                LXP_DOMAIN_SIGNATURE_PREIMAGE,
                                message, message_length);
    return status == LXP_OK ? LXP_OK : LXP_ERR_UNAUTHORIZED_DEBIT;
}

lxp_result lxp_send_build_transfer_set(const lxp_send *send,
                                       lx_account_registry *registry,
                                       lxp_transfer_leg *leg)
{
    if (send == NULL || registry == NULL || leg == NULL)
        return LXP_ERR_MALFORMED_SEND;
    (void)memset(leg, 0, sizeof(*leg));
    leg->from = account_by_id(registry, send->from);
    leg->to = account_by_id(registry, send->to);
    if (leg->from == NULL || leg->to == NULL)
        return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    (void)memcpy(leg->asset_id, send->asset, 32U);
    leg->amount = send->amount;
    leg->reason = LXP_REASON_PAYMENT;
    return LXP_OK;
}

lxp_result lxp_send_execute(const lxp_send *send,
                            lxp_send_environment *environment,
                            lxp_send_receipt_projection *receipt)
{
    uint8_t encoded[512];
    size_t encoded_length;
    uint8_t activity_hash[32];
    lxp_send_store_record record;
    lxp_transfer_leg leg;
    lxp_transfer_context context;
    lxp_transfer_set_result set_result;
    lxp_result status;
    if (send == NULL || environment == NULL || environment->store == NULL ||
        receipt == NULL) return LXP_ERR_MALFORMED_SEND;
    status = lxp_send_encode(send, encoded, sizeof(encoded), &encoded_length);
    if (status == LXP_OK)
        status = lxp_hash_activity_id(encoded, encoded_length, activity_hash);
    if (status != LXP_OK) return status;
    status = lxp_send_store_lookup(environment->store, send->idempotency_key,
                                   activity_hash, receipt);
    if (status != LXP_OK) return status;
    status = lxp_send_store_admit(environment->store);
    if (status != LXP_OK) return status;
    status = lxp_send_validate(send, environment);
    if (status != LXP_OK) return status;
    status = lxp_send_build_transfer_set(send, environment->accounts, &leg);
    if (status != LXP_OK) return status;
    (void)memset(&context, 0, sizeof(context));
    context.assets = environment->assets;
    context.asset_count = environment->asset_count;
    (void)memcpy(context.authorized_from, send->from, 32U);
    context.actor_sequence = send->sequence;
    context.sequence_account = leg.from;
    context.batch_timestamp = environment->batch_timestamp;
    context.expires_at = send->expires_at;
    context.protocol_system_capability =
        send->authorization.kind == LXP_AUTH_PROTOCOL_MODULE;
    context.debit_authority_kind =
        (lxp_authorization_kind)send->authorization.kind;
    context.allowance = environment->allowance;
    status = lxp_apply_transfer_set(&leg, 1U, &context, &set_result);
    if (status != LXP_OK) return status;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->from_before = set_result.legs[0].from_balance_before;
    receipt->from_after = set_result.legs[0].from_balance_after;
    receipt->to_before = set_result.legs[0].to_balance_before;
    receipt->to_after = set_result.legs[0].to_balance_after;
    (void)memcpy(receipt->transfer_set_root, set_result.transfer_set_root, 32U);
    (void)memset(&record, 0, sizeof(record));
    (void)memcpy(record.activity_hash, activity_hash, 32U);
    (void)memcpy(record.idempotency_key, send->idempotency_key, 32U);
    record.receipt = *receipt;
    return lxp_send_store_append(environment->store, &record);
}
