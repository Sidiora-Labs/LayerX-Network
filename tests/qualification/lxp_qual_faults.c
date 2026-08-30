#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_qualification.h"

#include "layerx/lxp_hash.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_projection.h"
#include "layerx/lxp_replica.h"
#include "layerx/lxp_sequencer.h"
#include "layerx/lxp_snapshot.h"

#include <errno.h>
#include <sqlite3.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct fault_fixture {
    char log_directory[128];
    char log_path[192];
    char database_path[128];
    char checkpoint_directory[128];
    char checkpoint_path[192];
    char checkpoint_temporary_path[196];
} fault_fixture;

typedef struct replay_transfer {
    uint64_t sequence[2];
    uint8_t phase[2];
    uint8_t amount[2];
    size_t count;
} replay_transfer;

typedef struct sim_vote {
    uint64_t batch_number;
    uint8_t root[32];
    uint8_t voters;
} sim_vote;

typedef struct sim_finality {
    sim_vote votes[8];
    size_t vote_count;
    uint64_t finalised_batch;
    uint8_t finalised_root[32];
    bool has_finalised;
} sim_finality;

static const uint8_t checkpoint_bytes[] = {
    0x4cU, 0x58U, 0x50U, 0x2fU, 0x66U, 0x61U, 0x75U, 0x6cU,
    0x74U, 0x2fU, 0x63U, 0x68U, 0x65U, 0x63U, 0x6bU, 0x70U,
    0x6fU, 0x69U, 0x6eU, 0x74U, 0x2fU, 0x76U, 0x31U
};

static lxp_result projection_record(uint64_t sequence,
                                    lxp_projection_record *record,
                                    uint8_t receipt[8])
{
    size_t i;
    if (record == NULL || receipt == NULL || sequence > UINT8_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    for (i = 0U; i < 8U; ++i)
        receipt[i] = (uint8_t)(0xa0U + (uint8_t)i + (uint8_t)sequence);
    record->activity_id[0] = (uint8_t)(0x10U + sequence);
    record->idempotency_key[0] = (uint8_t)(0x20U + sequence);
    record->account_id[0] = 0x31U;
    record->asset_id[0] = 0x41U;
    record->amount[15] = (uint8_t)(0x50U + sequence);
    record->receipt = receipt;
    record->receipt_length = 8U;
    record->result_code = LXP_OK;
    return LXP_OK;
}

static lxp_result append_transfer(lxp_log *log, uint64_t sequence)
{
    uint8_t activity[16] = { 0U };
    uint8_t state_diff[16] = { 0U };
    uint8_t receipt_data[8];
    uint8_t encoded[256];
    size_t encoded_length;
    lxp_projection_record record;
    lxp_result status;
    if (log == NULL || sequence > UINT8_MAX) return LXP_ERR_NON_CANONICAL;
    activity[0] = (uint8_t)sequence;
    activity[15] = (uint8_t)(0x50U + sequence);
    state_diff[0] = (uint8_t)sequence;
    state_diff[15] = (uint8_t)(0x50U + sequence);
    status = projection_record(sequence, &record, receipt_data);
    if (status == LXP_OK)
        status = lxp_projection_record_encode(&record, encoded,
                                               sizeof(encoded),
                                               &encoded_length);
    if (status == LXP_OK)
        status = lxp_log_append(log, LXP_LOG_ACTIVITY, sequence, activity,
                                (uint32_t)sizeof(activity), NULL);
    if (status == LXP_OK) status = lxp_log_sync(log);
    if (status == LXP_OK)
        status = lxp_log_append(log, LXP_LOG_STATE_DIFF, sequence, state_diff,
                                (uint32_t)sizeof(state_diff), NULL);
    if (status == LXP_OK)
        status = lxp_log_append(log, LXP_LOG_RECEIPT, sequence, encoded,
                                (uint32_t)encoded_length, NULL);
    if (status == LXP_OK) status = lxp_log_sync(log);
    return status;
}

static lxp_result execute_fault_workload(void *opaque)
{
    fault_fixture *fixture = (fault_fixture *)opaque;
    uint8_t receipt[8];
    uint8_t state_root[32];
    lxp_projection_record record;
    lxp_snapshot_manifest_record manifest;
    lxp_projection projection;
    lxp_log log;
    lxp_result status;
    status = lxp_log_segment_create(&log, fixture->log_directory, 0U, 16384U);
    if (status == LXP_OK) status = append_transfer(&log, 0U);
    if (status == LXP_OK)
        status = lxp_projection_open(&projection, fixture->database_path,
                                     "migrations/0001_projection.sql");
    if (status == LXP_OK) status = projection_record(0U, &record, receipt);
    if (status == LXP_OK) status = lxp_projection_apply(&projection, 0U,
                                                         &record);
    if (status == LXP_OK) status = lxp_projection_close(&projection);
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_STATE_NODE, checkpoint_bytes,
                                 sizeof(checkpoint_bytes), state_root);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest_build(checkpoint_bytes,
                    sizeof(checkpoint_bytes), 0U, state_root, &manifest);
    if (status == LXP_OK)
        status = lxp_snapshot_store_write(fixture->checkpoint_directory,
                    &manifest, checkpoint_bytes, sizeof(checkpoint_bytes));
    if (status == LXP_OK) status = lxp_log_close(&log);
    return status;
}

static lxp_result replay_record(void *opaque,
                                const lxp_log_record_header *header,
                                const uint8_t *body)
{
    replay_transfer *replay = (replay_transfer *)opaque;
    size_t index;
    if (replay == NULL || header == NULL ||
        (body == NULL && header->body_length != 0U) ||
        header->global_sequence >= 2U) return LXP_FATAL_REPLAY_DIVERGENCE;
    index = (size_t)header->global_sequence;
    if (header->record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
        if (replay->phase[index] != 0U || header->body_length != 16U)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        replay->sequence[index] = header->global_sequence;
        replay->phase[index] = 1U;
    } else if (header->record_kind == (uint8_t)LXP_LOG_STATE_DIFF) {
        if (replay->phase[index] != 1U || header->body_length != 16U)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        replay->amount[index] = body[15];
        replay->phase[index] = 2U;
    } else if (header->record_kind == (uint8_t)LXP_LOG_RECEIPT) {
        lxp_projection_record record;
        if (replay->phase[index] != 2U ||
            lxp_projection_record_decode(body, header->body_length,
                                         &record) != LXP_OK ||
            record.amount[15] != replay->amount[index])
            return LXP_FATAL_REPLAY_DIVERGENCE;
        replay->phase[index] = 3U;
        replay->count += 1U;
    }
    return LXP_OK;
}

static lxp_result projection_counts(lxp_projection *projection,
                                    uint64_t *receipts,
                                    uint64_t *balances,
                                    uint64_t *watermark,
                                    bool *has_watermark)
{
    static const char *const queries[2] = {
        "SELECT count(*) FROM receipts",
        "SELECT count(*) FROM balances"
    };
    uint64_t *outputs[2] = { receipts, balances };
    size_t i;
    if (projection == NULL || projection->database == NULL ||
        receipts == NULL || balances == NULL || watermark == NULL ||
        has_watermark == NULL) return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < 2U; ++i) {
        sqlite3_stmt *statement = NULL;
        sqlite3_int64 value;
        if (sqlite3_prepare_v2((sqlite3 *)projection->database, queries[i],
                               -1, &statement, NULL) != SQLITE_OK ||
            sqlite3_step(statement) != SQLITE_ROW) {
            (void)sqlite3_finalize(statement);
            return LXP_ERR_PROJECTION_STALE;
        }
        value = sqlite3_column_int64(statement, 0);
        (void)sqlite3_finalize(statement);
        if (value < 0) return LXP_ERR_PROJECTION_STALE;
        *outputs[i] = (uint64_t)value;
    }
    return lxp_projection_watermark(projection, watermark, has_watermark);
}

static lxp_result verify_checkpoint(const fault_fixture *fixture)
{
    struct stat information;
    uint8_t arena_storage[512];
    uint8_t state_root[32];
    lxp_snapshot_manifest_record stored;
    lxp_snapshot_manifest_record expected;
    lxp_byte_span snapshot;
    lxp_arena arena;
    lxp_result status;
    if (stat(fixture->checkpoint_path, &information) != 0) {
        return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
    }
    status = lxp_arena_init(&arena, arena_storage, sizeof(arena_storage));
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(fixture->checkpoint_path, &arena,
                                         &stored, &snapshot);
    if (status == LXP_OK &&
        (snapshot.length != sizeof(checkpoint_bytes) ||
         memcmp(snapshot.bytes, checkpoint_bytes, snapshot.length) != 0))
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_STATE_NODE, checkpoint_bytes,
                                 sizeof(checkpoint_bytes), state_root);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest_build(checkpoint_bytes,
                    sizeof(checkpoint_bytes), 0U, state_root, &expected);
    if (status == LXP_OK &&
        (stored.global_sequence != expected.global_sequence ||
         memcmp(stored.state_root, expected.state_root, 32U) != 0 ||
         memcmp(stored.snapshot_digest, expected.snapshot_digest, 32U) != 0))
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    return status;
}

static lxp_result fixture_prepare(fault_fixture *fixture)
{
    char log_template[] = "/tmp/lxp-qual-fault-log-XXXXXX";
    char db_template[] = "/tmp/lxp-qual-fault-db-XXXXXX";
    char checkpoint_template[] = "/tmp/lxp-qual-fault-checkpoint-XXXXXX";
    lxp_projection projection;
    int descriptor;
    if (fixture == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(fixture, 0, sizeof(*fixture));
    if (mkdtemp(log_template) == NULL || mkdtemp(checkpoint_template) == NULL)
        return LXP_ERR_IO;
    descriptor = mkstemp(db_template);
    if (descriptor < 0 || close(descriptor) != 0 || unlink(db_template) != 0)
        return LXP_ERR_IO;
    if (snprintf(fixture->log_directory, sizeof(fixture->log_directory),
                 "%s", log_template) < 0 ||
        snprintf(fixture->database_path, sizeof(fixture->database_path),
                 "%s", db_template) < 0 ||
        snprintf(fixture->checkpoint_directory,
                 sizeof(fixture->checkpoint_directory), "%s",
                 checkpoint_template) < 0 ||
        snprintf(fixture->log_path, sizeof(fixture->log_path), "%s/%020u.lxp",
                 fixture->log_directory, 0U) < 0 ||
        snprintf(fixture->checkpoint_path, sizeof(fixture->checkpoint_path),
                 "%s/%020u.lxs", fixture->checkpoint_directory, 0U) < 0 ||
        snprintf(fixture->checkpoint_temporary_path,
                 sizeof(fixture->checkpoint_temporary_path), "%s.tmp",
                 fixture->checkpoint_path) < 0)
        return LXP_ERR_LENGTH_LIMIT;
    if (lxp_projection_open(&projection, fixture->database_path,
                            "migrations/0001_projection.sql") != LXP_OK)
        return LXP_ERR_IO;
    return lxp_projection_close(&projection);
}

static void unlink_if_present(const char *path)
{
    if (path != NULL && unlink(path) != 0 && errno != ENOENT) return;
}

static void fixture_release(const fault_fixture *fixture)
{
    char wal[160];
    char shm[160];
    if (fixture == NULL) return;
    unlink_if_present(fixture->checkpoint_temporary_path);
    unlink_if_present(fixture->checkpoint_path);
    unlink_if_present(fixture->log_path);
    unlink_if_present(fixture->database_path);
    if (snprintf(wal, sizeof(wal), "%s-wal", fixture->database_path) > 0)
        unlink_if_present(wal);
    if (snprintf(shm, sizeof(shm), "%s-shm", fixture->database_path) > 0)
        unlink_if_present(shm);
    (void)rmdir(fixture->checkpoint_directory);
    (void)rmdir(fixture->log_directory);
}

static lxp_result verify_recovery(fault_fixture *fixture)
{
    replay_transfer before;
    replay_transfer after;
    lxp_projection projection;
    lxp_log log;
    uint64_t receipts;
    uint64_t balances;
    uint64_t watermark;
    uint64_t next;
    bool has_watermark;
    lxp_result status;
    (void)memset(&before, 0, sizeof(before));
    (void)memset(&after, 0, sizeof(after));
    (void)memset(&projection, 0, sizeof(projection));
    status = lxp_log_open(&log, fixture->log_path);
    if (status == LXP_OK) status = lxp_log_recover(&log, replay_record,
                                                   &before);
    if (status == LXP_OK &&
        (before.count > 1U ||
         (before.count == 1U && before.phase[0] != 3U) ||
         lxp_log_resume_sequence(&log) != before.count))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    next = before.count;
    if (status == LXP_OK) status = append_transfer(&log, next);
    if (status == LXP_OK) status = lxp_log_recover(&log, replay_record,
                                                   &after);
    if (status == LXP_OK &&
        (after.count != before.count + 1U ||
         lxp_log_resume_sequence(&log) != after.count))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK) {
        size_t i;
        for (i = 0U; i < after.count; ++i) {
            if (after.phase[i] != 3U || after.sequence[i] != i) {
                status = LXP_FATAL_REPLAY_DIVERGENCE;
                break;
            }
        }
    }
    if (status == LXP_OK)
        status = lxp_projection_open(&projection, fixture->database_path,
                                     "migrations/0001_projection.sql");
    if (status == LXP_OK)
        status = projection_counts(&projection, &receipts, &balances,
                                   &watermark, &has_watermark);
    if (status == LXP_OK) {
        bool empty = receipts == 0U && balances == 0U && !has_watermark;
        bool complete = before.count == 1U && receipts == 1U &&
                        balances == 1U && has_watermark && watermark == 0U;
        if (!empty && !complete) status = LXP_ERR_PROJECTION_STALE;
    }
    if (status == LXP_OK)
        status = lxp_projection_rebuild(&projection, &log,
                                        "migrations/0001_projection.sql");
    if (status == LXP_OK)
        status = projection_counts(&projection, &receipts, &balances,
                                   &watermark, &has_watermark);
    if (status == LXP_OK &&
        (receipts != after.count || balances != 1U || !has_watermark ||
         watermark + 1U != after.count))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (projection.database != NULL &&
        lxp_projection_close(&projection) != LXP_OK && status == LXP_OK)
        status = LXP_ERR_IO;
    if (status == LXP_OK) status = verify_checkpoint(fixture);
    if (lxp_log_close(&log) != LXP_OK && status == LXP_OK)
        status = LXP_ERR_IO;
    return status;
}

static lxp_result run_boundary(lxp_fault_boundary boundary,
                               uint32_t occurrence)
{
    fault_fixture fixture;
    int child_status = 0;
    lxp_result status = fixture_prepare(&fixture);
    if (status == LXP_OK)
        status = lxp_fault_crash_at_boundary(boundary, occurrence,
                    execute_fault_workload, &fixture, &child_status);
    if (status == LXP_OK) status = verify_recovery(&fixture);
    fixture_release(&fixture);
    return status;
}

lxp_result lxp_qual_fault_boundaries(void)
{
    static const struct {
        lxp_fault_boundary boundary;
        uint32_t occurrences;
    } cases[] = {
        { LXP_FAULT_LOG_HEADER_WRITTEN, 3U },
        { LXP_FAULT_LOG_BODY_WRITTEN, 3U },
        { LXP_FAULT_LOG_SYNCED, 2U },
        { LXP_FAULT_INDEX_RECEIPT_WRITTEN, 1U },
        { LXP_FAULT_INDEX_BALANCE_WRITTEN, 1U },
        { LXP_FAULT_INDEX_WATERMARK_WRITTEN, 1U },
        { LXP_FAULT_INDEX_COMMITTED, 1U },
        { LXP_FAULT_CHECKPOINT_HEADER_WRITTEN, 1U },
        { LXP_FAULT_CHECKPOINT_BODY_WRITTEN, 1U },
        { LXP_FAULT_CHECKPOINT_FILE_SYNCED, 1U },
        { LXP_FAULT_CHECKPOINT_RENAMED, 1U },
        { LXP_FAULT_CHECKPOINT_DIRECTORY_SYNCED, 1U }
    };
    size_t i;
    for (i = 0U; i < sizeof(cases) / sizeof(cases[0]); ++i) {
        uint32_t occurrence;
        for (occurrence = 1U; occurrence <= cases[i].occurrences;
             ++occurrence) {
            lxp_result status = run_boundary(cases[i].boundary, occurrence);
            if (status != LXP_OK) {
                (void)fprintf(stderr,
                              "fault boundary %u occurrence %u failed: %d\n",
                              (unsigned)cases[i].boundary,
                              (unsigned)occurrence, (int)status);
                return status;
            }
        }
    }
    return LXP_OK;
}

static void make_header(lxp_batch_header *header, uint64_t batch_number,
                        const uint8_t previous_root[32], uint8_t root_byte)
{
    (void)memset(header, 0, sizeof(*header));
    header->protocol_version = 1U;
    header->network_id = 77U;
    header->epoch = 4U;
    header->batch_number = batch_number;
    header->first_sequence = batch_number * 10U;
    header->last_sequence = header->first_sequence + 9U;
    (void)memcpy(header->previous_state_root, previous_root, 32U);
    header->resulting_state_root[0] = root_byte;
    header->timestamp_ms = 1000U + batch_number;
    header->sequencer_id[0] = 0x71U;
}

static lxp_result replica_accept(lxp_replica *replica,
                                 const lxp_batch_header *header)
{
    lxp_result status;
    if (replica == NULL || header == NULL) return LXP_ERR_NON_CANONICAL;
    if (!replica->has_head) {
        if (header->batch_number != 0U || header->first_sequence != 0U ||
            !lxp_ct_is_zero(header->previous_state_root, 32U))
            return LXP_ERR_BATCH_GAP;
        replica->head = *header;
        replica->has_head = true;
        return LXP_OK;
    }
    status = lxp_replica_chain_link(&replica->head, header);
    if (status == LXP_OK) replica->head = *header;
    return status;
}

static lxp_result finality_vote(sim_finality *finality, uint8_t voter,
                                const lxp_batch_header *header)
{
    size_t i;
    if (finality == NULL || voter >= 3U || header == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < finality->vote_count; ++i) {
        sim_vote *vote = &finality->votes[i];
        if (vote->batch_number == header->batch_number &&
            memcmp(vote->root, header->resulting_state_root, 32U) == 0) {
            uint8_t mask = (uint8_t)(1U << voter);
            vote->voters = (uint8_t)(vote->voters | mask);
            if ((vote->voters == 3U || vote->voters == 5U ||
                 vote->voters == 6U || vote->voters == 7U)) {
                if (finality->has_finalised &&
                    header->batch_number < finality->finalised_batch)
                    return LXP_OK;
                if (finality->has_finalised &&
                    finality->finalised_batch == header->batch_number &&
                    memcmp(finality->finalised_root,
                           header->resulting_state_root, 32U) != 0)
                    return LXP_FATAL_REPLAY_DIVERGENCE;
                finality->has_finalised = true;
                finality->finalised_batch = header->batch_number;
                (void)memcpy(finality->finalised_root,
                             header->resulting_state_root, 32U);
            }
            return LXP_OK;
        }
    }
    if (finality->vote_count == sizeof(finality->votes) /
        sizeof(finality->votes[0])) return LXP_ERR_LENGTH_LIMIT;
    finality->votes[finality->vote_count].batch_number = header->batch_number;
    (void)memcpy(finality->votes[finality->vote_count].root,
                 header->resulting_state_root, 32U);
    finality->votes[finality->vote_count].voters = (uint8_t)(1U << voter);
    finality->vote_count += 1U;
    return LXP_OK;
}

lxp_result lxp_partition_sim(void)
{
    uint8_t zero[32] = { 0U };
    lxp_batch_header batch[3];
    lxp_batch_header conflict;
    lxp_replica replicas[3];
    sim_finality finality;
    size_t i;
    lxp_result status = LXP_OK;
    (void)memset(replicas, 0, sizeof(replicas));
    (void)memset(&finality, 0, sizeof(finality));
    make_header(&batch[0], 0U, zero, 0x10U);
    make_header(&batch[1], 1U, batch[0].resulting_state_root, 0x20U);
    make_header(&batch[2], 2U, batch[1].resulting_state_root, 0x30U);
    conflict = batch[2];
    conflict.resulting_state_root[0] = 0x3fU;
    for (i = 0U; i < 3U && status == LXP_OK; ++i) {
        status = replica_accept(&replicas[i], &batch[0]);
        if (status == LXP_OK)
            status = finality_vote(&finality, (uint8_t)i, &batch[0]);
    }
    if (status == LXP_OK) status = replica_accept(&replicas[0], &batch[1]);
    if (status == LXP_OK) status = finality_vote(&finality, 0U, &batch[1]);
    if (status == LXP_OK) status = replica_accept(&replicas[1], &batch[1]);
    if (status == LXP_OK) status = finality_vote(&finality, 1U, &batch[1]);
    if (status == LXP_OK &&
        (!finality.has_finalised || finality.finalised_batch != 1U ||
         memcmp(finality.finalised_root, batch[1].resulting_state_root,
                32U) != 0)) status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK &&
        replica_accept(&replicas[2], &batch[2]) != LXP_ERR_BATCH_GAP)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK) status = replica_accept(&replicas[0], &batch[2]);
    if (status == LXP_OK) status = finality_vote(&finality, 0U, &batch[2]);
    if (status == LXP_OK) status = replica_accept(&replicas[1], &conflict);
    if (status == LXP_OK) status = finality_vote(&finality, 1U, &conflict);
    if (status == LXP_OK && finality.finalised_batch != 1U)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK) status = replica_accept(&replicas[2], &batch[1]);
    if (status == LXP_OK) status = finality_vote(&finality, 2U, &batch[1]);
    if (status == LXP_OK) status = replica_accept(&replicas[2], &batch[2]);
    if (status == LXP_OK) status = finality_vote(&finality, 2U, &batch[2]);
    if (status == LXP_OK &&
        (finality.finalised_batch != 2U ||
         memcmp(finality.finalised_root, batch[2].resulting_state_root,
                32U) != 0)) status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK) status = lxp_replica_halt(&replicas[1]);
    if (status == LXP_OK && (!replicas[1].halted ||
        !replicas[1].serving_finalised_history ||
        replicas[1].serving_current_state)) status = LXP_FATAL_INVARIANT;
    return status;
}

lxp_result lxp_sequencer_loss_sim(void)
{
    uint8_t zero[32] = { 0U };
    uint8_t next_id[32] = { 0x91U };
    lxp_batch_header checkpoint;
    lxp_batch_header candidate;
    lxp_batch_header unauthorized;
    lxp_sequencer_liveness liveness = { true, false, {0U}, 0U };
    uint8_t checkpoint_root[32];
    size_t attempt;
    lxp_result status;
    make_header(&checkpoint, 7U, zero, 0x70U);
    make_header(&candidate, 8U, checkpoint.resulting_state_root, 0x80U);
    (void)memcpy(checkpoint_root, checkpoint.resulting_state_root, 32U);
    status = lxp_sequencer_loss(&liveness);
    for (attempt = 0U; attempt < 4096U && status == LXP_OK; ++attempt) {
        if (lxp_sequencer_can_seal(&liveness, &candidate) !=
            LXP_ERR_MODULE_DISABLED) status = LXP_FATAL_INVARIANT;
    }
    if (status == LXP_OK &&
        memcmp(checkpoint.resulting_state_root, checkpoint_root, 32U) != 0)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK &&
        lxp_sequencer_handover_authorize(&liveness, zero, 8U) !=
        LXP_ERR_AUTH_SCOPE) status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = lxp_sequencer_handover_authorize(&liveness, next_id, 8U);
    unauthorized = candidate;
    unauthorized.sequencer_id[0] = 0x71U;
    (void)memcpy(candidate.sequencer_id, next_id, 32U);
    if (status == LXP_OK &&
        lxp_sequencer_can_seal(&liveness, &unauthorized) !=
        LXP_ERR_AUTH_SCOPE) status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = lxp_sequencer_can_seal(&liveness, &candidate);
    if (status == LXP_OK)
        status = lxp_batch_range_check(&checkpoint, &candidate);
    return status;
}
