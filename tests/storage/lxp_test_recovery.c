#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct replay_state {
    uint8_t balance;
    uint8_t expected;
    int have_expected;
    int force_divergence;
} replay_state;

static lxp_result replay(void *opaque, const lxp_log_record_header *header,
                         const uint8_t *body)
{
    replay_state *state = (replay_state *)opaque;
    if (header->body_length != 1U) return LXP_FATAL_REPLAY_DIVERGENCE;
    if (header->record_kind == (uint8_t)LXP_LOG_CHECKPOINT)
        state->balance = body[0];
    else if (header->record_kind == (uint8_t)LXP_LOG_RECEIPT) {
        state->expected = body[0];
        state->have_expected = 1;
    } else if (header->record_kind == (uint8_t)LXP_LOG_STATE_DIFF) {
        state->balance = (uint8_t)(state->balance + body[0]);
        if (state->force_divergence != 0 || state->have_expected == 0 ||
            state->balance != state->expected)
            return LXP_FATAL_REPLAY_DIVERGENCE;
        state->have_expected = 0;
    }
    return LXP_OK;
}

static int recover_existing_log(const char *prefix,
                                lxp_log_record_kind kind,
                                uint64_t sequence, uint8_t value)
{
    char directory[64];
    char path[128];
    lxp_log log;
    lxp_log_record_header header;
    uint8_t recovered = 0U;
    uint64_t durable_end;
    lxp_result recovery_status;
    int length;
    length = snprintf(directory, sizeof(directory), "/tmp/%s-XXXXXX", prefix);
    if (length < 0 || (size_t)length >= sizeof(directory) ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK)
        return 1;
    if (lxp_log_append(&log, kind, sequence, &value, 1U, NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK) return 1;
    durable_end = log.write_offset;
    if (lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK || log.write_offset != 0U)
        return 1;
    recovery_status = kind == LXP_LOG_CHECKPOINT ?
        lxp_log_recover(&log, NULL, NULL) :
        lxp_log_recover_complete_records(&log, NULL, NULL);
    if (recovery_status != LXP_OK ||
        log.write_offset != durable_end ||
        lxp_log_resume_sequence(&log) != sequence + 1U ||
        lxp_log_read(&log, 0U, &header, &recovered, sizeof(recovered)) !=
            LXP_OK ||
        header.record_kind != (uint8_t)kind ||
        header.global_sequence != sequence || recovered != value ||
        lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 1;
    return 0;
}

static int classify_read_failure(void)
{
    char directory[] = "/tmp/lxp-read-io-XXXXXX";
    char path[128];
    lxp_log log;
    uint64_t valid_end;
    uint64_t last;
    uint64_t next;
    lxp_result status;
    if (mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
        return 1;
    if (close(log.descriptor) != 0)
        return 1;
    status = lxp_log_scan_tail(&log, &valid_end, &last, &next);
    if (status != LXP_ERR_IO) {
        (void)fprintf(stderr, "closed-log scan returned %d\n", (int)status);
        return 1;
    }
    log.descriptor = -1;
    return unlink(path) == 0 && rmdir(directory) == 0 ? 0 : 1;
}

static int refuse_receipt_only_canonical(void)
{
    char directory[] = "/tmp/lxp-receipt-only-XXXXXX";
    char path[128];
    uint8_t body = 0x63U;
    lxp_log log;
    if (mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_RECEIPT, 9U, &body, 1U, NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK ||
        lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK ||
        lxp_log_recover(&log, NULL, NULL) != LXP_ERR_LOG_CORRUPT ||
        log.write_offset != 0U || lxp_log_close(&log) != LXP_OK ||
        unlink(path) != 0 || rmdir(directory) != 0) return 1;
    return 0;
}

static int recover_torn_tail(const char *prefix,
                             lxp_log_record_kind kind,
                             bool complete_records)
{
    char directory[64];
    char path[128];
    uint8_t body = 0x71U;
    const uint8_t partial[] = {'L', 'X', 'P', 'L', 1U, 0U, 0U};
    uint8_t after = 0xffU;
    uint64_t durable_end;
    lxp_log log;
    lxp_result status;
    int length = snprintf(directory, sizeof(directory),
                          "/tmp/%s-XXXXXX", prefix);
    if (length < 0 || (size_t)length >= sizeof(directory) ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        lxp_log_append(&log, kind, 13U, &body, 1U, NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK)
        return 1;
    durable_end = log.write_offset;
    if (pwrite(log.descriptor, partial, sizeof(partial),
               (off_t)durable_end) != (ssize_t)sizeof(partial) ||
        ftruncate(log.descriptor,
                  (off_t)(durable_end + sizeof(partial))) != 0 ||
        lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK)
        return 1;
    status = complete_records ?
        lxp_log_recover_complete_records(&log, NULL, NULL) :
        lxp_log_recover(&log, NULL, NULL);
    if (status != LXP_OK || log.write_offset != durable_end ||
        lxp_log_resume_sequence(&log) != 14U ||
        pread(log.descriptor, &after, 1U, (off_t)durable_end) != 1 ||
        after != 0U || lxp_log_close(&log) != LXP_OK ||
        unlink(path) != 0 || rmdir(directory) != 0)
        return 1;
    return 0;
}

static int refuse_corrupt_chain(const char *prefix,
                                lxp_log_record_kind kind,
                                bool complete_records)
{
    char directory[64];
    char path[128];
    uint8_t body = 0x82U;
    uint8_t corrupt;
    uint64_t second_offset;
    struct stat before;
    struct stat after;
    lxp_log log;
    lxp_result status;
    int length = snprintf(directory, sizeof(directory),
                          "/tmp/%s-XXXXXX", prefix);
    if (length < 0 || (size_t)length >= sizeof(directory) ||
        mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        lxp_log_append(&log, kind, 21U, &body, 1U, NULL) != LXP_OK ||
        lxp_log_append(&log, kind, 22U, &body, 1U, &second_offset) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK ||
        pread(log.descriptor, &corrupt, 1U,
              (off_t)(second_offset + 31U)) != 1)
        return 1;
    corrupt ^= 1U;
    if (pwrite(log.descriptor, &corrupt, 1U,
               (off_t)(second_offset + 31U)) != 1 ||
        fstat(log.descriptor, &before) != 0 ||
        lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK)
        return 1;
    status = complete_records ?
        lxp_log_recover_complete_records(&log, NULL, NULL) :
        lxp_log_recover(&log, NULL, NULL);
    if (status != LXP_ERR_LOG_CORRUPT || log.write_offset != 0U ||
        fstat(log.descriptor, &after) != 0 ||
        before.st_size != after.st_size ||
        lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

static int recover_from_checkpoint(void)
{
    char directory[] = "/tmp/lxp-checkpoint-restart-XXXXXX";
    char path[128];
    uint8_t checkpoint = 9U;
    uint8_t activity = 0x31U;
    uint8_t expected = 10U;
    uint8_t increment = 1U;
    replay_state state = {0U, 0U, 0, 0};
    lxp_log log;
    if (mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_CHECKPOINT, 5U, &checkpoint, 1U,
                       NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_ACTIVITY, 6U, &activity, 1U,
                       NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_RECEIPT, 6U, &expected, 1U,
                       NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_STATE_DIFF, 6U, &increment, 1U,
                       NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK ||
        lxp_log_close(&log) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK ||
        lxp_log_recover(&log, replay, &state) != LXP_OK ||
        state.balance != 10U || state.have_expected != 0 ||
        lxp_log_resume_sequence(&log) != 7U ||
        lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

int main(void)
{
    char directory[] = "/tmp/lxp-recovery-XXXXXX";
    char path[128];
    uint8_t one = 1U;
    uint8_t expected = 1U;
    uint8_t partial[7] = { 'L', 'X', 'P', 'L', 1U, 0U, 0U };
    lxp_log log;
    replay_state state = { 0U, 0U, 0, 0 };
    replay_state divergent = { 0U, 0U, 0, 1 };
    uint64_t incomplete_offset;
    if (mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK) {
        (void)fprintf(stderr, "recovery setup failed\n");
        return 1;
    }
    if (lxp_log_append(&log, LXP_LOG_ACTIVITY, 0U, &one, 1U, NULL) != LXP_OK ||
        lxp_log_sync(&log) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_RECEIPT, 0U, &expected, 1U, NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_STATE_DIFF, 0U, &one, 1U, NULL) != LXP_OK ||
        lxp_log_sync(&log) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_ACTIVITY, 1U, &one, 1U,
                       &incomplete_offset) != LXP_OK) {
        (void)fprintf(stderr, "recovery append failed\n");
        return 1;
    }
    if (pwrite(log.descriptor, partial, sizeof(partial),
               (off_t)log.write_offset) != (ssize_t)sizeof(partial) ||
        ftruncate(log.descriptor,
                  (off_t)(log.write_offset + sizeof(partial))) != 0 ||
        lxp_log_close(&log) != LXP_OK) {
        (void)fprintf(stderr, "partial tail write failed\n");
        return 1;
    }
    if (snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_open(&log, path) != LXP_OK) {
        (void)fprintf(stderr, "recovery open failed\n");
        return 1;
    }
    {
        lxp_result recovery_status = lxp_log_recover(&log, replay, &state);
        if (recovery_status != LXP_OK) {
            (void)fprintf(stderr, "recovery operation failed: %d\n",
                          (int)recovery_status);
            return 1;
        }
    }
    if (state.balance != 1U || state.have_expected != 0 ||
        lxp_log_resume_sequence(&log) != 1U ||
        log.write_offset != incomplete_offset) {
        (void)fprintf(stderr, "recovery state failed: %u %d %llu %llu %llu\n",
                      state.balance, state.have_expected,
                      (unsigned long long)lxp_log_resume_sequence(&log),
                      (unsigned long long)log.write_offset,
                      (unsigned long long)incomplete_offset);
        return 1;
    }
    if (lxp_log_recover(&log, replay, &divergent) !=
        LXP_FATAL_REPLAY_DIVERGENCE) {
        (void)fprintf(stderr, "divergence was not fatal\n");
        return 1;
    }
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 1;
    if (recover_existing_log("lxp-canonical-restart", LXP_LOG_CHECKPOINT,
                             7U, 0x41U) != 0 ||
        recover_existing_log("lxp-batch-restart", LXP_LOG_BATCH_HEADER,
                             11U, 0x52U) != 0) {
        (void)fprintf(stderr, "durable restart recovery failed\n");
        return 1;
    }
    if (classify_read_failure() != 0) {
        (void)fprintf(stderr, "recovery IO classification failed\n");
        return 1;
    }
    if (refuse_receipt_only_canonical() != 0) {
        (void)fprintf(stderr, "receipt-only canonical log was accepted\n");
        return 1;
    }
    if (recover_torn_tail("lxp-canonical-torn", LXP_LOG_CHECKPOINT,
                          false) != 0 ||
        recover_torn_tail("lxp-batch-torn", LXP_LOG_BATCH_HEADER,
                          true) != 0) {
        (void)fprintf(stderr, "torn-tail recovery failed\n");
        return 1;
    }
    if (refuse_corrupt_chain("lxp-canonical-corrupt", LXP_LOG_CHECKPOINT,
                             false) != 0 ||
        refuse_corrupt_chain("lxp-batch-corrupt", LXP_LOG_BATCH_HEADER,
                             true) != 0) {
        (void)fprintf(stderr, "corrupt-chain recovery was not refused\n");
        return 1;
    }
    if (recover_from_checkpoint() != 0) {
        (void)fprintf(stderr, "checkpoint restart replay failed\n");
        return 1;
    }
    return 0;
}
