#include "layerx/lxp_codec.h"
#include "layerx/lxp_protocol.h"

#include <string.h>

static lxp_result write_raw(lxp_codec_writer *writer, const uint8_t *bytes,
                            size_t length)
{
    if (writer == NULL || (bytes == NULL && length != 0U) ||
        writer->length > writer->capacity ||
        length > writer->capacity - writer->length) {
        return LXP_ERR_LENGTH_LIMIT;
    }
    if (length != 0U) {
        (void)memcpy(writer->bytes + writer->length, bytes, length);
    }
    writer->length += length;
    return LXP_OK;
}

static lxp_result read_raw(lxp_codec_reader *reader, uint8_t *bytes,
                           size_t length)
{
    if (reader == NULL || (bytes == NULL && length != 0U) ||
        reader->offset > reader->length ||
        length > reader->length - reader->offset) {
        return LXP_ERR_TRUNCATED;
    }
    if (length != 0U) {
        (void)memcpy(bytes, reader->bytes + reader->offset, length);
    }
    reader->offset += length;
    return LXP_OK;
}

lxp_result lxp_codec_writer_init(lxp_codec_writer *writer, lxp_arena *arena,
                                 size_t capacity)
{
    void *bytes = NULL;
    lxp_result result;
    if (writer == NULL || arena == NULL) return LXP_ERR_NON_CANONICAL;
    result = lxp_arena_alloc(arena, capacity, 1U, &bytes);
    if (result != LXP_OK) return result;
    writer->bytes = (uint8_t *)bytes;
    writer->capacity = capacity;
    writer->length = 0U;
    return LXP_OK;
}

lxp_result lxp_codec_reader_init(lxp_codec_reader *reader, const void *bytes,
                                 size_t length)
{
    if (reader == NULL || (bytes == NULL && length != 0U)) {
        return LXP_ERR_NON_CANONICAL;
    }
    reader->bytes = (const uint8_t *)bytes;
    reader->length = length;
    reader->offset = 0U;
    return LXP_OK;
}

lxp_result lxp_codec_write_u8(lxp_codec_writer *writer, uint8_t value)
{
    return write_raw(writer, &value, 1U);
}

lxp_result lxp_codec_write_u16(lxp_codec_writer *writer, uint16_t value)
{
    uint8_t out[2] = { (uint8_t)(value >> 8U), (uint8_t)value };
    return write_raw(writer, out, sizeof(out));
}

lxp_result lxp_codec_write_u32(lxp_codec_writer *writer, uint32_t value)
{
    uint8_t out[4];
    size_t i;
    for (i = 0U; i < sizeof(out); ++i) {
        out[sizeof(out) - 1U - i] = (uint8_t)(value >> (i * 8U));
    }
    return write_raw(writer, out, sizeof(out));
}

lxp_result lxp_codec_write_u64(lxp_codec_writer *writer, uint64_t value)
{
    uint8_t out[8];
    size_t i;
    for (i = 0U; i < sizeof(out); ++i) {
        out[sizeof(out) - 1U - i] = (uint8_t)(value >> (i * 8U));
    }
    return write_raw(writer, out, sizeof(out));
}

lxp_result lxp_codec_write_u128(lxp_codec_writer *writer,
                                lxp_u128 value)
{
    uint8_t encoded[16];
    lxp_result result = lxp_u128_to_be(value, encoded);
    if (result != LXP_OK) return result;
    return write_raw(writer, encoded, sizeof(encoded));
}

lxp_result lxp_codec_write_i32(lxp_codec_writer *writer, int32_t value)
{
    return lxp_codec_write_u32(writer, (uint32_t)value);
}

lxp_result lxp_codec_read_u8(lxp_codec_reader *reader, uint8_t *value)
{
    return read_raw(reader, value, 1U);
}

lxp_result lxp_codec_read_u16(lxp_codec_reader *reader, uint16_t *value)
{
    uint8_t in[2];
    lxp_result result;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    result = read_raw(reader, in, sizeof(in));
    if (result != LXP_OK) return result;
    *value = (uint16_t)(((uint16_t)in[0] << 8U) | (uint16_t)in[1]);
    return LXP_OK;
}

lxp_result lxp_codec_read_u32(lxp_codec_reader *reader, uint32_t *value)
{
    uint8_t in[4];
    size_t i;
    uint32_t out = 0U;
    lxp_result result;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    result = read_raw(reader, in, sizeof(in));
    if (result != LXP_OK) return result;
    for (i = 0U; i < sizeof(in); ++i) out = (out << 8U) | in[i];
    *value = out;
    return LXP_OK;
}

lxp_result lxp_codec_read_u64(lxp_codec_reader *reader, uint64_t *value)
{
    uint8_t in[8];
    size_t i;
    uint64_t out = 0U;
    lxp_result result;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    result = read_raw(reader, in, sizeof(in));
    if (result != LXP_OK) return result;
    for (i = 0U; i < sizeof(in); ++i) out = (out << 8U) | in[i];
    *value = out;
    return LXP_OK;
}

lxp_result lxp_codec_read_u128(lxp_codec_reader *reader, lxp_u128 *value)
{
    uint8_t encoded[16];
    lxp_result result;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    result = read_raw(reader, encoded, sizeof(encoded));
    if (result != LXP_OK) return result;
    return lxp_u128_from_be(encoded, value);
}

lxp_result lxp_codec_read_i32(lxp_codec_reader *reader, int32_t *value)
{
    uint32_t bits;
    lxp_result result;
    if (value == NULL) return LXP_ERR_NON_CANONICAL;
    result = lxp_codec_read_u32(reader, &bits);
    if (result != LXP_OK) return result;
    (void)memcpy(value, &bits, sizeof(bits));
    return LXP_OK;
}

lxp_result lxp_codec_write_bytes(lxp_codec_writer *writer, const void *bytes,
                                 size_t length, uint32_t maximum)
{
    lxp_result result;
    if (length > (size_t)maximum || length > UINT32_MAX) {
        return LXP_ERR_LENGTH_LIMIT;
    }
    result = lxp_codec_write_u32(writer, (uint32_t)length);
    if (result != LXP_OK) return result;
    return write_raw(writer, (const uint8_t *)bytes, length);
}

lxp_result lxp_codec_read_bytes(lxp_codec_reader *reader, lxp_byte_span *span,
                                uint32_t maximum)
{
    uint32_t length;
    lxp_result result;
    if (reader == NULL || span == NULL) return LXP_ERR_NON_CANONICAL;
    result = lxp_codec_read_u32(reader, &length);
    if (result != LXP_OK) return result;
    if (length > maximum) return LXP_ERR_LENGTH_LIMIT;
    if (reader->offset > reader->length ||
        (size_t)length > reader->length - reader->offset) {
        return LXP_ERR_TRUNCATED;
    }
    span->bytes = reader->bytes + reader->offset;
    span->length = length;
    reader->offset += length;
    return LXP_OK;
}

static int utf8_nfc_is_canonical(const uint8_t *text, size_t length)
{
    size_t i = 0U;
    while (i < length) {
        uint32_t codepoint;
        uint8_t first = text[i++];
        size_t continuation;
        if (first < 0x80U) continue;
        if (first >= 0xc2U && first <= 0xdfU) {
            codepoint = first & 0x1fU;
            continuation = 1U;
        } else if (first >= 0xe0U && first <= 0xefU) {
            codepoint = first & 0x0fU;
            continuation = 2U;
        } else if (first >= 0xf0U && first <= 0xf4U) {
            codepoint = first & 0x07U;
            continuation = 3U;
        } else return 0;
        if (continuation > length - i) return 0;
        while (continuation-- != 0U) {
            uint8_t next = text[i++];
            if ((next & 0xc0U) != 0x80U) return 0;
            codepoint = (codepoint << 6U) | (next & 0x3fU);
        }
        if ((codepoint < 0x80U) ||
            (codepoint < 0x800U && first >= 0xe0U) ||
            (codepoint < 0x10000U && first >= 0xf0U) ||
            codepoint > 0x10ffffU ||
            (codepoint >= 0xd800U && codepoint <= 0xdfffU) ||
            (codepoint >= 0x0300U && codepoint <= 0x036fU)) return 0;
    }
    return 1;
}

lxp_result lxp_codec_write_text(lxp_codec_writer *writer, const char *text,
                                size_t length, uint32_t maximum)
{
    if ((text == NULL && length != 0U) ||
        !utf8_nfc_is_canonical((const uint8_t *)text, length)) {
        return LXP_ERR_NON_CANONICAL;
    }
    return lxp_codec_write_bytes(writer, text, length, maximum);
}

lxp_result lxp_codec_write_seq(lxp_codec_writer *writer, uint32_t count,
                               uint32_t maximum)
{
    if (count > maximum) return LXP_ERR_LENGTH_LIMIT;
    return lxp_codec_write_u32(writer, count);
}

lxp_result lxp_codec_seq_check_sorted(const lxp_byte_span *keys, size_t count)
{
    size_t i;
    if (keys == NULL && count != 0U) return LXP_ERR_NON_CANONICAL;
    for (i = 1U; i < count; ++i) {
        size_t common = keys[i - 1U].length < keys[i].length ?
                        keys[i - 1U].length : keys[i].length;
        int order = memcmp(keys[i - 1U].bytes, keys[i].bytes, common);
        if (order > 0 || (order == 0 &&
            keys[i - 1U].length >= keys[i].length)) {
            return LXP_ERR_UNSORTED_SEQUENCE;
        }
    }
    return LXP_OK;
}

lxp_result lxp_codec_write_tag(lxp_codec_writer *writer, uint8_t tag,
                               uint8_t maximum_tag)
{
    if (tag > maximum_tag) return LXP_ERR_INVALID_TAG;
    return lxp_codec_write_u8(writer, tag);
}

lxp_result lxp_codec_read_tag(lxp_codec_reader *reader, uint8_t maximum_tag,
                              uint8_t *tag)
{
    lxp_result result = lxp_codec_read_u8(reader, tag);
    if (result != LXP_OK) return result;
    return *tag <= maximum_tag ? LXP_OK : LXP_ERR_INVALID_TAG;
}

lxp_result lxp_codec_write_struct_header(lxp_codec_writer *writer,
                                         uint16_t structure_tag)
{
    return lxp_codec_write_struct_header_version(
        writer, structure_tag, (uint16_t)LXP_PROTOCOL_VERSION_LEGACY);
}

lxp_result lxp_codec_write_struct_header_version(lxp_codec_writer *writer,
                                                 uint16_t structure_tag,
                                                 uint16_t protocol_version)
{
    lxp_result result;
    if (structure_tag == 0U ||
        !lxp_protocol_version_supported(protocol_version))
        return LXP_ERR_INVALID_TAG;
    result = lxp_codec_write_u16(writer, protocol_version);
    if (result != LXP_OK) return result;
    return lxp_codec_write_u16(writer, structure_tag);
}

lxp_result lxp_codec_read_struct_header(lxp_codec_reader *reader,
                                        uint16_t expected_structure_tag)
{
    uint16_t ignored;
    return lxp_codec_read_struct_header_version(
        reader, expected_structure_tag, &ignored);
}

lxp_result lxp_codec_read_struct_header_version(
    lxp_codec_reader *reader, uint16_t expected_structure_tag,
    uint16_t *protocol_version)
{
    uint16_t version;
    uint16_t structure_tag;
    lxp_result result = lxp_codec_read_u16(reader, &version);
    if (result != LXP_OK) return result;
    result = lxp_codec_read_u16(reader, &structure_tag);
    if (result != LXP_OK) return result;
    if (protocol_version == NULL || !lxp_protocol_version_supported(version) ||
        structure_tag != expected_structure_tag || expected_structure_tag == 0U) {
        return LXP_ERR_VERSION_UNSUPPORTED;
    }
    *protocol_version = version;
    return LXP_OK;
}

lxp_result lxp_codec_finish(const lxp_codec_reader *reader)
{
    if (reader == NULL || reader->offset > reader->length) {
        return LXP_ERR_TRUNCATED;
    }
    return reader->offset == reader->length ? LXP_OK : LXP_ERR_TRAILING_BYTES;
}

lxp_result lxp_codec_reject_unknown_field(uint16_t field_id,
                                          uint16_t maximum_field_id)
{
    return field_id == 0U || field_id > maximum_field_id ?
           LXP_ERR_UNKNOWN_FIELD : LXP_OK;
}
