#include "layerx/lxp_replica.h"
#include "layerx/lxp_kernel.h"

#include <stdlib.h>
#include <string.h>

struct lxp_replay_checkpoint {
    lxp_kernel *live;
    lxp_kernel kernel;
    lxp_state_snapshot *state;
};

void lxp_replay_checkpoint_destroy(lxp_replay_checkpoint *checkpoint)
{
    if (checkpoint == NULL) return;
    for (size_t i = 0U; i < checkpoint->kernel.blob_count; ++i)
        free(checkpoint->kernel.blobs[i].bytes);
    lxp_state_snapshot_destroy(checkpoint->state);
    free(checkpoint);
}

lxp_result lxp_replay_checkpoint_create(lxp_kernel *kernel,
    lxp_replay_checkpoint **checkpoint)
{
    lxp_replay_checkpoint *created;
    lxp_result status;
    size_t total = 0U;
    if (kernel == NULL || checkpoint == NULL || kernel->state == NULL ||
        kernel->journal == NULL || kernel->journal->open ||
        kernel->blob_count > LXP_KERNEL_MAX_BLOBS)
        return LXP_ERR_NON_CANONICAL;
    *checkpoint = NULL;
    created = calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    created->live = kernel;
    created->kernel = *kernel;
    for (size_t i = 0U; i < created->kernel.blob_count; ++i)
        created->kernel.blobs[i].bytes = NULL;
    status = lxp_state_snapshot_create(kernel->state, &created->state);
    for (size_t i = 0U; status == LXP_OK && i < kernel->blob_count; ++i) {
        const lxp_module_blob *blob = &kernel->blobs[i];
        if (blob->bytes == NULL || blob->length == 0U ||
            blob->length > LXP_KERNEL_MAX_BLOB_BYTES ||
            total > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES - blob->length) {
            status = LXP_FATAL_INVARIANT;
            break;
        }
        total += blob->length;
        created->kernel.blobs[i].bytes = malloc(blob->length);
        if (created->kernel.blobs[i].bytes == NULL) {
            status = LXP_ERR_ARENA_EXHAUSTED;
            break;
        }
        memcpy(created->kernel.blobs[i].bytes, blob->bytes, blob->length);
    }
    if (status == LXP_OK && total != kernel->blob_total_bytes)
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK) {
        lxp_replay_checkpoint_destroy(created);
        return status;
    }
    *checkpoint = created;
    return LXP_OK;
}

lxp_result lxp_replay_checkpoint_restore(lxp_replay_checkpoint *checkpoint)
{
    lxp_result status;
    lxp_kernel *live;
    if (checkpoint == NULL || checkpoint->live == NULL)
        return LXP_ERR_NON_CANONICAL;
    live = checkpoint->live;
    if (live->journal->open) {
        status = lxp_state_journal_rollback(live->journal);
        if (status != LXP_OK) return status;
    }
    status = lxp_state_snapshot_restore(checkpoint->state, live->state);
    if (status != LXP_OK) return status;
    for (size_t i = 0U; i < live->blob_count; ++i)
        free(live->blobs[i].bytes);
    *live = checkpoint->kernel;
    checkpoint->kernel.blob_count = 0U;
    checkpoint->live = NULL;
    return LXP_OK;
}
