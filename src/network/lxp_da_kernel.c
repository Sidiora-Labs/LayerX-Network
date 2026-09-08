#include "layerx/lxp_da.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state_diff.h"
#include "layerx/lxp_replica.h"
#include "../state/lxp_state_internal.h"

lxp_result lxp_da_recovery_from_kernel(
    const lxp_kernel *kernel, uint64_t receipt_watermark,
    uint64_t projection_watermark, lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_da_module_root roots[LXP_DA_MAX_MODULE_ROOTS];
    const lx_account_registry empty = {0};
    lxp_byte_span frontier;
    size_t count, i;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL ||
        kernel->state->accounts == NULL || arena == NULL || encoded == NULL ||
        receipt_watermark >= kernel->state->next_sequence ||
        projection_watermark > receipt_watermark)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_module_root_count(kernel, &count);
    if (status != LXP_OK) return status;
    if (count > LXP_DA_MAX_MODULE_ROOTS) return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        roots[i].module_id = (uint16_t)i;
        status = lxp_state_subtree_root(kernel, (uint16_t)i, roots[i].state_root);
    }
    if (status == LXP_OK)
        status = lxp_state_diff_encode(&empty, kernel->state->accounts,
                                       arena, &frontier);
    if (status == LXP_OK) {
        const lxp_da_recovery_input input = {roots, count, frontier,
            kernel->state->next_sequence, receipt_watermark, projection_watermark};
        status = lxp_da_recovery_metadata_encode(&input, arena, encoded);
    }
    return status;
}

lxp_result lxp_da_recovery_verify_kernel(
    const lxp_kernel *kernel, uint64_t receipt_watermark,
    uint64_t projection_watermark, lxp_byte_span encoded, lxp_arena *arena)
{
    lxp_byte_span recomputed;
    size_t mark;
    lxp_result status;
    if (arena == NULL || (encoded.bytes == NULL && encoded.length != 0U))
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_da_recovery_from_kernel(kernel, receipt_watermark,
                                         projection_watermark, arena, &recomputed);
    if (status == LXP_OK &&
        (encoded.length != recomputed.length ||
         lxp_ct_memcmp(encoded.bytes, recomputed.bytes, encoded.length) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_da_body_from_kernels(
    const lxp_batch_header *header,
    const lxp_kernel *before, const lxp_kernel *after,
    const lxp_byte_span *activities, size_t activity_count,
    const lxp_byte_span *receipts, size_t receipt_count,
    const lxp_byte_span *events, size_t event_count,
    const lxp_byte_span *oracles, size_t oracle_count,
    lxp_arena *arena, lxp_batch_body *body)
{
    lxp_batch_body built = {0};
    size_t mark;
    lxp_result status;
    if (header == NULL || before == NULL || after == NULL ||
        before->state == NULL || after->state == NULL ||
        before->state->accounts == NULL || after->state->accounts == NULL ||
        arena == NULL || body == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (header->first_sequence != before->state->next_sequence ||
        header->last_sequence == UINT64_MAX ||
        header->last_sequence + 1U != after->state->next_sequence ||
        lxp_ct_memcmp(header->previous_state_root,
                      before->current_state_root, 32U) != 0 ||
        lxp_ct_memcmp(header->resulting_state_root,
                      after->current_state_root, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    mark = lxp_arena_mark(arena);
    built.header = *header;
    status = lxp_replay_section_encode(activities, activity_count, arena, &built.activities);
    if (status == LXP_OK)
        status = lxp_da_receipt_section_encode(receipts, receipt_count,
                                               events, event_count, arena, &built.receipts);
    if (status == LXP_OK)
        status = lxp_replay_section_encode(events, event_count, arena, &built.events);
    if (status == LXP_OK)
        status = lxp_replay_section_encode(oracles, oracle_count, arena, &built.oracle_inputs);
    if (status == LXP_OK)
        status = lxp_state_diff_encode(before->state->accounts, after->state->accounts,
                                       arena, &built.state_diff);
    if (status == LXP_OK)
        status = lxp_da_recovery_from_kernel(after, header->last_sequence,
                                             header->last_sequence, arena,
                                             &built.recovery_metadata);
    if (status == LXP_OK)
        status = lxp_batch_availability_root(&built, arena,
                                              built.header.data_availability_root);
    if (status != LXP_OK) {
        (void)lxp_arena_reset(arena, mark);
        return status;
    }
    *body = built;
    return LXP_OK;
}
