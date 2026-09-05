#include "layerx/lxp_replica.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"

#include <string.h>

static lxp_result record_size(const lxp_replay_activity_output *record,
                              size_t *size)
{
    const lxp_byte_span *fields[4];
    size_t total = 73U;
    size_t i;
    fields[0] = &record->effects;
    fields[1] = &record->resulting_balance;
    fields[2] = &record->canonical_receipt;
    fields[3] = &record->canonical_events;
    for (i = 0U; i < 4U; ++i) {
        if ((fields[i]->bytes == NULL && fields[i]->length != 0U) ||
            fields[i]->length > LXP_MAX_REPLAY_FIELD_BYTES ||
            fields[i]->length > SIZE_MAX - total) return LXP_ERR_LENGTH_LIMIT;
        total += fields[i]->length;
    }
    *size = total;
    return LXP_OK;
}

static lxp_result record_encode(const lxp_replay_activity_output *record,
                                lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    size_t size;
    lxp_result status = record_size(record, &size);
    if (status != LXP_OK) return status;
    status = lxp_codec_writer_init(&writer, arena, size);
    if (status == LXP_OK) status = lxp_codec_write_u8(&writer, 1U);
    if (status == LXP_OK)
        status = lxp_codec_write_i32(&writer, record->result_code);
    if (status == LXP_OK)
        status = lxp_codec_write_u128(&writer, record->fee_charged);
    if (status == LXP_OK)
        status = lxp_codec_write_bytes(&writer, record->resulting_state_root,
                                       32U, 32U);
#define RECORD_BYTES(span) do { \
    if (status == LXP_OK) status = lxp_codec_write_bytes( \
        &writer, (span).bytes, (span).length, LXP_MAX_REPLAY_FIELD_BYTES); \
} while (0)
    RECORD_BYTES(record->effects);
    RECORD_BYTES(record->resulting_balance);
    RECORD_BYTES(record->canonical_receipt);
    RECORD_BYTES(record->canonical_events);
#undef RECORD_BYTES
    if (status != LXP_OK) return status;
    if (writer.length != size) return LXP_FATAL_INVARIANT;
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_replay_engine_init(
    lxp_replay_engine *engine,
    lxp_replay_parameter_version_fn parameter_version, void *context)
{
    if (engine == NULL || parameter_version == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(engine, 0, sizeof(*engine));
    engine->parameter_version = parameter_version;
    engine->context = context;
    return LXP_OK;
}

lxp_result lxp_replay_engine_register(lxp_replay_engine *engine,
                                      uint16_t version,
                                      lxp_replay_transition_fn transition)
{
    size_t i;
    if (engine == NULL || !lxp_protocol_version_supported(version) ||
        transition == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_protocol_version_uses_occupancy(version) &&
        engine->batch_finalize == NULL)
        return LXP_ERR_MODULE_DISABLED;
    for (i = 0U; i < engine->transition_count; ++i)
        if (engine->transitions[i].version == version)
            return LXP_ERR_NON_CANONICAL;
    if (engine->transition_count == LXP_MAX_REPLAY_TRANSITIONS)
        return LXP_ERR_LENGTH_LIMIT;
    engine->transitions[engine->transition_count].version = version;
    engine->transitions[engine->transition_count].transition = transition;
    engine->transition_count += 1U;
    return LXP_OK;
}

lxp_result lxp_replay_engine_register_batch_finalizer(
    lxp_replay_engine *engine, lxp_replay_batch_finalize_fn finalize,
    void *context)
{
    if (engine == NULL || finalize == NULL ||
        engine->batch_finalize != NULL)
        return LXP_ERR_NON_CANONICAL;
    engine->batch_finalize = finalize;
    engine->batch_finalize_context = context;
    return LXP_OK;
}

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

static lxp_replay_transition_fn transition_for(lxp_replay_engine *engine,
                                                uint16_t version)
{
    size_t i;
    for (i = 0U; i < engine->transition_count; ++i)
        if (engine->transitions[i].version == version)
            return engine->transitions[i].transition;
    return NULL;
}

lxp_result lxp_replay_batch(lxp_replay_engine *engine,
                            const lxp_batch_body *body,
                            const uint8_t starting_state_root[32],
                            lxp_arena *arena,
                            lxp_replay_batch_result *result)
{
    lxp_replay_transition_fn transition;
    lxp_byte_span *activities;
    lxp_byte_span *oracles;
    lxp_byte_span availability[5];
    size_t activity_count;
    size_t receipt_count;
    size_t oracle_count;
    size_t i;
    void *memory;
    uint32_t parameter_version;
    uint8_t current_root[32];
    lxp_batch_root_inputs root_inputs;
    lxp_result status;
    if (engine == NULL || body == NULL || starting_state_root == NULL ||
        arena == NULL || result == NULL || engine->parameter_version == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!lxp_protocol_version_supported(body->header.protocol_version))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (lxp_ct_memcmp(starting_state_root,
                      body->header.previous_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = engine->parameter_version(engine->context, body->header.epoch,
                                       &parameter_version);
    if (status != LXP_OK) return status;
    status = lxp_replay_section_decode(&body->activities, arena, &activities,
                                       &activity_count);
    if (status != LXP_OK) return status;
    transition = transition_for(engine, body->header.protocol_version);
    if (activity_count != 0U && transition == NULL)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (body->header.last_sequence < body->header.first_sequence)
        return LXP_ERR_BATCH_GAP;
    if (lxp_protocol_version_uses_occupancy(body->header.protocol_version)) {
        if (engine->batch_finalize == NULL || activity_count == SIZE_MAX ||
            activity_count >= LXP_MAX_BATCH_ACTIVITIES ||
            body->header.last_sequence - body->header.first_sequence !=
                (uint64_t)activity_count)
            return engine->batch_finalize == NULL ? LXP_ERR_MODULE_DISABLED :
                                                    LXP_ERR_BATCH_GAP;
        receipt_count = activity_count + 1U;
    } else {
        if (activity_count == 0U ||
            body->header.last_sequence - body->header.first_sequence !=
                (uint64_t)(activity_count - 1U))
            return LXP_ERR_BATCH_GAP;
        receipt_count = activity_count;
    }
    (void)memset(result, 0, sizeof(*result));
    if (activity_count != 0U) {
        status = lxp_arena_alloc(arena,
                                 activity_count * sizeof(*result->outputs),
                                 _Alignof(lxp_replay_activity_output), &memory);
        if (status != LXP_OK) return status;
        result->outputs = (lxp_replay_activity_output *)memory;
    }
    status = lxp_arena_alloc(arena,
                             receipt_count * sizeof(lxp_byte_span),
                             _Alignof(lxp_byte_span), &memory);
    if (status != LXP_OK) return status;
    result->encoded_receipts = (lxp_byte_span *)memory;
    if (activity_count != 0U) {
        status = lxp_arena_alloc(arena,
                                 activity_count * sizeof(lxp_byte_span),
                                 _Alignof(lxp_byte_span), &memory);
        if (status != LXP_OK) return status;
        result->encoded_events = (lxp_byte_span *)memory;
    }
    result->activity_count = activity_count;
    result->receipt_count = receipt_count;
    (void)memcpy(current_root, starting_state_root, 32U);
    for (i = 0U; i < activity_count; ++i) {
        status = transition(engine->context, body->header.protocol_version,
                            parameter_version, body->header.timestamp_ms,
                            body->header.first_sequence + i, activities[i],
                            current_root, arena, &result->outputs[i]);
        if (status != LXP_OK) return status;
        status = record_encode(&result->outputs[i], arena,
                               &result->encoded_receipts[i]);
        if (status != LXP_OK) return status;
        result->encoded_events[i] = result->outputs[i].canonical_events;
        (void)memcpy(current_root,
                     result->outputs[i].resulting_state_root, 32U);
    }
    if (lxp_protocol_version_uses_occupancy(body->header.protocol_version)) {
        status = engine->batch_finalize(
            engine->batch_finalize_context, &body->header,
            parameter_version, body->header.last_sequence, current_root,
            arena, &result->batch_maintenance_output);
        if (status != LXP_OK) return status;
        if (result->batch_maintenance_output.canonical_events.length != 0U)
            return LXP_FATAL_INVARIANT;
        status = record_encode(&result->batch_maintenance_output, arena,
                               &result->encoded_batch_maintenance_receipt);
        if (status != LXP_OK) return status;
        result->encoded_receipts[activity_count] =
            result->encoded_batch_maintenance_receipt;
        (void)memcpy(current_root,
                     result->batch_maintenance_output.resulting_state_root,
                     32U);
    }
    (void)memcpy(result->resulting_state_root, current_root, 32U);
    status = lxp_replay_section_encode(result->encoded_receipts,
                                       receipt_count, arena,
                                       &result->canonical_receipt_section);
    if (status == LXP_OK)
        status = lxp_replay_section_encode(result->encoded_events,
                                           activity_count, arena,
                                           &result->canonical_event_section);
    if (status != LXP_OK) return status;
    status = lxp_replay_section_decode(&body->oracle_inputs, arena, &oracles,
                                       &oracle_count);
    if (status != LXP_OK) return status;
    availability[0] = body->activities;
    availability[1] = result->canonical_receipt_section;
    availability[2] = body->oracle_inputs;
    availability[3] = body->state_diff;
    availability[4] = body->recovery_metadata;
    root_inputs = (lxp_batch_root_inputs){
        activities, activity_count,
        result->encoded_receipts, receipt_count,
        result->encoded_events, activity_count,
        oracles, oracle_count,
        availability, 5U
    };
    return lxp_batch_roots_compute(&root_inputs, arena, &result->roots);
}

lxp_result lxp_replay_verify_roots(const lxp_replay_batch_result *recomputed,
                                   const lxp_batch_body *published)
{
    if (recomputed == NULL || published == NULL)
        return LXP_ERR_NON_CANONICAL;
#define MATCH(left, right) \
    (lxp_ct_memcmp((left), (right), 32U) == 0)
    if (!MATCH(recomputed->resulting_state_root,
               published->header.resulting_state_root) ||
        !MATCH(recomputed->roots.activity_merkle_root,
               published->header.activity_merkle_root) ||
        !MATCH(recomputed->roots.receipt_merkle_root,
               published->header.receipt_merkle_root) ||
        !MATCH(recomputed->roots.event_merkle_root,
               published->header.event_merkle_root) ||
        !MATCH(recomputed->roots.oracle_root, published->header.oracle_root) ||
        !MATCH(recomputed->roots.data_availability_root,
               published->header.data_availability_root))
        return LXP_FATAL_REPLAY_DIVERGENCE;
#undef MATCH
    if (published->receipts.length !=
            recomputed->canonical_receipt_section.length ||
        published->events.length != recomputed->canonical_event_section.length ||
        lxp_ct_memcmp(published->receipts.bytes,
                      recomputed->canonical_receipt_section.bytes,
                      published->receipts.length) != 0 ||
        lxp_ct_memcmp(published->events.bytes,
                      recomputed->canonical_event_section.bytes,
                      published->events.length) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    return LXP_OK;
}
