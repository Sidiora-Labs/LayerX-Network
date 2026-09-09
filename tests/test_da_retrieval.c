#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_da.h"
#include "layerx/lxp_hash.h"
#include "support/lxp_real_replay.h"
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
    static uint8_t build_storage[16U * 1024U * 1024U];
    static uint8_t server_storage[16U * 1024U * 1024U];
    static uint8_t client_storage[16U * 1024U * 1024U];
    uint8_t genesis[32] = {0U};
    uint8_t activity_a[] = {1U, 3U, 5U, 7U};
    uint8_t activity_b[] = {2U, 4U, 6U, 8U, 10U};
    uint8_t oracle_a[] = {0x90U, 0x91U, 0x92U};
    lxp_byte_span activities[2] = {
        {activity_a, sizeof(activity_a)}, {activity_b, sizeof(activity_b)}
    };
    lxp_byte_span oracles[1] = {{oracle_a, sizeof(oracle_a)}};
    lxp_arena build_arena;
    lxp_arena server_arena;
    lxp_arena client_arena;
    static lxp_real_replay_fixture builder;
    static lxp_real_replay_fixture verifier;


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
        lxp_real_replay_init(&builder) != 0 ||
        lxp_real_replay_init(&verifier) != 0 ||
        lxp_da_store_init(&store, directory) != LXP_OK)
        return 1;
    (void)memcpy(genesis, builder.kernel.current_state_root, 32U);
    for (i = 0U; i < 2U; ++i)
        if (lxp_real_replay_activity(&builder, i, &build_arena, &activities[i]) != 0)
            return 1;
    if (lxp_real_replay_build(&builder, 1U, activities, 2U, oracles, 1U,
                              &build_arena, &body) != 0)
        return 1;
    verifier.execution.batch_number = 1U;
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
        lxp_hash_activity_id(activities[0].bytes, activities[0].length,
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
        lxp_da_verify_served_bytes(&fetched, &body.header, &verifier.engine,
                                   genesis, &client_arena, &replayed) !=
            LXP_OK ||
        replayed.canonical_receipt_section.length != body.receipts.length ||
        memcmp(replayed.canonical_receipt_section.bytes, body.receipts.bytes,
               body.receipts.length) != 0 ||
        memcmp(replayed.resulting_state_root,
               body.header.resulting_state_root, 32U) != 0)
        return 1;
    ((uint8_t *)fetched.chunks[0].bytes.bytes)[0] ^= 1U;
    if (lxp_da_verify_served_bytes(&fetched, &body.header, &verifier.engine,
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
