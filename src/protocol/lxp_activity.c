#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"

#include <stdlib.h>
#include <string.h>

enum { LXP_ACTIVITY_STRUCTURE_TAG = 0x1001, LXP_ACTIVITY_FIELD_COUNT = 12 };

uint16_t lxp_activity_module_id(uint32_t activity_type)
{
    return (uint16_t)(activity_type >> 16U);
}

uint16_t lxp_activity_type_ordinal(uint32_t activity_type)
{
    return (uint16_t)activity_type;
}

static lxp_result field(lxp_codec_writer *writer, uint8_t field_id)
{
    return lxp_codec_write_tag(writer, field_id, LXP_ACTIVITY_FIELD_COUNT);
}

static lxp_result encode_internal(const lxp_activity *activity, lxp_arena *arena,
                                  lxp_byte_span *encoded, int include_signature)
{
    lxp_codec_writer writer;
    lxp_result status;
    if (activity == NULL || arena == NULL || encoded == NULL ||
        activity->actor_did.length > LXP_MAX_DID_LENGTH ||
        activity->authority.length > LXP_MAX_PAYLOAD_BYTES ||
        activity->payload.length > LXP_MAX_PAYLOAD_BYTES ||
        (include_signature != 0 && activity->signature.length > 128U))
        return LXP_ERR_MALFORMED_ENVELOPE;
    status = lxp_codec_writer_init(&writer, arena, LXP_MAX_ACTIVITY_BYTES);
    if (status != LXP_OK) return status;
#define WRITE_FIELD(id, expression) do { \
    status = field(&writer, (id)); \
    if (status == LXP_OK) status = (expression); \
    if (status != LXP_OK) return status; \
} while (0)
    status = lxp_codec_write_struct_header_version(
        &writer, LXP_ACTIVITY_STRUCTURE_TAG, activity->protocol_version);
    if (status != LXP_OK) return status;
    status = lxp_codec_write_u8(&writer, include_signature != 0 ?
                                LXP_ACTIVITY_FIELD_COUNT : 11U);
    if (status != LXP_OK) return status;
    WRITE_FIELD(1U, lxp_codec_write_u16(&writer, activity->protocol_version));
    WRITE_FIELD(2U, lxp_codec_write_u32(&writer, activity->network_id));
    WRITE_FIELD(3U, lxp_codec_write_u32(&writer, activity->activity_type));
    WRITE_FIELD(4U, lxp_codec_write_bytes(&writer, activity->actor_did.bytes,
                activity->actor_did.length, LXP_MAX_DID_LENGTH));
    WRITE_FIELD(5U, lxp_codec_write_bytes(&writer, activity->authority.bytes,
                activity->authority.length, LXP_MAX_PAYLOAD_BYTES));
    WRITE_FIELD(6U, lxp_codec_write_u64(&writer, activity->account_sequence));
    WRITE_FIELD(7U, lxp_codec_write_u64(&writer,
                activity->timestamp_bound.not_before));
    status = lxp_codec_write_u64(&writer, activity->timestamp_bound.not_after);
    if (status != LXP_OK) return status;
    WRITE_FIELD(8U, lxp_codec_write_bytes(&writer, activity->idempotency_key,
                sizeof(activity->idempotency_key),
                (uint32_t)sizeof(activity->idempotency_key)));
    WRITE_FIELD(9U, lxp_codec_write_u128(&writer, activity->fee_limit));
    WRITE_FIELD(10U, lxp_codec_write_bytes(&writer, activity->payload_hash,
                sizeof(activity->payload_hash),
                (uint32_t)sizeof(activity->payload_hash)));
    WRITE_FIELD(11U, lxp_codec_write_bytes(&writer, activity->payload.bytes,
                activity->payload.length, LXP_MAX_PAYLOAD_BYTES));
    if (include_signature != 0) {
        WRITE_FIELD(12U, lxp_codec_write_bytes(&writer,
                    activity->signature.bytes, activity->signature.length,
                    128U));
    }
#undef WRITE_FIELD
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_activity_encode(const lxp_activity *activity, lxp_arena *arena,
                               lxp_byte_span *encoded)
{
    return encode_internal(activity, arena, encoded, 1);
}

static lxp_result expect_field(lxp_codec_reader *reader, uint8_t expected)
{
    uint8_t actual;
    lxp_result status = lxp_codec_read_tag(reader, LXP_ACTIVITY_FIELD_COUNT,
                                           &actual);
    if (status != LXP_OK || actual != expected) return LXP_ERR_MALFORMED_ENVELOPE;
    return LXP_OK;
}

lxp_result lxp_activity_decode(const uint8_t *bytes, size_t length,
                               lxp_activity *activity)
{
    lxp_codec_reader reader;
    lxp_activity decoded;
    lxp_byte_span span;
    uint16_t envelope_version;
    uint8_t count;
    lxp_result status;
    if (activity == NULL) return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memset(activity, 0, sizeof(*activity));
    (void)memset(&decoded, 0, sizeof(decoded));
    if ((bytes == NULL && length != 0U) || length > LXP_MAX_ACTIVITY_BYTES)
        return LXP_ERR_MALFORMED_ENVELOPE;
    status = lxp_codec_reader_init(&reader, bytes, length);
    if (status != LXP_OK ||
        lxp_codec_read_struct_header_version(
            &reader, LXP_ACTIVITY_STRUCTURE_TAG, &envelope_version) != LXP_OK ||
        lxp_codec_read_u8(&reader, &count) != LXP_OK ||
        count != LXP_ACTIVITY_FIELD_COUNT) return LXP_ERR_MALFORMED_ENVELOPE;
#define READ_FIELD(id, expression) do { \
    if (expect_field(&reader, (id)) != LXP_OK || (expression) != LXP_OK) \
        return LXP_ERR_MALFORMED_ENVELOPE; \
} while (0)
    READ_FIELD(1U, lxp_codec_read_u16(&reader, &decoded.protocol_version));
    if (decoded.protocol_version != envelope_version)
        return LXP_ERR_MALFORMED_ENVELOPE;
    READ_FIELD(2U, lxp_codec_read_u32(&reader, &decoded.network_id));
    READ_FIELD(3U, lxp_codec_read_u32(&reader, &decoded.activity_type));
    READ_FIELD(4U, lxp_codec_read_bytes(&reader, &decoded.actor_did,
                                       LXP_MAX_DID_LENGTH));
    READ_FIELD(5U, lxp_codec_read_bytes(&reader, &decoded.authority,
                                       LXP_MAX_PAYLOAD_BYTES));
    READ_FIELD(6U, lxp_codec_read_u64(&reader, &decoded.account_sequence));
    READ_FIELD(7U, lxp_codec_read_u64(&reader,
                                     &decoded.timestamp_bound.not_before));
    if (lxp_codec_read_u64(&reader, &decoded.timestamp_bound.not_after) != LXP_OK)
        return LXP_ERR_MALFORMED_ENVELOPE;
    READ_FIELD(8U, lxp_codec_read_bytes(&reader, &span, 32U));
    if (span.length != 32U) return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memcpy(decoded.idempotency_key, span.bytes, 32U);
    READ_FIELD(9U, lxp_codec_read_u128(&reader, &decoded.fee_limit));
    READ_FIELD(10U, lxp_codec_read_bytes(&reader, &span, 32U));
    if (span.length != 32U) return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memcpy(decoded.payload_hash, span.bytes, 32U);
    READ_FIELD(11U, lxp_codec_read_bytes(&reader, &decoded.payload,
                                        LXP_MAX_PAYLOAD_BYTES));
    READ_FIELD(12U, lxp_codec_read_bytes(&reader, &decoded.signature, 128U));
#undef READ_FIELD
    if (lxp_codec_finish(&reader) != LXP_OK)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *activity = decoded;
    return LXP_OK;
}

lxp_result lxp_activity_id(const uint8_t *canonical_activity, size_t length,
                           uint8_t identifier[32])
{
    if (length > LXP_MAX_ACTIVITY_BYTES) return LXP_ERR_LENGTH_LIMIT;
    return lxp_hash_activity_id(canonical_activity, length, identifier);
}

lxp_result lxp_activity_verify_payload_hash(const lxp_activity *activity)
{
    uint8_t computed[32];
    lxp_result status;
    if (activity == NULL) return LXP_ERR_MALFORMED_ENVELOPE;
    status = lxp_hash_payload(activity->payload.bytes, activity->payload.length,
                              computed);
    if (status != LXP_OK) return status;
    return lxp_ct_memcmp(activity->payload_hash, computed, sizeof(computed)) == 0 ?
           LXP_OK : LXP_ERR_PAYLOAD_HASH_MISMATCH;
}

lxp_result lxp_activity_check_envelope(const lxp_activity *activity,
                                       uint32_t executing_network_id)
{
    if (activity == NULL) return LXP_ERR_MALFORMED_ENVELOPE;
    if (!lxp_protocol_version_supported(activity->protocol_version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (!lxp_network_id_matches(executing_network_id, activity->network_id))
        return LXP_ERR_WRONG_NETWORK;
    return lxp_activity_verify_payload_hash(activity);
}

lxp_result lxp_activity_signing_preimage(const lxp_activity *activity,
                                         uint8_t preimage_hash[32])
{
    uint8_t *storage;
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_result status;
    if (activity == NULL || preimage_hash == NULL)
        return LXP_ERR_MALFORMED_ENVELOPE;
    storage = malloc(LXP_MAX_ACTIVITY_BYTES);
    if (storage == NULL) return LXP_ERR_IO;
    status = lxp_arena_init(&arena, storage, LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK)
        status = encode_internal(activity, &arena, &encoded, 0);
    if (status == LXP_OK)
        status = lxp_hash_signature_preimage(encoded.bytes, encoded.length,
                                             preimage_hash);
    lxp_secure_zero(storage, LXP_MAX_ACTIVITY_BYTES);
    free(storage);
    return status;
}
