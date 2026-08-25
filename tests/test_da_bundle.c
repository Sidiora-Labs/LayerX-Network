#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_da.h"

#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static int reconstruct(const lxp_da_bundle *bundle,
                       uint8_t *classes[LXP_DA_CLASS_COUNT],
                       const size_t capacities[LXP_DA_CLASS_COUNT],
                       size_t lengths[LXP_DA_CLASS_COUNT])
{
    uint8_t seen[LXP_DA_CLASS_COUNT] = {0U};
    size_t previous_class = 0U;
    size_t i;
    (void)memset(lengths, 0, sizeof(size_t) * LXP_DA_CLASS_COUNT);
    for (i = 0U; i < bundle->chunk_count; ++i) {
        const lxp_da_chunk *chunk = &bundle->chunks[i];
        size_t class_index;
        if (chunk->availability_class < LXP_DA_ACTIVITIES ||
            chunk->availability_class > LXP_DA_RECOVERY_METADATA)
            return 1;
        class_index = (size_t)chunk->availability_class - 1U;
        if (class_index < previous_class || chunk->chunk_index != i ||
            chunk->batch_number != bundle->batch_number ||
            chunk->class_offset != lengths[class_index] ||
            chunk->length != chunk->bytes.length ||
            chunk->length > 5U ||
            chunk->length > capacities[class_index] - lengths[class_index])
            return 1;
        if (chunk->length != 0U)
            (void)memcpy(classes[class_index] + lengths[class_index],
                         chunk->bytes.bytes, chunk->length);
        lengths[class_index] += chunk->length;
        seen[class_index] = 1U;
        previous_class = class_index;
    }
    for (i = 0U; i < LXP_DA_CLASS_COUNT; ++i)
        if (seen[i] == 0U) return 1;
    return 0;
}

int main(void)
{
    uint8_t arena_storage[65536];
    uint8_t rebuilt_arena_storage[65536];
    uint8_t changed_arena_storage[65536];
    uint8_t activities[] = {1U, 2U, 3U, 4U, 5U, 6U, 7U};
    uint8_t receipts[] = {11U, 12U, 13U, 14U};
    uint8_t oracle_inputs[] = {
        21U, 22U, 23U, 24U, 25U, 26U, 27U, 28U, 29U
    };
    uint8_t state_diff[] = {31U, 32U, 33U, 34U, 35U, 36U};
    uint8_t frontier[] = {41U, 42U, 43U, 44U, 45U};
    uint8_t rebuilt_activities[sizeof(activities)];
    uint8_t rebuilt_receipts[sizeof(receipts)];
    uint8_t rebuilt_oracles[sizeof(oracle_inputs)];
    uint8_t rebuilt_state_diff[sizeof(state_diff)];
    uint8_t rebuilt_recovery[256];
    uint8_t *rebuilt_classes[LXP_DA_CLASS_COUNT] = {
        rebuilt_activities, rebuilt_receipts, rebuilt_oracles,
        rebuilt_state_diff, rebuilt_recovery
    };
    size_t capacities[LXP_DA_CLASS_COUNT] = {
        sizeof(rebuilt_activities), sizeof(rebuilt_receipts),
        sizeof(rebuilt_oracles), sizeof(rebuilt_state_diff),
        sizeof(rebuilt_recovery)
    };
    size_t rebuilt_lengths[LXP_DA_CLASS_COUNT];
    lxp_da_module_root module_roots[2];
    lxp_da_recovery_input recovery_input;
    lxp_byte_span recovery_metadata;
    lxp_batch_body body;
    lxp_batch_body rebuilt_body;
    lxp_batch_body zero_body;
    lxp_da_bundle bundle;
    lxp_da_bundle rebuilt_bundle;
    lxp_da_bundle changed_bundle;
    lxp_da_bundle zero_bundle;
    lxp_arena arena;
    lxp_arena rebuilt_arena;
    lxp_arena changed_arena;
    uint8_t root[32];
    uint8_t rebuilt_root[32];
    uint8_t changed_root[32];
    uint8_t zero_root[32];
    lxp_batch_roots roots;
    lxp_batch_seal_input seal_input;
    lxp_batch_header header;
    lxp_log log;
    char directory[] = "/tmp/lxp-da-bundle-XXXXXX";
    char path[128];
    uint32_t saved_index;
    uint64_t saved_batch;
    size_t i;

    if (lxp_arena_init(&arena, arena_storage, sizeof(arena_storage)) != LXP_OK ||
        lxp_arena_init(&rebuilt_arena, rebuilt_arena_storage,
                       sizeof(rebuilt_arena_storage)) != LXP_OK ||
        lxp_arena_init(&changed_arena, changed_arena_storage,
                       sizeof(changed_arena_storage)) != LXP_OK)
        return 1;
    (void)memset(module_roots, 0, sizeof(module_roots));
    module_roots[0].module_id = 3U;
    module_roots[0].state_root[0] = 51U;
    module_roots[1].module_id = 9U;
    module_roots[1].state_root[0] = 52U;
    recovery_input = (lxp_da_recovery_input){
        module_roots, 2U, {frontier, sizeof(frontier)}, 901U, 887U, 889U
    };
    if (lxp_da_recovery_metadata_encode(&recovery_input, &arena,
                                        &recovery_metadata) != LXP_OK)
        return 1;
    (void)memset(&body, 0, sizeof(body));
    body.header.batch_number = 17U;
    body.activities = (lxp_byte_span){activities, sizeof(activities)};
    body.receipts = (lxp_byte_span){receipts, sizeof(receipts)};
    body.oracle_inputs = (lxp_byte_span){oracle_inputs, sizeof(oracle_inputs)};
    body.state_diff = (lxp_byte_span){state_diff, sizeof(state_diff)};
    body.recovery_metadata = recovery_metadata;
    if (lxp_da_bundle_build(&body, 5U, &arena, &bundle) != LXP_OK ||
        bundle.batch_number != 17U ||
        bundle.total_bytes != sizeof(activities) + sizeof(receipts) +
            sizeof(oracle_inputs) + sizeof(state_diff) +
            recovery_metadata.length ||
        lxp_da_bundle_root(&bundle, &arena, root) != LXP_OK ||
        reconstruct(&bundle, rebuilt_classes, capacities,
                    rebuilt_lengths) != 0)
        return 1;
    if (rebuilt_lengths[0] != sizeof(activities) ||
        rebuilt_lengths[1] != sizeof(receipts) ||
        rebuilt_lengths[2] != sizeof(oracle_inputs) ||
        rebuilt_lengths[3] != sizeof(state_diff) ||
        rebuilt_lengths[4] != recovery_metadata.length ||
        memcmp(rebuilt_activities, activities, sizeof(activities)) != 0 ||
        memcmp(rebuilt_receipts, receipts, sizeof(receipts)) != 0 ||
        memcmp(rebuilt_oracles, oracle_inputs, sizeof(oracle_inputs)) != 0 ||
        memcmp(rebuilt_state_diff, state_diff, sizeof(state_diff)) != 0 ||
        memcmp(rebuilt_recovery, recovery_metadata.bytes,
               recovery_metadata.length) != 0)
        return 1;

    (void)memset(&rebuilt_body, 0, sizeof(rebuilt_body));
    rebuilt_body.header.batch_number = 17U;
    rebuilt_body.activities = (lxp_byte_span){rebuilt_activities,
                                              rebuilt_lengths[0]};
    rebuilt_body.receipts = (lxp_byte_span){rebuilt_receipts,
                                            rebuilt_lengths[1]};
    rebuilt_body.oracle_inputs = (lxp_byte_span){rebuilt_oracles,
                                                 rebuilt_lengths[2]};
    rebuilt_body.state_diff = (lxp_byte_span){rebuilt_state_diff,
                                              rebuilt_lengths[3]};
    rebuilt_body.recovery_metadata = (lxp_byte_span){rebuilt_recovery,
                                                     rebuilt_lengths[4]};
    if (lxp_da_bundle_build(&rebuilt_body, 5U, &rebuilt_arena,
                            &rebuilt_bundle) != LXP_OK ||
        lxp_da_bundle_root(&rebuilt_bundle, &rebuilt_arena,
                           rebuilt_root) != LXP_OK ||
        memcmp(root, rebuilt_root, sizeof(root)) != 0)
        return 1;

    rebuilt_activities[0] ^= 1U;
    if (lxp_da_bundle_build(&rebuilt_body, 5U, &changed_arena,
                            &changed_bundle) != LXP_OK ||
        lxp_da_bundle_root(&changed_bundle, &changed_arena,
                           changed_root) != LXP_OK ||
        memcmp(root, changed_root, sizeof(root)) == 0)
        return 1;
    rebuilt_activities[0] ^= 1U;
    zero_body = body;
    zero_body.header.batch_number = 0U;
    if (lxp_arena_reset(&changed_arena, 0U) != LXP_OK ||
        lxp_da_bundle_build(&zero_body, 5U, &changed_arena,
                            &zero_bundle) != LXP_OK ||
        zero_bundle.batch_number != 0U ||
        lxp_da_bundle_root(&zero_bundle, &changed_arena, zero_root) != LXP_OK)
        return 1;
    saved_batch = bundle.chunks[1].batch_number;
    bundle.chunks[1].batch_number = saved_batch + 1U;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_UNSORTED_SEQUENCE)
        return 1;
    bundle.chunks[1].batch_number = saved_batch;
    saved_index = bundle.chunks[1].chunk_index;
    bundle.chunks[1].chunk_index = 0U;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_UNSORTED_SEQUENCE)
        return 1;
    bundle.chunks[1].chunk_index = saved_index;
    bundle.total_bytes += 1U;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    bundle.total_bytes -= 1U;
    bundle.chunks[0].bytes.length += 1U;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    bundle.chunks[0].bytes.length -= 1U;
    bundle.chunks[1].class_offset += 1U;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    bundle.chunks[1].class_offset -= 1U;
    bundle.chunks[0].availability_class = LXP_DA_RECEIPTS;
    if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
        LXP_ERR_UNSORTED_SEQUENCE)
        return 1;
    bundle.chunks[0].availability_class = LXP_DA_ACTIVITIES;
    {
        size_t saved_chunk_count = bundle.chunk_count;
        size_t saved_total = bundle.total_bytes;
        size_t retained_total = 0U;
        size_t recovery_index = 0U;
        while (recovery_index < bundle.chunk_count &&
               bundle.chunks[recovery_index].availability_class !=
                   LXP_DA_RECOVERY_METADATA) {
            retained_total += bundle.chunks[recovery_index].bytes.length;
            ++recovery_index;
        }
        if (recovery_index == bundle.chunk_count) return 1;
        bundle.chunk_count = recovery_index;
        bundle.total_bytes = retained_total;
        if (lxp_da_bundle_root(&bundle, &arena, changed_root) !=
            LXP_ERR_NON_CANONICAL)
            return 1;
        bundle.chunk_count = saved_chunk_count;
        bundle.total_bytes = saved_total;
    }

    (void)memset(&roots, 0, sizeof(roots));
    (void)memcpy(roots.data_availability_root, root, sizeof(root));
    (void)memset(&seal_input, 0, sizeof(seal_input));
    seal_input.protocol_version = 1U;
    seal_input.network_id = 44U;
    seal_input.epoch = 3U;
    seal_input.batch_number = 17U;
    seal_input.first_sequence = 880U;
    seal_input.last_sequence = 900U;
    seal_input.timestamp_ms = 1700000000000U;
    seal_input.sequencer_id[0] = 61U;
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
        lxp_log_segment_create(&log, directory, 0U, 16384U) != LXP_OK ||
        lxp_batch_seal(&header, &seal_input, &roots, &log, &arena) != LXP_OK ||
        memcmp(header.data_availability_root, root, sizeof(root)) != 0 ||
        lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;

    for (i = 0U; i < sizeof(root); ++i)
        if (root[i] != 0U) return 0;
    return 1;
}
