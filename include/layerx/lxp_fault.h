#ifndef LAYERX_LXP_FAULT_H
#define LAYERX_LXP_FAULT_H

#include "layerx/lxp_result.h"

#include <stdint.h>

typedef enum lxp_fault_boundary {
    LXP_FAULT_LOG_HEADER_WRITTEN = 1,
    LXP_FAULT_LOG_BODY_WRITTEN = 2,
    LXP_FAULT_LOG_SYNCED = 3,
    LXP_FAULT_INDEX_RECEIPT_WRITTEN = 4,
    LXP_FAULT_INDEX_BALANCE_WRITTEN = 5,
    LXP_FAULT_INDEX_WATERMARK_WRITTEN = 6,
    LXP_FAULT_INDEX_COMMITTED = 7,
    LXP_FAULT_CHECKPOINT_HEADER_WRITTEN = 8,
    LXP_FAULT_CHECKPOINT_BODY_WRITTEN = 9,
    LXP_FAULT_CHECKPOINT_FILE_SYNCED = 10,
    LXP_FAULT_CHECKPOINT_RENAMED = 11,
    LXP_FAULT_CHECKPOINT_DIRECTORY_SYNCED = 12,
    LXP_FAULT_ADMISSION_TEMP_WRITTEN = 13,
    LXP_FAULT_ADMISSION_TEMP_SYNCED = 14,
    LXP_FAULT_ADMISSION_RENAMED = 15,
    LXP_FAULT_ADMISSION_DIRECTORY_SYNCED = 16,
    LXP_FAULT_BATCH_WAL_WRITTEN = 17,
    LXP_FAULT_BATCH_WAL_SYNCED = 18,
    LXP_FAULT_BATCH_WAL_NAMED = 19,
    LXP_FAULT_BATCH_WAL_DIRECTORY_SYNCED = 20
} lxp_fault_boundary;

typedef lxp_result (*lxp_fault_workload_fn)(void *context);

lxp_result lxp_fault_arm(lxp_fault_boundary boundary, uint32_t occurrence);
void lxp_fault_disarm(void);
void lxp_fault_inject_point(lxp_fault_boundary boundary);
lxp_result lxp_fault_crash_at_boundary(lxp_fault_boundary boundary,
                                       uint32_t occurrence,
                                       lxp_fault_workload_fn workload,
                                       void *context,
                                       int *child_exit_status);

#endif
