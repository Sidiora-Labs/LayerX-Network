#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L

#include "lxp_verify_cli.h"

#include "layerx/lx_asset.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_genesis_builder.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_protocol.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define REQUIRE(condition) \
    do { \
        if (!(condition)) { \
            (void)fprintf(stderr, "test_verify_cli line %d: %s\n", \
                          __LINE__, #condition); \
            return 1; \
        } \
    } while (0)

enum {
    TEST_NETWORK_ID = 42,
    TEST_OTHER_NETWORK_ID = 43,
    TEST_BATCH_NUMBER = 7,
    TEST_FIRST_SEQUENCE = 9,
    TEST_LAST_SEQUENCE = 10,
    TEST_LEAF_COUNT = 2,
    TEST_ARENA_BYTES = 8 * 1024 * 1024,
    TEST_RECEIPT_BYTES = 4096,
    TEST_PROOF_BYTES = 4096,
    TEST_MANIFEST_BYTES = LXP_GENESIS_MAX_ENCODED_BYTES,
    TEST_ACTIVITY_BYTES = 4096
};

/*
 * Offsets into the proof material the daemon emits, from
 * cmd/layerxd/lxp_daemon_evidence.c write_merkle_proof and
 * write_signed_header: wire version, response kind and the asserted
 * activity id, then the inclusion proof, then the signed header.
 */
enum {
    BUNDLE_PREFIX_BYTES = 2 + 1 + 32,
    BUNDLE_PROOF_DEPTH = 1,
    BUNDLE_PROOF_BYTES = 4 + 4 + 1 + BUNDLE_PROOF_DEPTH * 32,
    BUNDLE_SIGNED_HEADER_OFFSET = BUNDLE_PREFIX_BYTES + BUNDLE_PROOF_BYTES,
    BUNDLE_FIRST_BATCH_OFFSET = BUNDLE_SIGNED_HEADER_OFFSET + 2 + 32 + 32,
    BUNDLE_LAST_BATCH_OFFSET = BUNDLE_FIRST_BATCH_OFFSET + 8,
    BUNDLE_TOTAL_BYTES = BUNDLE_LAST_BATCH_OFFSET + 8 + 4 +
        LXP_BATCH_HEADER_ENCODED_SIZE + 64
};

static const uint64_t TEST_TIMESTAMP_MS = UINT64_C(1700000000123);

typedef struct verify_fixture {
    uint8_t sequencer_private[32];
    uint8_t sequencer_public[32];
    uint8_t foreign_private[32];
    uint8_t actor_private[32];
    uint8_t manifest_bytes[TEST_MANIFEST_BYTES];
    size_t manifest_length;
    uint8_t foreign_manifest_bytes[TEST_MANIFEST_BYTES];
    size_t foreign_manifest_length;
    uint8_t other_network_manifest_bytes[TEST_MANIFEST_BYTES];
    size_t other_network_manifest_length;
    uint8_t canonical_activity[TEST_LEAF_COUNT][TEST_ACTIVITY_BYTES];
    size_t canonical_activity_length[TEST_LEAF_COUNT];
    uint8_t activity_id[TEST_LEAF_COUNT][32];
    lxp_merkle_proof activity_proof[TEST_LEAF_COUNT];
    uint8_t canonical_receipt[TEST_LEAF_COUNT][TEST_RECEIPT_BYTES];
    size_t canonical_receipt_length[TEST_LEAF_COUNT];
    uint8_t receipt_digest[TEST_LEAF_COUNT][32];
    lxp_merkle_proof receipt_proof[TEST_LEAF_COUNT];
    uint8_t canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t header_signature[64];
    lxp_sequencer_authorization authorization;
    uint8_t activity_bundle[TEST_PROOF_BYTES];
    size_t activity_bundle_length;
    uint8_t receipt_bundle[TEST_PROOF_BYTES];
    size_t receipt_bundle_length;
} verify_fixture;

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

static void programs_parameters(
    const uint8_t signer_public_key[32], const uint8_t asset_id[32],
    lx_programs_metering_schedule *metering,
    lx_programs_fee_genesis_parameters *fees)
{
    (void)memset(metering, 0, sizeof(*metering));
    metering->version = 1U;
    metering->coefficients[0] = 1U;
    metering->coefficients[1] = 1U;
    metering->coefficients[2] = 1U;
    metering->coefficients[3] = 1U;
    metering->coefficients[4] = 1U;
    metering->coefficients[5] = 8U;
    metering->coefficients[6] = 8U;
    metering->coefficients[7] = 64U;
    metering->coefficients[8] = 8U;
    metering->activation_batch = 1U;
    metering->authority_kind = LX_PROGRAMS_METERING_AUTHORITY_GENESIS;
    (void)lxp_hash_payload(signer_public_key, 32U,
                           metering->authority_digest);
    (void)memset(fees, 0, sizeof(*fees));
    fees->schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 100U
    };
    (void)memcpy(fees->occupancy_asset_id, asset_id, 32U);
    fees->target_occupancy_byte_batches = 100U;
    fees->response_denominator = 1U;
    fees->maximum_change_numerator = 1U;
    fees->maximum_change_denominator = 10U;
    fees->minimum_fee_units_per_occupancy_byte_batch = 1U;
    fees->maximum_fee_units_per_occupancy_byte_batch = 1000U;
}

static int build_manifest(const uint8_t signer_private_key[32],
                          uint32_t network_id, lxp_arena *arena,
                          uint8_t *out, size_t capacity, size_t *length)
{
    static const uint8_t parameter_key[32] = {
        'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
    };
    static lxp_genesis_manifest draft;
    static lxp_genesis_manifest manifest;
    lx_programs_metering_schedule metering;
    lx_programs_fee_genesis_parameters fee_parameters;
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_byte_span encoded_manifest;
    lxp_byte_span snapshot;
    uint8_t signer_public_key[32];
    uint8_t asset_id[32] = {0x85U};
    size_t mark = lxp_arena_mark(arena);
    if (raw_public_key(signer_private_key, signer_public_key) != 0) return 1;
    (void)memset(&draft, 0, sizeof(draft));
    draft.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    draft.network_id = network_id;
    draft.genesis_timestamp_ms = UINT64_C(1700000000000);
    draft.parameter_count = 1U;
    draft.parameters[0].module_id = LXP_MODULE_GOVERNANCE;
    (void)memcpy(draft.parameters[0].key, parameter_key,
                 sizeof(parameter_key));
    draft.parameters[0].value[31] = 1U;
    draft.guarantor_count = 1U;
    draft.guarantors[0].guarantor_id[0] = 1U;
    draft.guarantors[0].public_key[0] = 2U;
    draft.guarantors[0].public_key[32] = 3U;
    draft.guarantors[0].bond = (lxp_u128){0U, 0U};
    programs_parameters(signer_public_key, asset_id, &metering,
                        &fee_parameters);
    if (lxp_genesis_build_fresh_empty(
            &draft, asset_id, &metering, &fee_parameters,
            signer_private_key, arena, &manifest, &snapshot_manifest,
            &encoded_manifest, &snapshot) != LXP_OK ||
        encoded_manifest.length > capacity)
        return 1;
    (void)memcpy(out, encoded_manifest.bytes, encoded_manifest.length);
    *length = encoded_manifest.length;
    return lxp_arena_reset(arena, mark) == LXP_OK ? 0 : 1;
}

static int build_activity(verify_fixture *fixture, size_t index,
                          lxp_arena *arena)
{
    static const uint8_t did[] = "did:lxp:verify-cli";
    static const uint8_t payloads[TEST_LEAF_COUNT][5] = {
        {1U, 3U, 5U, 7U, 9U},
        {2U, 4U, 6U, 8U, 10U}
    };
    lxp_activity activity;
    lxp_byte_span encoded;
    uint8_t actor_public[32];
    uint8_t preimage[32];
    uint8_t signature[64];
    size_t mark = lxp_arena_mark(arena);
    (void)memset(&activity, 0, sizeof(activity));
    if (raw_public_key(fixture->actor_private, actor_public) != 0) return 1;
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = TEST_NETWORK_ID;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){did, sizeof(did) - 1U};
    activity.authority = (lxp_byte_span){actor_public, sizeof(actor_public)};
    activity.account_sequence = (uint64_t)index + 1U;
    activity.timestamp_bound.not_before = TEST_TIMESTAMP_MS - 100U;
    activity.timestamp_bound.not_after = TEST_TIMESTAMP_MS + 100U;
    activity.idempotency_key[0] = (uint8_t)(0x40U + index);
    activity.fee_limit = (lxp_u128){0U, 25U};
    activity.payload = (lxp_byte_span){payloads[index],
                                       sizeof(payloads[index])};
    if (lxp_hash_payload(activity.payload.bytes, activity.payload.length,
                         activity.payload_hash) != LXP_OK ||
        lxp_activity_signing_preimage(&activity, preimage) != LXP_OK ||
        raw_sign(fixture->actor_private, preimage, sizeof(preimage),
                 signature) != 0)
        return 1;
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    if (lxp_activity_encode(&activity, arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->canonical_activity[index]) ||
        lxp_activity_id(encoded.bytes, encoded.length,
                        fixture->activity_id[index]) != LXP_OK)
        return 1;
    fixture->canonical_activity_length[index] = encoded.length;
    (void)memcpy(fixture->canonical_activity[index], encoded.bytes,
                 encoded.length);
    return lxp_arena_reset(arena, mark) == LXP_OK ? 0 : 1;
}

static int build_receipt(verify_fixture *fixture, size_t index,
                         uint64_t sequence, const uint8_t previous_root[32],
                         const uint8_t resulting_root[32],
                         const uint8_t activity_root[32], lxp_arena *arena)
{
    lxp_ledger_receipt_input input;
    lxp_receipt receipt;
    lxp_byte_span encoded;
    size_t mark = lxp_arena_mark(arena);
    (void)memset(&input, 0, sizeof(input));
    (void)memcpy(input.transaction_id, fixture->activity_id[index], 32U);
    input.operation = (uint8_t)LX_ASSET_SEND;
    input.global_sequence = sequence;
    input.asset[0] = 0x85U;
    input.amount = (lxp_u128){0U, 25U};
    input.from[0] = 5U;
    input.from_balance_before = (lxp_u128){0U, 100U};
    input.from_balance_after = (lxp_u128){0U, 75U};
    input.from_sequence = (uint64_t)index + 1U;
    input.to[0] = 6U;
    input.to_balance_before = (lxp_u128){0U, 10U};
    input.to_balance_after = (lxp_u128){0U, 35U};
    input.transfer_set_root[0] = 7U;
    input.authorization_hash[0] = 8U;
    input.context_hash[0] = 9U;
    (void)memcpy(input.previous_state_root, previous_root, 32U);
    (void)memcpy(input.resulting_state_root, resulting_root, 32U);
    input.batch_id[0] = 0x77U;
    input.batch_id[31] = (uint8_t)(index + 1U);
    input.timestamp = TEST_TIMESTAMP_MS;
    input.leg_count = 1U;
    if (lxp_ledger_receipt_build(&receipt, &input) != LXP_OK) return 1;
    (void)memcpy(receipt.activity_root, activity_root, 32U);
    receipt.module_id = LXP_MODULE_ASSET;
    receipt.module_version = 1U;
    receipt.parameter_version = (uint32_t)(index + 1U);
    if (lxp_receipt_sign(&receipt, fixture->sequencer_private,
                         arena) != LXP_OK ||
        lxp_receipt_encode(&receipt, true, arena, &encoded) != LXP_OK ||
        encoded.length > sizeof(fixture->canonical_receipt[index]) ||
        lxp_receipt_digest(&receipt, arena,
                           fixture->receipt_digest[index]) != LXP_OK)
        return 1;
    fixture->canonical_receipt_length[index] = encoded.length;
    (void)memcpy(fixture->canonical_receipt[index], encoded.bytes,
                 encoded.length);
    return lxp_arena_reset(arena, mark) == LXP_OK ? 0 : 1;
}

static int build_batch(verify_fixture *fixture, lxp_arena *arena)
{
    uint8_t activity_hashes[TEST_LEAF_COUNT][32];
    uint8_t receipt_hashes[TEST_LEAF_COUNT][32];
    uint8_t activity_root[32];
    uint8_t receipt_root[32];
    uint8_t previous_root[32] = {0x91U};
    uint8_t resulting_root[32] = {0x5aU};
    lxp_batch_header header;
    lxp_byte_span encoded;
    size_t index;
    size_t mark;
    for (index = 0U; index < (size_t)TEST_LEAF_COUNT; ++index) {
        if (build_activity(fixture, index, arena) != 0 ||
            lxp_merkle_leaf_hash(
                fixture->canonical_activity[index],
                fixture->canonical_activity_length[index],
                activity_hashes[index]) != LXP_OK)
            return 1;
    }
    for (index = 0U; index < (size_t)TEST_LEAF_COUNT; ++index) {
        if (lxp_merkle_proof_generate(
                (const uint8_t (*)[32])activity_hashes,
                (size_t)TEST_LEAF_COUNT, index, arena,
                &fixture->activity_proof[index], activity_root) != LXP_OK)
            return 1;
    }
    for (index = 0U; index < (size_t)TEST_LEAF_COUNT; ++index) {
        if (build_receipt(fixture, index,
                          (uint64_t)TEST_FIRST_SEQUENCE + index,
                          previous_root, resulting_root, activity_root,
                          arena) != 0 ||
            lxp_merkle_leaf_hash(
                fixture->canonical_receipt[index],
                fixture->canonical_receipt_length[index],
                receipt_hashes[index]) != LXP_OK)
            return 1;
    }
    for (index = 0U; index < (size_t)TEST_LEAF_COUNT; ++index) {
        if (lxp_merkle_proof_generate(
                (const uint8_t (*)[32])receipt_hashes,
                (size_t)TEST_LEAF_COUNT, index, arena,
                &fixture->receipt_proof[index], receipt_root) != LXP_OK)
            return 1;
    }
    (void)memset(&fixture->authorization, 0, sizeof(fixture->authorization));
    (void)memcpy(fixture->authorization.public_key,
                 fixture->sequencer_public, 32U);
    (void)memcpy(fixture->authorization.sequencer_id,
                 fixture->sequencer_public, 32U);
    fixture->authorization.first_batch_number = TEST_BATCH_NUMBER;
    fixture->authorization.last_batch_number = TEST_BATCH_NUMBER;
    fixture->authorization.authorized = 1U;
    (void)memset(&header, 0, sizeof(header));
    header.protocol_version = LXP_PROTOCOL_VERSION;
    header.network_id = TEST_NETWORK_ID;
    header.epoch = 3U;
    header.batch_number = TEST_BATCH_NUMBER;
    header.first_sequence = TEST_FIRST_SEQUENCE;
    header.last_sequence = TEST_LAST_SEQUENCE;
    (void)memcpy(header.previous_state_root, previous_root, 32U);
    (void)memcpy(header.resulting_state_root, resulting_root, 32U);
    (void)memcpy(header.activity_merkle_root, activity_root, 32U);
    (void)memcpy(header.receipt_merkle_root, receipt_root, 32U);
    header.event_merkle_root[0] = 0x14U;
    header.data_availability_root[0] = 0x15U;
    header.oracle_root[0] = 0x16U;
    header.timestamp_ms = TEST_TIMESTAMP_MS;
    (void)memcpy(header.sequencer_id, fixture->sequencer_public, 32U);
    if (lxp_batch_sign(&header, fixture->sequencer_private,
                       &fixture->authorization, fixture->header_signature,
                       arena) != LXP_OK)
        return 1;
    mark = lxp_arena_mark(arena);
    if (lxp_batch_header_encode(&header, arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(fixture->canonical_header))
        return 1;
    (void)memcpy(fixture->canonical_header, encoded.bytes, encoded.length);
    return lxp_arena_reset(arena, mark) == LXP_OK ? 0 : 1;
}

/*
 * The bundles come out of the daemon's own producer so the verifier is held
 * to the wire format the public lx_getProof path actually emits.
 */
static int build_bundles(verify_fixture *fixture, lxp_arena *arena)
{
    lxp_daemon_activity_evidence evidence;
    lxp_byte_span value;
    lxp_byte_span proof;
    size_t mark = lxp_arena_mark(arena);
    (void)memset(&evidence, 0, sizeof(evidence));
    (void)memcpy(evidence.activity_id, fixture->activity_id[0], 32U);
    (void)memcpy(evidence.receipt_digest, fixture->receipt_digest[0], 32U);
    evidence.global_sequence = TEST_FIRST_SEQUENCE;
    evidence.batch_number = TEST_BATCH_NUMBER;
    evidence.canonical_activity = (lxp_byte_span){
        fixture->canonical_activity[0],
        fixture->canonical_activity_length[0]};
    evidence.activity_proof = fixture->activity_proof[0];
    evidence.canonical_receipt = (lxp_byte_span){
        fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0]};
    evidence.receipt_proof = fixture->receipt_proof[0];
    evidence.signed_header.authorization = fixture->authorization;
    evidence.signed_header.canonical_header = (lxp_byte_span){
        fixture->canonical_header, sizeof(fixture->canonical_header)};
    (void)memcpy(evidence.signed_header.signature,
                 fixture->header_signature, 64U);
    if (lxp_daemon_activity_evidence_wire_encode(
            &evidence, TEST_NETWORK_ID, 1U, arena, &value,
            &proof) != LXP_OK ||
        value.length != fixture->canonical_activity_length[0] ||
        lxp_ct_memcmp(value.bytes, fixture->canonical_activity[0],
                      value.length) != 0 ||
        proof.length > sizeof(fixture->activity_bundle))
        return 1;
    fixture->activity_bundle_length = proof.length;
    (void)memcpy(fixture->activity_bundle, proof.bytes, proof.length);
    if (lxp_daemon_activity_evidence_wire_encode(
            &evidence, TEST_NETWORK_ID, 3U, arena, &value,
            &proof) != LXP_OK ||
        value.length != fixture->canonical_receipt_length[0] ||
        lxp_ct_memcmp(value.bytes, fixture->canonical_receipt[0],
                      value.length) != 0 ||
        proof.length > sizeof(fixture->receipt_bundle))
        return 1;
    fixture->receipt_bundle_length = proof.length;
    (void)memcpy(fixture->receipt_bundle, proof.bytes, proof.length);
    return lxp_arena_reset(arena, mark) == LXP_OK ? 0 : 1;
}

static void store_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (8U * (7U - index)));
}

static int write_file(const char *path, const uint8_t *bytes, size_t length)
{
    FILE *file = fopen(path, "wb");
    int ok;
    if (file == NULL) return 1;
    ok = fwrite(bytes, 1U, length, file) == length;
    return fclose(file) == 0 && ok ? 0 : 1;
}

typedef struct cli_capture {
    char stdout_text[8192];
    char stderr_text[4096];
    int exit_code;
} cli_capture;

static int slurp(const char *path, char *out, size_t capacity)
{
    FILE *file = fopen(path, "rb");
    size_t length;
    if (file == NULL) return 1;
    length = fread(out, 1U, capacity - 1U, file);
    out[length] = '\0';
    return fclose(file) == 0 ? 0 : 1;
}

static int run_cli(const char *directory, int argc, char **argv,
                   const char *stdin_path, cli_capture *capture)
{
    char out_path[512];
    char err_path[512];
    int saved_out = dup(STDOUT_FILENO);
    int saved_err = dup(STDERR_FILENO);
    int saved_in = dup(STDIN_FILENO);
    int ok = 0;
    (void)snprintf(out_path, sizeof(out_path), "%s/cli-stdout", directory);
    (void)snprintf(err_path, sizeof(err_path), "%s/cli-stderr", directory);
    (void)memset(capture, 0, sizeof(*capture));
    if (saved_out < 0 || saved_err < 0 || saved_in < 0) return 1;
    if (freopen(out_path, "wb", stdout) == NULL ||
        freopen(err_path, "wb", stderr) == NULL ||
        (stdin_path != NULL && freopen(stdin_path, "rb", stdin) == NULL))
        ok = 1;
    if (ok == 0) capture->exit_code = lxp_verify_cli_main(argc, argv);
    (void)fflush(stdout);
    (void)fflush(stderr);
    (void)dup2(saved_out, STDOUT_FILENO);
    (void)dup2(saved_err, STDERR_FILENO);
    (void)dup2(saved_in, STDIN_FILENO);
    (void)close(saved_out);
    (void)close(saved_err);
    (void)close(saved_in);
    clearerr(stdout);
    clearerr(stderr);
    clearerr(stdin);
    if (ok != 0) return 1;
    return slurp(out_path, capture->stdout_text,
                 sizeof(capture->stdout_text)) != 0 ||
           slurp(err_path, capture->stderr_text,
                 sizeof(capture->stderr_text)) != 0 ? 1 : 0;
}

static int api_checks(const verify_fixture *fixture, lxp_arena *arena)
{
    static lxp_verify_trust_root trust_root;
    static lxp_verify_trust_root foreign_root;
    static lxp_verify_bundle_report bundle;
    static lxp_verify_receipt_report receipt;
    static uint8_t scratch[TEST_PROOF_BYTES];
    static uint8_t manifest_scratch[TEST_MANIFEST_BYTES];
    const char *stage = NULL;
    size_t index;

    REQUIRE(lxp_verify_trust_root_load(
        fixture->manifest_bytes, fixture->manifest_length, arena,
        &trust_root, &stage) == LXP_OK);
    REQUIRE(stage == NULL);
    REQUIRE(trust_root.network_id == (uint32_t)TEST_NETWORK_ID);
    REQUIRE(trust_root.guarantor_count == 1U);
    REQUIRE(!trust_root.guarantors[0].bonded);
    REQUIRE(!lxp_ct_is_zero(trust_root.manifest_commitment, 32U));
    /* The pinned authority is the sequencer that signs headers and receipts. */
    REQUIRE(lxp_ct_memcmp(trust_root.authority_public_key,
                          fixture->sequencer_public, 32U) == 0);
    REQUIRE(lxp_verify_trust_root_load(
        fixture->foreign_manifest_bytes, fixture->foreign_manifest_length,
        arena, &foreign_root, &stage) == LXP_OK);
    REQUIRE(lxp_ct_memcmp(foreign_root.authority_public_key,
                          trust_root.authority_public_key, 32U) != 0);

    /* A truncated or altered manifest is not a trust root. */
    REQUIRE(lxp_verify_trust_root_load(
        fixture->manifest_bytes, fixture->manifest_length - 1U, arena,
        &foreign_root, &stage) != LXP_OK);
    REQUIRE(stage != NULL);
    (void)memcpy(manifest_scratch, fixture->manifest_bytes,
                 fixture->manifest_length);
    manifest_scratch[fixture->manifest_length / 2U] = (uint8_t)(
        manifest_scratch[fixture->manifest_length / 2U] ^ 0x40U);
    REQUIRE(lxp_verify_trust_root_load(
        manifest_scratch, fixture->manifest_length, arena, &foreign_root,
        &stage) != LXP_OK);
    REQUIRE(stage != NULL);
    REQUIRE(lxp_verify_trust_root_load(
        fixture->foreign_manifest_bytes, fixture->foreign_manifest_length,
        arena, &foreign_root, &stage) == LXP_OK);

    REQUIRE(lxp_verify_receipt_offline_pinned(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], arena, &receipt,
        &stage) == LXP_OK);
    REQUIRE(stage == NULL);
    REQUIRE(lxp_ct_memcmp(receipt.activity_id,
                          fixture->activity_id[0], 32U) == 0);
    REQUIRE(lxp_ct_memcmp(receipt.receipt_digest,
                          fixture->receipt_digest[0], 32U) == 0);
    REQUIRE(receipt.global_sequence == (uint64_t)TEST_FIRST_SEQUENCE);
    REQUIRE(lxp_verify_receipt_offline_pinned(
        &foreign_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], arena, &receipt,
        &stage) != LXP_OK);
    REQUIRE(stage != NULL && strcmp(stage, "receipt-authority") == 0);

    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_activity[0],
        fixture->canonical_activity_length[0], fixture->activity_bundle,
        fixture->activity_bundle_length, arena, &bundle, &stage) == LXP_OK);
    REQUIRE(stage == NULL);
    REQUIRE(bundle.kind == (uint8_t)LXP_VERIFY_BUNDLE_ACTIVITY);
    REQUIRE(!bundle.receipt_present);
    REQUIRE(bundle.batch_number == (uint64_t)TEST_BATCH_NUMBER);
    REQUIRE(bundle.leaf_count == (uint32_t)TEST_LEAF_COUNT);
    REQUIRE(bundle.network_id == (uint32_t)TEST_NETWORK_ID);
    REQUIRE(lxp_ct_memcmp(bundle.activity_id,
                          fixture->activity_id[0], 32U) == 0);

    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], fixture->receipt_bundle,
        fixture->receipt_bundle_length, arena, &bundle, &stage) == LXP_OK);
    REQUIRE(bundle.kind == (uint8_t)LXP_VERIFY_BUNDLE_RECEIPT);
    REQUIRE(bundle.receipt_present);
    REQUIRE(lxp_ct_memcmp(bundle.receipt.receipt_digest,
                          fixture->receipt_digest[0], 32U) == 0);

    /* Evidence signed by another authority is refused before any decode. */
    REQUIRE(lxp_verify_bundle_offline(
        &foreign_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], fixture->receipt_bundle,
        fixture->receipt_bundle_length, arena, &bundle,
        &stage) == LXP_ERR_AUTH_SCOPE);
    REQUIRE(stage != NULL && strcmp(stage, "bundle-authority") == 0);

    /* A trust root for another network refuses the header. */
    REQUIRE(lxp_verify_trust_root_load(
        fixture->other_network_manifest_bytes,
        fixture->other_network_manifest_length, arena, &foreign_root,
        &stage) == LXP_OK);
    REQUIRE(foreign_root.network_id == (uint32_t)TEST_OTHER_NETWORK_ID);
    REQUIRE(lxp_verify_bundle_offline(
        &foreign_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], fixture->receipt_bundle,
        fixture->receipt_bundle_length, arena, &bundle,
        &stage) == LXP_ERR_WRONG_NETWORK);

    /* Truncated and extended proof material are both refused. */
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], fixture->receipt_bundle,
        fixture->receipt_bundle_length - 1U, arena, &bundle,
        &stage) != LXP_OK);
    REQUIRE(fixture->receipt_bundle_length + 1U <= sizeof(scratch));
    (void)memcpy(scratch, fixture->receipt_bundle,
                 fixture->receipt_bundle_length);
    scratch[fixture->receipt_bundle_length] = 0U;
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], scratch,
        fixture->receipt_bundle_length + 1U, arena, &bundle,
        &stage) == LXP_ERR_TRAILING_BYTES);

    /*
     * Every byte of the proof material is authenticated except the eight
     * that carry the responder's own last authorized batch number, which no
     * signature covers.  Widening it changes nothing because the verifier
     * narrows the authority to the batch the signed header names, and
     * moving the range off that batch is refused.
     */
    REQUIRE(fixture->receipt_bundle_length == (size_t)BUNDLE_TOTAL_BYTES);
    for (index = 0U; index < fixture->receipt_bundle_length; ++index) {
        if (index >= (size_t)BUNDLE_LAST_BATCH_OFFSET &&
            index < (size_t)BUNDLE_LAST_BATCH_OFFSET + 8U)
            continue;
        (void)memcpy(scratch, fixture->receipt_bundle,
                     fixture->receipt_bundle_length);
        scratch[index] = (uint8_t)(scratch[index] ^ 0x5aU);
        REQUIRE(lxp_verify_bundle_offline(
            &trust_root, fixture->canonical_receipt[0],
            fixture->canonical_receipt_length[0], scratch,
            fixture->receipt_bundle_length, arena, &bundle,
            &stage) != LXP_OK);
    }
    (void)memcpy(scratch, fixture->receipt_bundle,
                 fixture->receipt_bundle_length);
    store_u64(scratch + BUNDLE_LAST_BATCH_OFFSET, UINT64_C(4096));
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], scratch,
        fixture->receipt_bundle_length, arena, &bundle, &stage) == LXP_OK);
    REQUIRE(bundle.batch_number == (uint64_t)TEST_BATCH_NUMBER);
    store_u64(scratch + BUNDLE_FIRST_BATCH_OFFSET,
              (uint64_t)TEST_BATCH_NUMBER + 1U);
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], scratch,
        fixture->receipt_bundle_length, arena, &bundle,
        &stage) == LXP_ERR_AUTH_SCOPE);
    (void)memcpy(scratch, fixture->receipt_bundle,
                 fixture->receipt_bundle_length);
    store_u64(scratch + BUNDLE_FIRST_BATCH_OFFSET,
              (uint64_t)TEST_BATCH_NUMBER - 2U);
    store_u64(scratch + BUNDLE_LAST_BATCH_OFFSET,
              (uint64_t)TEST_BATCH_NUMBER - 1U);
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[0],
        fixture->canonical_receipt_length[0], scratch,
        fixture->receipt_bundle_length, arena, &bundle,
        &stage) == LXP_ERR_AUTH_SCOPE);

    /* The proved value cannot be swapped for the sibling leaf. */
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_receipt[1],
        fixture->canonical_receipt_length[1], fixture->receipt_bundle,
        fixture->receipt_bundle_length, arena, &bundle, &stage) != LXP_OK);
    REQUIRE(lxp_verify_bundle_offline(
        &trust_root, fixture->canonical_activity[0],
        fixture->canonical_activity_length[0], fixture->receipt_bundle,
        fixture->receipt_bundle_length, arena, &bundle, &stage) != LXP_OK);
    return 0;
}

static int cli_checks(const verify_fixture *fixture, const char *directory)
{
    char trust_path[512];
    char foreign_path[512];
    char receipt_path[512];
    char activity_path[512];
    char receipt_proof_path[512];
    char activity_proof_path[512];
    char tampered_path[512];
    cli_capture capture;
    uint8_t tampered[TEST_RECEIPT_BYTES];
    char *trust_root_argv[3];
    char *receipt_argv[5];
    char *bundle_argv[7];

    (void)snprintf(trust_path, sizeof(trust_path), "%s/genesis.manifest",
                   directory);
    (void)snprintf(foreign_path, sizeof(foreign_path),
                   "%s/foreign.manifest", directory);
    (void)snprintf(receipt_path, sizeof(receipt_path), "%s/receipt.bin",
                   directory);
    (void)snprintf(activity_path, sizeof(activity_path), "%s/activity.bin",
                   directory);
    (void)snprintf(receipt_proof_path, sizeof(receipt_proof_path),
                   "%s/receipt.proof", directory);
    (void)snprintf(activity_proof_path, sizeof(activity_proof_path),
                   "%s/activity.proof", directory);
    (void)snprintf(tampered_path, sizeof(tampered_path), "%s/tampered.bin",
                   directory);
    REQUIRE(write_file(trust_path, fixture->manifest_bytes,
                       fixture->manifest_length) == 0);
    REQUIRE(write_file(foreign_path, fixture->foreign_manifest_bytes,
                       fixture->foreign_manifest_length) == 0);
    REQUIRE(write_file(receipt_path, fixture->canonical_receipt[0],
                       fixture->canonical_receipt_length[0]) == 0);
    REQUIRE(write_file(activity_path, fixture->canonical_activity[0],
                       fixture->canonical_activity_length[0]) == 0);
    REQUIRE(write_file(receipt_proof_path, fixture->receipt_bundle,
                       fixture->receipt_bundle_length) == 0);
    REQUIRE(write_file(activity_proof_path, fixture->activity_bundle,
                       fixture->activity_bundle_length) == 0);
    (void)memcpy(tampered, fixture->canonical_receipt[0],
                 fixture->canonical_receipt_length[0]);
    tampered[fixture->canonical_receipt_length[0] / 2U] =
        (uint8_t)(tampered[fixture->canonical_receipt_length[0] / 2U] ^ 1U);
    REQUIRE(write_file(tampered_path, tampered,
                       fixture->canonical_receipt_length[0]) == 0);

    trust_root_argv[0] = "layerx-verify";
    trust_root_argv[1] = "trust-root";
    trust_root_argv[2] = "--trust-root";
    receipt_argv[0] = "layerx-verify";
    receipt_argv[1] = "receipt";
    receipt_argv[2] = "--trust-root";
    receipt_argv[4] = "--receipt";
    bundle_argv[0] = "layerx-verify";
    bundle_argv[1] = "bundle";
    bundle_argv[2] = "--trust-root";
    bundle_argv[4] = "--value";
    bundle_argv[6] = "--proof";

    {
        char *argv[4] = {trust_root_argv[0], trust_root_argv[1],
                         trust_root_argv[2], trust_path};
        REQUIRE(run_cli(directory, 4, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 0);
        REQUIRE(strstr(capture.stdout_text,
                       "layerx-verify trust-root ok\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "trust-root-network-id 42\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "trust-root-guarantor-count 1\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "trust-root-guarantor-bonded no\n") != NULL);
    }
    {
        char *argv[6] = {receipt_argv[0], receipt_argv[1], receipt_argv[2],
                         trust_path, receipt_argv[4], receipt_path};
        REQUIRE(run_cli(directory, 6, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 0);
        REQUIRE(strstr(capture.stdout_text,
                       "layerx-verify receipt ok\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "receipt-global-sequence 9\n") != NULL);
        REQUIRE(strstr(capture.stdout_text, "receipt-amount 25\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "receipt-result-code 0 LXP_OK\n") != NULL);
    }
    {
        /* Standard input carries the receipt when the path is "-". */
        char dash[2] = {'-', '\0'};
        char *argv[6] = {receipt_argv[0], receipt_argv[1], receipt_argv[2],
                         trust_path, receipt_argv[4], dash};
        cli_capture piped;
        REQUIRE(run_cli(directory, 6, argv, receipt_path, &piped) == 0);
        REQUIRE(piped.exit_code == 0);
        REQUIRE(strcmp(piped.stdout_text, capture.stdout_text) == 0);
    }
    {
        char *argv[6] = {receipt_argv[0], receipt_argv[1], receipt_argv[2],
                         foreign_path, receipt_argv[4], receipt_path};
        REQUIRE(run_cli(directory, 6, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 1);
        REQUIRE(capture.stdout_text[0] == '\0');
        REQUIRE(strstr(capture.stderr_text,
                       "refused receipt-authority") != NULL);
    }
    {
        char *argv[6] = {receipt_argv[0], receipt_argv[1], receipt_argv[2],
                         trust_path, receipt_argv[4], tampered_path};
        REQUIRE(run_cli(directory, 6, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 1);
        REQUIRE(strstr(capture.stderr_text, "layerx-verify: refused ") !=
                NULL);
    }
    {
        char *argv[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         trust_path, bundle_argv[4], activity_path,
                         bundle_argv[6], activity_proof_path};
        REQUIRE(run_cli(directory, 8, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 0);
        REQUIRE(strstr(capture.stdout_text,
                       "layerx-verify bundle ok\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "bundle-kind activity\n") != NULL);
        REQUIRE(strstr(capture.stdout_text,
                       "header-batch-number 7\n") != NULL);
        REQUIRE(strstr(capture.stdout_text, "receipt-batch-id ") == NULL);
    }
    {
        char *argv[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         trust_path, bundle_argv[4], receipt_path,
                         bundle_argv[6], receipt_proof_path};
        REQUIRE(run_cli(directory, 8, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 0);
        REQUIRE(strstr(capture.stdout_text, "bundle-kind receipt\n") != NULL);
        REQUIRE(strstr(capture.stdout_text, "receipt-batch-id ") != NULL);
        REQUIRE(strstr(capture.stdout_text, "bundle-leaf-count 2\n") != NULL);
    }
    {
        char dash[2] = {'-', '\0'};
        char *argv[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         trust_path, bundle_argv[4], dash,
                         bundle_argv[6], receipt_proof_path};
        cli_capture piped;
        REQUIRE(run_cli(directory, 8, argv, receipt_path, &piped) == 0);
        REQUIRE(piped.exit_code == 0);
        REQUIRE(strcmp(piped.stdout_text, capture.stdout_text) == 0);
    }
    {
        char *argv[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         foreign_path, bundle_argv[4], receipt_path,
                         bundle_argv[6], receipt_proof_path};
        REQUIRE(run_cli(directory, 8, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 1);
        REQUIRE(strstr(capture.stderr_text,
                       "refused bundle-authority") != NULL);
    }
    {
        char *argv[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         trust_path, bundle_argv[4], tampered_path,
                         bundle_argv[6], receipt_proof_path};
        REQUIRE(run_cli(directory, 8, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 1);
        REQUIRE(strstr(capture.stderr_text, "refused bundle-") != NULL);
    }
    {
        /* Two stdin inputs, unknown flags and missing flags are usage errors. */
        char dash[2] = {'-', '\0'};
        char *both[8] = {bundle_argv[0], bundle_argv[1], bundle_argv[2],
                         dash, bundle_argv[4], dash,
                         bundle_argv[6], receipt_proof_path};
        char *missing[4] = {receipt_argv[0], receipt_argv[1],
                            receipt_argv[2], trust_path};
        char *wrong_flag[6] = {receipt_argv[0], receipt_argv[1],
                               receipt_argv[2], trust_path,
                               bundle_argv[4], receipt_path};
        char *unknown[2] = {trust_root_argv[0], (char *)"attest"};
        REQUIRE(run_cli(directory, 8, both, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 2);
        REQUIRE(run_cli(directory, 4, missing, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 2);
        REQUIRE(run_cli(directory, 6, wrong_flag, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 2);
        REQUIRE(run_cli(directory, 2, unknown, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 2);
        REQUIRE(strstr(capture.stderr_text, "usage: layerx-verify") != NULL);
    }
    {
        char missing_path[512];
        char *argv[6];
        (void)snprintf(missing_path, sizeof(missing_path),
                       "%s/absent.manifest", directory);
        argv[0] = receipt_argv[0];
        argv[1] = receipt_argv[1];
        argv[2] = receipt_argv[2];
        argv[3] = missing_path;
        argv[4] = receipt_argv[4];
        argv[5] = receipt_path;
        REQUIRE(run_cli(directory, 6, argv, NULL, &capture) == 0);
        REQUIRE(capture.exit_code == 1);
        REQUIRE(strstr(capture.stderr_text,
                       "refused trust-root-read") != NULL);
    }
    return 0;
}

static void remove_directory(const char *directory)
{
    static const char *const names[] = {
        "cli-stdout", "cli-stderr", "genesis.manifest", "foreign.manifest",
        "receipt.bin", "activity.bin", "receipt.proof", "activity.proof",
        "tampered.bin"
    };
    char path[512];
    size_t index;
    for (index = 0U; index < sizeof(names) / sizeof(names[0]); ++index) {
        (void)snprintf(path, sizeof(path), "%s/%s", directory, names[index]);
        (void)remove(path);
    }
    (void)remove(directory);
}

int main(void)
{
    static uint8_t arena_bytes[TEST_ARENA_BYTES];
    static verify_fixture fixture;
    char directory[] = "/tmp/lxp-verify-cli-XXXXXX";
    lxp_arena arena;

    REQUIRE(lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) ==
            LXP_OK);
    (void)memset(fixture.sequencer_private, 0x11,
                 sizeof(fixture.sequencer_private));
    (void)memset(fixture.foreign_private, 0x22,
                 sizeof(fixture.foreign_private));
    (void)memset(fixture.actor_private, 0x29,
                 sizeof(fixture.actor_private));
    REQUIRE(raw_public_key(fixture.sequencer_private,
                           fixture.sequencer_public) == 0);
    REQUIRE(build_manifest(fixture.sequencer_private, TEST_NETWORK_ID,
                           &arena, fixture.manifest_bytes,
                           sizeof(fixture.manifest_bytes),
                           &fixture.manifest_length) == 0);
    REQUIRE(build_manifest(fixture.foreign_private, TEST_NETWORK_ID,
                           &arena, fixture.foreign_manifest_bytes,
                           sizeof(fixture.foreign_manifest_bytes),
                           &fixture.foreign_manifest_length) == 0);
    REQUIRE(build_manifest(fixture.sequencer_private, TEST_OTHER_NETWORK_ID,
                           &arena, fixture.other_network_manifest_bytes,
                           sizeof(fixture.other_network_manifest_bytes),
                           &fixture.other_network_manifest_length) == 0);
    REQUIRE(build_batch(&fixture, &arena) == 0);
    REQUIRE(build_bundles(&fixture, &arena) == 0);
    REQUIRE(api_checks(&fixture, &arena) == 0);
    REQUIRE(mkdtemp(directory) != NULL);
    if (cli_checks(&fixture, directory) != 0) {
        remove_directory(directory);
        return 1;
    }
    remove_directory(directory);
    (void)printf("test_verify_cli ok\n");
    return 0;
}
