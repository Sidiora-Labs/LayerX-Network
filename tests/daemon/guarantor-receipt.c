#include "../../cmd/layerx-guarantor/runtime.c"
#include <assert.h>

static int compare_executed_program_receipt(const lxp_receipt *source);
#define LXP_TEST_PROGRAM_ARTIFACT_OBSERVER compare_executed_program_receipt
#define occupancy_parameters receipt_fixture_occupancy_parameters
#define main guarantor_receipt_fixture_main
int guarantor_receipt_fixture_main(int argc, char **argv);
#include "../programs/test_call_activity.c"
#undef main
#undef occupancy_parameters
#undef LXP_TEST_PROGRAM_ARTIFACT_OBSERVER

static unsigned compared_receipts;

static int compare_executed_program_receipt(const lxp_receipt *source)
{
    gp_runtime *runtime = calloc(1U, sizeof(*runtime));
    uint8_t *bytes = malloc(3U * LXP_MAX_ACTIVITY_BYTES);
    lxp_receipt different = *source, signed_receipt = *source;
    uint8_t public_key[32];
    uint8_t *canonical_bytes = malloc(LXP_MAX_ACTIVITY_BYTES);
    lxp_arena canonical_arena;
    lxp_byte_span canonical;
    lxp_verified_receipt_index *index = malloc(sizeof(*index));
    void *occupied = NULL;
    assert(runtime != NULL && bytes != NULL && canonical_bytes != NULL && index != NULL);
    assert(lxp_arena_init(&canonical_arena, canonical_bytes, LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    runtime->expected = *source;
    assert(lxp_arena_init(&runtime->execution_arena, bytes,
        3U * LXP_MAX_ACTIVITY_BYTES) == LXP_OK);
    assert(lxp_arena_alloc(&runtime->execution_arena,
        LXP_MAX_ACTIVITY_BYTES + 1U, 1U, &occupied) == LXP_OK);
    size_t mark = lxp_arena_mark(&runtime->execution_arena);
    assert(compare_unsigned(runtime, source) == LXP_OK);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    ++different.parameter_version;
    assert(compare_unsigned(runtime, &different) == LXP_FATAL_REPLAY_DIVERGENCE);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    assert(executed_public_key(executed_sequencer_seed, public_key) == 0);
    assert(lxp_receipt_sign(&signed_receipt, executed_sequencer_seed,
        &canonical_arena) == LXP_OK);
    assert(lxp_receipt_encode(&signed_receipt, true, &canonical_arena, &canonical) == LXP_OK);
    assert(compare_signed(&signed_receipt, canonical, &runtime->execution_arena) == LXP_OK);
    assert(lxp_verified_receipt_index_init(index) == LXP_OK);
    assert(lxp_verified_receipt_index_add(index, &signed_receipt, public_key,
        &runtime->execution_arena) == LXP_OK);
    assert(index->count == 1U && index->entries[0].global_sequence == source->global_sequence);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    different = signed_receipt;
    ++different.parameter_version;
    assert(compare_signed(&different, canonical,
        &runtime->execution_arena) == LXP_FATAL_REPLAY_DIVERGENCE);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    assert(compare_signed(&signed_receipt,
        (lxp_byte_span){canonical.bytes, canonical.length - 1U},
        &runtime->execution_arena) == LXP_FATAL_REPLAY_DIVERGENCE);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    assert(lxp_arena_alloc(&runtime->execution_arena,
        LXP_MAX_ACTIVITY_BYTES, 1U, &occupied) == LXP_OK);
    mark = lxp_arena_mark(&runtime->execution_arena);
    assert(compare_unsigned(runtime, source) == LXP_ERR_ARENA_EXHAUSTED);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    assert(compare_signed(&signed_receipt, canonical,
        &runtime->execution_arena) == LXP_ERR_ARENA_EXHAUSTED);
    assert(lxp_arena_mark(&runtime->execution_arena) == mark);
    free(index);
    free(canonical_bytes);
    ++compared_receipts;
    free(bytes);
    free(runtime);
    return 0;
}

int main(void)
{
    assert(deploy_and_upgrade_persist_exact_artifacts_version(
        LXP_PROTOCOL_VERSION_STATE_COMMITMENT) == 0);
    assert(compared_receipts != 0U);
    return 0;
}
