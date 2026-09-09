#include "layerx/lxp_history.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"

#include <sqlite3.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { LXP_RECEIPT_STRUCTURE_TAG = 0x5201 };

typedef struct scan_record {
    uint64_t offset;
    lxp_log_record_header header;
    uint8_t *body;
} scan_record;

static char *read_file(const char *path)
{
    FILE *file;
    long length;
    char *contents;
    if (path == NULL) return NULL;
    file = fopen(path, "rb");
    if (file == NULL || fseek(file, 0L, SEEK_END) != 0) {
        if (file != NULL) (void)fclose(file);
        return NULL;
    }
    length = ftell(file);
    if (length < 0 || fseek(file, 0L, SEEK_SET) != 0) {
        (void)fclose(file);
        return NULL;
    }
    contents = malloc((size_t)length + 1U);
    if (contents == NULL || fread(contents, 1U, (size_t)length, file) !=
        (size_t)length) {
        free(contents);
        (void)fclose(file);
        return NULL;
    }
    contents[length] = '\0';
    (void)fclose(file);
    return contents;
}

static lxp_result record_load(const lxp_log *log, uint64_t offset,
                              scan_record *record)
{
    lxp_result status;
    if (log == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    record->offset = offset;
    status = lxp_log_read(log, offset, &record->header, NULL, 0U);
    if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) return status;
    if (record->header.body_length != 0U) {
        record->body = malloc(record->header.body_length);
        if (record->body == NULL) return LXP_ERR_IO;
        status = lxp_log_read(log, offset, &record->header, record->body,
                              record->header.body_length);
        if (status != LXP_OK) {
            free(record->body);
            record->body = NULL;
            return status;
        }
    }
    return LXP_OK;
}

static void record_release(scan_record *record)
{
    if (record == NULL) return;
    free(record->body);
    record->body = NULL;
}

static lxp_result receipt_transaction_id(const uint8_t *bytes, size_t length,
                                         uint8_t transaction_id[32])
{
    lxp_codec_reader reader;
    lxp_byte_span identifier;
    uint16_t version;
    lxp_result status;
    if (transaction_id == NULL || bytes == NULL || length < 4U) return LXP_ERR_NON_CANONICAL;
    status = lxp_codec_reader_init(&reader, bytes, length);
    if (status == LXP_OK)
        status = lxp_codec_read_struct_header(&reader,
                                              bytes[2] == 0x52U && bytes[3] == 0x02U ?
                                                  0x5202U : LXP_RECEIPT_STRUCTURE_TAG);
    if (status == LXP_OK) status = lxp_codec_read_u16(&reader, &version);
    if (status == LXP_OK && version == 0U) status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_codec_read_bytes(&reader, &identifier, 32U);
    if (status != LXP_OK || identifier.length != 32U)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(transaction_id, identifier.bytes, 32U);
    return LXP_OK;
}

static lxp_result checkpoint_identifier(const uint8_t *bytes, size_t length,
                                        uint8_t identifier[32])
{
    return lxp_hash_domain(LXP_DOMAIN_CHECKPOINT_CERTIFICATE, bytes, length,
                           identifier);
}

static lxp_result record_batch_number(const scan_record *record,
                                      uint64_t *batch_number, int *present)
{
    lxp_batch_body body;
    lxp_batch_header header;
    lxp_result status;
    *present = 0;
    if (record->header.record_kind == (uint8_t)LXP_LOG_BATCH_BODY) {
        status = lxp_batch_body_decode(record->body,
                                       record->header.body_length, &body);
        if (status != LXP_OK) return status;
        *batch_number = body.header.batch_number;
        *present = 1;
    } else if (record->header.record_kind == (uint8_t)LXP_LOG_BATCH_HEADER) {
        status = lxp_batch_header_decode(record->body,
                                         record->header.body_length, &header);
        if (status != LXP_OK) return status;
        *batch_number = header.batch_number;
        *present = 1;
    }
    return LXP_OK;
}

static int bind_optional_blob(sqlite3_stmt *statement, int column,
                              const uint8_t *bytes, int present)
{
    if (present != 0)
        return sqlite3_bind_blob(statement, column, bytes, 32,
                                 SQLITE_TRANSIENT);
    return sqlite3_bind_null(statement, column);
}

static lxp_result index_record(sqlite3_stmt *statement,
                               const scan_record *record)
{
    uint8_t checkpoint_id[32];
    uint8_t activity_id[32];
    uint8_t transaction_id[32];
    uint8_t idempotency_key[32];
    uint64_t batch_number = 0U;
    int has_batch = 0;
    int has_checkpoint = 0;
    int has_activity = 0;
    int has_transaction = 0;
    int has_idempotency = 0;
    lxp_result status = LXP_OK;
    if (record->offset > INT64_MAX || record->header.global_sequence > INT64_MAX)
        return LXP_ERR_PROJECTION_STALE;
    if (record->header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT) {
        status = checkpoint_identifier(record->body,
                                       record->header.body_length,
                                       checkpoint_id);
        has_checkpoint = status == LXP_OK;
    } else if (record->header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
        lxp_activity activity;
        status = lxp_activity_id(record->body, record->header.body_length,
                                 activity_id);
        if (status == LXP_OK)
            status = lxp_activity_decode(record->body,
                                         record->header.body_length,
                                         &activity);
        if (status == LXP_OK) {
            (void)memcpy(idempotency_key, activity.idempotency_key, 32U);
            has_activity = 1;
            has_idempotency = 1;
        }
    } else if (record->header.record_kind == (uint8_t)LXP_LOG_RECEIPT) {
        status = receipt_transaction_id(record->body,
                                        record->header.body_length,
                                        transaction_id);
        has_transaction = status == LXP_OK;
    } else {
        status = record_batch_number(record, &batch_number, &has_batch);
    }
    if (status != LXP_OK) return LXP_ERR_PROJECTION_STALE;
    if (sqlite3_reset(statement) != SQLITE_OK ||
        sqlite3_clear_bindings(statement) != SQLITE_OK ||
        sqlite3_bind_int64(statement, 1,
                           (sqlite3_int64)record->offset) != SQLITE_OK ||
        sqlite3_bind_int(statement, 2,
                         (int)record->header.record_kind) != SQLITE_OK ||
        sqlite3_bind_int64(statement, 3,
             (sqlite3_int64)record->header.global_sequence) != SQLITE_OK ||
        sqlite3_bind_int64(statement, 4,
             (sqlite3_int64)record->header.body_length) != SQLITE_OK)
        return LXP_ERR_PROJECTION_STALE;
    if ((has_batch != 0 && batch_number > INT64_MAX) ||
        (has_batch != 0 ? sqlite3_bind_int64(statement, 5,
                          (sqlite3_int64)batch_number) :
                          sqlite3_bind_null(statement, 5)) != SQLITE_OK ||
        bind_optional_blob(statement, 6, checkpoint_id, has_checkpoint) !=
            SQLITE_OK ||
        bind_optional_blob(statement, 7, activity_id, has_activity) !=
            SQLITE_OK ||
        bind_optional_blob(statement, 8, transaction_id, has_transaction) !=
            SQLITE_OK ||
        bind_optional_blob(statement, 9, idempotency_key, has_idempotency) !=
            SQLITE_OK || sqlite3_step(statement) != SQLITE_DONE)
        return LXP_ERR_PROJECTION_STALE;
    return LXP_OK;
}

lxp_result lxp_history_index_rebuild(lxp_history *history)
{
    static const char insert_sql[] =
        "INSERT INTO history_records(record_offset,record_kind,"
        "global_sequence,body_length,batch_number,checkpoint_id,activity_id,"
        "transaction_id,idempotency_key) VALUES(?,?,?,?,?,?,?,?,?)";
    sqlite3 *database;
    sqlite3_stmt *insert = NULL;
    sqlite3_stmt *meta = NULL;
    uint64_t offset = 0U;
    uint64_t count = 0U;
    lxp_result status = LXP_OK;
    if (history == NULL || history->database == NULL || history->log == NULL)
        return LXP_ERR_NON_CANONICAL;
    database = (sqlite3 *)history->database;
    if (history->log->write_offset > INT64_MAX ||
        sqlite3_exec(database, "BEGIN IMMEDIATE; DELETE FROM history_records;",
                     NULL, NULL, NULL) != SQLITE_OK ||
        sqlite3_prepare_v2(database, insert_sql, -1, &insert, NULL) != SQLITE_OK)
        status = LXP_ERR_PROJECTION_STALE;
    while (status == LXP_OK && offset < history->log->write_offset) {
        scan_record record;
        status = record_load(history->log, offset, &record);
        if (status != LXP_OK) break;
        status = index_record(insert, &record);
        offset += LXP_LOG_HEADER_BYTES + record.header.body_length;
        ++count;
        record_release(&record);
    }
    if (status == LXP_OK &&
        sqlite3_prepare_v2(database,
            "UPDATE history_index_meta SET log_end=?,record_count=? "
            "WHERE singleton=1", -1, &meta, NULL) == SQLITE_OK &&
        count <= INT64_MAX &&
        sqlite3_bind_int64(meta, 1,
             (sqlite3_int64)history->log->write_offset) == SQLITE_OK &&
        sqlite3_bind_int64(meta, 2, (sqlite3_int64)count) == SQLITE_OK &&
        sqlite3_step(meta) == SQLITE_DONE &&
        sqlite3_exec(database, "COMMIT", NULL, NULL, NULL) == SQLITE_OK) {
        (void)sqlite3_finalize(insert);
        (void)sqlite3_finalize(meta);
        return LXP_OK;
    }
    (void)sqlite3_finalize(insert);
    (void)sqlite3_finalize(meta);
    (void)sqlite3_exec(database, "ROLLBACK", NULL, NULL, NULL);
    return status == LXP_OK ? LXP_ERR_PROJECTION_STALE : status;
}

lxp_result lxp_history_open(lxp_history *history, const lxp_log *log,
                            const char *database_path,
                            const char *migration_path)
{
    sqlite3 *database = NULL;
    char *migration;
    char *error = NULL;
    lxp_result status;
    if (history == NULL || log == NULL || database_path == NULL ||
        migration_path == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(history, 0, sizeof(*history));
    if (sqlite3_open_v2(database_path, &database,
                        SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE |
                        SQLITE_OPEN_NOMUTEX, NULL) != SQLITE_OK) {
        if (database != NULL) (void)sqlite3_close(database);
        return LXP_ERR_IO;
    }
    migration = read_file(migration_path);
    if (migration == NULL ||
        sqlite3_exec(database, migration, NULL, NULL, &error) != SQLITE_OK) {
        free(migration);
        sqlite3_free(error);
        (void)sqlite3_close(database);
        return LXP_ERR_IO;
    }
    free(migration);
    history->log = log;
    history->database = database;
    status = lxp_history_index_rebuild(history);
    if (status != LXP_OK) {
        (void)sqlite3_close(database);
        (void)memset(history, 0, sizeof(*history));
    }
    return status;
}

lxp_result lxp_history_close(lxp_history *history)
{
    if (history == NULL || history->database == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (sqlite3_close((sqlite3 *)history->database) != SQLITE_OK)
        return LXP_ERR_IO;
    history->database = NULL;
    history->log = NULL;
    return LXP_OK;
}

static lxp_result history_match(const scan_record *record,
                                const lxp_history_query_spec *query,
                                int *matched)
{
    uint8_t identifier[32];
    uint64_t batch_number;
    int present;
    lxp_result status;
    *matched = 0;
    switch (query->kind) {
    case LXP_HISTORY_BY_CHECKPOINT_ID:
        if (record->header.record_kind != (uint8_t)LXP_LOG_CHECKPOINT)
            return LXP_OK;
        status = checkpoint_identifier(record->body,
                                       record->header.body_length, identifier);
        if (status == LXP_OK)
            *matched = memcmp(identifier, query->identifier, 32U) == 0;
        return status;
    case LXP_HISTORY_BY_BATCH_NUMBER:
        status = record_batch_number(record, &batch_number, &present);
        if (status == LXP_OK)
            *matched = present != 0 && batch_number == query->batch_number;
        return status;
    case LXP_HISTORY_BY_SEQUENCE_RANGE:
        *matched = record->header.global_sequence >= query->first_sequence &&
                   record->header.global_sequence <= query->last_sequence;
        return LXP_OK;
    case LXP_HISTORY_BY_ACTIVITY_ID:
        if (record->header.record_kind != (uint8_t)LXP_LOG_ACTIVITY)
            return LXP_OK;
        status = lxp_activity_id(record->body, record->header.body_length,
                                 identifier);
        if (status == LXP_OK)
            *matched = memcmp(identifier, query->identifier, 32U) == 0;
        return status;
    default:
        return LXP_ERR_NON_CANONICAL;
    }
}

static lxp_result history_measure(const lxp_history *history,
                                  const lxp_history_query_spec *query,
                                  size_t *count, size_t *total)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    *count = 0U;
    *total = 0U;
    while (status == LXP_OK && offset < history->log->write_offset) {
        scan_record record;
        int matched = 0;
        status = record_load(history->log, offset, &record);
        if (status != LXP_OK) break;
        status = history_match(&record, query, &matched);
        if (status == LXP_OK && matched != 0) {
            if (*count == LXP_HISTORY_MAX_RESULTS ||
                record.header.body_length > SIZE_MAX - *total) {
                status = LXP_ERR_LENGTH_LIMIT;
            } else {
                ++*count;
                *total += record.header.body_length;
                if (*total > query->maximum_response_bytes)
                    status = LXP_ERR_LENGTH_LIMIT;
            }
        }
        offset += LXP_LOG_HEADER_BYTES + record.header.body_length;
        record_release(&record);
    }
    return status;
}

lxp_result lxp_history_query(const lxp_history *history,
                             const lxp_history_query_spec *query,
                             lxp_arena *arena, lxp_history_result *result)
{
    uint64_t offset = 0U;
    size_t count;
    size_t total;
    size_t index = 0U;
    size_t mark;
    void *memory = NULL;
    lxp_result status;
    if (history == NULL || history->log == NULL || query == NULL ||
        arena == NULL || result == NULL || query->maximum_response_bytes == 0U ||
        (query->kind == LXP_HISTORY_BY_SEQUENCE_RANGE &&
         query->first_sequence > query->last_sequence))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(result, 0, sizeof(*result));
    status = history_measure(history, query, &count, &total);
    if (status != LXP_OK || count == 0U) return status;
    mark = lxp_arena_mark(arena);
    status = lxp_arena_alloc(arena, count * sizeof(*result->items),
                             _Alignof(lxp_history_item), &memory);
    if (status != LXP_OK) return status;
    result->items = (lxp_history_item *)memory;
    result->count = count;
    result->total_bytes = total;
    while (status == LXP_OK && offset < history->log->write_offset) {
        scan_record record;
        int matched = 0;
        status = record_load(history->log, offset, &record);
        if (status != LXP_OK) break;
        status = history_match(&record, query, &matched);
        if (status == LXP_OK && matched != 0) {
            void *bytes = NULL;
            status = lxp_arena_alloc(arena, record.header.body_length, 1U,
                                     &bytes);
            if (status == LXP_OK) {
                (void)memcpy(bytes, record.body, record.header.body_length);
                result->items[index].record_kind = record.header.record_kind;
                result->items[index].global_sequence =
                    record.header.global_sequence;
                result->items[index].canonical_bytes.bytes = bytes;
                result->items[index].canonical_bytes.length =
                    record.header.body_length;
                ++index;
            }
        }
        offset += LXP_LOG_HEADER_BYTES + record.header.body_length;
        record_release(&record);
    }
    if (status != LXP_OK || index != count) {
        (void)lxp_arena_reset(arena, mark);
        (void)memset(result, 0, sizeof(*result));
        return status == LXP_OK ? LXP_FATAL_INVARIANT : status;
    }
    return LXP_OK;
}

static lxp_result idempotency_sequence(const lxp_history *history,
                                       const uint8_t key[32],
                                       uint64_t *sequence, int *found)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    *found = 0;
    while (status == LXP_OK && offset < history->log->write_offset) {
        scan_record record;
        status = record_load(history->log, offset, &record);
        if (status != LXP_OK) break;
        if (record.header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
            lxp_activity activity;
            status = lxp_activity_decode(record.body, record.header.body_length,
                                         &activity);
            if (status == LXP_OK &&
                memcmp(activity.idempotency_key, key, 32U) == 0) {
                if (*found != 0 && *sequence != record.header.global_sequence)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    *sequence = record.header.global_sequence;
                    *found = 1;
                }
            }
        }
        offset += LXP_LOG_HEADER_BYTES + record.header.body_length;
        record_release(&record);
    }
    return status;
}

lxp_result lxp_receipt_lookup(const lxp_history *history,
                              const lxp_receipt_query *query,
                              lxp_arena *arena, lxp_byte_span *receipt)
{
    uint64_t offset = 0U;
    uint64_t sequence = 0U;
    int have_sequence = 0;
    lxp_result status = LXP_OK;
    if (history == NULL || history->log == NULL || query == NULL ||
        arena == NULL || receipt == NULL || query->maximum_response_bytes == 0U)
        return LXP_ERR_NON_CANONICAL;
    receipt->bytes = NULL;
    receipt->length = 0U;
    if (query->kind == LXP_RECEIPT_BY_GLOBAL_SEQUENCE) {
        sequence = query->global_sequence;
        have_sequence = 1;
    } else if (query->kind == LXP_RECEIPT_BY_IDEMPOTENCY_KEY) {
        status = idempotency_sequence(history, query->identifier, &sequence,
                                      &have_sequence);
    } else if (query->kind != LXP_RECEIPT_BY_TRANSACTION_ID) {
        return LXP_ERR_NON_CANONICAL;
    }
    while (status == LXP_OK && offset < history->log->write_offset) {
        scan_record record;
        int matched = 0;
        status = record_load(history->log, offset, &record);
        if (status != LXP_OK) break;
        if (record.header.record_kind == (uint8_t)LXP_LOG_RECEIPT) {
            if (query->kind == LXP_RECEIPT_BY_TRANSACTION_ID) {
                uint8_t transaction_id[32];
                status = receipt_transaction_id(record.body,
                                                record.header.body_length,
                                                transaction_id);
                matched = status == LXP_OK &&
                    memcmp(transaction_id, query->identifier, 32U) == 0;
            } else {
                matched = have_sequence != 0 &&
                    record.header.global_sequence == sequence;
            }
            if (status == LXP_OK && matched != 0) {
                void *bytes = NULL;
                if (record.header.body_length > query->maximum_response_bytes)
                    status = LXP_ERR_LENGTH_LIMIT;
                else
                    status = lxp_arena_alloc(arena,
                                             record.header.body_length, 1U,
                                             &bytes);
                if (status == LXP_OK) {
                    (void)memcpy(bytes, record.body,
                                 record.header.body_length);
                    receipt->bytes = bytes;
                    receipt->length = record.header.body_length;
                }
            }
        }
        offset += LXP_LOG_HEADER_BYTES + record.header.body_length;
        record_release(&record);
        if (receipt->bytes != NULL) return LXP_OK;
    }
    if (status != LXP_OK) return status;
    return LXP_ERR_UNKNOWN_ACTIVITY;
}
