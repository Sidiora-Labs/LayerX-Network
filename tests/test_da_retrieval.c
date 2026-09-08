#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_da.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_replica.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct service_context {
    lxp_da_store *store;
    lxp_arena *arena;
    uint64_t batch_number;
    uint64_t first_sequence;
    uint64_t last_sequence;
    uint8_t checkpoint_id[32];
    uint8_t activity_id[32];
} service_context;

static lxp_result parameter_version(void *context, uint64_t epoch,
                                    uint32_t *version)
{
    (void)context;
    if (epoch > UINT32_MAX - 30U) return LXP_ERR_OVERFLOW;
    *version = (uint32_t)epoch + 30U;
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
    (void)memcpy(material, previous_root, 32U);
    offset += 32U;
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
    output->result_code = LXP_OK;
    output->fee_charged = (lxp_u128){0U, parameters + activity.length};
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

static lxp_result fetch_chunk(void *context,
                              const lxp_da_retrieval_request *request,
                              uint32_t chunk_index, lxp_arena *unused,
                              lxp_byte_span *response)
{
    service_context *service = (service_context *)context;
    int matches = 0;
    (void)unused;
    switch (request->lookup_kind) {
    case LXP_DA_LOOKUP_CHECKPOINT_ID:
        matches = memcmp(request->checkpoint_id,
                         service->checkpoint_id, 32U) == 0;
        break;
    case LXP_DA_LOOKUP_BATCH_NUMBER:
        matches = request->batch_number == service->batch_number;
        break;
    case LXP_DA_LOOKUP_SEQUENCE_RANGE:
        matches = request->first_global_sequence >= service->first_sequence &&
            request->last_global_sequence <= service->last_sequence;
        break;
    case LXP_DA_LOOKUP_ACTIVITY_ID:
        matches = memcmp(request->activity_id,
                         service->activity_id, 32U) == 0;
        break;
    default:
        break;
    }
    if (!matches || lxp_arena_reset(service->arena, 0U) != LXP_OK)
        return LXP_ERR_DA_MISSING;
    return lxp_da_serve_chunk(service->store, service->batch_number,
                              chunk_index, service->arena, response);
}

int main(void)
{
    uint8_t build_storage[262144];
    uint8_t server_storage[262144];
    uint8_t client_storage[524288];
    uint8_t genesis[32] = {0U};
    uint8_t activity_a[] = {1U, 3U, 5U, 7U};
    uint8_t activity_b[] = {2U, 4U, 6U, 8U, 10U};
    uint8_t oracle_a[] = {0x90U, 0x91U, 0x92U};
    uint8_t state_diff[] = {0xa0U, 0xa1U, 0xa2U, 0xa3U};
    uint8_t recovery[] = {0xb0U, 0xb1U, 0xb2U};
    lxp_byte_span activities[2] = {
        {activity_a, sizeof(activity_a)}, {activity_b, sizeof(activity_b)}
    };
    lxp_byte_span oracles[1] = {{oracle_a, sizeof(oracle_a)}};
    lxp_arena build_arena;
    lxp_arena server_arena;
    lxp_arena client_arena;
    lxp_replay_engine build_engine;
    lxp_replay_engine client_engine;
    lxp_replay_batch_result built;
    lxp_replay_batch_result replayed;
    lxp_batch_body body;
    lxp_da_bundle bundle;
    lxp_da_bundle fetched;
    lxp_da_store store;
    lxp_da_retrieval_request request;
    service_context service;
    uint8_t fetched_root[32];
    uint8_t original_root[32];
    lxp_byte_span first_response;
    lxp_byte_span second_response;
    uint8_t first_copy[4096];
    size_t first_length;
    char directory[] = "/tmp/lxp-da-retrieval-XXXXXX";
    char path[LXP_DA_STORE_PATH_BYTES];
    size_t i;

    if (mkdtemp(directory) == NULL ||
        lxp_arena_init(&build_arena, build_storage,
                       sizeof(build_storage)) != LXP_OK ||
        lxp_arena_init(&server_arena, server_storage,
                       sizeof(server_storage)) != LXP_OK ||
        lxp_arena_init(&client_arena, client_storage,
                       sizeof(client_storage)) != LXP_OK ||
        lxp_replay_engine_init(&build_engine, parameter_version, NULL) !=
            LXP_OK ||
        lxp_replay_engine_register(&build_engine, 1U, transition) != LXP_OK ||
        lxp_replay_engine_init(&client_engine, parameter_version, NULL) !=
            LXP_OK ||
        lxp_replay_engine_register(&client_engine, 1U, transition) != LXP_OK ||
        lxp_da_store_init(&store, directory) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.protocol_version = 1U;
    body.header.network_id = 44U;
    body.header.epoch = 6U;
    body.header.batch_number = 31U;
    body.header.first_sequence = 500U;
    body.header.last_sequence = 501U;
    body.header.timestamp_ms = 1700000001000U;
    body.state_diff = (lxp_byte_span){state_diff, sizeof(state_diff)};
    body.recovery_metadata = (lxp_byte_span){recovery, sizeof(recovery)};
    if (lxp_replay_section_encode(activities, 2U, &build_arena,
                                  &body.activities) != LXP_OK ||
        lxp_replay_section_encode(oracles, 1U, &build_arena,
                                  &body.oracle_inputs) != LXP_OK ||
        lxp_replay_batch(&build_engine, &body, genesis, &build_arena,
                         &built) != LXP_OK)
        return 1;
    body.receipts = built.canonical_receipt_section;
    body.events = built.canonical_event_section;
    (void)memcpy(body.header.resulting_state_root,
                 built.resulting_state_root, 32U);
    (void)memcpy(body.header.activity_merkle_root,
                 built.roots.activity_merkle_root, 32U);
    (void)memcpy(body.header.receipt_merkle_root,
                 built.roots.receipt_merkle_root, 32U);
    (void)memcpy(body.header.event_merkle_root,
                 built.roots.event_merkle_root, 32U);
    (void)memcpy(body.header.oracle_root, built.roots.oracle_root, 32U);
    if (lxp_da_bundle_build(&body, LXP_DA_CANONICAL_CHUNK_BYTES, &build_arena, &bundle) != LXP_OK ||
        lxp_batch_availability_root(&body, &build_arena, original_root) != LXP_OK)
        return 1;
    (void)memcpy(body.header.data_availability_root, original_root, 32U);
    if (lxp_da_store_bundle(&store, &bundle, &build_arena) != LXP_OK)
        return 1;

    (void)memset(&service, 0, sizeof(service));
    service.store = &store;
    service.arena = &server_arena;
    service.batch_number = body.header.batch_number;
    service.first_sequence = body.header.first_sequence;
    service.last_sequence = body.header.last_sequence;
    if (lxp_batch_header_hash(&body.header, &build_arena,
                              service.checkpoint_id) != LXP_OK ||
        lxp_hash_activity_id(activity_a, sizeof(activity_a),
                             service.activity_id) != LXP_OK)
        return 1;

    if (lxp_da_serve_chunk(&store, body.header.batch_number, 0U,
                           &server_arena, &first_response) != LXP_OK ||
        first_response.length > sizeof(first_copy))
        return 1;
    first_length = first_response.length;
    (void)memcpy(first_copy, first_response.bytes, first_length);
    if (lxp_arena_reset(&server_arena, 0U) != LXP_OK ||
        lxp_da_serve_chunk(&store, body.header.batch_number, 0U,
                           &server_arena, &second_response) != LXP_OK ||
        second_response.length != first_length ||
        memcmp(first_copy, second_response.bytes, first_length) != 0)
        return 1;

    (void)memset(&request, 0, sizeof(request));
    request.lookup_kind = LXP_DA_LOOKUP_BATCH_NUMBER;
    request.batch_number = body.header.batch_number;
    if (lxp_da_fetch(&request, fetch_chunk, &service, &client_arena,
                     &fetched, fetched_root) != LXP_OK ||
        memcmp(fetched_root, original_root, 32U) != 0 ||
        lxp_da_verify_served_bytes(&fetched, &body.header, &client_engine,
                                   genesis, &client_arena, &replayed) !=
            LXP_OK ||
        replayed.canonical_receipt_section.length != body.receipts.length ||
        memcmp(replayed.canonical_receipt_section.bytes, body.receipts.bytes,
               body.receipts.length) != 0 ||
        memcmp(replayed.resulting_state_root,
               body.header.resulting_state_root, 32U) != 0)
        return 1;
    ((uint8_t *)fetched.chunks[0].bytes.bytes)[0] ^= 1U;
    if (lxp_da_verify_served_bytes(&fetched, &body.header, &client_engine,
                                   genesis, &client_arena, &replayed) ==
        LXP_OK)
        return 1;

    for (i = LXP_DA_LOOKUP_CHECKPOINT_ID;
         i <= LXP_DA_LOOKUP_ACTIVITY_ID; ++i) {
        if (lxp_arena_reset(&client_arena, 0U) != LXP_OK) return 1;
        (void)memset(&request, 0, sizeof(request));
        request.lookup_kind = (lxp_da_lookup_kind)i;
        request.batch_number = body.header.batch_number;
        request.first_global_sequence = body.header.first_sequence;
        request.last_global_sequence = body.header.last_sequence;
        (void)memcpy(request.checkpoint_id, service.checkpoint_id, 32U);
        (void)memcpy(request.activity_id, service.activity_id, 32U);
        if (lxp_da_fetch(&request, fetch_chunk, &service, &client_arena,
                         &fetched, fetched_root) != LXP_OK ||
            memcmp(fetched_root, original_root, 32U) != 0)
            return 1;
    }

    if (snprintf(path, sizeof(path), "%s/%020llu.lxda", directory,
                 (unsigned long long)body.header.batch_number) < 0 ||
        unlink(path) != 0 || rmdir(directory) != 0)
        return 1;
    return 0;
}
