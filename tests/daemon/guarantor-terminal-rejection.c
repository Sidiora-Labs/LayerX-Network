#include "../../cmd/layerx-guarantor/runtime.c"
#define occupancy_parameters terminal_fixture_occupancy_parameters
#define LXP_TEST_TERMINAL_REJECTION_MAIN existing_terminal_rejection_main
int existing_terminal_rejection_main(void);
#include "../protocol/lxp_test_terminal_rejection.c"
#undef occupancy_parameters

static int sign_fee(lxp_activity *activity, uint64_t fee, uint8_t signature[64])
{
    uint8_t digest[32];
    activity->fee_limit = (lxp_u128){0U, fee};
    CHECK(lxp_activity_signing_preimage(activity, digest) == LXP_OK);
    CHECK(terminal_sign(terminal_actor_seed, digest, 32U, signature) == 0);
    activity->signature = (lxp_byte_span){signature, 64U};
    CHECK(lxp_activity_verify_signature(activity) == LXP_OK);
    return 0;
}

static int bound_execution(terminal_fixture *fixture, lxp_activity *activity,
    lxp_kernel_execution *execution)
{
    lxp_byte_span encoded;
    lxp_batch_roots roots;
    uint8_t batch_id[32];
    memset(execution, 0, sizeof(*execution));
    terminal_execution(fixture, execution, fixture->state.next_sequence, 1U);
    CHECK(lxp_activity_encode(activity, &fixture->arena, &encoded) == LXP_OK);
    CHECK(lxp_daemon_batch_bind_prefix(&encoded, 1U, fixture->kernel.current_state_root,
        fixture->state.next_sequence, 1U, &fixture->arena, execution, &roots, batch_id) == LXP_OK);
    return 0;
}

static int unchanged(const terminal_fixture *fixture, const uint8_t root[32], uint64_t sequence)
{
    CHECK(fixture->state.next_sequence == sequence);
    CHECK(memcmp(fixture->kernel.current_state_root, root, 32U) == 0);
    CHECK(!fixture->journal.open);
    CHECK(fixture->actor->balance.hi == 0U && fixture->actor->balance.lo == 1000U);
    CHECK(fixture->actor->next_sequence == 1U);
    CHECK(lxp_u128_is_zero(fixture->treasury->balance));
    CHECK(lxp_u128_is_zero(fixture->recipient->balance));
    return 0;
}

static int close_fixture(terminal_fixture *fixture)
{
    CHECK(lxp_history_close(&fixture->history) == LXP_OK);
    CHECK(lxp_log_close(&fixture->feed_log) == LXP_OK);
    CHECK(lxp_log_close(&fixture->canonical_log) == LXP_OK);
    CHECK(pthread_mutex_destroy(&fixture->feed_mutex) == 0);
    CHECK(lxp_state_store_destroy(&fixture->state) == LXP_OK);
    lx_account_registry_release(&fixture->accounts);
    free(fixture->storage);
    free(fixture);
    return 0;
}

static int replay_fee_refusal(void)
{
    terminal_fixture *live = malloc(sizeof(*live)), *replay = malloc(sizeof(*replay));
    lxp_activity activity, different;
    lxp_kernel_execution execution, replay_execution;
    lxp_receipt expected, actual, altered;
    lxp_byte_span encoded, reproduced;
    uint8_t payload[512], signature[64], different_signature[64], root[32];
    size_t payload_length = 0U;
    CHECK(live != NULL && replay != NULL);
    CHECK(terminal_fixture_open(live) == 0 && terminal_fixture_open(replay) == 0);
    CHECK(terminal_build_send(live, &activity, payload, &payload_length) == 0);
    activity.account_sequence = live->identity->next_sequence;
    CHECK(sign_fee(&activity, 1001U, signature) == 0);
    CHECK(bound_execution(live, &activity, &execution) == 0);
    memcpy(root, live->kernel.current_state_root, 32U);
    uint64_t sequence = live->state.next_sequence;
    lxp_result refused = lxp_kernel_execute_activity(&live->kernel, &activity, &execution, &actual);
    if (refused != LXP_ERR_FEE_UNPAYABLE) fprintf(stderr, "actual terminal admission result=%d\n", (int)refused);
    CHECK(refused == LXP_ERR_FEE_UNPAYABLE);
    CHECK(unchanged(live, root, sequence) == 0);
    CHECK(lxp_kernel_terminal_rejection(&live->kernel, &activity, &execution,
        LXP_ERR_FEE_UNPAYABLE, &expected) == LXP_OK);
    CHECK(expected.result_code == LXP_ERR_FEE_UNPAYABLE && expected.effects.count == 0U);
    CHECK(lxp_u128_is_zero(expected.fee_charged));
    CHECK(lxp_receipt_verify(&expected, live->authorization.public_key, &live->arena) == LXP_OK);
    CHECK(lxp_receipt_encode(&expected, true, &live->arena, &encoded) == LXP_OK);
    CHECK(bound_execution(replay, &activity, &replay_execution) == 0);
    replay_execution.sequencer_private_key = NULL;
    replay_execution.replay_public_key = replay->authorization.public_key;
    altered = expected;
    altered.result_code = LXP_ERR_FEE_LIMIT;
    replay_execution.replay_receipt = &altered;
    CHECK(replay_execute_terminal(&replay->kernel, &activity, &replay_execution, &actual) == LXP_ERR_FEE_UNPAYABLE);
    CHECK(unchanged(replay, root, sequence) == 0);
    altered = expected;
    altered.sequencer_signature[0] ^= 1U;
    CHECK(replay_execute_terminal(&replay->kernel, &activity, &replay_execution, &actual) != LXP_OK);
    CHECK(unchanged(replay, root, sequence) == 0);
    replay_execution.replay_receipt = &expected;
    different = activity;
    CHECK(sign_fee(&different, 1002U, different_signature) == 0);
    CHECK(replay_execute_terminal(&replay->kernel, &different, &replay_execution, &actual) != LXP_OK);
    CHECK(unchanged(replay, root, sequence) == 0);
    CHECK(replay_execute_terminal(&replay->kernel, &activity, &replay_execution, &actual) == LXP_OK);
    CHECK(lxp_receipt_encode(&actual, true, &replay->arena, &reproduced) == LXP_OK);
    CHECK(encoded.length == reproduced.length && memcmp(encoded.bytes, reproduced.bytes, encoded.length) == 0);
    CHECK(unchanged(replay, live->kernel.current_state_root, sequence + 1U) == 0);
    replay_execution.global_sequence = replay->state.next_sequence;
    CHECK(replay_execute_terminal(&replay->kernel, &activity, &replay_execution, &actual) == LXP_ERR_IDEMPOTENT_REPLAY);
    CHECK(unchanged(replay, live->kernel.current_state_root, sequence + 1U) == 0);
    CHECK(close_fixture(live) == 0 && close_fixture(replay) == 0);
    return 0;
}

int main(void)
{
    CHECK(existing_terminal_rejection_main() == 0);
    CHECK(replay_fee_refusal() == 0);
    puts("guarantor terminal rejection replay passed");
    return 0;
}
