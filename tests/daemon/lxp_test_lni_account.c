#define main finality_evidence_fixture_main
#include "../storage/lxp_test_finality_evidence.c"
#undef main
#include "lxp_daemon_lni_account.h"

#define CHECK(x) do { if (!(x)) { fprintf(stderr, "LNI account evidence line %d\n", __LINE__); return 1; } } while (0)
static test_fixture fixture;
static uint8_t memory[TEST_ARENA_BYTES];
static lxp_daemon_protocol_owner owner;

int main(void)
{
    lxp_arena arena;
    lxp_daemon_evidence_store store;
    lxp_daemon_receipt_authority_store authority;
    lxp_log evidence_log = {.descriptor = -1}, receipt_log = {.descriptor = -1};
    lxp_daemon_account_evidence account, changed;
    lxp_programs_occupancy_receipt maintenance;
    lxp_receipt receipt;
    lxp_batch_header header;
    lxp_byte_span ordinary, maintained, encoded_header, value, proof;
    lxp_merkle_proof receipt_proofs[2], activity_proof;
    uint8_t leaves[2][32], root[32], signature[64];
    uint8_t input[90000];
    size_t input_length;
    char evidence_path[] = "/tmp/lxp-lni-account-evidence-XXXXXX";
    char receipt_path[] = "/tmp/lxp-lni-account-receipts-XXXXXX";
    int fd;
    CHECK(lxp_arena_init(&arena, memory, sizeof(memory)) == LXP_OK);
    CHECK(build_account_and_batch(&fixture, &arena, 0x13U) == 0);
    fd = mkstemp(evidence_path); CHECK(fd >= 0 && close(fd) == 0);
    fd = mkstemp(receipt_path); CHECK(fd >= 0 && close(fd) == 0);
    CHECK(lxp_log_open_or_create(&evidence_log, evidence_path, TEST_LOG_BYTES) == LXP_OK);
    CHECK(lxp_log_open_or_create(&receipt_log, receipt_path, TEST_LOG_BYTES) == LXP_OK);
    CHECK(lxp_daemon_evidence_open(&store, &evidence_log, TEST_NETWORK_ID,
        &fixture.authorization, fixture.initial_anchor, true, NULL, NULL, &arena) == LXP_OK);
    CHECK(lxp_daemon_receipt_authority_open(&authority, &receipt_log, &fixture.authorization) == LXP_OK);
    owner.kernel = &fixture.kernel;
    owner.network_id = TEST_NETWORK_ID;
    owner.receipt_authority = &authority;
    owner.evidence_store = &store;
    for (size_t i = 0U; i < 2U; ++i)
        CHECK(lxp_daemon_receipt_authority_append(&authority, fixture.canonical_receipt[i],
            fixture.canonical_receipt_length[i], fixture.canonical_header,
            sizeof(fixture.canonical_header), fixture.header_signature,
            &fixture.receipt_proof[i], &arena) == LXP_OK);
    CHECK(latest_account_evidence(&owner, fixture.account_id, fixture.asset_id,
        fixture.activity_id[1], &arena, &account) == LXP_OK);
    CHECK(account.format_version == 1U && account.canonical_receipt.length == fixture.canonical_receipt_length[1]);
    CHECK(memcmp(account.canonical_receipt.bytes, fixture.canonical_receipt[1], account.canonical_receipt.length) == 0);
    CHECK(latest_account_evidence(&owner, fixture.account_id, NULL,
        fixture.activity_id[0], &arena, &changed) == LXP_ERR_CONTEXT_MISMATCH);
    CHECK(lxp_daemon_account_evidence_wire_encode(&store, &account, &fixture.kernel,
        TEST_NETWORK_ID, fixture.account_id, 1U, 0U, NULL, &arena, &value, &proof) == LXP_OK);
    CHECK(proof.bytes[0] == 0U && proof.bytes[1] == 1U);
    changed = account; changed.signed_header.signature[0] ^= 1U;
    CHECK(lxp_daemon_account_evidence_wire_encode(&store, &changed, &fixture.kernel,
        TEST_NETWORK_ID, fixture.account_id, 1U, 0U, NULL, &arena, &value, &proof) != LXP_OK);
    CHECK(lxp_log_close(&receipt_log) == LXP_OK && unlink(receipt_path) == 0);
    CHECK(lxp_log_open_or_create(&receipt_log, receipt_path, TEST_LOG_BYTES) == LXP_OK);
    input_length = fread(input, 1U, sizeof(input), stdin);
    CHECK(input_length > 0U && input_length < sizeof(input) && feof(stdin));
    CHECK(lxp_programs_occupancy_receipt_decode(input, input_length, &maintenance) == LXP_OK);
    fixture.authorization.first_batch_number = maintenance.batch_number;
    fixture.authorization.last_batch_number = maintenance.batch_number;
    store.authorization = fixture.authorization;
    CHECK(lxp_daemon_receipt_authority_open(&authority, &receipt_log, &fixture.authorization) == LXP_OK);
    CHECK(lxp_receipt_decode(fixture.canonical_receipt[1], fixture.canonical_receipt_length[1], true, &receipt) == LXP_OK);
    receipt.global_sequence = maintenance.global_sequence - 1U;
    memcpy(maintenance.previous_state_root, receipt.resulting_state_root, 32U);
    fixture.state.next_sequence = maintenance.global_sequence + 1U;
    CHECK(lxp_state_root(&fixture.kernel, fixture.kernel.current_state_root) == LXP_OK);
    memcpy(maintenance.resulting_state_root, fixture.kernel.current_state_root, 32U);
    CHECK(lxp_programs_occupancy_receipt_encode(&maintenance, &arena, &maintained) == LXP_OK);
    CHECK(lxp_receipt_sign(&receipt, fixture.sequencer_private, &arena) == LXP_OK);
    CHECK(lxp_receipt_encode(&receipt, true, &arena, &ordinary) == LXP_OK);
    CHECK(lxp_merkle_leaf_hash(ordinary.bytes, ordinary.length, leaves[0]) == LXP_OK);
    CHECK(lxp_merkle_leaf_hash(maintained.bytes, maintained.length, leaves[1]) == LXP_OK);
    for (size_t i = 0U; i < 2U; ++i)
        CHECK(lxp_merkle_proof_generate((const uint8_t (*)[32])leaves, 2U, i, &arena, &receipt_proofs[i], root) == LXP_OK);
    CHECK(lxp_batch_header_decode(fixture.canonical_header, sizeof(fixture.canonical_header), &header) == LXP_OK);
    header.batch_number = maintenance.batch_number;
    header.first_sequence = receipt.global_sequence;
    header.last_sequence = maintenance.global_sequence;
    memcpy(header.previous_state_root, receipt.previous_state_root, 32U);
    memcpy(header.receipt_merkle_root, root, 32U);
    memcpy(header.resulting_state_root, maintenance.resulting_state_root, 32U);
    CHECK(lxp_merkle_leaf_hash(fixture.canonical_activity[1], fixture.canonical_activity_length[1], leaves[0]) == LXP_OK);
    CHECK(lxp_merkle_proof_generate((const uint8_t (*)[32])leaves, 1U, 0U, &arena, &activity_proof, header.activity_merkle_root) == LXP_OK);
    CHECK(lxp_batch_sign(&header, fixture.sequencer_private, &fixture.authorization, signature, &arena) == LXP_OK);
    CHECK(lxp_batch_header_encode(&header, &arena, &encoded_header) == LXP_OK);
    CHECK(lxp_daemon_receipt_authority_append(&authority, ordinary.bytes, ordinary.length,
        encoded_header.bytes, encoded_header.length, signature, &receipt_proofs[0], &arena) == LXP_OK);
    CHECK(lxp_daemon_receipt_authority_append_maintenance(&authority, maintained.bytes, maintained.length,
        encoded_header.bytes, encoded_header.length, signature, &receipt_proofs[1], &arena) == LXP_OK);
    CHECK(lxp_daemon_account_evidence_publish_batch_maintenance(&store, &fixture.kernel, maintained,
        &receipt_proofs[1], &fixture.authorization, encoded_header, signature, &arena) == LXP_OK);
    CHECK(lxp_daemon_activity_evidence_publish(&store,
        (lxp_byte_span){fixture.canonical_activity[1], fixture.canonical_activity_length[1]},
        &activity_proof, ordinary, &receipt_proofs[0], &fixture.authorization, encoded_header, signature, &arena, NULL) == LXP_OK);
    CHECK(latest_account_evidence(&owner, fixture.account_id, fixture.asset_id,
        fixture.activity_id[1], &arena, &account) == LXP_OK);
    CHECK(account.format_version == 2U && account.canonical_receipt.length == maintained.length);
    CHECK(memcmp(account.canonical_receipt.bytes, maintained.bytes, maintained.length) == 0);
    CHECK(latest_account_evidence(&owner, fixture.account_id, NULL,
        fixture.activity_id[0], &arena, &changed) != LXP_OK);
    CHECK(lxp_daemon_account_evidence_wire_encode(&store, &account, &fixture.kernel,
        TEST_NETWORK_ID, fixture.account_id, 1U, 0U, NULL, &arena, &value, &proof) == LXP_OK);
    CHECK(proof.bytes[0] == 0U && proof.bytes[1] == 2U);
    for (unsigned mutation = 0U; mutation < 5U; ++mutation) {
        changed = account;
        if (mutation == 0U) changed.signed_header.signature[0] ^= 1U;
        if (mutation == 1U) changed.account_root[0] ^= 1U;
        if (mutation == 2U) --changed.observed_sequence;
        if (mutation == 3U) changed.receipt_proof.leaf_index = 0U;
        if (mutation == 4U) {
            CHECK(account.canonical_receipt.length < sizeof(input));
            memcpy(input, account.canonical_receipt.bytes, account.canonical_receipt.length);
            input[account.canonical_receipt.length - 1U] ^= 1U;
            changed.canonical_receipt.bytes = input;
        }
        CHECK(lxp_daemon_account_evidence_wire_encode(&store, &changed, &fixture.kernel,
            TEST_NETWORK_ID, fixture.account_id, 1U, 0U, NULL, &arena, &value, &proof) != LXP_OK);
    }
    --fixture.state.next_sequence;
    CHECK(latest_account_evidence(&owner, fixture.account_id, NULL, NULL, &arena, &changed) != LXP_OK);
    ++fixture.state.next_sequence;
    fixture.kernel.current_state_root[0] ^= 1U;
    CHECK(latest_account_evidence(&owner, fixture.account_id, NULL, NULL, &arena, &changed) != LXP_OK);
    fixture.kernel.current_state_root[0] ^= 1U;
    CHECK(lxp_log_close(&receipt_log) == LXP_OK && unlink(receipt_path) == 0);
    CHECK(lxp_log_close(&evidence_log) == LXP_OK && unlink(evidence_path) == 0);
    lxp_state_store_destroy(&fixture.state);
    puts("ordinary and maintained LNI account evidence, activity binding and tamper refusals passed");
    return 0;
}
