#define _POSIX_C_SOURCE 200809L

#include "lxp_daemon_batch_wal.h"

#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

struct lxp_daemon_batch_wal_record {
    lxp_daemon_batch_wal_state state;
    lxp_daemon_batch_wal_input view;
    lxp_byte_span activities[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span receipts[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span events[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_merkle_proof proofs[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    uint8_t *owned;
    size_t owned_length;
};

static void boundary(lxp_kernel_batch_boundary *value, uint8_t tag,
                     uint64_t next_sequence)
{
    (void)memset(value, 0, sizeof(*value));
    value->canonical_state_root[0] = tag;
    value->receipt_state_root[0] = (uint8_t)(tag + 1U);
    value->next_sequence = next_sequence;
}

static int expect_classification(
    lxp_daemon_batch_wal_record *record,
    const lxp_kernel_batch_boundary *live,
    lxp_daemon_batch_wal_recovery expected)
{
    lxp_daemon_batch_wal_recovery actual = 0;
    return lxp_daemon_batch_wal_classify(record, live, &actual) == LXP_OK &&
        actual == expected ? 0 : 1;
}

static int classify_recovery_matrix(void)
{
    lxp_daemon_batch_wal_record record;
    lxp_kernel_batch_boundary unrelated;
    lxp_kernel_batch_boundary changed_root;
    lxp_daemon_batch_wal_recovery recovery;
    (void)memset(&record, 0, sizeof(record));
    boundary(&record.view.base, 0x11U, 8U);
    boundary(&record.view.settled, 0x21U, 10U);
    boundary(&unrelated, 0x31U, 12U);

    record.state = LXP_DAEMON_BATCH_WAL_PREPARED;
    if (expect_classification(&record, &record.view.base,
                              LXP_DAEMON_BATCH_WAL_DISCARD_BASE) != 0 ||
        expect_classification(&record, &record.view.settled,
                              LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED) != 0)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_ABORTED;
    if (expect_classification(&record, &record.view.base,
                              LXP_DAEMON_BATCH_WAL_ALREADY_ABORTED) != 0 ||
        lxp_daemon_batch_wal_classify(&record, &record.view.settled,
                                      &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_COMMITTED;
    if (expect_classification(&record, &record.view.settled,
                              LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED) != 0 ||
        lxp_daemon_batch_wal_classify(&record, &record.view.base,
                                      &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        lxp_daemon_batch_wal_classify(&record, &unrelated, &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_PREPARED;
    changed_root = record.view.base;
    changed_root.receipt_state_root[31] = 1U;
    if (lxp_daemon_batch_wal_classify(&record, &changed_root, &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        lxp_daemon_batch_wal_classify(NULL, &record.view.base, &recovery) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_daemon_batch_wal_classify(&record, NULL, &recovery) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_daemon_batch_wal_classify(&record, &record.view.base, NULL) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}

static int write_exact(int descriptor, const uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written <= 0) return 1;
        offset += (size_t)written;
    }
    return 0;
}

static int refuse_malformed_record(void)
{
    enum { MINIMUM_WAL_BYTES = 794 };
    char directory[] = "/tmp/lxp-batch-wal-corrupt-XXXXXX";
    char path[128];
    uint8_t bytes[MINIMUM_WAL_BYTES] = {0U};
    lxp_sequencer_authorization authorization;
    lxp_daemon_batch_wal_record *record = NULL;
    bool present = false;
    int descriptor;
    (void)memset(&authorization, 0, sizeof(authorization));
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/prepared-batch.lxw", directory) < 0)
        return 1;
    descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0 || write_exact(descriptor, bytes, sizeof(bytes)) != 0 ||
        fdatasync(descriptor) != 0 || close(descriptor) != 0 ||
        lxp_daemon_batch_wal_load(directory, &authorization, &record,
                                  &present) != LXP_ERR_LOG_CORRUPT ||
        record != NULL || present || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

static int sweep_interrupted_replacement(void)
{
    char directory[] = "/tmp/lxp-batch-wal-sweep-XXXXXX";
    char path[160];
    uint8_t byte = 1U;
    lxp_sequencer_authorization authorization;
    lxp_daemon_batch_wal_record *record = NULL;
    bool present = true;
    int descriptor;
    (void)memset(&authorization, 0, sizeof(authorization));
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/.prepared-batch.%llu.1.tmp",
                 directory, (unsigned long long)getpid()) < 0)
        return 1;
    descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0 || write_exact(descriptor, &byte, sizeof(byte)) != 0 ||
        fdatasync(descriptor) != 0 || close(descriptor) != 0 ||
        lxp_daemon_batch_wal_load(directory, &authorization, &record,
                                  &present) != LXP_OK ||
        record != NULL || present || access(path, F_OK) == 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

int main(void)
{
    return classify_recovery_matrix() != 0 ||
        refuse_malformed_record() != 0 ||
        sweep_interrupted_replacement() != 0 ? 1 : 0;
}
