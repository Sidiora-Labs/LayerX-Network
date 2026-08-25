#include "layerx/lxp_codec.h"

#include <limits.h>
#include <stdint.h>
#include <string.h>

int main(void)
{
    uint8_t storage[256];
    lxp_arena arena;
    lxp_codec_writer writer;
    lxp_codec_reader reader;
    const uint64_t values[] = { 0U, 1U, UINT64_MAX - 1U, UINT64_MAX };
    const int32_t signed_values[] = { INT32_MIN, -1, 0, INT32_MAX };
    lxp_u128 wide = { UINT64_C(0xff00000000000000), UINT64_C(0x00000000000000fe) };
    lxp_u128 wide_out;
    size_t i;

    if (lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_codec_writer_init(&writer, &arena, sizeof(storage)) != LXP_OK)
        return 1;
    for (i = 0U; i < sizeof(values) / sizeof(values[0]); ++i)
        if (lxp_codec_write_u64(&writer, values[i]) != LXP_OK) return 1;
    for (i = 0U; i < sizeof(signed_values) / sizeof(signed_values[0]); ++i)
        if (lxp_codec_write_i32(&writer, signed_values[i]) != LXP_OK) return 1;
    if (lxp_codec_write_u128(&writer, wide) != LXP_OK) return 1;
    if (lxp_codec_reader_init(&reader, writer.bytes, writer.length) != LXP_OK)
        return 1;
    {
        size_t offset = reader.offset;
        if (lxp_codec_read_u16(&reader, NULL) != LXP_ERR_NON_CANONICAL ||
            reader.offset != offset)
            return 1;
    }
    for (i = 0U; i < sizeof(values) / sizeof(values[0]); ++i) {
        uint64_t out = 0U;
        if (lxp_codec_read_u64(&reader, &out) != LXP_OK || out != values[i])
            return 1;
    }
    for (i = 0U; i < sizeof(signed_values) / sizeof(signed_values[0]); ++i) {
        int32_t out = 0;
        if (lxp_codec_read_i32(&reader, &out) != LXP_OK || out != signed_values[i])
            return 1;
    }
    if (lxp_codec_read_u128(&reader, &wide_out) != LXP_OK ||
        wide.hi != wide_out.hi || wide.lo != wide_out.lo) return 1;
    {
        uint8_t out = 0U;
        if (lxp_codec_read_u8(&reader, &out) != LXP_ERR_TRUNCATED) return 1;
    }
    return reader.offset == writer.length ? 0 : 1;
}
