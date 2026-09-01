#define _POSIX_C_SOURCE 200809L

#include "layerx/programs.h"

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_storage.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

static uint8_t nibble(uint8_t value)
{
    return value <= (uint8_t)'9' ? (uint8_t)(value - (uint8_t)'0') :
                                   (uint8_t)(value - (uint8_t)'a' + 10U);
}

static void decode_hex(const char *hex, uint8_t bytes[32])
{
    size_t i;
    for (i = 0U; i < 32U; ++i)
        bytes[i] = (uint8_t)((nibble((uint8_t)hex[i * 2U]) << 4U) |
                             nibble((uint8_t)hex[i * 2U + 1U]));
}

static int vector_bytes(const char *name, uint8_t *bytes, size_t length)
{
    static const char path[] =
        "tests/vectors/program_account_state_v2.vec";
    char line[1024];
    size_t name_length = strlen(name);
    FILE *input = fopen(path, "rb");
    if (input == NULL) return 1;
    while (fgets(line, sizeof(line), input) != NULL) {
        size_t line_length = strlen(line);
        size_t i;
        while (line_length != 0U &&
               (line[line_length - 1U] == '\n' ||
                line[line_length - 1U] == '\r'))
            line[--line_length] = '\0';
        if (line_length != name_length + 1U + length * 2U ||
            memcmp(line, name, name_length) != 0 ||
            line[name_length] != '=')
            continue;
        for (i = 0U; i < length; ++i) {
            uint8_t high = (uint8_t)line[name_length + 1U + i * 2U];
            uint8_t low = (uint8_t)line[name_length + 2U + i * 2U];
            if (!((high >= (uint8_t)'0' && high <= (uint8_t)'9') ||
                  (high >= (uint8_t)'a' && high <= (uint8_t)'f')) ||
                !((low >= (uint8_t)'0' && low <= (uint8_t)'9') ||
                  (low >= (uint8_t)'a' && low <= (uint8_t)'f'))) {
                (void)fclose(input);
                return 1;
            }
            bytes[i] = (uint8_t)((nibble(high) << 4U) | nibble(low));
        }
        return fclose(input) == 0 ? 0 : 1;
    }
    (void)fclose(input);
    return 1;
}

static int vector_u32(const char *name, uint32_t *value)
{
    static const char path[] =
        "tests/vectors/program_account_state_v2.vec";
    char line[128];
    size_t name_length = strlen(name);
    FILE *input = fopen(path, "rb");
    if (input == NULL || value == NULL) return 1;
    while (fgets(line, sizeof(line), input) != NULL) {
        char *end;
        unsigned long parsed;
        if (memcmp(line, name, name_length) != 0 || line[name_length] != '=')
            continue;
        parsed = strtoul(line + name_length + 1U, &end, 10);
        if ((*end != '\n' && *end != '\r' && *end != '\0') ||
            parsed > UINT32_MAX) {
            (void)fclose(input);
            return 1;
        }
        *value = (uint32_t)parsed;
        return fclose(input) == 0 ? 0 : 1;
    }
    (void)fclose(input);
    return 1;
}

static lxp_result vector_leaf(const uint8_t *key, size_t key_length,
                              const uint8_t *value, size_t value_length,
                              uint8_t digest[32])
{
    uint8_t preimage[512];
    size_t offset = 8U;
    size_t i;
    if (key == NULL || value == NULL || digest == NULL ||
        key_length > UINT32_MAX || value_length > UINT32_MAX ||
        key_length + value_length > sizeof(preimage) - 8U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < 4U; ++i) {
        preimage[i] = (uint8_t)(key_length >> (24U - i * 8U));
        preimage[4U + i] = (uint8_t)(value_length >> (24U - i * 8U));
    }
    (void)memcpy(preimage + offset, key, key_length);
    offset += key_length;
    (void)memcpy(preimage + offset, value, value_length);
    offset += value_length;
    return lxp_hash_domain(LXP_DOMAIN_STATE_LEAF, preimage, offset, digest);
}

static lxp_result vector_node(const uint8_t left[32], const uint8_t right[32],
                              uint8_t digest[32])
{
    uint8_t pair[64];
    (void)memcpy(pair, left, 32U);
    (void)memcpy(pair + 32U, right, 32U);
    return lxp_hash_domain(LXP_DOMAIN_STATE_NODE, pair, sizeof(pair), digest);
}

static void vector_write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void vector_write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static void feed_write_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static int close_feed_log(lxp_log *log, const char *directory)
{
    char path[160];
    int failed = 0;
    if (log == NULL || directory == NULL ||
        snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
        return 1;
    if (log->descriptor >= 0 && lxp_log_close(log) != LXP_OK) failed = 1;
    if (unlink(path) != 0) failed = 1;
    if (rmdir(directory) != 0) failed = 1;
    return failed;
}

static int feed_group_pairing_replay(void)
{
    enum {
        BASELINE_BYTES = 42,
        NOTICE_BYTES = 84,
        HEAD_BYTES = 114
    };
    char feed_directory[] = "/tmp/lxp-feed-pair-XXXXXX";
    char canonical_directory[] = "/tmp/lxp-feed-canonical-XXXXXX";
    static uint8_t scratch_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t baseline[BASELINE_BYTES] = {0};
    uint8_t notice[NOTICE_BYTES] = {0};
    uint8_t head[HEAD_BYTES] = {0};
    uint8_t baseline_root[32] = {0x11U};
    lx_programs_state_notice exposed[2];
    lx_programs_state_feed_store store;
    lxp_history history;
    lxp_arena scratch;
    lxp_log feed_log = {.descriptor = -1};
    lxp_log canonical_log = {.descriptor = -1};
    pthread_mutex_t mutex;
    size_t exposed_count = SIZE_MAX;
    uint64_t complete_through = UINT64_MAX;
    uint64_t scanned_through = UINT64_MAX;
    lxp_result open_status;
    lxp_result page_status;
    int failed = 0;

    baseline[0] = 1U;
    baseline[1] = 3U;
    vector_write_u64(baseline + 2U, 1U);
    (void)memcpy(baseline + 10U, baseline_root, 32U);
    notice[0] = 1U;
    notice[1] = 1U;
    vector_write_u64(notice + 2U, 1U);
    feed_write_u32(notice + 10U, 0U);
    (void)memset(notice + 14U, 0x22, 32U);
    feed_write_u32(notice + 46U, LX_PROGRAMS_ACCOUNT);
    notice[50U] = (uint8_t)(LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED >> 8U);
    notice[51U] = (uint8_t)LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED;
    (void)memset(notice + 52U, 0x33, 32U);
    head[0] = 1U;
    head[1] = 2U;
    vector_write_u64(head + 2U, 1U);
    (void)memset(head + 10U, 0x44, 32U);
    (void)memset(head + 42U, 0x55, 32U);
    vector_write_u64(head + 74U, 7U);
    (void)memset(head + 82U, 0x66, 32U);

    (void)memset(&history, 0, sizeof(history));
    if (mkdtemp(feed_directory) == NULL ||
        mkdtemp(canonical_directory) == NULL ||
        lxp_log_segment_create(&feed_log, feed_directory, 0U, 4096U) !=
            LXP_OK ||
        lxp_log_segment_create(&canonical_log, canonical_directory, 0U,
                               4096U) != LXP_OK ||
        lxp_arena_init(&scratch, scratch_bytes, sizeof(scratch_bytes)) !=
            LXP_OK ||
        pthread_mutex_init(&mutex, NULL) != 0) {
        failed = 1;
        goto cleanup;
    }
    history.log = &canonical_log;
    if (lxp_log_append(&feed_log, LXP_LOG_STATE_DIFF, 1U, baseline,
                       sizeof(baseline), NULL) != LXP_OK ||
        lxp_log_append(&feed_log, LXP_LOG_STATE_DIFF, 1U, notice,
                       sizeof(notice), NULL) != LXP_OK ||
        lxp_log_append(&feed_log, LXP_LOG_STATE_DIFF, 1U, head,
                       sizeof(head), NULL) != LXP_OK ||
        lxp_log_write_boundary(&feed_log) != LXP_OK) {
        failed = 1;
        goto cleanup_mutex;
    }
    open_status = lxp_programs_state_feed_store_open(
        &store, &feed_log, &canonical_log, &history, &scratch, &mutex);
    page_status = lxp_programs_state_feed_store_page(
        &store, 0U, 2U, exposed, &exposed_count, &complete_through,
        &scanned_through);
    if (open_status != LXP_ERR_LOG_CORRUPT ||
        page_status != LXP_ERR_LOG_CORRUPT || exposed_count != SIZE_MAX ||
        complete_through != UINT64_MAX || scanned_through != UINT64_MAX)
        failed = 1;
cleanup_mutex:
    if (pthread_mutex_destroy(&mutex) != 0) failed = 1;
cleanup:
    if (feed_log.descriptor >= 0 &&
        close_feed_log(&feed_log, feed_directory) != 0)
        failed = 1;
    if (canonical_log.descriptor >= 0 &&
        close_feed_log(&canonical_log, canonical_directory) != 0)
        failed = 1;
    return failed;
}

static int feed_runtime_bindings(void)
{
    char feed_directory[] = "/tmp/lxp-feed-runtime-XXXXXX";
    char canonical_directory[] = "/tmp/lxp-feed-runtime-canonical-XXXXXX";
    static uint8_t scratch_bytes[LXP_MAX_ACTIVITY_BYTES];
    static const uint8_t actor[] = "did:lx:feed";
    static const uint8_t authority[] = {1U, 2U};
    static const uint8_t payload[] = {3U, 4U};
    static const uint8_t signature[64] = {5U};
    uint8_t baseline_root[32] = {0x11U};
    uint8_t resulting_root[32] = {0x22U};
    uint8_t activity_root[32] = {0x33U};
    uint8_t batch_id[32] = {0x44U};
    uint8_t activity_id[32];
    uint8_t program_id[32] = {0x55U};
    uint8_t expected_digest[32];
    lx_programs_state_notice notices[2];
    lx_programs_state_feed_store store;
    lxp_effect_buffer effects;
    lxp_activity activity;
    lxp_activity wrong_activity;
    lxp_receipt receipt;
    lxp_receipt wrong_receipt;
    lxp_byte_span encoded;
    lxp_history history;
    lxp_arena scratch;
    lxp_log feed_log = {.descriptor = -1};
    lxp_log canonical_log = {.descriptor = -1};
    lxp_log_record_header head_header;
    pthread_mutex_t mutex;
    uint8_t head_body[114];
    uint64_t notice_boundary;
    uint64_t complete_through;
    uint64_t scanned_through;
    size_t notice_count;
    size_t mark;
    int failed = 0;

    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&receipt, 0, sizeof(receipt));
    (void)memset(&history, 0, sizeof(history));
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = 1U;
    activity.activity_type = LX_PROGRAMS_ACCOUNT;
    activity.actor_did = (lxp_byte_span){actor, sizeof(actor) - 1U};
    activity.authority = (lxp_byte_span){authority, sizeof(authority)};
    activity.account_sequence = 1U;
    activity.timestamp_bound.not_before = 1U;
    activity.timestamp_bound.not_after = 2U;
    activity.idempotency_key[0] = 1U;
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};

    if (mkdtemp(feed_directory) == NULL ||
        mkdtemp(canonical_directory) == NULL ||
        lxp_log_segment_create(&feed_log, feed_directory, 0U, 16384U) !=
            LXP_OK ||
        lxp_log_segment_create(&canonical_log, canonical_directory, 0U,
                               16384U) != LXP_OK ||
        lxp_arena_init(&scratch, scratch_bytes, sizeof(scratch_bytes)) !=
            LXP_OK ||
        pthread_mutex_init(&mutex, NULL) != 0) {
        failed = 1;
        goto cleanup;
    }
    history.log = &canonical_log;
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) !=
            LXP_OK ||
        lxp_activity_encode(&activity, &scratch, &encoded) != LXP_OK ||
        lxp_activity_id(encoded.bytes, encoded.length, activity_id) != LXP_OK ||
        lxp_arena_reset(&scratch, 0U) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(&receipt, activity_id, 1U, baseline_root,
                          resulting_root, activity_root, LXP_OK, &effects,
                          (lxp_u128){0U, 0U}, batch_id, LXP_MODULE_PROGRAMS,
                          1U, 1U) != LXP_OK) {
        failed = 1;
        goto cleanup_mutex;
    }
    receipt.timestamp = 7U;
    receipt.sequencer_signature[0] = 1U;
    wrong_receipt = receipt;
    wrong_receipt.timestamp = receipt.timestamp + 1U;
    wrong_activity = activity;
    ++wrong_activity.account_sequence;
    if (lxp_programs_state_feed_store_open(
            &store, &feed_log, &canonical_log, &history, &scratch, &mutex) !=
            LXP_OK ||
        lxp_programs_state_feed_store_anchor(
            &store, 1U, baseline_root) != LXP_OK ||
        store.feed.lock(store.feed.context) != LXP_OK) {
        failed = 1;
        goto cleanup_mutex;
    }
    if (store.feed.advance(store.feed.context, &wrong_activity, &receipt) !=
            LXP_FATAL_INVARIANT ||
        store.feed.append(store.feed.context, 1U, 0U, program_id,
                          LX_PROGRAMS_ACCOUNT,
                          LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED,
                          &receipt) != LXP_OK) {
        failed = 1;
        goto unlock;
    }
    notice_boundary = feed_log.write_offset;
    if (store.feed.append(store.feed.context, 1U, 1U, program_id,
                          LX_PROGRAMS_ACCOUNT,
                          LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED,
                          &wrong_receipt) != LXP_FATAL_INVARIANT ||
        feed_log.write_offset != notice_boundary ||
        store.feed.append(store.feed.context, 1U, 2U, program_id,
                          LX_PROGRAMS_ACCOUNT,
                          LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED,
                          &receipt) != LXP_ERR_UNSORTED_SEQUENCE ||
        feed_log.write_offset != notice_boundary ||
        store.feed.advance(store.feed.context, &activity, &wrong_receipt) !=
            LXP_FATAL_INVARIANT ||
        feed_log.write_offset != notice_boundary ||
        store.feed.advance(store.feed.context, &activity, &receipt) != LXP_OK ||
        lxp_log_read(&feed_log, notice_boundary, &head_header, head_body,
                     sizeof(head_body)) != LXP_OK ||
        head_header.body_length != sizeof(head_body) || head_body[1] != 2U ||
        memcmp(head_body + 82U, activity_id, 32U) != 0) {
        failed = 1;
        goto unlock;
    }
unlock:
    if (store.feed.unlock(store.feed.context) != LXP_OK) failed = 1;
    if (!failed) {
        mark = lxp_arena_mark(&scratch);
        if (lxp_receipt_digest(&receipt, &scratch, expected_digest) != LXP_OK)
            failed = 1;
        (void)lxp_arena_reset(&scratch, mark);
    }
    if (!failed &&
        (lxp_programs_state_feed_store_page(
             &store, 0U, 2U, notices, &notice_count, &complete_through,
             &scanned_through) != LXP_OK ||
         notice_count != 1U || complete_through != 1U ||
         scanned_through != 1U || notices[0].ordinal != 0U ||
         memcmp(notices[0].receipt_digest, expected_digest, 32U) != 0))
        failed = 1;
cleanup_mutex:
    if (pthread_mutex_destroy(&mutex) != 0) failed = 1;
cleanup:
    if (feed_log.descriptor >= 0 &&
        close_feed_log(&feed_log, feed_directory) != 0)
        failed = 1;
    if (canonical_log.descriptor >= 0 &&
        close_feed_log(&canonical_log, canonical_directory) != 0)
        failed = 1;
    return failed;
}

static int shared_registration_record(const char *prefix, size_t seed_length)
{
    static const uint8_t primary_prefix[] = "program-account\0p";
    uint8_t program[32], seed[LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    uint8_t asset[32], account[32], expected[512], seed_hash[32];
    uint8_t event[143], event_digest[32];
    uint8_t key[sizeof(primary_prefix) - 1U + 64U];
    uint8_t value[139U + LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    uint8_t commitment[32];
    char name[96];
    uint32_t sequence;
    size_t offset;
    if (prefix == NULL || seed_length > sizeof(seed)) return 1;
    if (snprintf(name, sizeof(name), "%s_program_id", prefix) < 0 ||
        vector_bytes(name, program, sizeof(program)) != 0 ||
        snprintf(name, sizeof(name), "%s_seed", prefix) < 0 ||
        vector_bytes(name, seed, seed_length) != 0 ||
        snprintf(name, sizeof(name), "%s_asset_id", prefix) < 0 ||
        vector_bytes(name, asset, sizeof(asset)) != 0 ||
        snprintf(name, sizeof(name), "%s_registered_sequence", prefix) < 0 ||
        vector_u32(name, &sequence) != 0 || sequence == 0U ||
        lxp_programs_account_derive(program, seed, seed_length, account) !=
            LXP_OK ||
        snprintf(name, sizeof(name), "%s_account_id", prefix) < 0 ||
        vector_bytes(name, expected, 32U) != 0 ||
        memcmp(account, expected, 32U) != 0 ||
        lxp_hash_sha256(seed, seed_length, seed_hash) != LXP_OK)
        return 1;

    offset = 0U;
    (void)memcpy(event + offset, "LXPA1", 5U); offset += 5U;
    (void)memcpy(event + offset, program, 32U); offset += 32U;
    (void)memcpy(event + offset, account, 32U); offset += 32U;
    (void)memcpy(event + offset, asset, 32U); offset += 32U;
    vector_write_u16(event + offset, (uint16_t)seed_length); offset += 2U;
    (void)memcpy(event + offset, seed_hash, 32U); offset += 32U;
    vector_write_u64(event + offset, sequence); offset += 8U;
    if (offset != sizeof(event) ||
        snprintf(name, sizeof(name), "%s_event", prefix) < 0 ||
        vector_bytes(name, expected, sizeof(event)) != 0 ||
        memcmp(event, expected, sizeof(event)) != 0 ||
        lxp_hash_sha256(event, sizeof(event), event_digest) != LXP_OK ||
        snprintf(name, sizeof(name), "%s_event_digest", prefix) < 0 ||
        vector_bytes(name, expected, 32U) != 0 ||
        memcmp(event_digest, expected, 32U) != 0)
        return 1;

    (void)memcpy(key, primary_prefix, sizeof(primary_prefix) - 1U);
    (void)memcpy(key + sizeof(primary_prefix) - 1U, program, 32U);
    (void)memcpy(key + sizeof(primary_prefix) - 1U + 32U,
                 seed_hash, 32U);
    if (snprintf(name, sizeof(name), "%s_primary_key", prefix) < 0 ||
        vector_bytes(name, expected, sizeof(key)) != 0 ||
        memcmp(key, expected, sizeof(key)) != 0)
        return 1;

    offset = 0U;
    value[offset++] = 2U;
    (void)memcpy(value + offset, program, 32U); offset += 32U;
    (void)memcpy(value + offset, account, 32U); offset += 32U;
    (void)memcpy(value + offset, asset, 32U); offset += 32U;
    vector_write_u16(value + offset, (uint16_t)seed_length); offset += 2U;
    vector_write_u64(value + offset, sequence); offset += 8U;
    (void)memcpy(value + offset, event_digest, 32U); offset += 32U;
    (void)memcpy(value + offset, seed, seed_length); offset += seed_length;
    if (snprintf(name, sizeof(name), "%s_primary_value", prefix) < 0 ||
        vector_bytes(name, expected, offset) != 0 ||
        memcmp(value, expected, offset) != 0 ||
        vector_leaf(key, sizeof(key), value, offset, commitment) != LXP_OK ||
        snprintf(name, sizeof(name), "%s_primary_commitment", prefix) < 0 ||
        vector_bytes(name, expected, 32U) != 0 ||
        memcmp(commitment, expected, 32U) != 0)
        return 1;
    return 0;
}

static int shared_registration_boundaries(void)
{
    uint8_t order_a[81], order_b[81], ordered_seed[1], seed_b[1];
    uint32_t version, max_seed, refused_seed, max_accounts, refused_accounts;
    uint32_t leaf_count, leaf_index, zero_leaf_count;
    if (vector_u32("version", &version) != 0 || version != 2U ||
        shared_registration_record("registration", 5U) != 0 ||
        shared_registration_record("empty", 0U) != 0 ||
        shared_registration_record("maximum",
            LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES) != 0 ||
        vector_bytes("order_a_primary_key", order_a, sizeof(order_a)) != 0 ||
        vector_bytes("order_b_primary_key", order_b, sizeof(order_b)) != 0 ||
        memcmp(order_b, order_a, sizeof(order_a)) >= 0 ||
        vector_bytes("ordered_first_seed", ordered_seed, 1U) != 0 ||
        vector_bytes("order_b_seed", seed_b, 1U) != 0 ||
        memcmp(ordered_seed, seed_b, 1U) != 0 ||
        vector_u32("max_seed_length", &max_seed) != 0 ||
        vector_u32("refused_seed_length", &refused_seed) != 0 ||
        max_seed != LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES ||
        refused_seed != LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES + 1U ||
        vector_u32("max_program_accounts", &max_accounts) != 0 ||
        vector_u32("refused_program_accounts", &refused_accounts) != 0 ||
        max_accounts != LX_ACCOUNT_REGISTRY_CAPACITY ||
        refused_accounts != LX_ACCOUNT_REGISTRY_CAPACITY + 1U ||
        vector_u32("malformed_proof_leaf_count", &leaf_count) != 0 ||
        vector_u32("malformed_proof_leaf_index", &leaf_index) != 0 ||
        vector_u32("zero_proof_leaf_count", &zero_leaf_count) != 0 ||
        leaf_count == 0U || leaf_index < leaf_count || zero_leaf_count != 0U)
        return 1;
    return 0;
}

static int shared_state_vectors(void)
{
    static const uint8_t module_name[] = "programs";
    const uint8_t keys[3] = {0U, 1U, 2U};
    const uint8_t values[3] = {0x10U, 0x20U, 0x30U};
    const char *leaf_names[3] = {"leaf0", "leaf1", "leaf2"};
    lx_account_registry registry;
    lx_account_registration registration;
    lx_account *account;
    uint8_t account_id[32], asset_id[32], expected[32], actual[32];
    uint8_t leaves[3][32], node01[32], node22[32], root[32];
    uint8_t outer0[32], outer9[32], programs_root[32], module_key[2];
    bool created;
    uint32_t leaf_count, leaf_index, proof_depth, max_depth, refused_depth;
    size_t i;
    if (shared_registration_boundaries() != 0 ||
        vector_bytes("account_id", account_id, 32U) != 0 ||
        vector_bytes("asset_id", asset_id, 32U) != 0 ||
        lx_account_registry_init(&registry) != LXP_OK ||
        lx_account_module_value_prepare(
            &registry, module_name, sizeof(module_name) - 1U, account_id,
            asset_id, 3U, &registration, &account, &created) != LXP_OK ||
        !created)
        return 1;
    account->balance = (lxp_u128){0U, 0x123456U};
    account->next_sequence = 7U;
    account->has_open_reference = true;
    if (lx_account_registration_commit(&registry, &registration, &account) !=
            LXP_OK ||
        lx_account_registry_root(&registry, actual) != LXP_OK ||
        vector_bytes("account_leaf", expected, 32U) != 0 ||
        memcmp(actual, expected, 32U) != 0)
        return 1;
    for (i = 0U; i < 3U; ++i)
        if (vector_leaf(&keys[i], 1U, &values[i], 1U, leaves[i]) != LXP_OK ||
            vector_bytes(leaf_names[i], expected, 32U) != 0 ||
            memcmp(leaves[i], expected, 32U) != 0)
            return 1;
    if (vector_node(leaves[0], leaves[1], node01) != LXP_OK ||
        vector_bytes("node01", expected, 32U) != 0 ||
        memcmp(node01, expected, 32U) != 0 ||
        vector_node(leaves[2], leaves[2], node22) != LXP_OK ||
        vector_bytes("node22", expected, 32U) != 0 ||
        memcmp(node22, expected, 32U) != 0 ||
        vector_node(node01, node22, root) != LXP_OK ||
        vector_bytes("tree_root", expected, 32U) != 0 ||
        memcmp(root, expected, 32U) != 0 ||
        vector_bytes("proof2_sibling0", expected, 32U) != 0 ||
        memcmp(leaves[2], expected, 32U) != 0 ||
        vector_bytes("proof2_sibling1", expected, 32U) != 0 ||
        memcmp(node01, expected, 32U) != 0 ||
        vector_u32("proof_leaf_count", &leaf_count) != 0 ||
        vector_u32("proof_leaf_index", &leaf_index) != 0 ||
        vector_u32("proof_depth", &proof_depth) != 0 ||
        leaf_count != 3U || leaf_index != 2U || proof_depth != 2U)
        return 1;
    module_key[0] = 0U; module_key[1] = 0U;
    if (vector_leaf(module_key, sizeof(module_key), root, 32U, outer0) !=
            LXP_OK ||
        vector_bytes("outer0_leaf", expected, 32U) != 0 ||
        memcmp(outer0, expected, 32U) != 0 ||
        vector_bytes("programs_root", programs_root, 32U) != 0)
        return 1;
    module_key[1] = 9U;
    if (vector_leaf(module_key, sizeof(module_key), programs_root, 32U,
                    outer9) != LXP_OK ||
        vector_bytes("outer9_leaf", expected, 32U) != 0 ||
        memcmp(outer9, expected, 32U) != 0 ||
        vector_node(outer0, outer9, actual) != LXP_OK ||
        vector_bytes("outer_root", expected, 32U) != 0 ||
        memcmp(actual, expected, 32U) != 0 ||
        vector_u32("max_proof_depth", &max_depth) != 0 ||
        vector_u32("refused_proof_depth", &refused_depth) != 0 ||
        max_depth != LXP_STATE_PROOF_MAX_DEPTH ||
        refused_depth != LXP_STATE_PROOF_MAX_DEPTH + 1U)
        return 1;
    return 0;
}

static int derivation_vectors(void)
{
    static const char empty_expected[] =
        "558c786d2c1f6371169ad993b4adb445e3081e410ce50bc7da1752005426fd40";
    static const char vault_expected[] =
        "ae8ecdd739892abd6f799dc19ebf0c5791eddf59db1c41e12f8ee22a590507f2";
    static const char binary_expected[] =
        "694e43962a8b89d0ee449629e50c6e8f2bf8492ba86c3b6e99782158235a29f5";
    static const char maximum_expected[] =
        "295fb65ee3e4ffa67749a0ea0e4c709c0f14f1223034ac8b5e7d1d76346aba24";
    static const uint8_t binary_seed[5] = {0x00U, 0xffU, 0x7fU, 0x80U, 0x01U};
    uint8_t program_id[32];
    uint8_t expected[32];
    uint8_t derived[32];
    uint8_t maximum[LX_PROGRAMS_ACCOUNT_MAX_SEED_BYTES];
    (void)memset(program_id, 1, sizeof(program_id));
    decode_hex(empty_expected, expected);
    if (lxp_programs_account_derive(program_id, NULL, 0U, derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    decode_hex(vault_expected, expected);
    if (lxp_programs_account_derive(program_id, (const uint8_t *)"vault", 5U,
                                    derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    {
        size_t i;
        for (i = 0U; i < sizeof(program_id); ++i)
            program_id[i] = (uint8_t)(i + 1U);
    }
    decode_hex(binary_expected, expected);
    if (lxp_programs_account_derive(program_id, binary_seed,
                                    sizeof(binary_seed), derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0)
        return 1;
    (void)memset(program_id, 0xab, sizeof(program_id));
    (void)memset(maximum, 0xcd, sizeof(maximum));
    decode_hex(maximum_expected, expected);
    if (lxp_programs_account_derive(program_id, maximum, sizeof(maximum),
                                    derived) != LXP_OK ||
        memcmp(derived, expected, sizeof(derived)) != 0 ||
        lxp_programs_account_derive(program_id, maximum,
            sizeof(maximum) + 1U, derived) != LXP_ERR_LENGTH_LIMIT)
        return 1;
    return 0;
}

static lxp_result count_binding(const lx_programs_account_binding *binding,
                                void *user)
{
    size_t *count = (size_t *)user;
    if (binding == NULL || binding->seed_length != 5U ||
        memcmp(binding->seed, "vault", 5U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    ++*count;
    return LXP_OK;
}

static lxp_result count_value_account(
    const lx_programs_value_account_view *view, void *user)
{
    size_t *count = (size_t *)user;
    if (view == NULL || view->binding.seed_length != 5U ||
        memcmp(view->binding.seed, "vault", 5U) != 0 ||
        !lxp_u128_is_zero(view->balance) || view->frozen ||
        view->observed_sequence != 7U ||
        lxp_ct_is_zero(view->account_root, 32U))
        return LXP_ERR_CONTEXT_MISMATCH;
    ++*count;
    return LXP_OK;
}

static int registry_boundaries(void)
{
    static const uint8_t module_name[] = "programs";
    lx_account_registry registry;
    lx_account_registration registration;
    lx_account *account;
    uint8_t account_id[32];
    uint8_t asset_id[32];
    bool created;

    (void)memset(account_id, 0x31, sizeof(account_id));
    (void)memset(asset_id, 0x42, sizeof(asset_id));
    if (lx_account_registry_init(&registry) != LXP_OK)
        return 1;
    registry.count = 1U;
    registry.accounts[0].kind = LX_ACCOUNT_AGENT_MAIN;
    (void)memcpy(registry.accounts[0].id, account_id, sizeof(account_id));
    if (lx_account_module_value_prepare(
            &registry, module_name, sizeof(module_name) - 1U, account_id,
            asset_id, 1U, &registration, &account, &created) !=
        LXP_ERR_ACCOUNT_ID_MISMATCH)
        return 1;

    if (lx_account_registry_init(&registry) != LXP_OK)
        return 1;
    registry.count = LX_ACCOUNT_REGISTRY_CAPACITY;
    if (lx_account_module_value_prepare(
            &registry, module_name, sizeof(module_name) - 1U, account_id,
            asset_id, 1U, &registration, &account, &created) !=
        LXP_ERR_ARENA_EXHAUSTED)
        return 1;
    return 0;
}

static int registration_law(void)
{
    static const uint8_t program_prefix[] = "program\0";
    static const uint8_t owner_prefix[] = "program-owner\0";
    uint8_t arena_bytes[4096];
    static uint8_t snapshot_bytes[1048576];
    lxp_arena arena;
    lxp_arena snapshot_arena;
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lx_account_registry accounts;
    lxp_transfer_asset_state assets[2];
    lx_programs_transfer_runtime runtime;
    lxp_module_ctx deploy_ctx;
    lxp_module_ctx legacy_ctx;
    lxp_module_ctx account_ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    lxp_authority_resolved authority;
    const lxp_module_registration *registration;
    const lxp_module_registration *legacy_registration;
    lxp_result module_result = LXP_OK;
    uint64_t parameters = 1U;
    uint8_t program_id[32];
    uint8_t expected_id[32];
    uint8_t deploy_key[sizeof(program_prefix) - 1U + 32U];
    uint8_t owner_key[sizeof(owner_prefix) - 1U + 32U];
    uint8_t deploy_record[71];
    uint8_t owner_record[33];
    uint8_t registration_payload[78];
    uint8_t before_root[32];
    uint8_t preview_root[32];
    uint8_t committed_root[32];
    uint8_t account_root[32];
    lx_account *account;
    lx_programs_account_binding binding;
    lx_programs_account_state_head state_head;
    lx_programs_value_account_view value_view;
    bool created;
    size_t visited = 0U;
    size_t value_visited = 0U;
    lxp_byte_span snapshot;
    lxp_snapshot_manifest_record manifest;
    lxp_verified_receipt_index receipt_index;
    lxp_state_store restored_store;
    lxp_state_journal restored_journal;
    lxp_kernel restored_kernel;
    lx_account_registry restored_accounts;
    lx_programs_transfer_runtime restored_runtime;

    (void)memset(program_id, 1, sizeof(program_id));
    (void)memset(assets, 0, sizeof(assets));
    (void)memset(&runtime, 0, sizeof(runtime));
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(registration_payload, 0, sizeof(registration_payload));
    (void)memset(deploy_record, 0, sizeof(deploy_record));
    (void)memset(owner_record, 0, sizeof(owner_record));
    (void)memset(&receipt_index, 0, sizeof(receipt_index));
    (void)memcpy(assets[0].asset_id, "asset-one", 9U);
    (void)memcpy(assets[1].asset_id, "asset-two", 9U);
    assets[0].registered = true;
    assets[1].registered = true;
    runtime.accounts = &accounts;
    runtime.assets = assets;
    runtime.asset_count = 2U;
    (void)memcpy(deploy_key, program_prefix, sizeof(program_prefix) - 1U);
    (void)memcpy(deploy_key + sizeof(program_prefix) - 1U,
                 program_id, 32U);
    (void)memcpy(owner_key, owner_prefix, sizeof(owner_prefix) - 1U);
    (void)memcpy(owner_key + sizeof(owner_prefix) - 1U, program_id, 32U);
    (void)memcpy(registration_payload, program_id, 32U);
    (void)memcpy(registration_payload + 32U, "LXPA1", 5U);
    (void)memcpy(registration_payload + 37U, assets[0].asset_id, 32U);
    registration_payload[72] = 5U;
    (void)memcpy(registration_payload + 73U, "vault", 5U);
    activity.activity_type = LX_PROGRAMS_ACCOUNT;
    activity.payload = (lxp_byte_span){registration_payload,
                                       sizeof(registration_payload)};
    (void)memset(authority.principal, 0x44, 32U);
    deploy_record[0] = 1U;
    (void)memcpy(deploy_record + 1U, authority.principal, 32U);
    (void)memset(deploy_record + 33U, 0x51, 32U);
    deploy_record[66] = 2U;
    deploy_record[70] = 1U;
    owner_record[0] = 1U;
    (void)memcpy(owner_record + 1U, authority.principal, 32U);

    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_arena_init(&snapshot_arena, snapshot_bytes,
                       sizeof(snapshot_bytes)) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lxp_state_store_init(&store, 7U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_set_epoch(&kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v3()) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS,
                                       &runtime) != LXP_OK ||
        lxp_module_ctx_init(&deploy_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            1U, 1U, 7U, 10000U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&deploy_ctx, deploy_key, sizeof(deploy_key),
                       deploy_record, sizeof(deploy_record)) != LXP_OK ||
        lxp_ctx_kv_put(&deploy_ctx, owner_key, sizeof(owner_key),
                       owner_record, sizeof(owner_record)) != LXP_OK ||
        lxp_module_ctx_commit(&deploy_ctx) != LXP_OK ||
        lxp_state_root(&kernel, before_root) != LXP_OK ||
        lxp_state_journal_open(&store, 7U, &journal) != LXP_OK ||
        lxp_module_ctx_init(&legacy_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            2U, 1U, 7U, 10000U, &arena, false) != LXP_OK)
        return 1;
    legacy_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&legacy_ctx, &effects) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_REGISTRY, 1U,
                                       &legacy_registration) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_ACCOUNT, 1U,
                                       &registration) != LXP_OK ||
        lxp_programs_account_derive(program_id, (const uint8_t *)"vault", 5U,
                                    expected_id) != LXP_OK)
        return 1;
    activity.activity_type = LX_PROGRAMS_REGISTRY;
    if (lxp_kernel_dispatch(legacy_registration, &legacy_ctx, &activity,
                            &authority, &effects, &module_result) != LXP_OK ||
        module_result != LXP_OK || legacy_ctx.staged_account_count != 0U ||
        legacy_ctx.staged_count != 0U || effects.count != 1U ||
        effects.effects[0].event_type != LX_PROGRAMS_EVENT_REGISTRY_READ)
        return 1;
    lxp_module_ctx_rollback(&legacy_ctx);
    if (lxp_module_ctx_init(&account_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            3U, 1U, 7U, 10000U, &arena, false) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&account_ctx, &effects) != LXP_OK)
        return 1;
    activity.activity_type = LX_PROGRAMS_ACCOUNT;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    module_result = LXP_OK;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_VERSION_UNSUPPORTED ||
        account_ctx.staged_account_count != 0U ||
        account_ctx.staged_count != 0U || effects.count != 0U)
        return 1;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    (void)memset(authority.principal, 0x45, 32U);
    module_result = LXP_OK;
    if (lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_ERR_AUTH_SCOPE ||
        account_ctx.staged_account_count != 0U ||
        account_ctx.staged_count != 0U || effects.count != 0U)
        return 1;
    (void)memset(authority.principal, 0x44, 32U);
    module_result = LXP_OK;
    if (lxp_kernel_dispatch(registration, &account_ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK ||
        module_result != LXP_OK || accounts.count != 0U ||
        account_ctx.staged_account_count != 1U ||
        account_ctx.staged_count != 2U ||
        account_ctx.staged_accounts[0].account.kind !=
            LX_ACCOUNT_MODULE_VALUE ||
        !account_ctx.staged_accounts[0].account.has_asset ||
        memcmp(account_ctx.staged_accounts[0].account.id,
               expected_id, 32U) != 0 ||
        memcmp(account_ctx.staged_accounts[0].account.asset_id,
               assets[0].asset_id, 32U) != 0 ||
        effects.count != 1U ||
        effects.effects[0].event_type !=
            LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED ||
        !store.account_root_required ||
        lxp_module_ctx_prepare_commit(&account_ctx) != LXP_OK ||
        lxp_module_ctx_preview_state_root(&account_ctx, &journal,
                                          preview_root) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&account_ctx) != LXP_OK ||
        accounts.count != 1U || kernel.module_kv_count != 4U ||
        lx_account_open(&accounts, accounts.accounts[0].name,
                        accounts.accounts[0].name_length, expected_id, 8U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &account) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        lxp_state_root(&kernel, committed_root) != LXP_OK ||
        memcmp(preview_root, committed_root, 32U) != 0 ||
        memcmp(before_root, committed_root, 32U) == 0)
        return 1;

    if (lxp_state_journal_open(&store, 8U, &journal) != LXP_OK ||
        lxp_module_ctx_init(&account_ctx, &kernel, LXP_MODULE_PROGRAMS,
                            4U, 1U, 8U, 10000U, &arena, true) != LXP_OK)
        return 1;
    account_ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    receipt_index.count = 1U;
    (void)memset(receipt_index.entries[0].receipt_digest, 0x77, 32U);
    receipt_index.entries[0].global_sequence = 7U;
    receipt_index.entries[0].timestamp = 7U;
    (void)memcpy(receipt_index.entries[0].resulting_state_root,
                 committed_root, 32U);
    account_ctx.verified_receipts = &receipt_index;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&account_ctx, &effects) != LXP_OK ||
        lxp_programs_account_register(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            assets[0].asset_id, &account, &created) != LXP_OK ||
        created || account != &accounts.accounts[0] || effects.count != 0U ||
        lxp_programs_account_lookup(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            &binding, &account) != LXP_OK ||
        account != &accounts.accounts[0] ||
        lxp_programs_account_lookup_id(
            &account_ctx, expected_id, &binding, &account) != LXP_OK ||
        lxp_programs_account_iter(&account_ctx, program_id, count_binding,
                                  &visited) != LXP_OK ||
        visited != 1U ||
        binding.record_version != 2U ||
        binding.registered_sequence != 7U ||
        lxp_ct_is_zero(binding.registration_event_digest, 32U) ||
        lx_account_registry_root(&accounts, account_root) != LXP_OK ||
        lxp_programs_account_state_head_read(
            &account_ctx, program_id,
            receipt_index.entries[0].receipt_digest, &state_head) != LXP_OK ||
        state_head.observed_sequence != 7U || state_head.observed_at != 7U ||
        memcmp(state_head.receipt_digest,
               receipt_index.entries[0].receipt_digest, 32U) != 0 ||
        memcmp(state_head.state_root, committed_root, 32U) != 0 ||
        memcmp(state_head.account_root, account_root, 32U) != 0 ||
        lxp_programs_value_account_read(
            &account_ctx, expected_id,
            receipt_index.entries[0].receipt_digest, &value_view) != LXP_OK ||
        memcmp(value_view.binding.account_id, expected_id, 32U) != 0 ||
        memcmp(value_view.binding.asset_id, assets[0].asset_id, 32U) != 0 ||
        !lxp_u128_is_zero(value_view.balance) || value_view.frozen ||
        value_view.observed_sequence != 7U ||
        memcmp(value_view.receipt_digest,
               receipt_index.entries[0].receipt_digest, 32U) != 0 ||
        memcmp(value_view.state_root, committed_root, 32U) != 0 ||
        memcmp(value_view.account_root, account_root, 32U) != 0 ||
        lxp_programs_value_account_iter(
            &account_ctx, program_id,
            receipt_index.entries[0].receipt_digest, count_value_account,
            &value_visited) != LXP_OK ||
        value_visited != 1U ||
        lxp_programs_account_register(
            &account_ctx, program_id, (const uint8_t *)"vault", 5U,
            assets[1].asset_id, &account, &created) !=
                LXP_ERR_ASSET_MISMATCH)
        return 1;
    lxp_module_ctx_rollback(&account_ctx);
    if (lxp_state_journal_rollback(&journal) != LXP_OK ||
        accounts.count != 1U || kernel.module_kv_count != 4U)
        return 1;

    if (lxp_snapshot_write(&kernel, 7U, &snapshot_arena, &snapshot) != LXP_OK ||
        lxp_snapshot_manifest_build(snapshot.bytes, snapshot.length, 7U,
                                    committed_root, committed_root,
                                    &manifest) != LXP_OK ||
        lx_account_registry_init(&restored_accounts) != LXP_OK ||
        lxp_state_store_init(&restored_store, 0U) != LXP_OK ||
        lxp_kernel_create(&restored_kernel, &restored_store,
                          &restored_journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&restored_kernel,
                                   programs_module_registration()) != LXP_OK ||
        lxp_kernel_set_epoch(&restored_kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&restored_kernel,
                                   programs_module_registration_v3()) !=
            LXP_OK)
        return 1;
    restored_runtime = runtime;
    restored_runtime.accounts = &restored_accounts;
    if (lxp_kernel_bind_module_runtime(&restored_kernel, LXP_MODULE_PROGRAMS,
                                       &restored_runtime) != LXP_OK ||
        lxp_snapshot_load(snapshot.bytes, snapshot.length, &manifest,
                          &restored_kernel) != LXP_OK ||
        restored_accounts.count != 1U ||
        memcmp(restored_accounts.accounts[0].id, expected_id, 32U) != 0 ||
        lxp_state_root(&restored_kernel, before_root) != LXP_OK ||
        memcmp(before_root, committed_root, 32U) != 0 ||
        lxp_state_store_destroy(&restored_store) != LXP_OK ||
        lxp_state_store_destroy(&store) != LXP_OK)
        return 1;
    return 0;
}

int main(void)
{
    if (derivation_vectors() != 0) return 1;
    if (shared_state_vectors() != 0) return 1;
    if (registry_boundaries() != 0) return 1;
    if (feed_group_pairing_replay() != 0) return 1;
    if (feed_runtime_bindings() != 0) return 1;
    return registration_law();
}
