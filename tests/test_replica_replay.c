#include "layerx/lxp_hash.h"
#include "layerx/lxp_replica.h"

#include <stdint.h>
#include <string.h>

static lxp_result parameter_version(void *context, uint64_t epoch,
                                    uint32_t *version)
{
    (void)context;
    if (epoch > UINT32_MAX - 10U) return LXP_ERR_OVERFLOW;
    *version = (uint32_t)epoch + 10U;
    return LXP_OK;
}

static lxp_result transition(void *context, uint16_t transition_version,
                             uint32_t parameters, uint64_t timestamp,
                             uint64_t sequence, lxp_byte_span activity,
                             const uint8_t previous_root[32], lxp_arena *arena,
                             lxp_replay_activity_output *output)
{
    uint8_t *material;
    void *memory;
    size_t length = 32U + 2U + 4U + 8U + 8U + activity.length;
    size_t offset = 0U;
    size_t i;
    lxp_result status = lxp_arena_alloc(arena, length, 1U, &memory);
    (void)context;
    if (status != LXP_OK) return status;
    material = (uint8_t *)memory;
    (void)memcpy(material, previous_root, 32U); offset += 32U;
    material[offset++] = (uint8_t)(transition_version >> 8U);
    material[offset++] = (uint8_t)transition_version;
    for (i = 0U; i < 4U; ++i)
        material[offset + 3U - i] = (uint8_t)(parameters >> (i * 8U));
    offset += 4U;
    for (i = 0U; i < 8U; ++i)
        material[offset + 7U - i] = (uint8_t)(timestamp >> (i * 8U));
    offset += 8U;
    for (i = 0U; i < 8U; ++i)
        material[offset + 7U - i] = (uint8_t)(sequence >> (i * 8U));
    offset += 8U;
    (void)memcpy(material + offset, activity.bytes, activity.length);
    status = lxp_hash_sha256(material, length, output->resulting_state_root);
    if (status != LXP_OK) return status;
    output->result_code = activity.length == 0U ? LXP_ERR_NON_CANONICAL : LXP_OK;
    output->fee_charged = (lxp_u128){
        0U, (uint64_t)activity.length + parameters
    };
    output->effects = activity;
    output->resulting_balance = (lxp_byte_span){
        output->resulting_state_root, 16U
    };
    output->canonical_receipt = (lxp_byte_span){
        output->resulting_state_root, 32U
    };
    output->canonical_events = activity;
    return LXP_OK;
}

static int build_batch(lxp_replay_engine *engine, lxp_batch_body *body,
                       uint16_t version, uint64_t epoch, uint64_t batch,
                       uint64_t first_sequence,
                       const uint8_t previous_root[32],
                       const lxp_byte_span *activities, size_t count,
                       lxp_arena *arena, lxp_replay_batch_result *built)
{
    lxp_byte_span empty;
    static const uint8_t state_diff[] = { 9U, 8U };
    static const uint8_t recovery[] = { 7U, 6U };
    (void)memset(body, 0, sizeof(*body));
    body->header.protocol_version = version;
    body->header.network_id = 1U;
    body->header.epoch = epoch;
    body->header.batch_number = batch;
    body->header.first_sequence = first_sequence;
    body->header.last_sequence = first_sequence + count - 1U;
    body->header.timestamp_ms = 1000U + batch;
    (void)memcpy(body->header.previous_state_root, previous_root, 32U);
    if (lxp_replay_section_encode(activities, count, arena,
                                  &body->activities) != LXP_OK ||
        lxp_replay_section_encode(NULL, 0U, arena, &empty) != LXP_OK)
        return 11;
    body->oracle_inputs = empty;
    body->state_diff = (lxp_byte_span){ state_diff, sizeof(state_diff) };
    body->recovery_metadata = (lxp_byte_span){ recovery, sizeof(recovery) };
    if (lxp_replay_batch(engine, body, previous_root, arena, built) != LXP_OK)
        return 12;
    body->receipts = built->canonical_receipt_section;
    body->events = built->canonical_event_section;
    (void)memcpy(body->header.resulting_state_root,
                 built->resulting_state_root, 32U);
    (void)memcpy(body->header.activity_merkle_root,
                 built->roots.activity_merkle_root, 32U);
    (void)memcpy(body->header.receipt_merkle_root,
                 built->roots.receipt_merkle_root, 32U);
    (void)memcpy(body->header.event_merkle_root,
                 built->roots.event_merkle_root, 32U);
    (void)memcpy(body->header.oracle_root, built->roots.oracle_root, 32U);
    (void)memcpy(body->header.data_availability_root,
                 built->roots.data_availability_root, 32U);
    return lxp_replay_verify_roots(built, body) == LXP_OK ? 0 : 13;
}

int main(void)
{
    uint8_t history_storage[131072];
    uint8_t replay_storage[131072];
    uint8_t genesis[32] = { 0U };
    uint8_t a0[] = { 1U, 2U };
    uint8_t a1[] = { 3U, 4U };
    uint8_t a2[] = { 5U, 6U };
    uint8_t a3[] = { 7U, 8U };
    lxp_byte_span first_activities[2] = {{a0,2U},{a1,2U}};
    lxp_byte_span second_activities[2] = {{a2,2U},{a3,2U}};
    lxp_arena history_arena;
    lxp_arena replay_arena;
    lxp_replay_engine engine;
    lxp_batch_body first;
    lxp_batch_body second;
    lxp_replay_batch_result built_first;
    lxp_replay_batch_result built_second;
    lxp_replay_batch_result replayed_first;
    lxp_replay_batch_result replayed_second;
    lxp_replay_batch_result snapshot_second;
    uint8_t full_root[32];
    int built_status;
    if (lxp_arena_init(&history_arena, history_storage,
                       sizeof(history_storage)) != LXP_OK ||
        lxp_arena_init(&replay_arena, replay_storage,
                       sizeof(replay_storage)) != LXP_OK ||
        lxp_replay_engine_init(&engine, parameter_version, NULL) != LXP_OK ||
        lxp_replay_engine_register(&engine, 1U, transition) != LXP_OK)
        return 1;
    built_status = build_batch(&engine, &first, 1U, 1U, 0U, 0U, genesis,
                               first_activities, 2U, &history_arena,
                               &built_first);
    if (built_status != 0) return built_status;
    built_status = build_batch(&engine, &second, 1U, 2U, 1U, 2U,
                               first.header.resulting_state_root,
                               second_activities, 2U, &history_arena,
                               &built_second);
    if (built_status != 0) return built_status + 20;
    if (lxp_replay_batch(&engine, &first, genesis, &replay_arena,
                         &replayed_first) != LXP_OK ||
        lxp_replay_verify_roots(&replayed_first, &first) != LXP_OK ||
        lxp_replay_batch(&engine, &second, replayed_first.resulting_state_root,
                         &replay_arena, &replayed_second) != LXP_OK ||
        lxp_replay_verify_roots(&replayed_second, &second) != LXP_OK)
        return 41;
    (void)memcpy(full_root, replayed_second.resulting_state_root, 32U);
    if (lxp_arena_reset(&replay_arena, 0U) != LXP_OK ||
        lxp_replay_batch(&engine, &second,
                         first.header.resulting_state_root, &replay_arena,
                         &snapshot_second) != LXP_OK ||
        lxp_replay_verify_roots(&snapshot_second, &second) != LXP_OK ||
        memcmp(full_root, snapshot_second.resulting_state_root, 32U) != 0)
        return 42;
    ((uint8_t *)second.receipts.bytes)[second.receipts.length - 1U] ^= 1U;
    return lxp_replay_verify_roots(&snapshot_second, &second) ==
           LXP_FATAL_REPLAY_DIVERGENCE ? 0 : 1;
}
