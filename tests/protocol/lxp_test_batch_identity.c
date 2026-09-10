#include "layerx/lxp_batch_identity.h"

#include "layerx/lxp_arena.h"
#include "layerx/lxp_codec.h"
#include "layerx/lxp_kernel.h"
#include "lxp_daemon_batch_wal.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static const uint8_t pinned_previous_state_root[32] = {
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11,
    0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x11
};
static const uint8_t pinned_activity_id[32] = {
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22,
    0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22, 0x22
};
static const uint8_t pinned_activity_merkle_root[32] = {
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33,
    0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33, 0x33
};
static const uint8_t pinned_activity_identity[32] = {
    0x7e, 0xa6, 0x30, 0x2b, 0x68, 0x1e, 0x3b, 0xac,
    0x3a, 0x66, 0x6c, 0xa0, 0xa6, 0xba, 0xe2, 0x43,
    0xe5, 0xbe, 0x6b, 0x2d, 0xe6, 0x3e, 0xaf, 0x8a,
    0xbb, 0xb0, 0x72, 0xa3, 0x9e, 0xb5, 0x2d, 0xac
};
static const uint8_t pinned_committed_identity[32] = {
    0x1a, 0x4e, 0xff, 0x4b, 0x89, 0xf4, 0xcb, 0x83,
    0x0e, 0x37, 0xa5, 0xa4, 0x15, 0x0b, 0x4a, 0x84,
    0x29, 0xdb, 0x11, 0x60, 0x41, 0x45, 0xa8, 0x40,
    0x28, 0xdc, 0xa1, 0xdd, 0xee, 0xf8, 0x49, 0xf3
};

enum {
    PINNED_GLOBAL_SEQUENCE = 7,
    PINNED_BATCH_NUMBER = 5,
    PINNED_FIRST_SEQUENCE = 7,
    PINNED_COMMITTED_LAST_SEQUENCE = 9
};

static int failures;

static void check(int condition, const char *name)
{
    if (!condition) {
        (void)fprintf(stderr, "lxp_test_batch_identity: %s\n", name);
        ++failures;
    }
}

static void pinned_preimages(void)
{
    uint8_t activity[LXP_BATCH_IDENTITY_ACTIVITY_PREIMAGE_SIZE];
    uint8_t committed[LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE];
    uint8_t identity[32];
    check(sizeof(activity) == 80U, "activity preimage size");
    check(sizeof(committed) == 88U, "committed preimage size");
    check(lxp_batch_identity_activity_preimage(
              pinned_previous_state_root, pinned_activity_id,
              PINNED_GLOBAL_SEQUENCE, PINNED_BATCH_NUMBER, activity) == LXP_OK,
          "activity preimage");
    check(memcmp(activity, pinned_previous_state_root, 32U) == 0 &&
              memcmp(activity + 32U, pinned_activity_id, 32U) == 0 &&
              activity[71] == PINNED_GLOBAL_SEQUENCE &&
              activity[79] == PINNED_BATCH_NUMBER,
          "activity preimage layout");
    check(lxp_batch_identity_committed_preimage(
              pinned_previous_state_root, pinned_activity_merkle_root,
              PINNED_FIRST_SEQUENCE, PINNED_COMMITTED_LAST_SEQUENCE,
              PINNED_BATCH_NUMBER, committed) == LXP_OK,
          "committed preimage");
    check(memcmp(committed, pinned_previous_state_root, 32U) == 0 &&
              memcmp(committed + 32U, pinned_activity_merkle_root, 32U) == 0 &&
              committed[71] == PINNED_FIRST_SEQUENCE &&
              committed[79] == PINNED_COMMITTED_LAST_SEQUENCE &&
              committed[87] == PINNED_BATCH_NUMBER,
          "committed preimage layout");
    check(lxp_batch_identity_activity(
              pinned_previous_state_root, pinned_activity_id,
              PINNED_GLOBAL_SEQUENCE, PINNED_BATCH_NUMBER, identity) == LXP_OK &&
              memcmp(identity, pinned_activity_identity, 32U) == 0,
          "pinned activity identity");
    check(lxp_batch_identity_committed(
              pinned_previous_state_root, pinned_activity_merkle_root,
              PINNED_FIRST_SEQUENCE, PINNED_COMMITTED_LAST_SEQUENCE,
              PINNED_BATCH_NUMBER, identity) == LXP_OK &&
              memcmp(identity, pinned_committed_identity, 32U) == 0,
          "pinned committed identity");
    check(memcmp(pinned_activity_identity, pinned_committed_identity, 32U) != 0,
          "the two identity forms stay distinct");
}

static void refused_inputs(void)
{
    uint8_t identity[32];
    uint8_t preimage[LXP_BATCH_IDENTITY_COMMITTED_PREIMAGE_SIZE];
    check(lxp_batch_identity_activity(NULL, pinned_activity_id, 1U, 1U,
                                      identity) == LXP_ERR_NON_CANONICAL,
          "activity identity refuses a missing state root");
    check(lxp_batch_identity_activity(pinned_previous_state_root, NULL, 1U, 1U,
                                      identity) == LXP_ERR_NON_CANONICAL,
          "activity identity refuses a missing activity");
    check(lxp_batch_identity_activity(pinned_previous_state_root,
                                      pinned_activity_id, 1U, 1U,
                                      NULL) == LXP_ERR_NON_CANONICAL,
          "activity identity refuses a missing output");
    check(lxp_batch_identity_committed(pinned_previous_state_root,
                                       pinned_activity_merkle_root, 0U, 4U, 1U,
                                       identity) == LXP_ERR_NON_CANONICAL,
          "committed identity refuses an unset first sequence");
    check(lxp_batch_identity_committed(pinned_previous_state_root,
                                       pinned_activity_merkle_root, 5U, 4U, 1U,
                                       identity) == LXP_ERR_NON_CANONICAL,
          "committed identity refuses an inverted range");
    check(lxp_batch_identity_committed_preimage(
              pinned_previous_state_root, NULL, 1U, 1U, 1U,
              preimage) == LXP_ERR_NON_CANONICAL,
          "committed preimage refuses a missing activity root");
}

static void committed_range(void)
{
    uint64_t last = 0U;
    check(lxp_batch_identity_committed_last_sequence(4U, 9U, false, &last) ==
              LXP_OK && last == 9U,
          "an unmaintained batch commits to its published last sequence");
    check(lxp_batch_identity_committed_last_sequence(4U, 9U, true, &last) ==
              LXP_OK && last == 8U,
          "a maintained batch excludes its maintenance sequence");
    check(lxp_batch_identity_committed_last_sequence(4U, 4U, false, &last) ==
              LXP_OK && last == 4U,
          "a single-activity batch commits to one sequence");
    check(lxp_batch_identity_committed_last_sequence(4U, 4U, true, &last) ==
              LXP_ERR_NON_CANONICAL,
          "a maintained batch must commit to at least one activity");
    check(lxp_batch_identity_committed_last_sequence(0U, 4U, false, &last) ==
              LXP_ERR_NON_CANONICAL,
          "the committed range refuses an unset first sequence");
    check(lxp_batch_identity_committed_last_sequence(5U, 4U, false, &last) ==
              LXP_ERR_NON_CANONICAL,
          "the committed range refuses an inverted range");
    check(lxp_batch_identity_committed_last_sequence(4U, 9U, false, NULL) ==
              LXP_ERR_NON_CANONICAL,
          "the committed range refuses a missing output");
}

static void daemon_binding_matches_the_shared_definition(void)
{
    static uint8_t arena_memory[1U << 20];
    static const uint8_t first_activity[6] = {0x01, 0x02, 0x03, 0x04, 0x05, 0x06};
    static const uint8_t second_activity[5] = {0x07, 0x08, 0x09, 0x0a, 0x0b};
    lxp_byte_span activities[2] = {
        {first_activity, sizeof(first_activity)},
        {second_activity, sizeof(second_activity)}
    };
    lxp_kernel_execution *executions = calloc(2U, sizeof(*executions));
    lxp_batch_roots roots;
    lxp_arena arena;
    uint8_t bound[32], expected[32];
    uint64_t committed_last = 0U;
    size_t index;
    if (executions == NULL) {
        check(0, "execution allocation");
        return;
    }
    (void)memset(&roots, 0, sizeof(roots));
    check(lxp_arena_init(&arena, arena_memory, sizeof(arena_memory)) == LXP_OK,
          "arena");
    check(lxp_daemon_batch_bind_prefix(activities, 2U,
                                       pinned_previous_state_root,
                                       PINNED_FIRST_SEQUENCE,
                                       PINNED_BATCH_NUMBER, &arena, executions,
                                       &roots, bound) == LXP_OK,
          "daemon batch binding");
    check(lxp_batch_identity_committed_last_sequence(
              PINNED_FIRST_SEQUENCE, PINNED_FIRST_SEQUENCE + 2U, true,
              &committed_last) == LXP_OK &&
              committed_last == PINNED_FIRST_SEQUENCE + 1U,
          "a maintained two-activity batch commits through its second activity");
    check(lxp_batch_identity_committed(pinned_previous_state_root,
                                       roots.activity_merkle_root,
                                       PINNED_FIRST_SEQUENCE, committed_last,
                                       PINNED_BATCH_NUMBER,
                                       expected) == LXP_OK &&
              memcmp(bound, expected, 32U) == 0,
          "the daemon binds the shared committed identity");
    for (index = 0U; index < 2U; ++index) {
        check(memcmp(executions[index].batch_id, expected, 32U) == 0,
              "every execution carries the batch identity");
        check(memcmp(executions[index].activity_root,
                     roots.activity_merkle_root, 32U) == 0,
              "every execution carries the committed activity root");
        check(executions[index].global_sequence ==
                  PINNED_FIRST_SEQUENCE + (uint64_t)index,
              "every execution carries its own sequence");
    }
    check(lxp_batch_identity_committed(pinned_previous_state_root,
                                       roots.activity_merkle_root,
                                       PINNED_FIRST_SEQUENCE,
                                       PINNED_FIRST_SEQUENCE + 2U,
                                       PINNED_BATCH_NUMBER,
                                       expected) == LXP_OK &&
              memcmp(bound, expected, 32U) != 0,
          "the maintenance sequence changes the committed identity");
    free(executions);
}

int main(void)
{
    pinned_preimages();
    refused_inputs();
    committed_range();
    daemon_binding_matches_the_shared_definition();
    if (failures != 0) {
        (void)fprintf(stderr, "lxp_test_batch_identity: %d checks failed\n",
                      failures);
        return 1;
    }
    (void)printf("lxp_test_batch_identity: ok\n");
    return 0;
}
