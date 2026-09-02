#ifndef LAYERX_LXP_SNAPSHOT_H
#define LAYERX_LXP_SNAPSHOT_H

#include "layerx/lxp_kernel.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LXP_SNAPSHOT_MODULE_ROOT_COUNT = LXP_MODULE_RESERVED_COUNT + 1,
    LXP_SNAPSHOT_FORMAT_LEGACY = 0x1804,
    LXP_SNAPSHOT_FORMAT_BLOBS = 0x1805,
    LXP_SNAPSHOT_FORMAT_VERSION = LXP_SNAPSHOT_FORMAT_BLOBS,
    LXP_SNAPSHOT_MAX_BLOBS = LXP_KERNEL_MAX_BLOBS,
    LXP_SNAPSHOT_MAX_BLOB_BYTES = LXP_KERNEL_MAX_BLOB_BYTES,
    LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES = LXP_KERNEL_MAX_BLOB_TOTAL_BYTES,
    LXP_SNAPSHOT_BLOB_SECTION_BYTES = 12,
    LXP_SNAPSHOT_BLOB_ENTRY_BYTES = 42
};

typedef struct lxp_snapshot_manifest_record {
    uint64_t global_sequence;
    uint8_t canonical_state_root[32];
    uint8_t receipt_state_root[32];
    uint8_t snapshot_digest[32];
} lxp_snapshot_manifest_record;

lxp_result lxp_snapshot_write(const lxp_kernel *kernel,
                              uint64_t global_sequence, lxp_arena *arena,
                              lxp_byte_span *snapshot);
lxp_result lxp_snapshot_manifest_build(const uint8_t *snapshot,
                                       size_t snapshot_length,
                                       uint64_t global_sequence,
                                       const uint8_t canonical_state_root[32],
                                       const uint8_t receipt_state_root[32],
                                       lxp_snapshot_manifest_record *manifest);
lxp_result lxp_snapshot_manifest(const uint8_t *snapshot,
                                 size_t snapshot_length,
                                 uint64_t global_sequence,
                                 const uint8_t canonical_state_root[32],
                                 const uint8_t receipt_state_root[32],
                                 lxp_snapshot_manifest_record *manifest);
lxp_result lxp_snapshot_verify_root(const lxp_kernel *kernel,
                                    const lxp_snapshot_manifest_record *manifest);
lxp_result lxp_snapshot_load(const uint8_t *snapshot, size_t snapshot_length,
                             const lxp_snapshot_manifest_record *manifest,
                             lxp_kernel *kernel);
lxp_result lxp_snapshot_store_write(const char *directory,
                                    const lxp_snapshot_manifest_record *manifest,
                                    const uint8_t *snapshot,
                                    size_t snapshot_length);
lxp_result lxp_snapshot_store_read(const char *path, lxp_arena *arena,
                                   lxp_snapshot_manifest_record *manifest,
                                   lxp_byte_span *snapshot);

#endif
