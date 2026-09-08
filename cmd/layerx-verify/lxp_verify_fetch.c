#include "layerx/lxp_da.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_replica.h"

#include <string.h>

enum { LXP_DA_RESPONSE_HEADER_BYTES = 98 };

static uint32_t get_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | (uint32_t)in[3];
}

static uint64_t get_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static int nonzero(const uint8_t value[32])
{
    uint8_t combined = 0U;
    size_t i;
    for (i = 0U; i < 32U; ++i) combined |= value[i];
    return combined != 0U;
}

static lxp_result request_validate(const lxp_da_retrieval_request *request)
{
    if (request == NULL) return LXP_ERR_NON_CANONICAL;
    switch (request->lookup_kind) {
    case LXP_DA_LOOKUP_CHECKPOINT_ID:
        return nonzero(request->checkpoint_id) ? LXP_OK :
            LXP_ERR_NON_CANONICAL;
    case LXP_DA_LOOKUP_BATCH_NUMBER:
        return LXP_OK;
    case LXP_DA_LOOKUP_SEQUENCE_RANGE:
        return request->first_global_sequence <=
            request->last_global_sequence ? LXP_OK : LXP_ERR_NON_CANONICAL;
    case LXP_DA_LOOKUP_ACTIVITY_ID:
        return nonzero(request->activity_id) ? LXP_OK :
            LXP_ERR_NON_CANONICAL;
    default:
        return LXP_ERR_NON_CANONICAL;
    }
}

static lxp_result response_decode(lxp_byte_span response,
                                  uint64_t *batch_number,
                                  uint32_t *chunk_index,
                                  uint32_t *chunk_count,
                                  lxp_da_chunk *chunk,
                                  uint8_t root[32])
{
    uint32_t length;
    if (response.bytes == NULL ||
        response.length < LXP_DA_RESPONSE_HEADER_BYTES ||
        memcmp(response.bytes, "LXDR", 4U) != 0 ||
        response.bytes[4] != 1U)
        return LXP_ERR_NON_CANONICAL;
    length = get_u32(response.bytes + 30U);
    if (length > LXP_DA_MAX_CHUNK_BYTES ||
        response.length != LXP_DA_RESPONSE_HEADER_BYTES + length)
        return LXP_ERR_LENGTH_LIMIT;
    *batch_number = get_u64(response.bytes + 5U);
    *chunk_index = get_u32(response.bytes + 13U);
    *chunk_count = get_u32(response.bytes + 17U);
    chunk->batch_number = *batch_number;
    chunk->chunk_index = *chunk_index;
    chunk->availability_class = (lxp_da_class)response.bytes[21];
    chunk->class_offset = get_u64(response.bytes + 22U);
    chunk->length = length;
    chunk->bytes = (lxp_byte_span){
        response.bytes + LXP_DA_RESPONSE_HEADER_BYTES, length
    };
    (void)memcpy(chunk->chunk_hash, response.bytes + 34U, 32U);
    (void)memcpy(root, response.bytes + 66U, 32U);
    return LXP_OK;
}

lxp_result lxp_da_fetch(const lxp_da_retrieval_request *request,
                        lxp_da_chunk_fetch_fn fetch_chunk,
                        void *fetch_context, lxp_arena *arena,
                        lxp_da_bundle *bundle, uint8_t root[32])
{
    lxp_byte_span response;
    lxp_da_chunk first;
    lxp_da_chunk decoded;
    lxp_da_chunk *chunks;
    uint8_t response_root[32];
    uint8_t rebuilt_root[32];
    uint64_t batch_number;
    uint64_t decoded_batch;
    uint32_t index;
    uint32_t count;
    uint32_t decoded_count;
    void *memory;
    size_t total = 0U;
    size_t i;
    lxp_result status = request_validate(request);
    if (status != LXP_OK || fetch_chunk == NULL || arena == NULL ||
        bundle == NULL || root == NULL)
        return status == LXP_OK ? LXP_ERR_NON_CANONICAL : status;
    status = fetch_chunk(fetch_context, request, 0U, arena, &response);
    if (status == LXP_OK)
        status = response_decode(response, &batch_number, &index, &count,
                                 &first, response_root);
    if (status != LXP_OK) return status;
    if (index != 0U || count < LXP_DA_CLASS_COUNT ||
        count > LXP_DA_MAX_CHUNKS ||
        (request->lookup_kind == LXP_DA_LOOKUP_BATCH_NUMBER &&
         batch_number != request->batch_number))
        return LXP_ERR_DA_MISSING;
    status = lxp_arena_alloc(arena, (size_t)count * sizeof(*chunks),
                             _Alignof(lxp_da_chunk), &memory);
    if (status != LXP_OK) return status;
    chunks = (lxp_da_chunk *)memory;
    for (i = 0U; i < count; ++i) {
        uint8_t *copied = NULL;
        if (i == 0U) {
            decoded = first;
        } else {
            status = fetch_chunk(fetch_context, request, (uint32_t)i,
                                 arena, &response);
            if (status == LXP_OK)
                status = response_decode(response, &decoded_batch, &index,
                                         &decoded_count, &decoded,
                                         rebuilt_root);
            if (status != LXP_OK) return status;
            if (decoded_batch != batch_number || index != i ||
                decoded_count != count ||
                lxp_ct_memcmp(rebuilt_root, response_root, 32U) != 0)
                return LXP_ERR_DA_MISSING;
        }
        if (decoded.length != 0U) {
            status = lxp_arena_alloc(arena, decoded.length, 1U, &memory);
            if (status != LXP_OK) return status;
            copied = (uint8_t *)memory;
            (void)memcpy(copied, decoded.bytes.bytes, decoded.length);
        }
        chunks[i] = decoded;
        chunks[i].bytes = (lxp_byte_span){copied, decoded.length};
        if (decoded.length > SIZE_MAX - total) return LXP_ERR_LENGTH_LIMIT;
        total += decoded.length;
    }
    bundle->chunks = chunks;
    bundle->chunk_count = count;
    bundle->batch_number = batch_number;
    bundle->total_bytes = total;
    status = lxp_da_bundle_root(bundle, arena, rebuilt_root);
    if (status != LXP_OK ||
        lxp_ct_memcmp(rebuilt_root, response_root, 32U) != 0)
        return status == LXP_ERR_ARENA_EXHAUSTED ? status :
            LXP_ERR_DA_MISSING;
    (void)memcpy(root, rebuilt_root, 32U);
    return LXP_OK;
}

static lxp_result reconstruct_classes(const lxp_da_bundle *bundle,
                                      lxp_arena *arena,
                                      lxp_byte_span classes[5])
{
    size_t lengths[5] = {0U, 0U, 0U, 0U, 0U};
    size_t written[5] = {0U, 0U, 0U, 0U, 0U};
    uint8_t seen[5] = {0U, 0U, 0U, 0U, 0U};
    size_t previous_class = 0U;
    size_t i;
    void *memory;
    lxp_result status;
    for (i = 0U; i < bundle->chunk_count; ++i) {
        const lxp_da_chunk *chunk = &bundle->chunks[i];
        size_t class_index;
        if (chunk->availability_class < LXP_DA_ACTIVITIES ||
            chunk->availability_class > LXP_DA_RECOVERY_METADATA)
            return LXP_ERR_DA_MISSING;
        class_index = (size_t)chunk->availability_class - 1U;
        if (class_index < previous_class ||
            chunk->class_offset != lengths[class_index] ||
            chunk->length > SIZE_MAX - lengths[class_index])
            return LXP_ERR_DA_MISSING;
        lengths[class_index] += chunk->length;
        seen[class_index] = 1U;
        previous_class = class_index;
    }
    for (i = 0U; i < LXP_DA_CLASS_COUNT; ++i) {
        if (seen[i] == 0U) return LXP_ERR_DA_MISSING;
        status = lxp_arena_alloc(arena, lengths[i], 1U, &memory);
        if (status != LXP_OK) return status;
        classes[i] = (lxp_byte_span){(const uint8_t *)memory, lengths[i]};
    }
    for (i = 0U; i < bundle->chunk_count; ++i) {
        const lxp_da_chunk *chunk = &bundle->chunks[i];
        size_t class_index = (size_t)chunk->availability_class - 1U;
        if (chunk->length != 0U)
            (void)memcpy((uint8_t *)classes[class_index].bytes +
                         written[class_index], chunk->bytes.bytes,
                         chunk->length);
        written[class_index] += chunk->length;
    }
    return LXP_OK;
}

lxp_result lxp_da_verify_served_bytes(
    const lxp_da_bundle *bundle, const lxp_batch_header *header,
    struct lxp_replay_engine *engine,
    const uint8_t starting_state_root[32], lxp_arena *arena,
    struct lxp_replay_batch_result *replayed)
{
    lxp_batch_body body;
    lxp_byte_span classes[LXP_DA_CLASS_COUNT];
    uint8_t da_root[32];
    lxp_result status;
    if (bundle == NULL || header == NULL || engine == NULL ||
        starting_state_root == NULL || arena == NULL || replayed == NULL ||
        bundle->batch_number != header->batch_number)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_da_bundle_root(bundle, arena, da_root);
    if (status != LXP_OK)
        return status == LXP_ERR_ARENA_EXHAUSTED ? status :
            LXP_ERR_DA_MISSING;
    if (status == LXP_OK &&
        lxp_ct_memcmp(da_root, header->data_availability_root, 32U) != 0)
        status = LXP_ERR_DA_MISSING;
    if (status == LXP_OK)
        status = reconstruct_classes(bundle, arena, classes);
    if (status != LXP_OK) return status;
    (void)memset(&body, 0, sizeof(body));
    body.header = *header;
    body.activities = classes[0];
    body.receipts = classes[1];
    body.oracle_inputs = classes[2];
    body.state_diff = classes[3];
    body.recovery_metadata = classes[4];
    status = lxp_batch_availability_root(&body, arena, da_root);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(da_root, header->data_availability_root, 32U) != 0)
        return LXP_ERR_DA_MISSING;
    status = lxp_replay_batch(engine, &body, starting_state_root, arena,
                              replayed);
    if (status != LXP_OK) return status;
#define ROOT_MATCH(field) \
    (lxp_ct_memcmp(replayed->roots.field, header->field, 32U) == 0)
    if (lxp_ct_memcmp(replayed->resulting_state_root,
                      header->resulting_state_root, 32U) != 0 ||
        !ROOT_MATCH(activity_merkle_root) ||
        !ROOT_MATCH(receipt_merkle_root) ||
        !ROOT_MATCH(event_merkle_root) || !ROOT_MATCH(oracle_root) ||
        !ROOT_MATCH(data_availability_root) ||
        replayed->canonical_receipt_section.length != body.receipts.length ||
        lxp_ct_memcmp(replayed->canonical_receipt_section.bytes,
                      body.receipts.bytes, body.receipts.length) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
#undef ROOT_MATCH
    return LXP_OK;
}
