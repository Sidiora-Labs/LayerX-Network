#include "layerx/lxp_state_diff.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_kernel.h"

#include <stdio.h>
#include <string.h>

#define REQUIRE(expression) do { \
    if (!(expression)) { \
        (void)fprintf(stderr, "line %d: %s\n", __LINE__, #expression); \
        return 1; \
    } \
} while (0)

int main(void)
{
    static lx_account_registry before, after;
    static uint8_t storage[1048576];
    static const uint8_t account_id[32] = {
        0xdb,0xf9,0x40,0xaa,0x4c,0x1f,0x58,0x7b,
        0x73,0xf3,0xb6,0x5d,0xa0,0xde,0xc9,0x27,
        0x60,0xe0,0xbd,0x01,0x87,0xf1,0xf3,0x3c,
        0x65,0x9e,0xa0,0x43,0xca,0xeb,0xdc,0xdf
    };
    uint8_t expected[158] = {0};
    uint8_t malformed[159];
    uint8_t root[32], independent[32];
    static uint8_t activity_bytes[65537];
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_state_diff_entry *entries;
    lxp_batch_body body = {0};
    lxp_da_bundle bundle;
    size_t count, i, mark;
    lxp_byte_span tagged, *decoded_receipts, *decoded_events;
    size_t receipt_count, event_count;
    static const uint8_t tagged_vector[] = {
        1U, 0U, 0U, 0U, 2U, 0x12U, 0x34U,
        2U, 0U, 0U, 0U, 1U, 0x56U
    };
    const lxp_byte_span receipt_items[] = {{tagged_vector + 5U, 2U}};
    const lxp_byte_span event_items[] = {{tagged_vector + 12U, 1U}};
    REQUIRE(lxp_arena_init(&arena, storage, sizeof(storage)) == LXP_OK);
    REQUIRE(lxp_da_receipt_section_encode(receipt_items, 1U, event_items, 1U,
                                          &arena, &tagged) == LXP_OK);
    REQUIRE(tagged.length == sizeof(tagged_vector));
    REQUIRE(memcmp(tagged.bytes, tagged_vector, sizeof(tagged_vector)) == 0);
    REQUIRE(lxp_da_receipt_section_decode(tagged, &arena, &decoded_receipts,
        &receipt_count, &decoded_events, &event_count) == LXP_OK);
    REQUIRE(receipt_count == 1U && event_count == 1U);
    REQUIRE(decoded_receipts[0].length == 2U && decoded_events[0].length == 1U);
    REQUIRE(memcmp(decoded_receipts[0].bytes, receipt_items[0].bytes, 2U) == 0);
    REQUIRE(decoded_events[0].bytes[0] == 0x56U);
    for (i = 1U; i < sizeof(tagged_vector); ++i) {
        if (i == 7U) continue;
        mark = lxp_arena_mark(&arena);
        REQUIRE(lxp_da_receipt_section_decode((lxp_byte_span){tagged_vector, i},
            &arena, &decoded_receipts, &receipt_count,
            &decoded_events, &event_count) != LXP_OK);
        REQUIRE(decoded_receipts == NULL && decoded_events == NULL);
        REQUIRE(receipt_count == 0U && event_count == 0U);
        REQUIRE(lxp_arena_mark(&arena) == mark);
    }
    (void)memcpy(malformed, tagged_vector, sizeof(tagged_vector));
    malformed[0] = 0U;
    REQUIRE(lxp_da_receipt_section_decode((lxp_byte_span){malformed, sizeof(tagged_vector)},
        &arena, &decoded_receipts, &receipt_count,
        &decoded_events, &event_count) == LXP_ERR_NON_CANONICAL);
    malformed[0] = 2U;
    malformed[7] = 1U;
    REQUIRE(lxp_da_receipt_section_decode((lxp_byte_span){malformed, sizeof(tagged_vector)},
        &arena, &decoded_receipts, &receipt_count,
        &decoded_events, &event_count) == LXP_ERR_NON_CANONICAL);
    REQUIRE(lx_account_registry_init(&before) == LXP_OK);
    REQUIRE(lx_account_registry_init(&after) == LXP_OK);
    after.count = 1U;
    (void)memcpy(after.accounts[0].id, account_id, 32U);
    (void)memcpy(after.accounts[0].name, "system:fees", 11U);
    after.accounts[0].name_length = 11U;
    after.accounts[0].kind = LX_ACCOUNT_SYSTEM_FEES;
    after.accounts[0].created_at_sequence = 7U;
    expected[3] = 1U;
    expected[7] = 32U;
    (void)memcpy(expected + 8U, account_id, 32U);
    expected[43] = 114U;
    expected[45] = 11U;
    (void)memcpy(expected + 46U, "system:fees", 11U);
    expected[57] = 10U;
    expected[122] = 7U;
    REQUIRE(lxp_state_diff_encode(&before, &after, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length == sizeof(expected));
    REQUIRE(memcmp(encoded.bytes, expected, sizeof(expected)) == 0);
    REQUIRE(lxp_state_diff_decode(encoded, &arena, &entries, &count) == LXP_OK);
    REQUIRE(count == 1U && memcmp(entries[0].account_id, account_id, 32U) == 0);
    REQUIRE(entries[0].leaf.length == 114U);
    for (i = 0U; i < sizeof(expected); ++i) {
        mark = lxp_arena_mark(&arena);
        REQUIRE(lxp_state_diff_decode((lxp_byte_span){expected, i},
                                      &arena, &entries, &count) != LXP_OK);
        REQUIRE(entries == NULL && count == 0U && lxp_arena_mark(&arena) == mark);
    }
    (void)memcpy(malformed, expected, sizeof(expected));
    malformed[158] = 0U;
    REQUIRE(lxp_state_diff_decode((lxp_byte_span){malformed, sizeof(malformed)},
                                  &arena, &entries, &count) != LXP_OK);
    malformed[157] = 2U;
    REQUIRE(lxp_state_diff_decode((lxp_byte_span){malformed, sizeof(expected)},
                                  &arena, &entries, &count) != LXP_OK);
    REQUIRE(lxp_state_diff_encode(&after, &after, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length == 4U && memcmp(encoded.bytes, "\0\0\0\0", 4U) == 0);
    REQUIRE(lxp_state_diff_encode(&after, &before, &arena, &encoded) == LXP_FATAL_REPLAY_DIVERGENCE);
    before.count = 1U;
    before.accounts[0] = after.accounts[0];
    after.accounts[0].next_sequence = 1U;
    REQUIRE(lxp_state_diff_encode(&before, &after, &arena, &encoded) == LXP_OK);
    REQUIRE(encoded.length == sizeof(expected));
    REQUIRE(lxp_state_diff_decode(encoded, &arena, &entries, &count) == LXP_OK);
    REQUIRE(count == 1U);
    body.header.batch_number = 9U;
    body.state_diff = encoded;
    mark = lxp_arena_mark(&arena);
    REQUIRE(lxp_batch_availability_root(&body, &arena, root) == LXP_OK);
    REQUIRE(lxp_arena_mark(&arena) == mark);
    REQUIRE(lxp_da_bundle_build(&body, 65536U, &arena, &bundle) == LXP_OK);
    REQUIRE(lxp_da_bundle_root(&bundle, &arena, independent) == LXP_OK);
    REQUIRE(memcmp(root, independent, 32U) == 0);
    ++body.header.batch_number;
    REQUIRE(lxp_batch_availability_root(&body, &arena, independent) == LXP_OK);
    REQUIRE(memcmp(root, independent, 32U) != 0);
    body.activities = (lxp_byte_span){activity_bytes, sizeof(activity_bytes)};
    REQUIRE(lxp_batch_availability_root(&body, &arena, root) == LXP_OK);
    REQUIRE(lxp_da_bundle_build(&body, 65536U, &arena, &bundle) == LXP_OK);
    REQUIRE(bundle.chunk_count == 6U);
    REQUIRE(bundle.chunks[0].length == 65536U && bundle.chunks[1].length == 1U);
    REQUIRE(bundle.chunks[1].class_offset == 65536U);
    REQUIRE(lxp_da_bundle_root(&bundle, &arena, independent) == LXP_OK);
    REQUIRE(memcmp(root, independent, 32U) == 0);
    activity_bytes[65536] = 1U;
    REQUIRE(lxp_batch_availability_root(&body, &arena, independent) == LXP_OK);
    REQUIRE(memcmp(root, independent, 32U) != 0);
    mark = lxp_arena_mark(&arena);
    body.activities = (lxp_byte_span){NULL, 1U};
    (void)memcpy(independent, root, 32U);
    REQUIRE(lxp_batch_availability_root(&body, &arena, independent) != LXP_OK);
    REQUIRE(memcmp(root, independent, 32U) == 0 && lxp_arena_mark(&arena) == mark);
    {
        static lxp_state_store state;
        static lxp_state_journal journal;
        static lxp_kernel kernel;
        uint64_t parameters = 1U;
        lxp_byte_span recovery;
        uint8_t *copy;
        void *memory;
        REQUIRE(lxp_state_store_init(&state, 9U) == LXP_OK);
        REQUIRE(lxp_state_store_bind_accounts(&state, &after) == LXP_OK);
        REQUIRE(lxp_kernel_create(&kernel, &state, &journal, &parameters, 1U) == LXP_OK);
        REQUIRE(lxp_da_recovery_from_kernel(&kernel, 8U, 8U, &arena, &recovery) == LXP_OK);
        mark = lxp_arena_mark(&arena);
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 8U, recovery, &arena) == LXP_OK);
        REQUIRE(lxp_arena_mark(&arena) == mark);
        REQUIRE(lxp_arena_alloc(&arena, recovery.length + 1U, 1U, &memory) == LXP_OK);
        copy = memory;
        (void)memcpy(copy, recovery.bytes, recovery.length);
        for (i = 0U; i < recovery.length; ++i) {
            copy[i] ^= 1U;
            mark = lxp_arena_mark(&arena);
            REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 8U,
                (lxp_byte_span){copy, recovery.length}, &arena) == LXP_FATAL_REPLAY_DIVERGENCE);
            REQUIRE(lxp_arena_mark(&arena) == mark);
            copy[i] ^= 1U;
            REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 8U,
                (lxp_byte_span){copy, i}, &arena) == LXP_FATAL_REPLAY_DIVERGENCE);
        }
        copy[recovery.length] = 0U;
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 8U,
            (lxp_byte_span){copy, recovery.length + 1U}, &arena) == LXP_FATAL_REPLAY_DIVERGENCE);
        ++after.accounts[0].next_sequence;
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 8U, recovery, &arena) == LXP_FATAL_REPLAY_DIVERGENCE);
        --after.accounts[0].next_sequence;
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 7U, recovery, &arena) == LXP_FATAL_REPLAY_DIVERGENCE);
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 9U, 8U, recovery, &arena) == LXP_ERR_NON_CANONICAL);
        REQUIRE(lxp_da_recovery_verify_kernel(&kernel, 8U, 9U, recovery, &arena) == LXP_ERR_NON_CANONICAL);
        REQUIRE(lxp_state_store_destroy(&state) == LXP_OK);
    }
    return 0;
}
