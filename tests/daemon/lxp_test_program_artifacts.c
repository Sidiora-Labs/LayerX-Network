#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"
#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static unsigned artifact_observer_calls;
static int artifact_store_observer(const lxp_receipt *executed);
#define LXP_TEST_PROGRAM_ARTIFACT_OBSERVER artifact_store_observer
#define main program_activity_fixture_main
#include "../programs/test_call_activity.c"
#undef main
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

int main(void)
{
    if (deploy_and_upgrade_persist_exact_artifacts() != 0) return 1;
    if (deploy_and_upgrade_persist_exact_artifacts_version(
            LXP_PROTOCOL_VERSION_STATE_COMMITMENT) != 0) return 1;
    return artifact_observer_calls == 2U ? 0 : 1;
}
