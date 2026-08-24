#ifndef LAYERX_LXP_CODEC_H
#define LAYERX_LXP_CODEC_H

#include "layerx/lxp_arena.h"
#include "layerx/lxp_u128.h"

#include <stddef.h>
#include <stdint.h>

typedef struct lxp_codec_writer {
    uint8_t *bytes;
    size_t capacity;
    size_t length;
} lxp_codec_writer;
#define lxp_codec_writer lxp_codec_writer

typedef struct lxp_codec_reader {
    const uint8_t *bytes;
    size_t length;
    size_t offset;
} lxp_codec_reader;
#define lxp_codec_reader lxp_codec_reader

typedef struct lxp_byte_span {
    const uint8_t *bytes;
    size_t length;
} lxp_byte_span;

lxp_result lxp_codec_writer_init(lxp_codec_writer *writer, lxp_arena *arena,
                                 size_t capacity);
lxp_result lxp_codec_reader_init(lxp_codec_reader *reader, const void *bytes,
                                 size_t length);
lxp_result lxp_codec_write_u8(lxp_codec_writer *writer, uint8_t value);
lxp_result lxp_codec_write_u16(lxp_codec_writer *writer, uint16_t value);
lxp_result lxp_codec_write_u32(lxp_codec_writer *writer, uint32_t value);
lxp_result lxp_codec_write_u64(lxp_codec_writer *writer, uint64_t value);
lxp_result lxp_codec_write_u128(lxp_codec_writer *writer,
                                lxp_u128 value);
lxp_result lxp_codec_write_i32(lxp_codec_writer *writer, int32_t value);
lxp_result lxp_codec_read_u8(lxp_codec_reader *reader, uint8_t *value);
lxp_result lxp_codec_read_u16(lxp_codec_reader *reader, uint16_t *value);
lxp_result lxp_codec_read_u32(lxp_codec_reader *reader, uint32_t *value);
lxp_result lxp_codec_read_u64(lxp_codec_reader *reader, uint64_t *value);
lxp_result lxp_codec_read_u128(lxp_codec_reader *reader, lxp_u128 *value);
lxp_result lxp_codec_read_i32(lxp_codec_reader *reader, int32_t *value);
lxp_result lxp_codec_write_bytes(lxp_codec_writer *writer, const void *bytes,
                                 size_t length, uint32_t maximum);
lxp_result lxp_codec_read_bytes(lxp_codec_reader *reader, lxp_byte_span *span,
                                uint32_t maximum);
lxp_result lxp_codec_write_text(lxp_codec_writer *writer, const char *text,
                                size_t length, uint32_t maximum);
lxp_result lxp_codec_write_seq(lxp_codec_writer *writer, uint32_t count,
                               uint32_t maximum);
lxp_result lxp_codec_seq_check_sorted(const lxp_byte_span *keys, size_t count);
lxp_result lxp_codec_write_tag(lxp_codec_writer *writer, uint8_t tag,
                               uint8_t maximum_tag);
lxp_result lxp_codec_read_tag(lxp_codec_reader *reader, uint8_t maximum_tag,
                              uint8_t *tag);
lxp_result lxp_codec_write_struct_header(lxp_codec_writer *writer,
                                         uint16_t structure_tag);
lxp_result lxp_codec_write_struct_header_version(lxp_codec_writer *writer,
                                                 uint16_t structure_tag,
                                                 uint16_t protocol_version);
lxp_result lxp_codec_read_struct_header(lxp_codec_reader *reader,
                                        uint16_t expected_structure_tag);
lxp_result lxp_codec_read_struct_header_version(
    lxp_codec_reader *reader, uint16_t expected_structure_tag,
    uint16_t *protocol_version);
lxp_result lxp_codec_finish(const lxp_codec_reader *reader);
lxp_result lxp_codec_reject_unknown_field(uint16_t field_id,
                                          uint16_t maximum_field_id);

#endif
