#include "layerx/lxp_batch.h"
#include "layerx/lxp_hash.h"

#include <string.h>

enum { LXP_BATCH_HEADER_STRUCTURE_TAG = 0x1701,
       LXP_BATCH_HEADER_FIELD_COUNT = 15 };

static lxp_result write_field(lxp_codec_writer *writer, uint8_t id)
{
    return lxp_codec_write_tag(writer, id, LXP_BATCH_HEADER_FIELD_COUNT);
}

static lxp_result write_hash(lxp_codec_writer *writer, const uint8_t hash[32])
{
    return lxp_codec_write_bytes(writer, hash, 32U, 32U);
}

lxp_result lxp_batch_header_encode(const lxp_batch_header *header,
                                   lxp_arena *arena,
                                   lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    lxp_result status;
    if (header == NULL || arena == NULL || encoded == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_codec_writer_init(&writer, arena,
                                   LXP_BATCH_HEADER_ENCODED_SIZE);
    if (status != LXP_OK) return status;
#define WRITE(id, expression) do { \
    status = write_field(&writer, (id)); \
    if (status == LXP_OK) status = (expression); \
    if (status != LXP_OK) return status; \
} while (0)
    status = lxp_codec_write_struct_header_version(
        &writer, LXP_BATCH_HEADER_STRUCTURE_TAG, header->protocol_version);
    if (status != LXP_OK) return status;
    status = lxp_codec_write_u8(&writer, LXP_BATCH_HEADER_FIELD_COUNT);
    if (status != LXP_OK) return status;
    WRITE(1U, lxp_codec_write_u16(&writer, header->protocol_version));
    WRITE(2U, lxp_codec_write_u32(&writer, header->network_id));
    WRITE(3U, lxp_codec_write_u64(&writer, header->epoch));
    WRITE(4U, lxp_codec_write_u64(&writer, header->batch_number));
    WRITE(5U, lxp_codec_write_u64(&writer, header->first_sequence));
    WRITE(6U, lxp_codec_write_u64(&writer, header->last_sequence));
    WRITE(7U, write_hash(&writer, header->previous_state_root));
    WRITE(8U, write_hash(&writer, header->resulting_state_root));
    WRITE(9U, write_hash(&writer, header->activity_merkle_root));
    WRITE(10U, write_hash(&writer, header->receipt_merkle_root));
    WRITE(11U, write_hash(&writer, header->event_merkle_root));
    WRITE(12U, write_hash(&writer, header->data_availability_root));
    WRITE(13U, write_hash(&writer, header->oracle_root));
    WRITE(14U, lxp_codec_write_u64(&writer, header->timestamp_ms));
    WRITE(15U, write_hash(&writer, header->sequencer_id));
#undef WRITE
    if (writer.length != LXP_BATCH_HEADER_ENCODED_SIZE)
        return LXP_FATAL_INVARIANT;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

static lxp_result expect_field(lxp_codec_reader *reader, uint8_t expected)
{
    uint8_t actual;
    lxp_result status = lxp_codec_read_tag(reader,
                                           LXP_BATCH_HEADER_FIELD_COUNT,
                                           &actual);
    if (status == LXP_ERR_INVALID_TAG) return LXP_ERR_UNKNOWN_FIELD;
    if (status != LXP_OK) return status;
    return actual == expected ? LXP_OK : LXP_ERR_UNKNOWN_FIELD;
}

static lxp_result read_hash(lxp_codec_reader *reader, uint8_t hash[32])
{
    lxp_byte_span span;
    lxp_result status = lxp_codec_read_bytes(reader, &span, 32U);
    if (status != LXP_OK) return status;
    if (span.length != 32U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(hash, span.bytes, 32U);
    return LXP_OK;
}

lxp_result lxp_batch_header_decode(const uint8_t *bytes, size_t length,
                                   lxp_batch_header *header)
{
    lxp_codec_reader reader;
    uint8_t count;
    uint16_t envelope_version;
    lxp_result status;
    if (header == NULL || (bytes == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    if (length > LXP_BATCH_HEADER_ENCODED_SIZE)
        return LXP_ERR_TRAILING_BYTES;
    (void)memset(header, 0, sizeof(*header));
    status = lxp_codec_reader_init(&reader, bytes, length);
    if (status != LXP_OK) return status;
    status = lxp_codec_read_struct_header_version(
        &reader, LXP_BATCH_HEADER_STRUCTURE_TAG, &envelope_version);
    if (status != LXP_OK) return status;
    status = lxp_codec_read_u8(&reader, &count);
    if (status != LXP_OK) return status;
    if (count != LXP_BATCH_HEADER_FIELD_COUNT) return LXP_ERR_NON_CANONICAL;
#define READ(id, expression) do { \
    status = expect_field(&reader, (id)); \
    if (status == LXP_OK) status = (expression); \
    if (status != LXP_OK) return status; \
} while (0)
    READ(1U, lxp_codec_read_u16(&reader, &header->protocol_version));
    if (header->protocol_version != envelope_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    READ(2U, lxp_codec_read_u32(&reader, &header->network_id));
    READ(3U, lxp_codec_read_u64(&reader, &header->epoch));
    READ(4U, lxp_codec_read_u64(&reader, &header->batch_number));
    READ(5U, lxp_codec_read_u64(&reader, &header->first_sequence));
    READ(6U, lxp_codec_read_u64(&reader, &header->last_sequence));
    READ(7U, read_hash(&reader, header->previous_state_root));
    READ(8U, read_hash(&reader, header->resulting_state_root));
    READ(9U, read_hash(&reader, header->activity_merkle_root));
    READ(10U, read_hash(&reader, header->receipt_merkle_root));
    READ(11U, read_hash(&reader, header->event_merkle_root));
    READ(12U, read_hash(&reader, header->data_availability_root));
    READ(13U, read_hash(&reader, header->oracle_root));
    READ(14U, lxp_codec_read_u64(&reader, &header->timestamp_ms));
    READ(15U, read_hash(&reader, header->sequencer_id));
#undef READ
    return lxp_codec_finish(&reader);
}

lxp_result lxp_batch_header_hash(const lxp_batch_header *header,
                                 lxp_arena *arena, uint8_t digest[32])
{
    lxp_byte_span encoded;
    lxp_result status;
    if (digest == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_batch_header_encode(header, arena, &encoded);
    if (status != LXP_OK) return status;
    return lxp_hash_domain(LXP_DOMAIN_BATCH_HEADER, encoded.bytes,
                           encoded.length, digest);
}
