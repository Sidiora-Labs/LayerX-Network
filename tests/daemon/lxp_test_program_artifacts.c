#define OPENSSL_API_COMPAT 0x10100000L
#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static unsigned artifact_observer_calls;
static unsigned web_request_observer_calls;
static int artifact_store_observer(const lxp_receipt *executed);
static int web_request_store_observer(const lxp_receipt *executed);
int web_program_path_main(int argc, char **argv);
#define LXP_TEST_PROGRAM_ARTIFACT_OBSERVER artifact_store_observer
#define LXP_TEST_WEB_REQUEST_OBSERVER web_request_store_observer
#define LXP_TEST_WEB_PROGRAM_PATH_MAIN web_program_path_main
#include "../test_web_program_path.c"
#undef LXP_TEST_WEB_PROGRAM_PATH_MAIN
#undef LXP_TEST_WEB_REQUEST_OBSERVER
#undef LXP_TEST_PROGRAM_ARTIFACT_OBSERVER

static void artifact_hex(const uint8_t *bytes, size_t length, char *text)
{
    static const char digits[] = "0123456789abcdef";
    size_t index;
    for (index = 0U; index < length; ++index) {
        text[index * 2U] = digits[bytes[index] >> 4U];
        text[index * 2U + 1U] = digits[bytes[index] & 15U];
    }
    text[length * 2U] = '\0';
}

static int artifact_store_observer(const lxp_receipt *executed)
{
    static const uint8_t secret[32] = {0x39U};
    static const uint8_t bearer[] = "artifact-fixture-owned-bearer";
    char directory[] = "/tmp/lxp-program-artifacts-XXXXXX";
    char path[256], corrupt_path[256], legacy_path[256], route[256];
    char activity_hex[65], digest_hex[65];
    uint8_t digest[32], signature[64];
    uint8_t *storage = NULL, *body = NULL;
    char *terminal_hex = NULL, *graph_hex = NULL, *expected = NULL;
    size_t expected_capacity, mark, public_length = 32U;
    lxp_receipt receipt = *executed;
    lxp_batch_header batch = {0};
    lxp_sequencer_authorization authorization = {0};
    lxp_merkle_proof proof = {0};
    lxp_arena arena;
    lxp_byte_span canonical_receipt, canonical_header, canonical_events;
    lxp_daemon_receipt_evidence evidence;
    lxp_daemon_receipt_authority_store store, reopened, corrupted, legacy;
    lxp_daemon_protocol_owner *owner = NULL;
    lxp_daemon_protocol_response response;
    lxp_log log = {.descriptor = -1}, corrupt_log = {.descriptor = -1};
    lxp_log legacy_log = {.descriptor = -1};
    lxp_log_record_header record;
    EVP_PKEY *key = NULL;
    uint64_t offset;
    bool mutex_ready = false, directory_ready = false;
    int result = 1;
#define REQUIRE(expression) do { if (!(expression)) { \
    (void)fprintf(stderr, "program artifact check failed at line %d\n", __LINE__); \
    goto done; } } while (0)
    REQUIRE(receipt.result_code == LXP_OK && receipt.program_outcome.present &&
            receipt.program_outcome.terminal_payload.length != 0U &&
            receipt.program_outcome.call_graph_payload.length != 0U);
    storage = malloc(16U * LXP_MAX_ACTIVITY_BYTES);
    owner = calloc(1U, sizeof(*owner));
    REQUIRE(storage != NULL && owner != NULL);
    REQUIRE(lxp_arena_init(&arena, storage, 16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, secret, sizeof(secret));
    REQUIRE(key != NULL && EVP_PKEY_get_raw_public_key(
                key, authorization.public_key, &public_length) == 1 && public_length == 32U);
    memcpy(authorization.sequencer_id, authorization.public_key, 32U);
    authorization.authorized = 1U;
    authorization.first_batch_number = 1U;
    authorization.last_batch_number = 1U;
    REQUIRE(lxp_receipt_sign(&receipt, secret, &arena) == LXP_OK);
    REQUIRE(lxp_receipt_encode(&receipt, true, &arena, &canonical_receipt) == LXP_OK);
    REQUIRE(lxp_receipt_digest(&receipt, &arena, digest) == LXP_OK);
    batch.protocol_version = receipt.protocol_version;
    batch.network_id = 42U;
    batch.epoch = 1U;
    batch.batch_number = 1U;
    batch.first_sequence = receipt.global_sequence;
    batch.last_sequence = receipt.global_sequence;
    batch.timestamp_ms = receipt.timestamp;
    memcpy(batch.previous_state_root, receipt.previous_state_root, 32U);
    memcpy(batch.resulting_state_root, receipt.resulting_state_root, 32U);
    memcpy(batch.activity_merkle_root, receipt.activity_root, 32U);
    memcpy(batch.sequencer_id, authorization.sequencer_id, 32U);
    REQUIRE(lxp_merkle_leaf_hash(canonical_receipt.bytes, canonical_receipt.length,
                                batch.receipt_merkle_root) == LXP_OK);
    REQUIRE(lxp_programs_project_receipt_events(&receipt, &arena, &canonical_events) == LXP_OK);
    REQUIRE(lxp_merkle_leaf_hash(canonical_events.bytes, canonical_events.length,
                                batch.event_merkle_root) == LXP_OK);
    REQUIRE(lxp_merkle_leaf_hash(NULL, 0U, batch.oracle_root) == LXP_OK);
    memcpy(batch.data_availability_root, batch.oracle_root, 32U);
    proof.leaf_count = 1U;
    REQUIRE(lxp_batch_sign(&batch, secret, &authorization, signature, &arena) == LXP_OK);
    REQUIRE(lxp_batch_header_encode(&batch, &arena, &canonical_header) == LXP_OK);
    REQUIRE(mkdtemp(directory) != NULL);
    directory_ready = true;
    REQUIRE(snprintf(path, sizeof(path), "%s/authority.log", directory) > 0);
    REQUIRE(snprintf(corrupt_path, sizeof(corrupt_path), "%s/corrupt.log", directory) > 0);
    REQUIRE(snprintf(legacy_path, sizeof(legacy_path), "%s/legacy.log", directory) > 0);
    REQUIRE(lxp_log_open_or_create(&log, path, 16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&store, &log, &authorization) == LXP_OK);
    {
        lxp_result append_status = lxp_daemon_receipt_authority_append_artifacts(&store,
            canonical_receipt.bytes, canonical_receipt.length,
            canonical_header.bytes, canonical_header.length, signature, &proof, &arena,
            receipt.program_outcome.terminal_payload,
            receipt.program_outcome.call_graph_payload);
        if (append_status != LXP_OK)
            (void)fprintf(stderr, "artifact append result=%d zero_batch=%u\n",
                (int)append_status, lxp_ct_is_zero(receipt.batch_id, 32U) ? 1U : 0U);
        REQUIRE(append_status == LXP_OK);
    }
    REQUIRE(lxp_log_close(&log) == LXP_OK);
    REQUIRE(lxp_log_open(&log, path) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&reopened, &log, &authorization) == LXP_OK);
    mark = lxp_arena_mark(&arena);
    REQUIRE(lxp_daemon_receipt_authority_lookup(&reopened, digest, &arena, &evidence) == LXP_OK);
    REQUIRE(evidence.format_version == 2U &&
        evidence.terminal_payload.length == receipt.program_outcome.terminal_payload.length &&
        evidence.call_graph.length == receipt.program_outcome.call_graph_payload.length &&
        memcmp(evidence.terminal_payload.bytes, receipt.program_outcome.terminal_payload.bytes,
               evidence.terminal_payload.length) == 0 &&
        memcmp(evidence.call_graph.bytes, receipt.program_outcome.call_graph_payload.bytes,
               evidence.call_graph.length) == 0);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    REQUIRE(pthread_mutex_init(&owner->mutex, NULL) == 0);
    mutex_ready = true;
    owner->attached = true;
    owner->receipt_authority = &reopened;
    memcpy(owner->bearer_token, bearer, sizeof(bearer) - 1U);
    owner->bearer_token_length = sizeof(bearer) - 1U;
    artifact_hex(receipt.activity_id, 32U, activity_hex);
    artifact_hex(digest, 32U, digest_hex);
    REQUIRE(snprintf(route, sizeof(route),
        "/v1/programs/activities/%s/artifacts?receipt_digest=%s", activity_hex, digest_hex) > 0);
    terminal_hex = malloc(receipt.program_outcome.terminal_payload.length * 2U + 1U);
    graph_hex = malloc(receipt.program_outcome.call_graph_payload.length * 2U + 1U);
    expected_capacity = 256U + receipt.program_outcome.terminal_payload.length * 2U +
        receipt.program_outcome.call_graph_payload.length * 2U;
    expected = malloc(expected_capacity);
    REQUIRE(terminal_hex != NULL && graph_hex != NULL && expected != NULL);
    artifact_hex(receipt.program_outcome.terminal_payload.bytes,
                 receipt.program_outcome.terminal_payload.length, terminal_hex);
    artifact_hex(receipt.program_outcome.call_graph_payload.bytes,
                 receipt.program_outcome.call_graph_payload.length, graph_hex);
    REQUIRE(snprintf(expected, expected_capacity,
        "{\"activity_id\":\"%s\",\"receipt_digest\":\"%s\",\"terminal_payload\":\"%s\",\"call_graph\":\"%s\"}",
        activity_hex, digest_hex, terminal_hex, graph_hex) > 0);
    REQUIRE(lxp_daemon_protocol_route(owner, bearer, sizeof(bearer) - 1U,
                "GET", route, &arena, &response) == LXP_OK && response.status == 200U);
    REQUIRE(response.body.length == strlen(expected) &&
            memcmp(response.body.bytes, expected, response.body.length) == 0);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    REQUIRE(lxp_daemon_protocol_route(owner, NULL, 0U,
                "GET", route, &arena, &response) == LXP_OK && response.status == 401U);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    route[24] = route[24] == '0' ? '1' : '0';
    REQUIRE(lxp_daemon_protocol_route(owner, bearer, sizeof(bearer) - 1U,
                "GET", route, &arena, &response) == LXP_OK && response.status == 503U);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    REQUIRE(snprintf(route, sizeof(route),
        "/v1/programs/activities/%s/artifacts?receipt_digest=%s", activity_hex, digest_hex) > 0);
    REQUIRE(lxp_log_read(&log, 0U, &record, NULL, 0U) == LXP_ERR_LENGTH_LIMIT);
    body = malloc(record.body_length);
    REQUIRE(body != NULL && lxp_log_read(&log, 0U, &record, body, record.body_length) == LXP_OK);
    {
        const size_t changed_offsets[3] = {
            record.body_length - 1U,
            5U + 32U + 32U + 8U + 2U + canonical_header.length,
            5U};
        size_t attempt;
        for (attempt = 0U; attempt < 4U; ++attempt) {
            uint32_t body_length = record.body_length;
            if (attempt < 3U) body[changed_offsets[attempt]] ^= 1U;
            else --body_length;
            REQUIRE(lxp_log_open_or_create(&corrupt_log, corrupt_path,
                                          16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
            REQUIRE(lxp_log_append(&corrupt_log, LXP_LOG_STATE_DIFF, receipt.global_sequence,
                    body, body_length, &offset) == LXP_OK);
            REQUIRE(lxp_log_write_boundary(&corrupt_log) == LXP_OK);
            REQUIRE(lxp_daemon_receipt_authority_open(
                &corrupted, &corrupt_log, &authorization) != LXP_OK);
            REQUIRE(lxp_log_close(&corrupt_log) == LXP_OK);
            REQUIRE(unlink(corrupt_path) == 0);
            if (attempt < 3U) body[changed_offsets[attempt]] ^= 1U;
        }
    }
    REQUIRE(lxp_log_open_or_create(&legacy_log, legacy_path,
                                  16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&legacy, &legacy_log, &authorization) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_append(&legacy,
        canonical_receipt.bytes, canonical_receipt.length, canonical_header.bytes,
        canonical_header.length, signature, &proof, &arena) == LXP_OK);
    REQUIRE(lxp_log_close(&legacy_log) == LXP_OK);
    REQUIRE(lxp_log_open(&legacy_log, legacy_path) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&legacy, &legacy_log, &authorization) == LXP_OK);
    owner->receipt_authority = &legacy;
    REQUIRE(lxp_daemon_protocol_route(owner, bearer, sizeof(bearer) - 1U,
                "GET", route, &arena, &response) == LXP_OK && response.status == 503U);
    ++artifact_observer_calls;
    result = 0;
done:
    if (mutex_ready) (void)pthread_mutex_destroy(&owner->mutex);
    if (log.descriptor >= 0) (void)lxp_log_close(&log);
    if (corrupt_log.descriptor >= 0) (void)lxp_log_close(&corrupt_log);
    if (legacy_log.descriptor >= 0) (void)lxp_log_close(&legacy_log);
    if (directory_ready) {
        (void)unlink(path); (void)unlink(corrupt_path); (void)unlink(legacy_path);
        (void)rmdir(directory);
    }
    EVP_PKEY_free(key);
    free(expected); free(graph_hex); free(terminal_hex);
    free(body); free(owner); free(storage);
#undef REQUIRE
    return result;
}

static int web_request_event(lxp_byte_span list, const uint8_t **program,
                             lxp_byte_span *data)
{
    static const uint8_t domain[] = "LayerX/programs/events/v1";
    size_t cursor = sizeof(domain);
    size_t found = 0U;
    uint32_t count, index;
    if (list.bytes == NULL || list.length < sizeof(domain) + 4U ||
        memcmp(list.bytes, domain, sizeof(domain)) != 0)
        return 1;
    count = path_read_u32(list.bytes + cursor);
    cursor += 4U;
    for (index = 0U; index < count; ++index) {
        const uint8_t *event_program;
        const uint8_t *topic;
        uint32_t topic_length, data_length;
        if (list.length - cursor < 32U + 32U + 8U + 1U + 4U) return 1;
        event_program = list.bytes + cursor;
        cursor += 32U + 32U + 8U + 1U;
        topic_length = path_read_u32(list.bytes + cursor);
        cursor += 4U;
        if (list.length - cursor < (size_t)topic_length + 4U) return 1;
        topic = list.bytes + cursor;
        cursor += topic_length;
        data_length = path_read_u32(list.bytes + cursor);
        cursor += 4U;
        if (list.length - cursor < data_length) return 1;
        if (topic_length == LX_WEB_REQUEST_TOPIC_BYTES &&
            memcmp(topic, LX_WEB_REQUEST_TOPIC, LX_WEB_REQUEST_TOPIC_BYTES) == 0) {
            *program = event_program;
            *data = (lxp_byte_span){list.bytes + cursor, data_length};
            ++found;
        }
        cursor += data_length;
    }
    return cursor == list.length && found == 1U ? 0 : 1;
}

static int events_page(lxp_daemon_protocol_owner *owner, const uint8_t *bearer,
                       size_t bearer_length, const char *route, lxp_arena *arena,
                       uint16_t status, const char *expected)
{
    lxp_daemon_protocol_response response = {0};
    size_t mark = lxp_arena_mark(arena);
    int result = lxp_daemon_protocol_route(owner, bearer, bearer_length, "GET",
                                           route, arena, &response) == LXP_OK &&
                         response.status == status &&
                         (expected == NULL ||
                          (response.body.length == strlen(expected) &&
                           memcmp(response.body.bytes, expected,
                                  response.body.length) == 0)) ? 0 : 1;
    if (result != 0)
        (void)fprintf(stderr, "events route %s answered %u %.*s\n", route,
                      (unsigned)response.status, (int)response.body.length,
                      (const char *)response.body.bytes);
    (void)lxp_arena_reset(arena, mark);
    return result;
}

static int web_request_store_observer(const lxp_receipt *executed)
{
    static const uint8_t secret[32] = {0x3aU};
    static const uint8_t bearer[] = "web-request-fixture-owned-bearer";
    static const char other_topic_hex[] =
        "504158454552585f4f544845525f544f5049435f5631";
    char directory[] = "/tmp/lxp-program-events-XXXXXX";
    char path[256], plain_path[256], route[512], other_route[512];
    char topic_hex[2U * LX_WEB_REQUEST_TOPIC_BYTES + 1U];
    char program_hex[65], activity_hex[65], digest_hex[65];
    uint8_t digest[32], signature[64];
    uint8_t *storage = NULL, *tampered = NULL;
    char *data_hex = NULL, *expected = NULL, *empty = NULL;
    const uint8_t *event_program = NULL;
    lxp_byte_span event_data = {NULL, 0U};
    lxp_byte_span list = executed->program_outcome.event_envelope_payload;
    lxp_byte_span request_payload = {NULL, 0U};
    size_t expected_capacity, mark, public_length = 32U;
    uint64_t request_id = 0U, sequence, head;
    uint8_t request_kind = 0U;
    lxp_receipt receipt = *executed;
    lxp_batch_header batch = {0};
    lxp_sequencer_authorization authorization = {0};
    lxp_merkle_proof proof = {0};
    lxp_arena arena;
    lxp_byte_span canonical_receipt, canonical_header, canonical_events;
    lxp_daemon_receipt_evidence evidence;
    lxp_daemon_receipt_authority_store store, reopened, plain;
    lxp_daemon_protocol_owner *owner = NULL;
    lxp_log log = {.descriptor = -1}, plain_log = {.descriptor = -1};
    EVP_PKEY *key = NULL;
    bool mutex_ready = false, directory_ready = false;
    int result = 1;
#define REQUIRE(expression) do { if (!(expression)) { \
    (void)fprintf(stderr, "program events check failed at line %d\n", __LINE__); \
    goto done; } } while (0)
    REQUIRE(receipt.result_code == LXP_OK && receipt.program_outcome.present &&
            receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS &&
            list.length != 0U);
    REQUIRE(web_request_event(list, &event_program, &event_data) == 0);
    REQUIRE(lx_web_request_record_decode(event_data.bytes, event_data.length,
                                         &request_id, &request_kind,
                                         &request_payload) == LXP_OK);
    REQUIRE(request_id == path_request && request_kind == LX_WEB_KIND_FETCH &&
            request_payload.length == PATH_PAYLOAD_BYTES &&
            memcmp(request_payload.bytes, path_payload, PATH_PAYLOAD_BYTES) == 0);
    storage = malloc(16U * LXP_MAX_ACTIVITY_BYTES);
    owner = calloc(1U, sizeof(*owner));
    tampered = malloc(list.length);
    REQUIRE(storage != NULL && owner != NULL && tampered != NULL);
    REQUIRE(lxp_arena_init(&arena, storage, 16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, secret, sizeof(secret));
    REQUIRE(key != NULL && EVP_PKEY_get_raw_public_key(
                key, authorization.public_key, &public_length) == 1 && public_length == 32U);
    memcpy(authorization.sequencer_id, authorization.public_key, 32U);
    authorization.authorized = 1U;
    authorization.first_batch_number = 1U;
    authorization.last_batch_number = 1U;
    REQUIRE(lxp_receipt_sign(&receipt, secret, &arena) == LXP_OK);
    REQUIRE(lxp_receipt_encode(&receipt, true, &arena, &canonical_receipt) == LXP_OK);
    REQUIRE(lxp_receipt_digest(&receipt, &arena, digest) == LXP_OK);
    sequence = receipt.global_sequence;
    REQUIRE(sequence != 0U && sequence < UINT64_MAX);
    head = sequence + 1U;
    batch.protocol_version = receipt.protocol_version;
    batch.network_id = 42U;
    batch.epoch = 1U;
    batch.batch_number = 1U;
    batch.first_sequence = sequence;
    batch.last_sequence = sequence;
    batch.timestamp_ms = receipt.timestamp;
    memcpy(batch.previous_state_root, receipt.previous_state_root, 32U);
    memcpy(batch.resulting_state_root, receipt.resulting_state_root, 32U);
    memcpy(batch.activity_merkle_root, receipt.activity_root, 32U);
    memcpy(batch.sequencer_id, authorization.sequencer_id, 32U);
    REQUIRE(lxp_merkle_leaf_hash(canonical_receipt.bytes, canonical_receipt.length,
                                batch.receipt_merkle_root) == LXP_OK);
    REQUIRE(lxp_programs_project_receipt_events(&receipt, &arena, &canonical_events) == LXP_OK);
    REQUIRE(lxp_merkle_leaf_hash(canonical_events.bytes, canonical_events.length,
                                batch.event_merkle_root) == LXP_OK);
    REQUIRE(lxp_merkle_leaf_hash(NULL, 0U, batch.oracle_root) == LXP_OK);
    memcpy(batch.data_availability_root, batch.oracle_root, 32U);
    proof.leaf_count = 1U;
    REQUIRE(lxp_batch_sign(&batch, secret, &authorization, signature, &arena) == LXP_OK);
    REQUIRE(lxp_batch_header_encode(&batch, &arena, &canonical_header) == LXP_OK);
    REQUIRE(mkdtemp(directory) != NULL);
    directory_ready = true;
    REQUIRE(snprintf(path, sizeof(path), "%s/authority.log", directory) > 0);
    REQUIRE(snprintf(plain_path, sizeof(plain_path), "%s/plain.log", directory) > 0);
    REQUIRE(lxp_log_open_or_create(&log, path, 16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&store, &log, &authorization) == LXP_OK);
    memcpy(tampered, list.bytes, list.length);
    tampered[list.length - 1U] ^= 1U;
    REQUIRE(lxp_daemon_receipt_authority_append_event_list(&store,
        canonical_receipt.bytes, canonical_receipt.length,
        canonical_header.bytes, canonical_header.length, signature, &proof, &arena,
        receipt.program_outcome.terminal_payload,
        receipt.program_outcome.call_graph_payload,
        (lxp_byte_span){tampered, list.length}) != LXP_OK);
    REQUIRE(store.record_count == 0U && log.write_offset == 0U);
    REQUIRE(lxp_daemon_receipt_authority_append_event_list(&store,
        canonical_receipt.bytes, canonical_receipt.length,
        canonical_header.bytes, canonical_header.length, signature, &proof, &arena,
        receipt.program_outcome.terminal_payload,
        receipt.program_outcome.call_graph_payload, list) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_append_event_list(&store,
        canonical_receipt.bytes, canonical_receipt.length,
        canonical_header.bytes, canonical_header.length, signature, &proof, &arena,
        receipt.program_outcome.terminal_payload,
        receipt.program_outcome.call_graph_payload, list) == LXP_OK);
    REQUIRE(store.record_count == 1U);
    REQUIRE(lxp_log_close(&log) == LXP_OK);
    REQUIRE(lxp_log_open(&log, path) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&reopened, &log, &authorization) == LXP_OK);
    mark = lxp_arena_mark(&arena);
    REQUIRE(lxp_daemon_receipt_authority_lookup(&reopened, digest, &arena, &evidence) == LXP_OK);
    REQUIRE(evidence.format_version == 4U && evidence.global_sequence == sequence &&
            evidence.event_list.length == list.length &&
            memcmp(evidence.event_list.bytes, list.bytes, list.length) == 0 &&
            evidence.terminal_payload.length ==
                receipt.program_outcome.terminal_payload.length &&
            evidence.call_graph.length == receipt.program_outcome.call_graph_payload.length);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    REQUIRE(pthread_mutex_init(&owner->mutex, NULL) == 0);
    mutex_ready = true;
    owner->attached = true;
    owner->receipt_authority = &reopened;
    memcpy(owner->bearer_token, bearer, sizeof(bearer) - 1U);
    owner->bearer_token_length = sizeof(bearer) - 1U;
    artifact_hex(receipt.activity_id, 32U, activity_hex);
    artifact_hex(digest, 32U, digest_hex);
    REQUIRE(snprintf(route, sizeof(route),
        "/v1/programs/activities/%s/artifacts?receipt_digest=%s", activity_hex, digest_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, NULL) == 0);
    artifact_hex((const uint8_t *)LX_WEB_REQUEST_TOPIC, LX_WEB_REQUEST_TOPIC_BYTES, topic_hex);
    artifact_hex(event_program, 32U, program_hex);
    data_hex = malloc(event_data.length * 2U + 1U);
    expected_capacity = 512U + event_data.length * 2U;
    expected = malloc(expected_capacity);
    empty = malloc(128U);
    REQUIRE(data_hex != NULL && expected != NULL && empty != NULL);
    artifact_hex(event_data.bytes, event_data.length, data_hex);
    REQUIRE(snprintf(expected, expected_capacity,
        "{\"events\":[{\"sequence\":%llu,\"program_id\":\"%s\",\"topic\":\"%s\","
        "\"data\":\"%s\"}],\"next_sequence\":%llu}",
        (unsigned long long)sequence, program_hex, topic_hex, data_hex,
        (unsigned long long)head) > 0);
    REQUIRE(snprintf(empty, 128U, "{\"events\":[],\"next_sequence\":%llu}",
                     (unsigned long long)head) > 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/0/256", topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, expected) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/%llu/1", topic_hex,
                     (unsigned long long)sequence) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, expected) == 0);
    REQUIRE(events_page(owner, NULL, 0U, route, &arena, 401U, NULL) == 0);
    REQUIRE(snprintf(other_route, sizeof(other_route), "/v1/programs/events/%s/0/256",
                     other_topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, other_route, &arena, 200U, empty) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/%llu/256", topic_hex,
                     (unsigned long long)head) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, empty) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/%llu/256", topic_hex,
                     (unsigned long long)(head + 7U)) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, empty) == 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U,
        "/v1/programs/events/504158454552585F5745425F524551554553545F5631/0/256",
        &arena, 503U, NULL) == 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U,
        "/v1/programs/events/504/0/256", &arena, 503U, NULL) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/0/0", topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 503U, NULL) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/0/257", topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 503U, NULL) == 0);
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/x/256", topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 503U, NULL) == 0);
    REQUIRE(lxp_log_open_or_create(&plain_log, plain_path,
                                  16U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&plain, &plain_log, &authorization) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_append_artifacts(&plain,
        canonical_receipt.bytes, canonical_receipt.length,
        canonical_header.bytes, canonical_header.length, signature, &proof, &arena,
        receipt.program_outcome.terminal_payload,
        receipt.program_outcome.call_graph_payload) == LXP_OK);
    REQUIRE(lxp_log_close(&plain_log) == LXP_OK);
    REQUIRE(lxp_log_open(&plain_log, plain_path) == LXP_OK);
    REQUIRE(lxp_daemon_receipt_authority_open(&plain, &plain_log, &authorization) == LXP_OK);
    mark = lxp_arena_mark(&arena);
    REQUIRE(lxp_daemon_receipt_authority_lookup(&plain, digest, &arena, &evidence) == LXP_OK &&
            evidence.format_version == 2U && evidence.event_list.length == 0U);
    REQUIRE(lxp_arena_reset(&arena, mark) == LXP_OK);
    owner->receipt_authority = &plain;
    REQUIRE(snprintf(route, sizeof(route), "/v1/programs/events/%s/0/256", topic_hex) > 0);
    REQUIRE(events_page(owner, bearer, sizeof(bearer) - 1U, route, &arena, 200U, empty) == 0);
    ++web_request_observer_calls;
    result = 0;
done:
    if (mutex_ready) (void)pthread_mutex_destroy(&owner->mutex);
    if (log.descriptor >= 0) (void)lxp_log_close(&log);
    if (plain_log.descriptor >= 0) (void)lxp_log_close(&plain_log);
    if (directory_ready) {
        (void)unlink(path); (void)unlink(plain_path);
        (void)rmdir(directory);
    }
    EVP_PKEY_free(key);
    free(empty); free(expected); free(data_hex);
    free(tampered); free(owner); free(storage);
#undef REQUIRE
    return result;
}

int main(int argc, char **argv)
{
    if (argc != 2) {
        (void)fprintf(stderr, "usage: %s web-reader.wasm\n", argv[0]);
        return 2;
    }
    if (deploy_and_upgrade_persist_exact_artifacts() != 0) return 1;
    if (deploy_and_upgrade_persist_exact_artifacts_version(
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT) != 0) return 1;
    if (web_program_path_main(argc, argv) != 0) return 1;
    return artifact_observer_calls == 2U && web_request_observer_calls == 2U ? 0 : 1;
}
