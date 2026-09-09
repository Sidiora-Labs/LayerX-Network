#include "layerx/lxp_da.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state_diff.h"
#include "layerx/lxp_replica.h"
#include "../state/lxp_state_internal.h"

#include <string.h>

static void store_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void store_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

lxp_result lxp_da_chunk_hash(lxp_da_chunk *chunk)
{
    uint8_t metadata[25];
    lxp_hash_context hash;
    size_t tag_length;
    const uint8_t *tag = lxp_domain_tag(LXP_DOMAIN_DA_CHUNK, &tag_length);
    lxp_result status;
    if (chunk == NULL || chunk->availability_class < LXP_DA_ACTIVITIES ||
        chunk->availability_class > LXP_DA_RECOVERY_METADATA ||
        chunk->length > LXP_DA_MAX_CHUNK_BYTES ||
        chunk->bytes.length != (size_t)chunk->length ||
        (chunk->bytes.bytes == NULL && chunk->bytes.length != 0U) ||
        UINT64_MAX - chunk->class_offset < (uint64_t)chunk->length)
        return LXP_ERR_NON_CANONICAL;
    if (tag == NULL) return LXP_ERR_INVALID_TAG;
    store_u64(metadata, chunk->batch_number);
    store_u32(metadata + 8U, chunk->chunk_index);
    metadata[12] = (uint8_t)chunk->availability_class;
    store_u64(metadata + 13U, chunk->class_offset);
    store_u32(metadata + 21U, chunk->length);
    lxp_hash_init(&hash);
    status = lxp_hash_update(&hash, tag, tag_length);
    if (status == LXP_OK)
        status = lxp_hash_update(&hash, metadata, sizeof(metadata));
    if (status == LXP_OK)
        status = lxp_hash_update(&hash, chunk->bytes.bytes,
                                 chunk->bytes.length);
    return status == LXP_OK ? lxp_hash_final(&hash, chunk->chunk_hash) : status;
}

lxp_result lxp_da_recovery_metadata_encode(
    const lxp_da_recovery_input *input, lxp_arena *arena,
    lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    size_t capacity;
    size_t i;
    lxp_result status;
    if (input == NULL || arena == NULL || encoded == NULL ||
        input->module_root_count > LXP_DA_MAX_MODULE_ROOTS ||
        (input->module_roots == NULL && input->module_root_count != 0U) ||
        input->account_tree_frontier.length >
            LXP_DA_MAX_ACCOUNT_FRONTIER_BYTES ||
        (input->account_tree_frontier.bytes == NULL &&
         input->account_tree_frontier.length != 0U))
        return LXP_ERR_NON_CANONICAL;
    for (i = 1U; i < input->module_root_count; ++i)
        if (input->module_roots[i - 1U].module_id >=
            input->module_roots[i].module_id)
            return LXP_ERR_UNSORTED_SEQUENCE;
    capacity = 4U + input->module_root_count * 38U + 4U +
               input->account_tree_frontier.length + 24U;
    status = lxp_codec_writer_init(&writer, arena, capacity);
    if (status == LXP_OK)
        status = lxp_codec_write_seq(&writer,
            (uint32_t)input->module_root_count, LXP_DA_MAX_MODULE_ROOTS);
    for (i = 0U; status == LXP_OK && i < input->module_root_count; ++i) {
        status = lxp_codec_write_u16(&writer,
                                     input->module_roots[i].module_id);
        if (status == LXP_OK)
            status = lxp_codec_write_bytes(&writer,
                input->module_roots[i].state_root, 32U, 32U);
    }
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer,
            input->account_tree_frontier.bytes,
            input->account_tree_frontier.length,
            LXP_DA_MAX_ACCOUNT_FRONTIER_BYTES);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, input->next_global_sequence);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, input->receipt_watermark);
    if (status == LXP_OK)
        status = lxp_codec_write_u64(&writer, input->projection_watermark);
    if (status != LXP_OK) return status;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

static size_t class_chunk_count(size_t length, size_t chunk_size)
{
    return length == 0U ? 1U : 1U + (length - 1U) / chunk_size;
}

lxp_result lxp_da_bundle_build(const lxp_batch_body *body, size_t chunk_size,
                               lxp_arena *arena, lxp_da_bundle *bundle)
{
    const lxp_byte_span classes[LXP_DA_CLASS_COUNT] = {
        body == NULL ? (lxp_byte_span){NULL, 0U} : body->activities,
        body == NULL ? (lxp_byte_span){NULL, 0U} : body->receipts,
        body == NULL ? (lxp_byte_span){NULL, 0U} : body->oracle_inputs,
        body == NULL ? (lxp_byte_span){NULL, 0U} : body->state_diff,
        body == NULL ? (lxp_byte_span){NULL, 0U} : body->recovery_metadata
    };
    size_t count = 0U;
    size_t total = 0U;
    size_t index = 0U;
    size_t class_index;
    void *memory;
    lxp_result status = LXP_OK;
    if (body == NULL || arena == NULL || bundle == NULL || chunk_size == 0U ||
        chunk_size > LXP_DA_MAX_CHUNK_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (class_index = 0U; class_index < LXP_DA_CLASS_COUNT; ++class_index) {
        size_t class_count;
        if (classes[class_index].bytes == NULL &&
            classes[class_index].length != 0U)
            return LXP_ERR_NON_CANONICAL;
        class_count = class_chunk_count(classes[class_index].length,
                                        chunk_size);
        if (class_count > LXP_DA_MAX_CHUNKS - count ||
            classes[class_index].length > SIZE_MAX - total)
            return LXP_ERR_LENGTH_LIMIT;
        count += class_count;
        total += classes[class_index].length;
    }
    status = lxp_arena_alloc(arena, count * sizeof(lxp_da_chunk),
                             _Alignof(lxp_da_chunk), &memory);
    if (status != LXP_OK) return status;
    bundle->chunks = (lxp_da_chunk *)memory;
    bundle->chunk_count = count;
    bundle->batch_number = body->header.batch_number;
    bundle->total_bytes = total;
    for (class_index = 0U; status == LXP_OK &&
         class_index < LXP_DA_CLASS_COUNT; ++class_index) {
        uint64_t offset = 0U;
        do {
            lxp_da_chunk *chunk = &bundle->chunks[index];
            size_t remaining = classes[class_index].length - (size_t)offset;
            size_t length = remaining < chunk_size ? remaining : chunk_size;
            chunk->batch_number = body->header.batch_number;
            chunk->chunk_index = (uint32_t)index;
            chunk->availability_class = (lxp_da_class)(class_index + 1U);
            chunk->class_offset = offset;
            chunk->length = (uint32_t)length;
            chunk->bytes.bytes = length == 0U ? NULL :
                classes[class_index].bytes + (size_t)offset;
            chunk->bytes.length = length;
            status = lxp_da_chunk_hash(chunk);
            offset += length;
            ++index;
        } while (status == LXP_OK &&
                 offset < classes[class_index].length);
    }
    return status;
}

lxp_result lxp_da_bundle_root(const lxp_da_bundle *bundle, lxp_arena *arena,
                              uint8_t root[32])
{
    uint8_t (*hashes)[32];
    void *memory;
    size_t mark;
    size_t i;
    size_t total = 0U;
    uint64_t class_offset = 0U;
    lxp_da_class expected_class = LXP_DA_ACTIVITIES;
    bool class_has_chunk = false;
    lxp_result status;
    if (bundle == NULL || arena == NULL || root == NULL ||
        bundle->chunks == NULL || bundle->chunk_count < LXP_DA_CLASS_COUNT ||
        bundle->chunk_count > LXP_DA_MAX_CHUNKS)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_arena_alloc(arena, bundle->chunk_count * 32U,
                             _Alignof(uint64_t), &memory);
    if (status != LXP_OK) return status;
    hashes = (uint8_t (*)[32])memory;
    for (i = 0U; i < bundle->chunk_count; ++i) {
        lxp_da_chunk copy = bundle->chunks[i];
        if (copy.chunk_index != i || copy.batch_number != bundle->batch_number) {
            status = LXP_ERR_UNSORTED_SEQUENCE;
            break;
        }
        if (copy.availability_class != expected_class) {
            if (copy.availability_class !=
                    (lxp_da_class)((unsigned)expected_class + 1U) ||
                !class_has_chunk) {
                status = LXP_ERR_UNSORTED_SEQUENCE;
                break;
            }
            expected_class = copy.availability_class;
            class_offset = 0U;
            class_has_chunk = false;
        }
        if ((class_has_chunk && class_offset == 0U) ||
            copy.class_offset != class_offset ||
            copy.bytes.length > SIZE_MAX - total) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        status = lxp_da_chunk_hash(&copy);
        if (status != LXP_OK ||
            lxp_ct_memcmp(copy.chunk_hash,
                          bundle->chunks[i].chunk_hash, 32U) != 0) {
            status = status == LXP_OK ? LXP_ERR_ROOT_MISMATCH : status;
            break;
        }
        class_offset += copy.length;
        total += copy.bytes.length;
        class_has_chunk = true;
        (void)memcpy(hashes[i], copy.chunk_hash, 32U);
    }
    if (status == LXP_OK &&
        (expected_class != LXP_DA_RECOVERY_METADATA || !class_has_chunk ||
         total != bundle->total_bytes))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_merkle_build((const uint8_t (*)[32])hashes,
                                  bundle->chunk_count, arena, root);
    (void)lxp_arena_reset(arena, mark);
    return status;
}


lxp_result lxp_batch_availability_root(const lxp_batch_body *body,
                                       lxp_arena *arena, uint8_t root[32])
{
    lxp_da_bundle bundle;
    uint8_t computed[32];
    size_t mark;
    lxp_result status;
    if (body == NULL || arena == NULL || root == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_da_bundle_build(body, LXP_DA_CANONICAL_CHUNK_BYTES,
                                 arena, &bundle);
    if (status == LXP_OK)
        status = lxp_da_bundle_root(&bundle, arena, computed);
    (void)lxp_arena_reset(arena, mark);
    if (status == LXP_OK) (void)memcpy(root, computed, sizeof(computed));
    return status;
}

lxp_result lxp_da_receipt_section_encode(
    const lxp_byte_span *receipts, size_t receipt_count,
    const lxp_byte_span *events, size_t event_count,
    lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    size_t capacity = 0U, i, group;
    lxp_result status;
    if (arena == NULL || encoded == NULL ||
        (receipts == NULL && receipt_count != 0U) ||
        (events == NULL && event_count != 0U) ||
        receipt_count > LXP_MAX_BATCH_ACTIVITIES ||
        event_count > LXP_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    for (group = 0U; group < 2U; ++group) {
        const lxp_byte_span *items = group == 0U ? receipts : events;
        size_t count = group == 0U ? receipt_count : event_count;
        for (i = 0U; i < count; ++i) {
            if ((items[i].bytes == NULL && items[i].length != 0U) ||
                items[i].length > LXP_MAX_BATCH_BODY_BYTES ||
                capacity > LXP_MAX_BATCH_BODY_BYTES - items[i].length ||
                LXP_MAX_BATCH_BODY_BYTES - capacity - items[i].length < 5U)
                return LXP_ERR_LENGTH_LIMIT;
            capacity += 5U + items[i].length;
        }
    }
    status = lxp_codec_writer_init(&writer, arena, capacity);
    for (group = 0U; status == LXP_OK && group < 2U; ++group) {
        const lxp_byte_span *items = group == 0U ? receipts : events;
        size_t count = group == 0U ? receipt_count : event_count;
        for (i = 0U; status == LXP_OK && i < count; ++i) {
            status = lxp_codec_write_u8(&writer, (uint8_t)(group + 1U));
            if (status == LXP_OK)
                status = lxp_codec_write_bytes(&writer, items[i].bytes,
                    items[i].length, LXP_MAX_BATCH_BODY_BYTES);
        }
    }
    if (status == LXP_OK)
        *encoded = (lxp_byte_span){writer.bytes, writer.length};
    return status;
}



lxp_result lxp_da_receipt_section_decode(
    lxp_byte_span encoded, lxp_arena *arena,
    lxp_byte_span **receipts, size_t *receipt_count,
    lxp_byte_span **events, size_t *event_count)
{
    lxp_codec_reader reader;
    lxp_byte_span *groups[2] = {NULL, NULL};
    size_t counts[2] = {0U, 0U}, positions[2] = {0U, 0U};
    size_t mark, pass, i;
    lxp_result status = LXP_OK;
    if (arena == NULL || receipts == NULL || receipt_count == NULL ||
        events == NULL || event_count == NULL)
        return LXP_ERR_NON_CANONICAL;
    *receipts = NULL;
    *events = NULL;
    *receipt_count = 0U;
    *event_count = 0U;
    mark = lxp_arena_mark(arena);
    for (pass = 0U; status == LXP_OK && pass < 2U; ++pass) {
        uint8_t previous = 1U;
        status = lxp_codec_reader_init(&reader, encoded.bytes, encoded.length);
        while (status == LXP_OK && reader.offset < encoded.length) {
            uint8_t kind;
            lxp_byte_span item;
            status = lxp_codec_read_u8(&reader, &kind);
            if (status == LXP_OK && (kind < previous || kind > 2U))
                status = LXP_ERR_NON_CANONICAL;
            if (status == LXP_OK)
                status = lxp_codec_read_bytes(&reader, &item, LXP_MAX_BATCH_BODY_BYTES);
            if (status != LXP_OK) break;
            previous = kind;
            i = (size_t)kind - 1U;
            if (pass == 0U) {
                if (counts[i] == LXP_MAX_BATCH_ACTIVITIES) {
                    status = LXP_ERR_LENGTH_LIMIT;
                    break;
                }
                ++counts[i];
            } else {
                groups[i][positions[i]++] = item;
            }
        }
        if (status == LXP_OK) status = lxp_codec_finish(&reader);
        if (pass == 0U) {
            for (i = 0U; status == LXP_OK && i < 2U; ++i) {
                void *memory = NULL;
                if (counts[i] != 0U)
                    status = lxp_arena_alloc(arena, counts[i] * sizeof(lxp_byte_span),
                                             _Alignof(lxp_byte_span), &memory);
                groups[i] = memory;
            }
        }
    }
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    *receipts = groups[0];
    *events = groups[1];
    *receipt_count = counts[0];
    *event_count = counts[1];
    return LXP_OK;
}


lxp_result lxp_da_bundle_body(const lxp_da_bundle *bundle,
                              const lxp_batch_header *header,
                              lxp_arena *arena, lxp_batch_body *body)
{
    lxp_batch_body built = {0};
    lxp_byte_span classes[LXP_DA_CLASS_COUNT] = {{0}};
    lxp_byte_span *receipts, *events;
    size_t receipt_count, event_count, i, kind, mark;
    uint8_t root[32];
    lxp_result status;
    if (bundle == NULL || header == NULL || arena == NULL || body == NULL ||
        bundle->batch_number != header->batch_number)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_da_bundle_root(bundle, arena, root);
    if (status == LXP_OK && lxp_ct_memcmp(root, header->data_availability_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    for (kind = 1U; status == LXP_OK && kind <= LXP_DA_CLASS_COUNT; ++kind) {
        size_t length = 0U, offset = 0U;
        bool present = false;
        void *memory = NULL;
        for (i = 0U; i < bundle->chunk_count; ++i) {
            const lxp_da_chunk *chunk = &bundle->chunks[i];
            if ((size_t)chunk->availability_class != kind) continue;
            if (chunk->class_offset != length || chunk->length > LXP_MAX_BATCH_BODY_BYTES - length) {
                status = LXP_ERR_DA_MISSING;
                break;
            }
            length += chunk->length;
            present = true;
        }
        if (status == LXP_OK && !present) status = LXP_ERR_DA_MISSING;
        if (status == LXP_OK && length != 0U)
            status = lxp_arena_alloc(arena, length, 1U, &memory);
        if (status != LXP_OK) break;
        for (i = 0U; i < bundle->chunk_count; ++i) {
            const lxp_da_chunk *chunk = &bundle->chunks[i];
            if ((size_t)chunk->availability_class != kind) continue;
            if (chunk->length != 0U)
                (void)memcpy((uint8_t *)memory + offset, chunk->bytes.bytes, chunk->length);
            offset += chunk->length;
        }
        classes[kind - 1U] = (lxp_byte_span){memory, length};
    }
    built.header = *header;
    built.activities = classes[0];
    built.receipts = classes[1];
    built.oracle_inputs = classes[2];
    built.state_diff = classes[3];
    built.recovery_metadata = classes[4];
    if (status == LXP_OK)
        status = lxp_da_receipt_section_decode(built.receipts, arena,
            &receipts, &receipt_count, &events, &event_count);
    if (status == LXP_OK)
        status = lxp_replay_section_encode(events, event_count, arena, &built.events);
    if (status == LXP_OK)
        status = lxp_batch_availability_root(&built, arena, root);
    if (status == LXP_OK && lxp_ct_memcmp(root, header->data_availability_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    *body = built;
    return LXP_OK;
}
