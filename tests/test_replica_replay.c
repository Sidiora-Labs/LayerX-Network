#include "support/lxp_real_replay.h"

#define CHECK(value) do { if (!(value)) { \
    (void)fprintf(stderr, "replay check failed at %d\n", __LINE__); return 1; \
} } while (0)

int main(void)
{
    static uint8_t history_storage[16U * 1024U * 1024U];
    static uint8_t replay_storage[16U * 1024U * 1024U];
    lxp_real_replay_fixture *builder = calloc(1U, sizeof(*builder));
    lxp_real_replay_fixture *verifier = calloc(1U, sizeof(*verifier));
    lxp_real_replay_fixture *snapshot = calloc(1U, sizeof(*snapshot));
    lxp_state_snapshot *captured = NULL;
    lxp_arena history_arena, replay_arena;
    lxp_byte_span activities[4];
    lxp_batch_body first, second, altered;
    lxp_replay_batch_result replayed_first, replayed_second, snapshot_second;
    uint8_t genesis[32], full_root[32];
    size_t i;
    CHECK(builder != NULL && verifier != NULL && snapshot != NULL);
    CHECK(lxp_real_replay_init(builder) == 0 && lxp_real_replay_init(verifier) == 0);
    CHECK(lxp_arena_init(&history_arena, history_storage, sizeof(history_storage)) == LXP_OK);
    CHECK(lxp_arena_init(&replay_arena, replay_storage, sizeof(replay_storage)) == LXP_OK);
    (void)memcpy(genesis, builder->kernel.current_state_root, 32U);
    for (i = 0U; i < 4U; ++i)
        CHECK(lxp_real_replay_activity(builder, i, &history_arena, &activities[i]) == 0);
    CHECK(lxp_real_replay_build(builder, 1U, activities, 2U, NULL, 0U,
                                &history_arena, &first) == 0);
    CHECK(lxp_state_snapshot_create(&builder->state, &captured) == LXP_OK);
    snapshot->kernel = builder->kernel;
    snapshot->kernel.state = lxp_state_snapshot_store_for_prepare(captured);
    snapshot->kernel.journal = &snapshot->journal;
    snapshot->parameters = builder->parameters;
    snapshot->kernel.parameter_set = &snapshot->parameters;
    snapshot->identities = builder->identities;
    snapshot->authority = builder->authority;
    snapshot->fees = builder->fees;
    snapshot->asset = builder->asset;
    snapshot->transfer_asset = builder->transfer_asset;
    snapshot->runtime = builder->runtime;
    snapshot->runtime.accounts = lxp_state_snapshot_accounts_for_prepare(captured);
    snapshot->runtime.assets = &snapshot->asset;
    snapshot->runtime.transfer_assets = &snapshot->transfer_asset;
    CHECK(lxp_kernel_bind_module_runtime(&snapshot->kernel, LXP_MODULE_ASSET,
                                         &snapshot->runtime) == LXP_OK);
    snapshot->execution = builder->execution;
    snapshot->execution.identities = &snapshot->identities;
    snapshot->execution.authority = &snapshot->authority;
    snapshot->execution.fee_parameters = &snapshot->fees;
    snapshot->execution.batch_number = 2U;
    CHECK(lxp_replay_engine_init(&snapshot->engine, lxp_real_replay_parameters, snapshot) == LXP_OK);
    CHECK(lxp_replay_engine_bind_kernel(&snapshot->engine, &snapshot->kernel) == LXP_OK);
    CHECK(lxp_replay_engine_register(&snapshot->engine, 1U, lxp_real_replay_transition) == LXP_OK);
    CHECK(lxp_real_replay_build(builder, 2U, activities + 2U, 2U, NULL, 0U,
                                &history_arena, &second) == 0);
    CHECK(lxp_replay_batch(&verifier->engine, &first, genesis, &replay_arena,
                           &replayed_first) == LXP_OK);
    CHECK(lxp_replay_verify_roots(&replayed_first, &first) == LXP_OK);
    verifier->execution.batch_number = 2U;
    CHECK(lxp_replay_batch(&verifier->engine, &second, replayed_first.resulting_state_root,
                           &replay_arena, &replayed_second) == LXP_OK);
    CHECK(lxp_replay_verify_roots(&replayed_second, &second) == LXP_OK);
    (void)memcpy(full_root, replayed_second.resulting_state_root, 32U);
    CHECK(lxp_arena_reset(&replay_arena, 0U) == LXP_OK);
    CHECK(lxp_replay_batch(&snapshot->engine, &second, first.header.resulting_state_root,
                           &replay_arena, &snapshot_second) == LXP_OK);
    CHECK(lxp_replay_verify_roots(&snapshot_second, &second) == LXP_OK);
    CHECK(memcmp(full_root, snapshot_second.resulting_state_root, 32U) == 0);
    altered = second;
    ((uint8_t *)altered.receipts.bytes)[altered.receipts.length - 1U] ^= 1U;
    CHECK(lxp_replay_verify_roots(&snapshot_second, &altered) == LXP_FATAL_REPLAY_DIVERGENCE);
    ((uint8_t *)altered.receipts.bytes)[altered.receipts.length - 1U] ^= 1U;
    CHECK(lxp_state_store_destroy(&builder->state) == LXP_OK);
    CHECK(lxp_state_store_destroy(&verifier->state) == LXP_OK);
    lxp_state_snapshot_destroy(captured);
    free(builder);
    free(verifier);
    free(snapshot);
    return 0;
}
