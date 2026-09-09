#include "layerx/lxp_da.h"
#include "layerx/lxp_crypto.h"

#include <string.h>

lxp_result lxp_da_store_read_verified(const lxp_da_store *store,
                                     uint64_t batch_number,
                                     const uint8_t expected_root[32],
                                     lxp_arena *arena,
                                     lxp_da_bundle *bundle)
{
    lxp_da_bundle candidate;
    uint8_t stored_root[32];
    uint8_t recomputed_root[32];
    size_t mark;
    lxp_result status;
    if (store == NULL || expected_root == NULL || arena == NULL ||
        bundle == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(bundle, 0, sizeof(*bundle));
    mark = lxp_arena_mark(arena);
    status = lxp_da_store_read_bundle(store, batch_number, arena,
                                     &candidate, stored_root);
    if (status == LXP_OK)
        status = lxp_da_bundle_root(&candidate, arena, recomputed_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(stored_root, recomputed_root, 32U) != 0 ||
         lxp_ct_memcmp(expected_root, recomputed_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    *bundle = candidate;
    return LXP_OK;
}

lxp_result lxp_da_log_read_body(const lxp_log *log, uint64_t batch_number,
                               lxp_arena *arena, lxp_batch_body *body)
{
    uint64_t offset = 0U;
    size_t mark;
    lxp_result status = LXP_OK;
    if (log == NULL || arena == NULL || body == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    while (status == LXP_OK && offset < log->write_offset) {
        lxp_log_record_header record;
        status = lxp_log_read(log, offset, &record, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        status = LXP_OK;
        if (record.record_kind == (uint8_t)LXP_LOG_BATCH_BODY) {
            void *memory;
            lxp_batch_body candidate;
            uint8_t root[32];
            if (record.body_length > LXP_MAX_BATCH_BODY_BYTES) {
                status = LXP_ERR_LENGTH_LIMIT;
                break;
            }
            status = lxp_arena_alloc(arena, record.body_length, 1U, &memory);
            if (status == LXP_OK)
                status = lxp_log_read(log, offset, &record, memory, record.body_length);
            if (status == LXP_OK)
                status = lxp_batch_body_decode(memory, record.body_length, &candidate);
            if (status == LXP_OK && candidate.header.batch_number == batch_number) {
                status = lxp_batch_availability_root(&candidate, arena, root);
                if (status == LXP_OK && lxp_ct_memcmp(root,
                    candidate.header.data_availability_root, 32U) != 0)
                    status = LXP_ERR_ROOT_MISMATCH;
                if (status == LXP_OK) {
                    *body = candidate;
                    return LXP_OK;
                }
                break;
            }
            (void)lxp_arena_reset(arena, mark);
        }
        if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES - record.body_length) {
            status = LXP_ERR_OVERFLOW;
            break;
        }
        offset += LXP_LOG_HEADER_BYTES + record.body_length;
    }
    (void)lxp_arena_reset(arena, mark);
    return status == LXP_OK ? LXP_ERR_DA_MISSING : status;
}

lxp_result lxp_da_log_store_body(lxp_log *log, const lxp_batch_body *body,
                                lxp_arena *arena)
{
    lxp_batch_body existing;
    lxp_byte_span canonical, previous;
    size_t mark;
    lxp_result status;
    if (log == NULL || body == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_body_encode(body, arena, &canonical);
    if (status == LXP_OK) {
        status = lxp_da_log_read_body(log, body->header.batch_number, arena, &existing);
        if (status == LXP_OK) {
            status = lxp_batch_body_encode(&existing, arena, &previous);
            if (status == LXP_OK && (previous.length != canonical.length ||
                lxp_ct_memcmp(previous.bytes, canonical.bytes, canonical.length) != 0))
                status = LXP_FATAL_REPLAY_DIVERGENCE;
        } else if (status == LXP_ERR_DA_MISSING) {
            status = lxp_log_append(log, (uint8_t)LXP_LOG_BATCH_BODY,
                body->header.last_sequence, canonical.bytes, (uint32_t)canonical.length, NULL);
            if (status == LXP_OK) status = lxp_log_write_boundary(log);
        }
    }
    (void)lxp_arena_reset(arena, mark);
    return status;
}
