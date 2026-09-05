#ifndef LAYERX_LXP_DAEMON_BATCH_WAL_H
#define LAYERX_LXP_DAEMON_BATCH_WAL_H

#include "layerx/lxp_batch.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_merkle.h"

#include <stddef.h>
#include <stdbool.h>
#include <stdint.h>

enum { LXP_DAEMON_BATCH_WAL_MAX_ITEMS = 64 };

typedef enum lxp_daemon_batch_wal_state {
    LXP_DAEMON_BATCH_WAL_PREPARED = 1,
    LXP_DAEMON_BATCH_WAL_ABORTED = 2,
    LXP_DAEMON_BATCH_WAL_COMMITTED = 3
} lxp_daemon_batch_wal_state;

typedef enum lxp_daemon_batch_wal_recovery {
    LXP_DAEMON_BATCH_WAL_DISCARD_BASE = 1,
    LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED = 2,
    LXP_DAEMON_BATCH_WAL_ALREADY_ABORTED = 3,
    LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED = 4
} lxp_daemon_batch_wal_recovery;

typedef struct lxp_daemon_batch_wal_input {
    uint16_t protocol_version;
    uint32_t network_id;
    uint64_t epoch;
    uint64_t batch_number;
    uint64_t timestamp_ms;
    uint32_t parameter_version;
    uint32_t fee_schedule_version;
    uint32_t metering_schedule_version;
    uint64_t first_sequence;
    uint64_t last_sequence;
    size_t count;
    lxp_kernel_batch_boundary base;
    lxp_kernel_batch_boundary settled;
    uint8_t publication_digest[32];
    lxp_sequencer_authorization authorization;
    lxp_byte_span canonical_header;
    uint8_t header_signature[64];
    const lxp_byte_span *activities;
    const lxp_byte_span *receipts;
    const lxp_byte_span *events;
    const lxp_byte_span *terminal_payloads;
    const lxp_byte_span *call_graphs;
    const lxp_merkle_proof *receipt_proofs;
} lxp_daemon_batch_wal_input;

typedef struct lxp_daemon_batch_wal_record
    lxp_daemon_batch_wal_record;

typedef lxp_result (*lxp_daemon_batch_wal_checkpoint_fn)(
    void *context, const lxp_kernel_batch_boundary *settled);

lxp_result lxp_daemon_batch_bind_prefix(
    const lxp_byte_span *canonical_activities, size_t count,
    const uint8_t base_state_root[32],
    uint64_t first_sequence, uint64_t batch_number,
    lxp_arena *arena, lxp_kernel_execution *executions,
    lxp_batch_roots *roots, uint8_t batch_id[32]);

lxp_result lxp_daemon_batch_wal_write_prepared(
    const char *checkpoint_directory,
    const lxp_daemon_batch_wal_input *input,
    uint8_t fsynced_publication_digest[32]);
lxp_result lxp_daemon_batch_wal_commit_kernel(
    const char *checkpoint_directory,
    const lxp_daemon_batch_wal_input *input,
    lxp_kernel *kernel, lxp_identity_store *identities,
    const lxp_activity *activities,
    lxp_kernel_prepared_batch *prepared,
    lxp_daemon_batch_wal_checkpoint_fn checkpoint,
    void *checkpoint_context,
    lxp_daemon_batch_wal_record **record);
lxp_result lxp_daemon_batch_wal_load(
    const char *checkpoint_directory,
    const lxp_sequencer_authorization *authorization,
    lxp_daemon_batch_wal_record **record, bool *present);
lxp_result lxp_daemon_batch_wal_classify(
    const lxp_daemon_batch_wal_record *record,
    const lxp_kernel_batch_boundary *live,
    lxp_daemon_batch_wal_recovery *recovery);
lxp_result lxp_daemon_batch_wal_transition(
    const char *checkpoint_directory,
    lxp_daemon_batch_wal_record *record,
    const lxp_kernel_batch_boundary *live,
    lxp_daemon_batch_wal_state state);
lxp_result lxp_daemon_batch_wal_retire(
    const char *checkpoint_directory,
    const lxp_daemon_batch_wal_record *record,
    const lxp_kernel_batch_boundary *live);
const lxp_daemon_batch_wal_input *lxp_daemon_batch_wal_view(
    const lxp_daemon_batch_wal_record *record);
lxp_daemon_batch_wal_state lxp_daemon_batch_wal_record_state(
    const lxp_daemon_batch_wal_record *record);
void lxp_daemon_batch_wal_destroy(lxp_daemon_batch_wal_record *record);

#endif
