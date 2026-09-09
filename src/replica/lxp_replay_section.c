#include "layerx/lxp_replica.h"
#include "layerx/lxp_protocol.h"

lxp_result lxp_replay_section_encode(const lxp_byte_span *items, size_t count,
                                     lxp_arena *arena,
                                     lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    size_t capacity = 4U;
    size_t i;
    lxp_result status;
    if ((items == NULL && count != 0U) || arena == NULL || encoded == NULL ||
        count > LXP_MAX_BATCH_ACTIVITIES) return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < count; ++i) {
        if ((items[i].bytes == NULL && items[i].length != 0U) ||
            items[i].length > LXP_MAX_REPLAY_FIELD_BYTES ||
            items[i].length > SIZE_MAX - capacity - 4U)
            return LXP_ERR_LENGTH_LIMIT;
        capacity += 4U + items[i].length;
    }
    status = lxp_codec_writer_init(&writer, arena, capacity);
    if (status == LXP_OK)
        status = lxp_codec_write_seq(&writer, (uint32_t)count,
                                     LXP_MAX_BATCH_ACTIVITIES);
    for (i = 0U; status == LXP_OK && i < count; ++i)
        status = lxp_codec_write_bytes(&writer, items[i].bytes,
                                       items[i].length,
                                       LXP_MAX_REPLAY_FIELD_BYTES);
    if (status != LXP_OK) return status;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_replay_section_decode(const lxp_byte_span *section,
                                     lxp_arena *arena,
                                     lxp_byte_span **items, size_t *count)
{
    lxp_codec_reader reader;
    lxp_byte_span *decoded = NULL;
    void *memory = NULL;
    uint32_t item_count;
    size_t i;
    lxp_result status;
    if (section == NULL || arena == NULL || items == NULL || count == NULL ||
        (section->bytes == NULL && section->length != 0U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_codec_reader_init(&reader, section->bytes, section->length);
    if (status == LXP_OK) status = lxp_codec_read_u32(&reader, &item_count);
    if (status != LXP_OK) return status;
    if (item_count > LXP_MAX_BATCH_ACTIVITIES) return LXP_ERR_LENGTH_LIMIT;
    if (item_count != 0U) {
        status = lxp_arena_alloc(arena,
                                 (size_t)item_count * sizeof(*decoded),
                                 _Alignof(lxp_byte_span), &memory);
        if (status != LXP_OK) return status;
        decoded = (lxp_byte_span *)memory;
    }
    for (i = 0U; i < item_count; ++i) {
        status = lxp_codec_read_bytes(&reader, &decoded[i],
                                      LXP_MAX_REPLAY_FIELD_BYTES);
        if (status != LXP_OK) return status;
    }
    status = lxp_codec_finish(&reader);
    if (status != LXP_OK) return status;
    *items = decoded;
    *count = item_count;
    return LXP_OK;
}
