#include "layerx/lxp_replica.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"
#include "layerx/programs.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state_diff.h"

#include <string.h>

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

lxp_result lxp_replay_engine_bind_transaction(lxp_replay_engine *engine,
    lxp_replay_transaction_begin_fn begin,
    lxp_replay_transaction_finish_fn finish)
{
    if (engine == NULL || begin == NULL || finish == NULL ||
        engine->transaction_begin != NULL || engine->transaction_finish != NULL)
        return LXP_ERR_NON_CANONICAL;
    engine->transaction_begin = begin;
    engine->transaction_finish = finish;
    return LXP_OK;
}

lxp_result lxp_replay_engine_bind_kernel(
    lxp_replay_engine *engine, const lxp_kernel *kernel)
{
    if (engine == NULL || kernel == NULL || kernel->state == NULL ||
        kernel->state->accounts == NULL || engine->kernel != NULL)
        return LXP_ERR_NON_CANONICAL;
    engine->kernel = kernel;
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

static lxp_replay_transition_fn transition_for(lxp_replay_engine *engine,
                                                uint16_t version)
{
    size_t i;
    for (i = 0U; i < engine->transition_count; ++i)
        if (engine->transitions[i].version == version)
            return engine->transitions[i].transition;
    return NULL;
}

static lxp_result replay_batch(lxp_replay_engine *engine, bool publication,
                            const lxp_batch_body *body,
                            const uint8_t starting_state_root[32],
                            lxp_arena *arena,
                            lxp_replay_batch_result *result)
{
    lxp_replay_transition_fn transition;
    lxp_byte_span *activities;
    lxp_byte_span *oracles;
    size_t activity_count;
    size_t receipt_count;
    size_t oracle_count;
    size_t published_count = 0U;
    lxp_byte_span *published_receipts = NULL;
    lxp_byte_span *published_events = NULL;
    size_t published_event_count = 0U;
    bool maintenance_present;
    size_t i;
    void *memory;
    uint32_t parameter_version;
    uint8_t current_root[32];
    lxp_batch_root_inputs root_inputs;
    lx_account_registry *before = NULL;
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
    if (engine->kernel == NULL || engine->kernel->state == NULL ||
        engine->kernel->state->accounts == NULL)
        return LXP_ERR_MODULE_DISABLED;
    if (lxp_ct_memcmp(engine->kernel->current_state_root,
                      starting_state_root, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = lxp_arena_alloc(arena, sizeof(*before),
                             _Alignof(lx_account_registry), &memory);
    if (status != LXP_OK) return status;
    before = memory;
    {
        const lx_account_registry *live = engine->kernel->state->accounts;
        lx_account *before_slots = NULL;
        lx_account_index_entry *before_index = NULL;
        if (live->count != 0U) {
            status = lxp_arena_alloc(arena, live->count * sizeof(*before_slots),
                                     _Alignof(lx_account), &memory);
            if (status != LXP_OK) return status;
            before_slots = memory;
            status = lxp_arena_alloc(arena, live->count * sizeof(*before_index),
                                     _Alignof(lx_account_index_entry), &memory);
            if (status != LXP_OK) return status;
            before_index = memory;
        }
        status = lx_account_registry_borrow(live, before_slots, before_index,
                                            live->count, before);
        if (status != LXP_OK) return status;
    }
    transition = transition_for(engine, body->header.protocol_version);
    if (activity_count != 0U && transition == NULL)
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (body->header.last_sequence < body->header.first_sequence)
        return LXP_ERR_BATCH_GAP;
    maintenance_present = lxp_protocol_version_uses_occupancy(body->header.protocol_version);
    if (publication) {
        status = lxp_da_receipt_section_decode(body->receipts, arena,
            &published_receipts, &published_count,
            &published_events, &published_event_count);
        if (status == LXP_OK && published_event_count != activity_count)
            status = LXP_ERR_BATCH_GAP;
        if (status != LXP_OK) return status;
        if (published_count != activity_count && published_count != activity_count + 1U)
            return LXP_ERR_BATCH_GAP;
        maintenance_present = published_count == activity_count + 1U;
        if (maintenance_present) {
            lxp_programs_occupancy_receipt record;
            status = lxp_programs_occupancy_receipt_decode(
                published_receipts[activity_count].bytes,
                published_receipts[activity_count].length, &record);
            if (status != LXP_OK) return status;
            if (!lxp_protocol_version_uses_occupancy(body->header.protocol_version) ||
                record.batch_number != body->header.batch_number ||
                record.global_sequence != body->header.last_sequence ||
                record.parameter_version != parameter_version)
                return LXP_ERR_CONTEXT_MISMATCH;
        }
    }
    if (maintenance_present) {
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
        result->encoded_receipts[i] = result->outputs[i].canonical_receipt;
        status = result->encoded_receipts[i].length != 0U ? LXP_OK : LXP_ERR_NON_CANONICAL;
        if (status != LXP_OK) return status;
        result->encoded_events[i] = result->outputs[i].canonical_events;
        (void)memcpy(current_root,
                     result->outputs[i].resulting_state_root, 32U);
    }
    if (maintenance_present) {
        status = engine->batch_finalize(
            engine->batch_finalize_context, &body->header,
            parameter_version, body->header.last_sequence, current_root,
            arena, &result->batch_maintenance_output);
        if (status != LXP_OK) return status;
        if (result->batch_maintenance_output.canonical_events.length != 0U)
            return LXP_FATAL_INVARIANT;
        result->encoded_batch_maintenance_receipt =
            result->batch_maintenance_output.canonical_receipt;
        status = result->encoded_batch_maintenance_receipt.length != 0U ? LXP_OK : LXP_ERR_NON_CANONICAL;
        if (status != LXP_OK) return status;
        result->encoded_receipts[activity_count] =
            result->encoded_batch_maintenance_receipt;
        (void)memcpy(current_root,
                     result->batch_maintenance_output.resulting_state_root,
                     32U);
    }
    (void)memcpy(result->resulting_state_root, current_root, 32U);
    if (lxp_ct_memcmp(engine->kernel->current_state_root, current_root, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = lxp_state_diff_encode(before, engine->kernel->state->accounts,
                                   arena, &result->canonical_state_diff);
    if (status != LXP_OK) return status;
    if (body->state_diff.length != result->canonical_state_diff.length ||
        lxp_ct_memcmp(body->state_diff.bytes, result->canonical_state_diff.bytes,
                      body->state_diff.length) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = lxp_da_recovery_verify_kernel(engine->kernel,
        body->header.last_sequence, body->header.last_sequence,
        body->recovery_metadata, arena);
    if (status != LXP_OK) return status;
    status = lxp_da_receipt_section_encode(result->encoded_receipts,
                                       receipt_count, result->encoded_events,
                                       activity_count, arena,
                                       &result->canonical_receipt_section);
    if (status == LXP_OK)
        status = lxp_replay_section_encode(result->encoded_events,
                                           activity_count, arena,
                                           &result->canonical_event_section);
    if (status != LXP_OK) return status;
    status = lxp_replay_section_decode(&body->oracle_inputs, arena, &oracles,
                                       &oracle_count);
    if (status != LXP_OK) return status;
    root_inputs = (lxp_batch_root_inputs){
        activities, activity_count,
        result->encoded_receipts, receipt_count,
        result->encoded_events, activity_count,
        oracles, oracle_count,
        NULL, 0U
    };
    status = lxp_batch_roots_compute(&root_inputs, arena, &result->roots);
    if (status == LXP_OK) {
        lxp_batch_body recomputed = *body;
        recomputed.receipts = result->canonical_receipt_section;
        recomputed.state_diff = result->canonical_state_diff;
        status = lxp_batch_availability_root(&recomputed, arena,
                                              result->roots.data_availability_root);
    }
    return status;
}

lxp_result lxp_replay_batch(lxp_replay_engine *engine,
    const lxp_batch_body *body, const uint8_t starting_state_root[32],
    lxp_arena *arena, lxp_replay_batch_result *result)
{
    return replay_batch(engine, false, body, starting_state_root, arena, result);
}

lxp_result lxp_replay_batch_publication(lxp_replay_engine *engine,
    const lxp_batch_body *body, const uint8_t starting_state_root[32],
    lxp_arena *arena, lxp_replay_batch_result *result)
{
    lxp_result status = replay_batch(engine, true, body, starting_state_root, arena, result);
    if (status == LXP_OK) status = lxp_replay_verify_roots(result, body);
    return status;
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
