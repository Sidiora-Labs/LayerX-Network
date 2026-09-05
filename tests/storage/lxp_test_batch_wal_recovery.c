#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L

#include "lxp_daemon_batch_wal.h"
#include "layerx/lxp_crypto.h"
#include "layerx/programs.h"

#include <openssl/evp.h>
#include <stdbool.h>
#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

struct lxp_daemon_batch_wal_record {
    lxp_daemon_batch_wal_state state;
    lxp_daemon_batch_wal_input view;
    lxp_byte_span activities[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span receipts[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span events[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span terminal_payloads[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_byte_span call_graphs[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    lxp_merkle_proof proofs[LXP_DAEMON_BATCH_WAL_MAX_ITEMS];
    uint8_t *owned;
    size_t owned_length;
};

enum {
    TEST_NETWORK_ID = 42,
    TEST_BATCH_NUMBER = 7,
    TEST_FIRST_SEQUENCE = 17,
    TEST_ARENA_BYTES = 2 * LXP_MAX_ACTIVITY_BYTES + 65536
};

static const uint64_t TEST_TIMESTAMP_MS = UINT64_C(1700000000123);

typedef struct canonical_batch_fixture {
    lxp_daemon_batch_wal_input input;
    lxp_byte_span activities[1];
    lxp_byte_span receipts[1];
    lxp_byte_span events[1];
    lxp_merkle_proof receipt_proofs[1];
    uint8_t canonical_activity[2048];
    size_t canonical_activity_length;
    uint8_t canonical_receipt[LXP_STATE_MAX_RECEIPT_BYTES];
    size_t canonical_receipt_length;
    uint8_t canonical_events[64];
    size_t canonical_events_length;
    uint8_t canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t sequencer_private[32];
    uint8_t actor_private[32];
} canonical_batch_fixture;

static int raw_public_key(const uint8_t private_key[32],
                          uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &length) == 1 &&
        length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int raw_sign(const uint8_t private_key[32], const uint8_t *message,
                    size_t message_length, uint8_t signature[64])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(
        EVP_PKEY_ED25519, NULL, private_key, 32U);
    EVP_MD_CTX *context = key == NULL ? NULL : EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length,
                       message, message_length) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static void boundary(lxp_kernel_batch_boundary *value, uint8_t tag,
                     uint64_t next_sequence)
{
    (void)memset(value, 0, sizeof(*value));
    value->canonical_state_root[0] = tag;
    value->receipt_state_root[0] = (uint8_t)(tag + 1U);
    value->next_sequence = next_sequence;
}

static int build_canonical_batch(canonical_batch_fixture *fixture,
                                 bool invalid_activity_signature)
{
    static uint8_t arena_memory[TEST_ARENA_BYTES];
    static const uint8_t actor_did[] = "did:lxp:batch-wal-signature";
    static const uint8_t payload[] = {1U, 3U, 5U, 7U, 9U};
    lxp_arena arena;
    lxp_activity activity;
    lxp_kernel_execution execution;
    lxp_batch_roots roots;
    lxp_effect_buffer effects;
    lxp_receipt receipt;
    lxp_receipt decoded_receipt;
    lxp_batch_header header;
    lxp_byte_span encoded;
    lxp_byte_span projected_events;
    uint8_t actor_public[32];
    uint8_t sequencer_public[32];
    uint8_t activity_preimage[32];
    uint8_t activity_signature[64];
    uint8_t activity_id[32];
    uint8_t batch_id[32];
    uint8_t receipt_hashes[1][32];
    uint8_t receipt_root[32];
    (void)memset(fixture, 0, sizeof(*fixture));
    (void)memset(fixture->sequencer_private, 0x17,
                 sizeof(fixture->sequencer_private));
    (void)memset(fixture->actor_private, 0x29,
                 sizeof(fixture->actor_private));
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&execution, 0, sizeof(execution));
    (void)memset(&receipt, 0, sizeof(receipt));
    (void)memset(&decoded_receipt, 0, sizeof(decoded_receipt));
    (void)memset(&header, 0, sizeof(header));
    if (lxp_arena_init(&arena, arena_memory, sizeof(arena_memory)) != LXP_OK ||
        raw_public_key(fixture->actor_private, actor_public) != 0 ||
        raw_public_key(fixture->sequencer_private, sequencer_public) != 0)
        return 1;

    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = TEST_NETWORK_ID;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){actor_did, sizeof(actor_did) - 1U};
    activity.authority = (lxp_byte_span){actor_public, sizeof(actor_public)};
    activity.account_sequence = 1U;
    activity.timestamp_bound.not_before = TEST_TIMESTAMP_MS - 100U;
    activity.timestamp_bound.not_after = TEST_TIMESTAMP_MS + 100U;
    activity.idempotency_key[0] = 0x41U;
    activity.fee_limit = (lxp_u128){0U, 25U};
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    if (lxp_hash_payload(activity.payload.bytes, activity.payload.length,
                         activity.payload_hash) != LXP_OK ||
        lxp_activity_signing_preimage(&activity, activity_preimage) != LXP_OK ||
        raw_sign(fixture->actor_private, activity_preimage,
                 sizeof(activity_preimage), activity_signature) != 0)
        return 1;
    if (invalid_activity_signature) activity_signature[0] ^= 1U;
    activity.signature = (lxp_byte_span){activity_signature,
                                         sizeof(activity_signature)};
    if (lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->canonical_activity))
        return 1;
    fixture->canonical_activity_length = encoded.length;
    (void)memcpy(fixture->canonical_activity, encoded.bytes, encoded.length);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_activity_id(fixture->canonical_activity,
                        fixture->canonical_activity_length,
                        activity_id) != LXP_OK)
        return 1;
    fixture->activities[0] = (lxp_byte_span){
        fixture->canonical_activity, fixture->canonical_activity_length};

    boundary(&fixture->input.base, 0x31U, TEST_FIRST_SEQUENCE);
    boundary(&fixture->input.settled, 0x41U, TEST_FIRST_SEQUENCE + 1U);
    if (lxp_daemon_batch_bind_prefix(
            fixture->activities, 1U,
            fixture->input.base.receipt_state_root,
            TEST_FIRST_SEQUENCE, TEST_BATCH_NUMBER, &arena,
            &execution, &roots, batch_id) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(
            &receipt, activity_id, TEST_FIRST_SEQUENCE,
            fixture->input.base.receipt_state_root,
            fixture->input.settled.receipt_state_root,
            roots.activity_merkle_root, LXP_OK, &effects,
            (lxp_u128){0U, 1U}, batch_id, 1U, 1U, 1U) != LXP_OK)
        return 1;
    receipt.timestamp = TEST_TIMESTAMP_MS;
    if (lxp_receipt_sign(&receipt, fixture->sequencer_private,
                         &arena) != LXP_OK ||
        lxp_receipt_encode(&receipt, true, &arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->canonical_receipt))
        return 1;
    fixture->canonical_receipt_length = encoded.length;
    (void)memcpy(fixture->canonical_receipt, encoded.bytes, encoded.length);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_receipt_decode(fixture->canonical_receipt,
                           fixture->canonical_receipt_length, true,
                           &decoded_receipt) != LXP_OK ||
        lxp_programs_project_receipt_events(
            &decoded_receipt, &arena, &projected_events) != LXP_OK ||
        projected_events.length > sizeof(fixture->canonical_events))
        return 1;
    fixture->canonical_events_length = projected_events.length;
    (void)memcpy(fixture->canonical_events, projected_events.bytes,
                 projected_events.length);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK) return 1;
    fixture->receipts[0] = (lxp_byte_span){
        fixture->canonical_receipt, fixture->canonical_receipt_length};
    fixture->events[0] = (lxp_byte_span){
        fixture->canonical_events, fixture->canonical_events_length};
    if (lxp_merkle_leaf_hash(fixture->canonical_receipt,
                             fixture->canonical_receipt_length,
                             receipt_hashes[0]) != LXP_OK ||
        lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, 1U, 0U, &arena,
            &fixture->receipt_proofs[0], receipt_root) != LXP_OK ||
        lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){
                fixture->activities, 1U, fixture->receipts, 1U,
                fixture->events, 1U, NULL, 0U, NULL, 0U},
            &arena, &roots) != LXP_OK ||
        lxp_ct_memcmp(receipt_root, roots.receipt_merkle_root, 32U) != 0)
        return 1;

    (void)memcpy(fixture->input.authorization.public_key,
                 sequencer_public, 32U);
    (void)memcpy(fixture->input.authorization.sequencer_id,
                 sequencer_public, 32U);
    fixture->input.authorization.first_batch_number = TEST_BATCH_NUMBER;
    fixture->input.authorization.last_batch_number = TEST_BATCH_NUMBER;
    fixture->input.authorization.authorized = 1U;
    header.protocol_version = LXP_PROTOCOL_VERSION;
    header.network_id = TEST_NETWORK_ID;
    header.epoch = 3U;
    header.batch_number = TEST_BATCH_NUMBER;
    header.first_sequence = TEST_FIRST_SEQUENCE;
    header.last_sequence = TEST_FIRST_SEQUENCE;
    (void)memcpy(header.previous_state_root,
                 fixture->input.base.receipt_state_root, 32U);
    (void)memcpy(header.resulting_state_root,
                 fixture->input.settled.receipt_state_root, 32U);
    (void)memcpy(header.activity_merkle_root,
                 roots.activity_merkle_root, 32U);
    (void)memcpy(header.receipt_merkle_root,
                 roots.receipt_merkle_root, 32U);
    (void)memcpy(header.event_merkle_root,
                 roots.event_merkle_root, 32U);
    (void)memcpy(header.data_availability_root,
                 roots.data_availability_root, 32U);
    (void)memcpy(header.oracle_root, roots.oracle_root, 32U);
    header.timestamp_ms = TEST_TIMESTAMP_MS;
    (void)memcpy(header.sequencer_id,
                 fixture->input.authorization.sequencer_id, 32U);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_batch_sign(&header, fixture->sequencer_private,
                       &fixture->input.authorization,
                       fixture->input.header_signature, &arena) != LXP_OK ||
        lxp_batch_header_encode(&header, &arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(fixture->canonical_header))
        return 1;
    (void)memcpy(fixture->canonical_header, encoded.bytes, encoded.length);

    fixture->input.protocol_version = LXP_PROTOCOL_VERSION;
    fixture->input.network_id = TEST_NETWORK_ID;
    fixture->input.epoch = 3U;
    fixture->input.batch_number = TEST_BATCH_NUMBER;
    fixture->input.timestamp_ms = TEST_TIMESTAMP_MS;
    fixture->input.parameter_version = 1U;
    fixture->input.fee_schedule_version = 1U;
    fixture->input.metering_schedule_version = 1U;
    fixture->input.first_sequence = TEST_FIRST_SEQUENCE;
    fixture->input.last_sequence = TEST_FIRST_SEQUENCE;
    fixture->input.count = 1U;
    fixture->input.canonical_header = (lxp_byte_span){
        fixture->canonical_header, sizeof(fixture->canonical_header)};
    fixture->input.activities = fixture->activities;
    fixture->input.receipts = fixture->receipts;
    fixture->input.events = fixture->events;
    fixture->input.receipt_proofs = fixture->receipt_proofs;
    return lxp_kernel_batch_publication_digest(
        &fixture->input.base, &fixture->input.settled,
        fixture->activities, fixture->receipts, fixture->events, 1U,
        fixture->input.publication_digest) == LXP_OK ? 0 : 1;
}

static int expect_classification(
    lxp_daemon_batch_wal_record *record,
    const lxp_kernel_batch_boundary *live,
    lxp_daemon_batch_wal_recovery expected)
{
    lxp_daemon_batch_wal_recovery actual = 0;
    return lxp_daemon_batch_wal_classify(record, live, &actual) == LXP_OK &&
        actual == expected ? 0 : 1;
}

static int refuse_invalid_canonical_activity_signature(void)
{
    char directory[] = "/tmp/lxp-batch-wal-signature-XXXXXX";
    char path[160];
    canonical_batch_fixture fixture;
    lxp_activity decoded_activity;
    uint8_t fsynced_digest[32];
    int path_length;
    if (mkdtemp(directory) == NULL) return 1;
    path_length = snprintf(path, sizeof(path), "%s/prepared-batch.lxw",
                           directory);
    if (path_length < 0 || (size_t)path_length >= sizeof(path) ||
        build_canonical_batch(&fixture, false) != 0 ||
        lxp_daemon_batch_wal_write_prepared(
            directory, &fixture.input, fsynced_digest) != LXP_OK ||
        lxp_ct_memcmp(fsynced_digest, fixture.input.publication_digest,
                      sizeof(fsynced_digest)) != 0 ||
        unlink(path) != 0 ||
        build_canonical_batch(&fixture, true) != 0 ||
        lxp_activity_decode(fixture.canonical_activity,
                            fixture.canonical_activity_length,
                            &decoded_activity) != LXP_OK ||
        lxp_activity_verify_signature(&decoded_activity) !=
            LXP_ERR_BAD_SIGNATURE ||
        lxp_daemon_batch_wal_write_prepared(
            directory, &fixture.input, fsynced_digest) !=
            LXP_ERR_BAD_SIGNATURE ||
        access(path, F_OK) == 0 || rmdir(directory) != 0)
        return 1;
    return 0;
}

static int classify_recovery_matrix(void)
{
    lxp_daemon_batch_wal_record record;
    lxp_kernel_batch_boundary unrelated;
    lxp_kernel_batch_boundary changed_root;
    lxp_daemon_batch_wal_recovery recovery;
    (void)memset(&record, 0, sizeof(record));
    boundary(&record.view.base, 0x11U, 8U);
    boundary(&record.view.settled, 0x21U, 10U);
    boundary(&unrelated, 0x31U, 12U);

    record.state = LXP_DAEMON_BATCH_WAL_PREPARED;
    if (expect_classification(&record, &record.view.base,
                              LXP_DAEMON_BATCH_WAL_DISCARD_BASE) != 0 ||
        expect_classification(&record, &record.view.settled,
                              LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED) != 0)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_ABORTED;
    if (expect_classification(&record, &record.view.base,
                              LXP_DAEMON_BATCH_WAL_ALREADY_ABORTED) != 0 ||
        lxp_daemon_batch_wal_classify(&record, &record.view.settled,
                                      &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_COMMITTED;
    if (expect_classification(&record, &record.view.settled,
                              LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED) != 0 ||
        lxp_daemon_batch_wal_classify(&record, &record.view.base,
                                      &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        lxp_daemon_batch_wal_classify(&record, &unrelated, &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE)
        return 1;
    record.state = LXP_DAEMON_BATCH_WAL_PREPARED;
    changed_root = record.view.base;
    changed_root.receipt_state_root[31] = 1U;
    if (lxp_daemon_batch_wal_classify(&record, &changed_root, &recovery) !=
            LXP_FATAL_REPLAY_DIVERGENCE ||
        lxp_daemon_batch_wal_classify(NULL, &record.view.base, &recovery) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_daemon_batch_wal_classify(&record, NULL, &recovery) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_daemon_batch_wal_classify(&record, &record.view.base, NULL) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}

static int write_exact(int descriptor, const uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written <= 0) return 1;
        offset += (size_t)written;
    }
    return 0;
}

static int recover_both_schemas(void)
{
    canonical_batch_fixture fixture;
    lxp_byte_span empty_artifacts[1] = {{0}};
    uint8_t digest[32];
    char directory[] = "/tmp/lxp-batch-wal-schemas-XXXXXX";
    char path[160];
    if (build_canonical_batch(&fixture, false) != 0 ||
        mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/prepared-batch.lxw", directory) < 0)
        return 1;
    for (unsigned version = 1U; version <= 2U; ++version) {
        lxp_daemon_batch_wal_record *record = NULL;
        const lxp_daemon_batch_wal_input *view;
        uint8_t prefix[12];
        bool present = false;
        int descriptor;
        if (version == 2U) {
            fixture.input.terminal_payloads = empty_artifacts;
            if (lxp_daemon_batch_wal_write_prepared(directory, &fixture.input, digest) == LXP_OK)
                return 1;
            fixture.input.call_graphs = empty_artifacts;
        }
        if (lxp_daemon_batch_wal_write_prepared(directory, &fixture.input, digest) != LXP_OK ||
            lxp_daemon_batch_wal_load(directory, &fixture.input.authorization,
                                      &record, &present) != LXP_OK ||
            !present || record == NULL)
            return 1;
        view = lxp_daemon_batch_wal_view(record);
        if (view == NULL ||
            view->activities[0].length != fixture.activities[0].length ||
            memcmp(view->activities[0].bytes, fixture.activities[0].bytes,
                   fixture.activities[0].length) != 0 ||
            view->receipts[0].length != fixture.receipts[0].length ||
            memcmp(view->receipts[0].bytes, fixture.receipts[0].bytes,
                   fixture.receipts[0].length) != 0 ||
            (version == 1U && (view->terminal_payloads != NULL || view->call_graphs != NULL)) ||
            (version == 2U && (view->terminal_payloads == NULL || view->call_graphs == NULL ||
                              view->terminal_payloads[0].length != 0U ||
                              view->call_graphs[0].length != 0U)))
            return 1;
        lxp_daemon_batch_wal_destroy(record);
        record = NULL;
        descriptor = open(path, O_RDWR | O_CLOEXEC);
        if (descriptor < 0 || read(descriptor, prefix, sizeof(prefix)) != (ssize_t)sizeof(prefix) ||
            prefix[8] != 0U || prefix[9] != version)
            return 1;
        if (version == 2U) {
            off_t length = lseek(descriptor, 0, SEEK_END);
            uint8_t last;
            uint8_t corrupt;
            if (length <= 1 || pread(descriptor, &last, 1U, length - 1) != 1)
                return 1;
            corrupt = (uint8_t)(last ^ 1U);
            if (pwrite(descriptor, &corrupt, 1U, length - 1) != 1 ||
                lxp_daemon_batch_wal_load(directory, &fixture.input.authorization,
                                          &record, &present) != LXP_ERR_LOG_CORRUPT ||
                record != NULL || pwrite(descriptor, &last, 1U, length - 1) != 1 ||
                ftruncate(descriptor, length - 1) != 0 ||
                lxp_daemon_batch_wal_load(directory, &fixture.input.authorization,
                                          &record, &present) == LXP_OK || record != NULL)
                return 1;
        }
        if (close(descriptor) != 0 || unlink(path) != 0) return 1;
    }
    return rmdir(directory) != 0;
}

static int refuse_malformed_record(void)
{
    enum { MINIMUM_WAL_BYTES = 794 };
    char directory[] = "/tmp/lxp-batch-wal-corrupt-XXXXXX";
    char path[128];
    uint8_t bytes[MINIMUM_WAL_BYTES] = {0U};
    lxp_sequencer_authorization authorization;
    lxp_daemon_batch_wal_record *record = NULL;
    bool present = false;
    int descriptor;
    (void)memset(&authorization, 0, sizeof(authorization));
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/prepared-batch.lxw", directory) < 0)
        return 1;
    descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0 || write_exact(descriptor, bytes, sizeof(bytes)) != 0 ||
        fdatasync(descriptor) != 0 || close(descriptor) != 0 ||
        lxp_daemon_batch_wal_load(directory, &authorization, &record,
                                  &present) != LXP_ERR_LOG_CORRUPT ||
        record != NULL || present || unlink(path) != 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

static int sweep_interrupted_replacement(void)
{
    char directory[] = "/tmp/lxp-batch-wal-sweep-XXXXXX";
    char path[160];
    uint8_t byte = 1U;
    lxp_sequencer_authorization authorization;
    lxp_daemon_batch_wal_record *record = NULL;
    bool present = true;
    int descriptor;
    (void)memset(&authorization, 0, sizeof(authorization));
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/.prepared-batch.%llu.1.tmp",
                 directory, (unsigned long long)getpid()) < 0)
        return 1;
    descriptor = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0 || write_exact(descriptor, &byte, sizeof(byte)) != 0 ||
        fdatasync(descriptor) != 0 || close(descriptor) != 0 ||
        lxp_daemon_batch_wal_load(directory, &authorization, &record,
                                  &present) != LXP_OK ||
        record != NULL || present || access(path, F_OK) == 0 ||
        rmdir(directory) != 0)
        return 1;
    return 0;
}

int main(void)
{
    return recover_both_schemas() != 0 ||
        refuse_invalid_canonical_activity_signature() != 0 ||
        classify_recovery_matrix() != 0 ||
        refuse_malformed_record() != 0 ||
        sweep_interrupted_replacement() != 0 ? 1 : 0;
}
