#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_receipt.h"

#include <sqlite3.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct served_history {
    uint8_t activities[4096];
    uint8_t receipts[4096];
    uint8_t events[4096];
    uint8_t oracles[4096];
    uint8_t batch[8192];
    size_t activity_length;
    size_t receipt_length;
    size_t event_length;
    size_t oracle_length;
    size_t batch_length;
    size_t calls;
} served_history;

static lxp_result receive_exact(void *context, uint8_t record_kind,
                                uint64_t global_sequence,
                                const uint8_t *bytes, size_t length)
{
    served_history *served = (served_history *)context;
    uint8_t *destination = NULL;
    size_t *destination_length = NULL;
    if (global_sequence != 10U) return LXP_ERR_SEQUENCE_MISMATCH;
    if (record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
        destination = served->activities;
        destination_length = &served->activity_length;
    } else if (record_kind == (uint8_t)LXP_LOG_RECEIPT) {
        destination = served->receipts;
        destination_length = &served->receipt_length;
    } else if (record_kind == (uint8_t)LXP_LOG_ORACLE) {
        destination = served->oracles;
        destination_length = &served->oracle_length;
    } else if (record_kind == (uint8_t)LXP_LOG_BATCH_BODY) {
        destination = served->batch;
        destination_length = &served->batch_length;
    } else {
        ++served->calls;
        return LXP_OK;
    }
    if (length > 4096U || *destination_length != 0U)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(destination, bytes, length);
    *destination_length = length;
    ++served->calls;
    return LXP_OK;
}

static lxp_result count_only(void *context, uint8_t record_kind,
                             uint64_t global_sequence,
                             const uint8_t *bytes, size_t length)
{
    size_t *calls = (size_t *)context;
    (void)record_kind;
    (void)global_sequence;
    (void)bytes;
    (void)length;
    ++*calls;
    return LXP_OK;
}

static int span_equal(lxp_byte_span span, const uint8_t *bytes, size_t length)
{
    return span.length == length && memcmp(span.bytes, bytes, length) == 0;
}

int main(void)
{
    uint8_t *arena_storage;
    uint8_t activity_storage[4096];
    uint8_t receipt_storage[4096];
    uint8_t batch_storage[8192];
    uint8_t checkpoint[] = { 0x41U, 0x52U, 0x43U, 0x48U, 1U, 2U, 3U };
    uint8_t event[] = { 0x45U, 0x56U, 0x45U, 0x4eU, 0x54U };
    uint8_t oracle[] = { 0x4fU, 0x52U, 0x41U, 0x43U, 0x4cU, 0x45U };
    uint8_t actor[] = { 'd', 'i', 'd', ':', 'l', 'x', ':', 'a' };
    uint8_t authority[] = { 1U, 2U };
    uint8_t payload[] = { 9U, 8U, 7U };
    uint8_t signature[64] = { 6U };
    uint8_t activity_id[32];
    uint8_t checkpoint_id[32];
    uint8_t state_root[32] = { 3U };
    uint8_t batch_id[32] = { 4U };
    lxp_activity activity;
    lxp_receipt receipt_value;
    lxp_effect_buffer effects;
    lxp_batch_body batch;
    lxp_batch_body served_batch;
    lxp_batch_root_inputs root_inputs;
    lxp_batch_roots roots;
    lxp_byte_span encoded;
    lxp_byte_span activity_span;
    lxp_byte_span receipt_span;
    lxp_byte_span event_span = { event, sizeof(event) };
    lxp_byte_span oracle_span = { oracle, sizeof(oracle) };
    lxp_history history;
    lxp_history_query_spec query;
    lxp_history_result result;
    lxp_receipt_query receipt_query;
    lxp_byte_span looked_up;
    lxp_arena arena;
    lxp_log log;
    served_history served;
    char directory[] = "/tmp/lxp-history-XXXXXX";
    char log_path[160] = {0};
    char database_path[160] = {0};
    size_t short_calls = 0U;
    int result_code = 1;

    arena_storage = malloc(8U * 1024U * 1024U);
    if (arena_storage == NULL || mkdtemp(directory) == NULL ||
        snprintf(log_path, sizeof(log_path), "%s/%020u.lxp", directory, 0U) < 0 ||
        snprintf(database_path, sizeof(database_path), "%s/history.sqlite", directory) < 0 ||
        lxp_log_segment_create(&log, directory, 0U, 131072U) != LXP_OK ||
        lxp_arena_init(&arena, arena_storage, 8U * 1024U * 1024U) != LXP_OK)
        goto cleanup_storage;

    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = 1U;
    activity.network_id = 77U;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){ actor, sizeof(actor) };
    activity.authority = (lxp_byte_span){ authority, sizeof(authority) };
    activity.account_sequence = 2U;
    activity.timestamp_bound.not_before = 100U;
    activity.timestamp_bound.not_after = 200U;
    activity.idempotency_key[0] = 0xa5U;
    activity.payload = (lxp_byte_span){ payload, sizeof(payload) };
    activity.signature = (lxp_byte_span){ signature, sizeof(signature) };
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK ||
        lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(activity_storage)) goto cleanup_log;
    activity_span = (lxp_byte_span){ activity_storage, encoded.length };
    (void)memcpy(activity_storage, encoded.bytes, encoded.length);
    if (lxp_activity_id(activity_span.bytes, activity_span.length,
                        activity_id) != LXP_OK ||
        lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(&receipt_value, activity_id, 10U, state_root,
                          state_root, state_root, LXP_OK, &effects,
                          (lxp_u128){0U, 0U}, batch_id, 1U, 1U, 1U) != LXP_OK)
        goto cleanup_log;
    receipt_value.sequencer_signature[0] = 7U;
    if (lxp_receipt_encode(&receipt_value, true, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(receipt_storage)) goto cleanup_log;
    receipt_span = (lxp_byte_span){ receipt_storage, encoded.length };
    (void)memcpy(receipt_storage, encoded.bytes, encoded.length);
    root_inputs = (lxp_batch_root_inputs){
        &activity_span, 1U, &receipt_span, 1U, &event_span, 1U,
        &oracle_span, 1U, NULL, 0U
    };
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_batch_roots_compute(&root_inputs, &arena, &roots) != LXP_OK)
        goto cleanup_log;
    (void)memset(&batch, 0, sizeof(batch));
    batch.header.protocol_version = 1U;
    batch.header.network_id = 77U;
    batch.header.batch_number = 22U;
    batch.header.first_sequence = 10U;
    batch.header.last_sequence = 10U;
    (void)memcpy(batch.header.activity_merkle_root,
                 roots.activity_merkle_root, 32U);
    (void)memcpy(batch.header.receipt_merkle_root,
                 roots.receipt_merkle_root, 32U);
    (void)memcpy(batch.header.event_merkle_root, roots.event_merkle_root, 32U);
    (void)memcpy(batch.header.oracle_root, roots.oracle_root, 32U);
    batch.sequencer_signature[0] = 8U;
    batch.activities = activity_span;
    batch.receipts = receipt_span;
    batch.events = event_span;
    batch.oracle_inputs = oracle_span;
    if (lxp_batch_availability_root(&batch, &arena,
            batch.header.data_availability_root) != LXP_OK) goto cleanup_log;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_batch_body_encode(&batch, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(batch_storage)) goto cleanup_log;
    (void)memcpy(batch_storage, encoded.bytes, encoded.length);
    if (lxp_hash_domain(LXP_DOMAIN_CHECKPOINT_CERTIFICATE, checkpoint,
                        sizeof(checkpoint), checkpoint_id) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_ACTIVITY, 10U, activity_span.bytes,
                       (uint32_t)activity_span.length, NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_RECEIPT, 10U, receipt_span.bytes,
                       (uint32_t)receipt_span.length, NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_ORACLE, 10U, oracle, sizeof(oracle), NULL) !=
                       LXP_OK ||
        lxp_log_append(&log, LXP_LOG_BATCH_BODY, 10U, batch_storage,
                       (uint32_t)encoded.length, NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_CHECKPOINT, 10U, checkpoint,
                       sizeof(checkpoint), NULL) != LXP_OK ||
        lxp_log_sync(&log) != LXP_OK ||
        lxp_history_open(&history, &log, database_path,
                         "migrations/0007_history_index.sql") != LXP_OK)
        goto cleanup_log;

    (void)memset(&query, 0, sizeof(query));
    query.kind = LXP_HISTORY_BY_ACTIVITY_ID;
    (void)memcpy(query.identifier, activity_id, 32U);
    query.maximum_response_bytes = sizeof(activity_storage);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_query(&history, &query, &arena, &result) != LXP_OK ||
        result.count != 1U ||
        !span_equal(result.items[0].canonical_bytes,
                    activity_span.bytes, activity_span.length)) goto cleanup_history;
    query.kind = LXP_HISTORY_BY_BATCH_NUMBER;
    query.batch_number = 22U;
    query.maximum_response_bytes = sizeof(batch_storage) * 2U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_query(&history, &query, &arena, &result) != LXP_OK ||
        result.count != 1U || result.items[0].record_kind != LXP_LOG_BATCH_BODY ||
        !span_equal(result.items[0].canonical_bytes, batch_storage,
                    encoded.length)) goto cleanup_history;
    query.kind = LXP_HISTORY_BY_CHECKPOINT_ID;
    (void)memcpy(query.identifier, checkpoint_id, 32U);
    query.maximum_response_bytes = 64U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_query(&history, &query, &arena, &result) != LXP_OK ||
        result.count != 1U ||
        !span_equal(result.items[0].canonical_bytes, checkpoint,
                    sizeof(checkpoint))) goto cleanup_history;

    (void)memset(&receipt_query, 0, sizeof(receipt_query));
    receipt_query.kind = LXP_RECEIPT_BY_TRANSACTION_ID;
    (void)memcpy(receipt_query.identifier, activity_id, 32U);
    receipt_query.maximum_response_bytes = sizeof(receipt_storage);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_receipt_lookup(&history, &receipt_query, &arena, &looked_up) != LXP_OK ||
        !span_equal(looked_up, receipt_span.bytes, receipt_span.length) ||
        lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_receipt_lookup(&history, &receipt_query, &arena, &looked_up) != LXP_OK ||
        !span_equal(looked_up, receipt_span.bytes, receipt_span.length))
        goto cleanup_history;
    receipt_query.kind = LXP_RECEIPT_BY_IDEMPOTENCY_KEY;
    (void)memcpy(receipt_query.identifier, activity.idempotency_key, 32U);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_receipt_lookup(&history, &receipt_query, &arena, &looked_up) != LXP_OK ||
        !span_equal(looked_up, receipt_span.bytes, receipt_span.length))
        goto cleanup_history;
    receipt_query.kind = LXP_RECEIPT_BY_GLOBAL_SEQUENCE;
    receipt_query.global_sequence = 10U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_receipt_lookup(&history, &receipt_query, &arena, &looked_up) != LXP_OK ||
        !span_equal(looked_up, receipt_span.bytes, receipt_span.length))
        goto cleanup_history;

    if (sqlite3_exec((sqlite3 *)history.database,
        "UPDATE history_records SET record_offset=99999 WHERE record_kind=1",
        NULL, NULL, NULL) != SQLITE_OK) goto cleanup_history;
    query.kind = LXP_HISTORY_BY_ACTIVITY_ID;
    (void)memcpy(query.identifier, activity_id, 32U);
    query.maximum_response_bytes = sizeof(activity_storage);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_query(&history, &query, &arena, &result) != LXP_OK ||
        result.count != 1U ||
        !span_equal(result.items[0].canonical_bytes,
                    activity_span.bytes, activity_span.length) ||
        lxp_history_index_rebuild(&history) != LXP_OK) goto cleanup_history;

    (void)memset(&served, 0, sizeof(served));
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_serve_range(&history, 10U, 10U, 32768U,
                                receive_exact, &served, &arena) != LXP_OK ||
        served.calls != 5U ||
        memcmp(served.activities, activity_span.bytes,
               activity_span.length) != 0 ||
        memcmp(served.receipts, receipt_span.bytes, receipt_span.length) != 0 ||
        memcmp(served.oracles, oracle, sizeof(oracle)) != 0 ||
        served.batch_length != encoded.length ||
        memcmp(served.batch, batch_storage, encoded.length) != 0 ||
        lxp_batch_body_decode(served.batch, served.batch_length,
                              &served_batch) != LXP_OK)
        goto cleanup_history;
    activity_span = served_batch.activities;
    receipt_span = served_batch.receipts;
    event_span = served_batch.events;
    oracle_span = served_batch.oracle_inputs;
    root_inputs = (lxp_batch_root_inputs){
        &activity_span, 1U, &receipt_span, 1U, &event_span, 1U,
        &oracle_span, 1U, NULL, 0U
    };
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_batch_roots_compute(&root_inputs, &arena, &roots) != LXP_OK ||
        lxp_batch_availability_root(&served_batch, &arena, roots.data_availability_root) != LXP_OK ||
        memcmp(roots.activity_merkle_root,
               batch.header.activity_merkle_root, 32U) != 0 ||
        memcmp(roots.receipt_merkle_root,
               batch.header.receipt_merkle_root, 32U) != 0 ||
        memcmp(roots.event_merkle_root, batch.header.event_merkle_root, 32U) != 0 ||
        memcmp(roots.oracle_root, batch.header.oracle_root, 32U) != 0 ||
        memcmp(roots.data_availability_root,
               batch.header.data_availability_root, 32U) != 0)
        goto cleanup_history;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_history_serve_range(&history, 10U, 10U, 1U,
                                count_only, &short_calls, &arena) !=
            LXP_ERR_LENGTH_LIMIT || short_calls != 0U)
        goto cleanup_history;
    result_code = 0;

cleanup_history:
    if (lxp_history_close(&history) != LXP_OK) result_code = 1;
cleanup_log:
    if (lxp_log_close(&log) != LXP_OK) result_code = 1;
cleanup_storage:
    (void)unlink(database_path);
    {
        char wal_path[192];
        char shm_path[192];
        if (snprintf(wal_path, sizeof(wal_path), "%s-wal", database_path) >= 0)
            (void)unlink(wal_path);
        if (snprintf(shm_path, sizeof(shm_path), "%s-shm", database_path) >= 0)
            (void)unlink(shm_path);
    }
    (void)unlink(log_path);
    (void)rmdir(directory);
    free(arena_storage);
    return result_code;
}
