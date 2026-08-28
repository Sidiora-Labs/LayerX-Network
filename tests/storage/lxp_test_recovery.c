#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
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
    if (mkdtemp(directory) == NULL ||
        lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
        return 1;
    if (close(log.descriptor) != 0 ||
        lxp_log_scan_tail(&log, &valid_end, &last, &next) != LXP_ERR_IO)
        return 1;
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
    if (classify_read_failure() != 0 ||
        refuse_receipt_only_canonical() != 0) {
        (void)fprintf(stderr, "recovery classification failed\n");
        return 1;
    }
    return 0;
}
