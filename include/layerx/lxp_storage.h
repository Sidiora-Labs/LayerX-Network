#ifndef LAYERX_LXP_STORAGE_H
#define LAYERX_LXP_STORAGE_H

#include "layerx/lxp_result.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

#define LXP_LOG_MAGIC UINT32_C(0x4c58504c)
#define LXP_LOG_HEADER_BYTES 32U

typedef enum lxp_log_record_kind {
    LXP_LOG_ACTIVITY = 1,
    LXP_LOG_RECEIPT = 2,
    LXP_LOG_BATCH_HEADER = 3,
    LXP_LOG_CHECKPOINT = 4,
    LXP_LOG_STATE_DIFF = 5,
    LXP_LOG_ORACLE = 6,
    LXP_LOG_GENESIS = 7,
    LXP_LOG_REPLICA_ACK = 8,
    LXP_LOG_BATCH_BODY = 9
} lxp_log_record_kind;

typedef struct lxp_log_record_header {
    uint32_t magic;
    uint8_t record_kind;
    uint8_t reserved[3];
    uint64_t global_sequence;
    uint32_t body_length;
    uint32_t body_crc32c;
    uint64_t previous_record_offset;
} lxp_log_record_header;
#define lxp_log_record_header lxp_log_record_header

typedef struct lxp_log {
    int descriptor;
    uint64_t segment_sequence;
    uint64_t capacity;
    uint64_t write_offset;
    uint64_t previous_record_offset;
    uint64_t next_sequence;
    uint64_t durable_offset;
    uint64_t durable_previous_record_offset;
    uint64_t durable_next_sequence;
    uint64_t durable_generation;
    bool has_durable_marker;
} lxp_log;

uint32_t lxp_log_crc32c(const void *bytes, size_t length);
lxp_result lxp_log_segment_create(lxp_log *log, const char *directory,
                                  uint64_t segment_sequence,
                                  uint64_t segment_size);
lxp_result lxp_log_open(lxp_log *log, const char *path);
lxp_result lxp_log_append(lxp_log *log, lxp_log_record_kind kind,
                          uint64_t global_sequence, const void *body,
                          uint32_t body_length, uint64_t *record_offset);
lxp_result lxp_log_read(const lxp_log *log, uint64_t record_offset,
                        lxp_log_record_header *header, void *body,
                        size_t body_capacity);
lxp_result lxp_log_close(lxp_log *log);
lxp_result lxp_log_sync(lxp_log *log);
lxp_result lxp_log_write_boundary(lxp_log *log);
lxp_result lxp_log_durable_head(const lxp_log *log, uint64_t *global_sequence);
bool lxp_log_fault_point(uint32_t boundary, uint32_t abort_boundary);

typedef lxp_result (*lxp_log_replay_fn)(void *context,
                                       const lxp_log_record_header *header,
                                       const uint8_t *body);
lxp_result lxp_log_scan_tail(const lxp_log *log, uint64_t *valid_end,
                             uint64_t *last_record_offset,
                             uint64_t *next_sequence);
lxp_result lxp_log_truncate_partial(lxp_log *log, uint64_t valid_end);
lxp_result lxp_log_recover(lxp_log *log, lxp_log_replay_fn replay,
                           void *context);
lxp_result lxp_log_recover_complete_records(lxp_log *log,
                                            lxp_log_replay_fn replay,
                                            void *context);
uint64_t lxp_log_resume_sequence(const lxp_log *log);

#endif
