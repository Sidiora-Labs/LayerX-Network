#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/obj_mac.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

enum {
    TEST_NETWORK_ID = 42,
    TEST_BATCH_NUMBER = 7,
    TEST_FIRST_SEQUENCE = 9,
    TEST_LAST_SEQUENCE = 10,
    TEST_PAXEER_CHAIN_ID = 31337,
    TEST_LOG_BYTES = 4 * 1024 * 1024,
    TEST_ARENA_BYTES = 8 * 1024 * 1024
};

static const uint64_t TEST_TIMESTAMP_MS = UINT64_C(1700000000123);

static void report_stage_failure(const char *stage)
{
    (void)fprintf(stderr, "lxp_test_finality_evidence: %s failed\n", stage);
}

typedef struct test_fixture {
    lx_account_registry accounts;
    lx_account *account;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    bool state_initialized;
    lxp_sequencer_authorization authorization;
    uint8_t sequencer_private[32];
    uint8_t actor_private[32];
    uint8_t account_id[32];
    uint8_t asset_id[32];
    uint8_t initial_anchor[32];
    uint8_t canonical_activity[2][LXP_MAX_ACTIVITY_BYTES];
    size_t canonical_activity_length[2];
    uint8_t activity_id[2][32];
    lxp_merkle_proof activity_proof[2];
    uint8_t canonical_receipt[2][LXP_STATE_MAX_RECEIPT_BYTES];
    size_t canonical_receipt_length[2];
    uint8_t receipt_digest[2][32];
    lxp_merkle_proof receipt_proof[2];
    uint8_t canonical_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t header_signature[64];
    lxp_daemon_account_evidence account_evidence;
    lxp_guarantor_ctx guarantors[3];
    lxp_guarantor_set bonded_set;
    lxp_guarantor_cert certificate;
    lxp_finalisation_requirements requirements;
    lxp_daemon_settlement_registration_evidence settlement;
    uint8_t checkpoint_payload[LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES];
    size_t checkpoint_payload_length;
    uint8_t finality_proof[LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES];
    size_t finality_proof_length;
    uint8_t checkpoint_id[32];
} test_fixture;

typedef struct finality_authority {
    uint8_t initial_anchor[32];
    lxp_daemon_settlement_registration_evidence independently_finalized;
    uint64_t calls;
} finality_authority;

static bool settlement_equal(
    const lxp_daemon_settlement_registration_evidence *left,
    const lxp_daemon_settlement_registration_evidence *right)
{
    return left->paxeer_chain_id == right->paxeer_chain_id &&
        lxp_ct_memcmp(left->settlement_contract,
                      right->settlement_contract, 20U) == 0 &&
        lxp_ct_memcmp(left->checkpoint_id,
                      right->checkpoint_id, 32U) == 0 &&
        lxp_ct_memcmp(left->transaction_id,
                      right->transaction_id, 32U) == 0 &&
        left->observed_block_number == right->observed_block_number &&
        left->observed_at_ms == right->observed_at_ms;
}

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

static int secp_key_pair(uint8_t value, uint8_t private_key[32],
                         uint8_t public_key[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *private_value = BN_new();
    const EC_GROUP *group = key == NULL ? NULL : EC_KEY_get0_group(key);
    EC_POINT *point = group == NULL ? NULL : EC_POINT_new(group);
    size_t public_length = 0U;
    (void)memset(private_key, 0, 32U);
    private_key[31] = value;
    if (key != NULL && private_value != NULL && point != NULL &&
        BN_bin2bn(private_key, 32, private_value) != NULL &&
        EC_POINT_mul(group, point, private_value, NULL, NULL, NULL) == 1 &&
        EC_KEY_set_private_key(key, private_value) == 1 &&
        EC_KEY_set_public_key(key, point) == 1)
        public_length = EC_POINT_point2oct(
            group, point, POINT_CONVERSION_COMPRESSED,
            public_key, 33U, NULL);
    EC_POINT_free(point);
    BN_free(private_value);
    EC_KEY_free(key);
    return public_length == 33U ? 0 : 1;
}

static lxp_result verify_finality_authority(
    void *opaque, const lxp_guarantor_cert *certificate,
    const lxp_guarantor_set *bonded_set,
    const lxp_finalisation_requirements *requirements,
    const lxp_daemon_settlement_registration_evidence *settlement)
{
    finality_authority *authority = (finality_authority *)opaque;
    lxp_finalisation_state state;
    uint8_t arena_memory[256U * 1024U];
    lxp_arena arena;
    uint8_t checkpoint_id[32];
    bool finalisable = false;
    lxp_result status;
    if (authority == NULL || certificate == NULL || bonded_set == NULL ||
        requirements == NULL || settlement == NULL)
        return LXP_ERR_NON_CANONICAL;
    ++authority->calls;
    (void)memset(&state, 0, sizeof(state));
    (void)memcpy(state.settlement_anchor, authority->initial_anchor, 32U);
    status = lxp_arena_init(&arena, arena_memory, sizeof(arena_memory));
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(
            &certificate->checkpoint, &arena, checkpoint_id);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(checkpoint_id, settlement->checkpoint_id, 32U) != 0 ||
         !settlement_equal(settlement,
                           &authority->independently_finalized) ||
         certificate->attestation_count == 0U ||
         certificate->attestations[0].paxeer_chain_id !=
             settlement->paxeer_chain_id ||
         lxp_ct_memcmp(certificate->attestations[0]
                           .paxeer_settlement_contract,
                       settlement->settlement_contract, 20U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_checkpoint_finalisable(
            &state, certificate, bonded_set, requirements,
            &arena, &finalisable);
    return status == LXP_OK && finalisable ? LXP_OK :
           status == LXP_OK ? LXP_ERR_ATTESTATION_THRESHOLD : status;
}

static int build_activity(test_fixture *fixture, size_t index,
                          uint64_t account_sequence, lxp_arena *arena)
{
    static const uint8_t did[] = "did:lxp:finality-evidence";
    static const uint8_t payloads[2][5] = {
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
    if (raw_public_key(fixture->actor_private, actor_public) != 0)
        return 1;
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = TEST_NETWORK_ID;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){did, sizeof(did) - 1U};
    activity.authority = (lxp_byte_span){actor_public, sizeof(actor_public)};
    activity.account_sequence = account_sequence;
    activity.timestamp_bound.not_before = TEST_TIMESTAMP_MS - 100U;
    activity.timestamp_bound.not_after = TEST_TIMESTAMP_MS + 100U;
    activity.idempotency_key[0] = (uint8_t)(0x40U + index);
    activity.fee_limit = (lxp_u128){0U, 25U};
    activity.payload = (lxp_byte_span){payloads[index], sizeof(payloads[index])};
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

static int build_receipt(test_fixture *fixture, size_t index,
                         uint64_t sequence, const uint8_t previous_root[32],
                         const uint8_t resulting_root[32],
                         const uint8_t activity_root[32], lxp_arena *arena)
{
    lxp_effect_buffer effects;
    lxp_receipt receipt;
    lxp_byte_span encoded;
    uint8_t batch_id[32] = {0U};
    size_t mark = lxp_arena_mark(arena);
    batch_id[0] = 0x77U;
    batch_id[31] = (uint8_t)(index + 1U);
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_receipt_build(
            &receipt, fixture->activity_id[index], sequence,
            previous_root, resulting_root, activity_root, LXP_OK,
            &effects, (lxp_u128){0U, (uint64_t)index + 1U}, batch_id,
            1U, 1U, (uint32_t)(index + 1U)) != LXP_OK)
        return 1;
    receipt.timestamp = TEST_TIMESTAMP_MS;
    if (lxp_receipt_sign(&receipt, fixture->sequencer_private, arena) != LXP_OK ||
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

static int build_account_and_batch(test_fixture *fixture, lxp_arena *arena,
                                   uint8_t sequencer_seed)
{
    static const uint8_t account_name[] = "agent:did:key:evidence:main";
    static const uint8_t second_name[] = "agent:did:key:evidence:budget:ops";
    lx_account *second;
    uint8_t second_id[32];
    uint8_t activity_hashes[2][32];
    uint8_t receipt_hashes[2][32];
    uint8_t activity_root[32];
    uint8_t receipt_root[32];
    uint8_t first_resulting_root[32] = {0U};
    uint8_t sequencer_public[32];
    uint64_t parameters = 1U;
    lxp_byte_span activity_spans[2];
    lxp_byte_span receipt_spans[2];
    lxp_byte_span event_spans[2];
    lxp_receipt decoded_receipts[2];
    lxp_batch_roots roots;
    lxp_batch_header header;
    lxp_byte_span encoded;
    size_t mark;
    size_t index;
    (void)memset(fixture->sequencer_private, sequencer_seed,
                 sizeof(fixture->sequencer_private));
    (void)memset(fixture->actor_private, 0x29,
                 sizeof(fixture->actor_private));
    fixture->asset_id[0] = 0xa5U;
    fixture->initial_anchor[0] = 0x91U;
    if (raw_public_key(fixture->sequencer_private, sequencer_public) != 0 ||
        lx_account_registry_init(&fixture->accounts) != LXP_OK ||
        lx_account_id_from_string(account_name, sizeof(account_name) - 1U,
                                  fixture->account_id) != LXP_OK ||
        lx_account_id_from_string(second_name, sizeof(second_name) - 1U,
                                  second_id) != LXP_OK ||
        lx_account_open(&fixture->accounts, account_name,
                        sizeof(account_name) - 1U, fixture->account_id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL,
                        &fixture->account) != LXP_OK ||
        lx_account_open(&fixture->accounts, second_name,
                        sizeof(second_name) - 1U, second_id, 2U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &second) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            fixture->account, fixture->asset_id,
            (lxp_u128){0U, 5000000U}, 3U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(
            second, fixture->asset_id,
            (lxp_u128){0U, 7000000U}, 4U) != LXP_OK ||
        lxp_state_store_init(&fixture->state,
                             TEST_LAST_SEQUENCE + 1U) != LXP_OK) {
        report_stage_failure("account fixture initialization");
        return 1;
    }
    fixture->state_initialized = true;
    if (lxp_state_store_bind_accounts(&fixture->state,
                                      &fixture->accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&fixture->state) != LXP_OK ||
        lxp_kernel_create(&fixture->kernel, &fixture->state,
                          &fixture->journal, &parameters, 1U) != LXP_OK ||
        lxp_state_root(&fixture->kernel,
                       fixture->kernel.current_state_root) != LXP_OK) {
        report_stage_failure("account fixture state root");
        return 1;
    }
    for (index = 0U; index < 2U; ++index) {
        if (build_activity(fixture, index,
                           (uint64_t)index + 1U, arena) != 0 ||
            lxp_merkle_leaf_hash(
                fixture->canonical_activity[index],
                fixture->canonical_activity_length[index],
                activity_hashes[index]) != LXP_OK) {
            report_stage_failure("activity fixture");
            return 1;
        }
    }
    if (lxp_merkle_proof_generate(
            (const uint8_t (*)[32])activity_hashes, 2U, 0U, arena,
            &fixture->activity_proof[0], activity_root) != LXP_OK ||
        lxp_merkle_proof_generate(
            (const uint8_t (*)[32])activity_hashes, 2U, 1U, arena,
            &fixture->activity_proof[1], activity_root) != LXP_OK) {
        report_stage_failure("activity proof fixture");
        return 1;
    }
    first_resulting_root[0] = 0x5aU;
    if (build_receipt(fixture, 0U, TEST_FIRST_SEQUENCE,
                      fixture->initial_anchor, first_resulting_root,
                      activity_root, arena) != 0 ||
        build_receipt(fixture, 1U, TEST_LAST_SEQUENCE,
                      first_resulting_root,
                      fixture->kernel.current_state_root,
                      activity_root, arena) != 0) {
        report_stage_failure("receipt fixture");
        return 1;
    }
    for (index = 0U; index < 2U; ++index) {
        if (lxp_merkle_leaf_hash(
                fixture->canonical_receipt[index],
                fixture->canonical_receipt_length[index],
                receipt_hashes[index]) != LXP_OK) {
            report_stage_failure("receipt leaf fixture");
            return 1;
        }
    }
    if (lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, 2U, 0U, arena,
            &fixture->receipt_proof[0], receipt_root) != LXP_OK ||
        lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, 2U, 1U, arena,
            &fixture->receipt_proof[1], receipt_root) != LXP_OK) {
        report_stage_failure("receipt proof fixture");
        return 1;
    }
    for (index = 0U; index < 2U; ++index) {
        activity_spans[index] = (lxp_byte_span){
            fixture->canonical_activity[index],
            fixture->canonical_activity_length[index]};
        receipt_spans[index] = (lxp_byte_span){
            fixture->canonical_receipt[index],
            fixture->canonical_receipt_length[index]};
        if (lxp_receipt_decode(receipt_spans[index].bytes,
                               receipt_spans[index].length, true,
                               &decoded_receipts[index]) != LXP_OK ||
            lxp_programs_project_receipt_events(
                &decoded_receipts[index], arena,
                &event_spans[index]) != LXP_OK) {
            report_stage_failure("receipt event projection fixture");
            return 1;
        }
    }
    if (lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){
                activity_spans, 2U, receipt_spans, 2U, event_spans, 2U,
                NULL, 0U, NULL, 0U},
            arena, &roots) != LXP_OK ||
        lxp_ct_memcmp(roots.activity_merkle_root, activity_root, 32U) != 0 ||
        lxp_ct_memcmp(roots.receipt_merkle_root, receipt_root, 32U) != 0) {
        report_stage_failure("batch root fixture");
        return 1;
    }
    (void)memset(&fixture->authorization, 0,
                 sizeof(fixture->authorization));
    (void)memcpy(fixture->authorization.public_key,
                 sequencer_public, 32U);
    (void)memcpy(fixture->authorization.sequencer_id,
                 sequencer_public, 32U);
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
    (void)memcpy(header.previous_state_root,
                 fixture->initial_anchor, 32U);
    (void)memcpy(header.resulting_state_root,
                 fixture->kernel.current_state_root, 32U);
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
                 fixture->authorization.sequencer_id, 32U);
    if (lxp_batch_sign(&header, fixture->sequencer_private,
                       &fixture->authorization,
                       fixture->header_signature, arena) != LXP_OK) {
        report_stage_failure("batch signature fixture");
        return 1;
    }
    mark = lxp_arena_mark(arena);
    if (lxp_batch_header_encode(&header, arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(fixture->canonical_header)) {
        report_stage_failure("batch header fixture");
        return 1;
    }
    (void)memcpy(fixture->canonical_header, encoded.bytes, encoded.length);
    if (lxp_arena_reset(arena, mark) != LXP_OK) {
        report_stage_failure("batch header arena reset");
        return 1;
    }
    if (lxp_daemon_account_evidence_build(
            &fixture->kernel, TEST_NETWORK_ID, fixture->account_id,
            fixture->receipt_digest[1], TEST_TIMESTAMP_MS,
            (lxp_byte_span){fixture->canonical_receipt[1],
                            fixture->canonical_receipt_length[1]},
            &fixture->receipt_proof[1], &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                            sizeof(fixture->canonical_header)},
            fixture->header_signature, arena,
            &fixture->account_evidence) != LXP_OK) {
        report_stage_failure("nested account fixture");
        return 1;
    }
    return 0;
}

static int build_finality(test_fixture *fixture, lxp_arena *arena)
{
    static const uint8_t validity_proof[] = {0x56U, 0x50U, 0x31U, 0x01U};
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation attestations[3];
    lxp_byte_span payload;
    lxp_byte_span proof;
    size_t index;
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    if (lxp_batch_header_decode(
            fixture->canonical_header, sizeof(fixture->canonical_header),
            &checkpoint.header) != LXP_OK)
        return 1;
    checkpoint.validity_proof =
        (lxp_byte_span){validity_proof, sizeof(validity_proof)};
    if (lxp_guarantor_set_init(&fixture->bonded_set) != LXP_OK)
        return 1;
    for (index = 0U; index < 3U; ++index) {
        lxp_guarantor_bond_state bond;
        lxp_guarantor_ctx *guarantor = &fixture->guarantors[index];
        (void)memset(guarantor, 0, sizeof(*guarantor));
        guarantor->guarantor_id[0] = (uint8_t)(index + 1U);
        guarantor->ready_to_sign = true;
        guarantor->possesses_availability = true;
        guarantor->bond_view.bonded = true;
        guarantor->protocol_version = LXP_PROTOCOL_VERSION;
        guarantor->network_id = TEST_NETWORK_ID;
        guarantor->paxeer_chain_id = TEST_PAXEER_CHAIN_ID;
        guarantor->paxeer_settlement_contract[0] = 0xc1U;
        if (secp_key_pair((uint8_t)(index + 1U),
                          guarantor->paxeer_private_key,
                          guarantor->paxeer_public_key) != 0)
            return 1;
        (void)memset(&bond, 0, sizeof(bond));
        bond.guarantor_id[0] = (uint8_t)(index + 1U);
        (void)memcpy(bond.public_key,
                     guarantor->paxeer_public_key, 33U);
        bond.bond_amount = (lxp_u128){0U, 1000U};
        bond.joined_epoch = 1U;
        bond.active = true;
        if (lxp_guarantor_set_apply(&fixture->bonded_set, index + 1U,
                                    true, &bond) != LXP_OK ||
            lxp_guarantor_attest(
                guarantor, &checkpoint, true, true,
                TEST_TIMESTAMP_MS + 50U + (uint64_t)index,
                arena, &attestations[index]) != LXP_OK)
            return 1;
    }
    if (lxp_guarantor_cert_assemble(
            &checkpoint, attestations, 3U, 2U,
            &fixture->certificate) != LXP_OK)
        return 1;
    (void)memset(&fixture->requirements, 0,
                 sizeof(fixture->requirements));
    fixture->requirements.checkpoint_epoch = checkpoint.header.epoch;
    fixture->requirements.challenge_window_end_ms = TEST_TIMESTAMP_MS + 10U;
    fixture->requirements.checkpoint_deadline_ms = TEST_TIMESTAMP_MS + 500U;
    fixture->requirements.now_ms = TEST_TIMESTAMP_MS + 100U;
    fixture->requirements.threshold = 2U;
    fixture->requirements.minimum_bond = (lxp_u128){0U, 500U};
    fixture->requirements.availability_challenges_answered = true;
    (void)memset(&fixture->settlement, 0, sizeof(fixture->settlement));
    fixture->settlement.paxeer_chain_id = TEST_PAXEER_CHAIN_ID;
    fixture->settlement.settlement_contract[0] = 0xc1U;
    fixture->settlement.transaction_id[0] = 0xd1U;
    fixture->settlement.observed_block_number = 9001U;
    fixture->settlement.observed_at_ms = TEST_TIMESTAMP_MS + 200U;
    if (lxp_checkpoint_certificate_hash(
            &fixture->certificate.checkpoint, arena,
            fixture->checkpoint_id) != LXP_OK)
        return 1;
    (void)memcpy(fixture->settlement.checkpoint_id,
                 fixture->checkpoint_id, 32U);
    if (lxp_daemon_finality_evidence_encode(
            &fixture->certificate, &fixture->bonded_set,
            &fixture->requirements, 0U, &fixture->settlement,
            arena, &payload, &proof) != LXP_OK ||
        payload.length > sizeof(fixture->checkpoint_payload) ||
        proof.length == 0U ||
        proof.length > sizeof(fixture->finality_proof))
        return 1;
    fixture->checkpoint_payload_length = payload.length;
    fixture->finality_proof_length = proof.length;
    (void)memcpy(fixture->checkpoint_payload,
                 payload.bytes, payload.length);
    (void)memcpy(fixture->finality_proof, proof.bytes, proof.length);
    return 0;
}

static lxp_result open_store(
    const char *path, test_fixture *fixture,
    finality_authority *authority, bool allow_initialize,
    lxp_log *log, lxp_daemon_evidence_store *store,
    lxp_arena *arena, uint8_t *arena_memory)
{
    lxp_result status = lxp_arena_init(
        arena, arena_memory, TEST_ARENA_BYTES);
    if (status == LXP_OK) status = lxp_log_open(log, path);
    if (status == LXP_OK)
        status = lxp_daemon_evidence_open(
            store, log, TEST_NETWORK_ID, &fixture->authorization,
            fixture->initial_anchor, allow_initialize,
            verify_finality_authority,
            authority, arena);
    return status;
}

static int publish_child(const char *path, test_fixture *fixture,
                         finality_authority *authority, unsigned kind)
{
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_arena arena;
    lxp_log log;
    lxp_daemon_evidence_store store;
    uint8_t digest[32];
    lxp_daemon_finality_evidence finality;
    lxp_result status;
    if (arena_memory == NULL) return 1;
    status = open_store(path, fixture, authority, true, &log, &store,
                        &arena, arena_memory);
    if (status == LXP_OK && kind == LXP_DAEMON_EVIDENCE_ACCOUNT)
        status = lxp_daemon_account_evidence_publish(
            &store, &fixture->account_evidence, &arena, digest);
    else if (status == LXP_OK && kind == LXP_DAEMON_EVIDENCE_ACTIVITY)
        status = lxp_daemon_activity_evidence_publish(
            &store,
            (lxp_byte_span){fixture->canonical_activity[0],
                fixture->canonical_activity_length[0]},
            &fixture->activity_proof[0],
            (lxp_byte_span){fixture->canonical_receipt[0],
                fixture->canonical_receipt_length[0]},
            &fixture->receipt_proof[0], &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena, digest);
    else if (status == LXP_OK && kind == LXP_DAEMON_EVIDENCE_FINALITY)
        status = lxp_daemon_finality_evidence_register(
            &store,
            (lxp_byte_span){fixture->checkpoint_payload,
                fixture->checkpoint_payload_length},
            (lxp_byte_span){fixture->finality_proof,
                fixture->finality_proof_length},
            &arena, &finality);
    if (status != LXP_OK) return 1;
    _exit((int)(80U + kind));
}

static int publish_then_crash(const char *path, test_fixture *fixture,
                              finality_authority *authority, unsigned kind)
{
    pid_t child = fork();
    int status;
    if (child < 0) return 1;
    if (child == 0)
        _exit(publish_child(path, fixture, authority, kind));
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status) ||
        WEXITSTATUS(status) != (int)(80U + kind))
        return 1;
    return 0;
}

static int reject_account_mutations(
    lxp_daemon_evidence_store *store, test_fixture *fixture,
    lxp_arena *arena)
{
    lxp_daemon_account_evidence changed;
    uint64_t count = store->record_count;
    uint64_t offset = store->log->write_offset;
    uint8_t digest[32];
    size_t asset_offset = 2U + fixture->account->name_length + 1U + 16U;
#define REJECT_ACCOUNT_MUTATION(statement)                                      \
    do {                                                                        \
        changed = fixture->account_evidence;                                    \
        statement;                                                              \
        if (lxp_daemon_account_evidence_publish(                                \
                store, &changed, arena, digest) == LXP_OK ||                    \
            store->record_count != count || store->log->write_offset != offset) \
            return 1;                                                           \
    } while (0)
    REJECT_ACCOUNT_MUTATION(changed.account_id[0] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.account_leaf_value[asset_offset] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.account_leaf_value[asset_offset - 1U] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.account_root[0] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.universal_root[0] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.resulting_state_root[0] ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.account_proof.depth = 0U);
    REJECT_ACCOUNT_MUTATION(changed.account_tree_proof.leaf_index ^= 1U);
    REJECT_ACCOUNT_MUTATION(changed.universal_root_proof.siblings[0][0] ^= 1U);
#undef REJECT_ACCOUNT_MUTATION
    return 0;
}

static int reject_activity_mutations(
    lxp_daemon_evidence_store *store, test_fixture *fixture,
    lxp_arena *arena)
{
    uint64_t count = store->record_count;
    uint64_t offset = store->log->write_offset;
    uint8_t digest[32];
    uint8_t changed_activity[LXP_MAX_ACTIVITY_BYTES];
    uint8_t changed_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t changed_signature[64];
    lxp_merkle_proof changed_proof;
#define REJECT_ACTIVITY(activity_span, activity_proof_value, receipt_span,      \
                        receipt_proof_value, header_span, signature_value)      \
    do {                                                                        \
        if (lxp_daemon_activity_evidence_publish(                               \
                store, activity_span, activity_proof_value, receipt_span,       \
                receipt_proof_value, &fixture->authorization, header_span,      \
                signature_value, arena, digest) == LXP_OK ||                    \
            store->record_count != count || store->log->write_offset != offset) \
            return 1;                                                           \
    } while (0)
    (void)memcpy(changed_activity, fixture->canonical_activity[0],
                 fixture->canonical_activity_length[0]);
    changed_activity[fixture->canonical_activity_length[0] - 1U] ^= 1U;
    REJECT_ACTIVITY(
        ((lxp_byte_span){changed_activity,
                         fixture->canonical_activity_length[0]}),
        &fixture->activity_proof[0],
        ((lxp_byte_span){fixture->canonical_receipt[0],
                         fixture->canonical_receipt_length[0]}),
        &fixture->receipt_proof[0],
        ((lxp_byte_span){fixture->canonical_header,
                         sizeof(fixture->canonical_header)}),
        fixture->header_signature);
    REJECT_ACTIVITY(
        ((lxp_byte_span){fixture->canonical_activity[0],
                         fixture->canonical_activity_length[0]}),
        &fixture->activity_proof[0],
        ((lxp_byte_span){fixture->canonical_receipt[1],
                         fixture->canonical_receipt_length[1]}),
        &fixture->receipt_proof[1],
        ((lxp_byte_span){fixture->canonical_header,
                         sizeof(fixture->canonical_header)}),
        fixture->header_signature);
    changed_proof = fixture->receipt_proof[0];
    changed_proof.siblings[0][0] ^= 1U;
    REJECT_ACTIVITY(
        ((lxp_byte_span){fixture->canonical_activity[0],
                         fixture->canonical_activity_length[0]}),
        &fixture->activity_proof[0],
        ((lxp_byte_span){fixture->canonical_receipt[0],
                         fixture->canonical_receipt_length[0]}),
        &changed_proof,
        ((lxp_byte_span){fixture->canonical_header,
                         sizeof(fixture->canonical_header)}),
        fixture->header_signature);
    (void)memcpy(changed_header, fixture->canonical_header,
                 sizeof(changed_header));
    changed_header[24] ^= 1U;
    REJECT_ACTIVITY(
        ((lxp_byte_span){fixture->canonical_activity[0],
                         fixture->canonical_activity_length[0]}),
        &fixture->activity_proof[0],
        ((lxp_byte_span){fixture->canonical_receipt[0],
                         fixture->canonical_receipt_length[0]}),
        &fixture->receipt_proof[0],
        ((lxp_byte_span){changed_header, sizeof(changed_header)}),
        fixture->header_signature);
    (void)memcpy(changed_signature, fixture->header_signature,
                 sizeof(changed_signature));
    changed_signature[0] ^= 1U;
    REJECT_ACTIVITY(
        ((lxp_byte_span){fixture->canonical_activity[0],
                         fixture->canonical_activity_length[0]}),
        &fixture->activity_proof[0],
        ((lxp_byte_span){fixture->canonical_receipt[0],
                         fixture->canonical_receipt_length[0]}),
        &fixture->receipt_proof[0],
        ((lxp_byte_span){fixture->canonical_header,
                         sizeof(fixture->canonical_header)}),
        changed_signature);
#undef REJECT_ACTIVITY
    return 0;
}

static int reject_finality_mutations(
    lxp_daemon_evidence_store *store, test_fixture *fixture,
    finality_authority *authority, lxp_arena *arena)
{
    lxp_guarantor_cert changed_certificate;
    lxp_guarantor_set changed_set;
    lxp_daemon_settlement_registration_evidence changed_settlement;
    uint8_t unused_private[32];
    uint8_t unused_public[33];
    lxp_byte_span payload;
    lxp_byte_span proof;
    lxp_daemon_finality_evidence evidence;
    uint64_t count = store->record_count;
    uint64_t offset = store->log->write_offset;
    uint64_t calls = authority->calls;
    size_t mark;
#define REJECT_FINALITY(label, certificate_value, set_value, settlement_value)  \
    do {                                                                        \
        lxp_result encode_status;                                               \
        lxp_result register_status = LXP_OK;                                    \
        mark = lxp_arena_mark(arena);                                           \
        encode_status = lxp_daemon_finality_evidence_encode(                    \
            certificate_value, set_value, &fixture->requirements, 0U,          \
            settlement_value, arena, &payload, &proof);                        \
        if (encode_status == LXP_OK)                                            \
            register_status = lxp_daemon_finality_evidence_register(            \
                store, payload, proof, arena, &evidence);                       \
        if (encode_status != LXP_OK || register_status == LXP_OK ||             \
            store->record_count != count || store->log->write_offset != offset) \
        {                                                                       \
            (void)fprintf(stderr,                                               \
                "lxp_test_finality_evidence: finality mutation %s "             \
                "encode=%d register=%d records=%llu offset=%llu\n",            \
                label, (int)encode_status, (int)register_status,                \
                (unsigned long long)store->record_count,                        \
                (unsigned long long)store->log->write_offset);                  \
            return 1;                                                           \
        }                                                                       \
        if (lxp_arena_reset(arena, mark) != LXP_OK) return 1;                   \
    } while (0)
    changed_certificate = fixture->certificate;
    (void)memset(changed_certificate.attestations[0].signature, 0, 64U);
    changed_certificate.attestations[0].signature_v = 27U;
    (void)memset(changed_certificate.attestations[1].signature, 0, 64U);
    changed_certificate.attestations[1].signature_v = 27U;
    REJECT_FINALITY("unsigned", &changed_certificate, &fixture->bonded_set,
                    &fixture->settlement);
    changed_certificate = fixture->certificate;
    changed_certificate.checkpoint.header.network_id++;
    changed_settlement = fixture->settlement;
    if (lxp_checkpoint_certificate_hash(
            &changed_certificate.checkpoint, arena,
            changed_settlement.checkpoint_id) != LXP_OK)
        return 1;
    REJECT_FINALITY("wrong network", &changed_certificate,
                    &fixture->bonded_set,
                    &changed_settlement);
    changed_set = fixture->bonded_set;
    if (secp_key_pair(9U, unused_private, unused_public) != 0) return 1;
    (void)memcpy(changed_set.records[0].public_key,
                 unused_public, sizeof(unused_public));
    (void)memcpy(
        changed_set.records[0].signer_authorizations[0].public_key,
        unused_public, sizeof(unused_public));
    if (secp_key_pair(10U, unused_private, unused_public) != 0) return 1;
    (void)memcpy(changed_set.records[1].public_key,
                 unused_public, sizeof(unused_public));
    (void)memcpy(
        changed_set.records[1].signer_authorizations[0].public_key,
        unused_public, sizeof(unused_public));
    REJECT_FINALITY("wrong bonded set", &fixture->certificate, &changed_set,
                    &fixture->settlement);
    changed_settlement = fixture->settlement;
    changed_settlement.paxeer_chain_id++;
    REJECT_FINALITY("wrong settlement chain", &fixture->certificate,
                    &fixture->bonded_set,
                    &changed_settlement);
    changed_settlement = fixture->settlement;
    changed_settlement.settlement_contract[0] ^= 1U;
    REJECT_FINALITY("wrong settlement contract", &fixture->certificate,
                    &fixture->bonded_set,
                    &changed_settlement);
    changed_settlement = fixture->settlement;
    changed_settlement.transaction_id[0] ^= 1U;
    REJECT_FINALITY("wrong settlement transaction", &fixture->certificate,
                    &fixture->bonded_set,
                    &changed_settlement);
    if (authority->calls < calls + 6U) {
        (void)fprintf(stderr,
            "lxp_test_finality_evidence: finality authority calls=%llu "
            "expected-at-least=%llu\n",
            (unsigned long long)authority->calls,
            (unsigned long long)(calls + 6U));
        return 1;
    }
#undef REJECT_FINALITY
    return 0;
}

static int verify_recovered(
    lxp_daemon_evidence_store *store, test_fixture *fixture,
    lxp_arena *arena, unsigned expected_records,
    uint8_t expected_finality_digest[32])
{
    lxp_daemon_account_evidence account;
    lxp_daemon_activity_evidence activity;
    lxp_daemon_finality_evidence finality;
    if (store->record_count != expected_records)
        return 1;
    if (expected_records >= 1U &&
        (lxp_daemon_account_evidence_lookup(
             store, fixture->account_id,
             fixture->kernel.current_state_root,
             arena, &account) != LXP_OK ||
         account.account_leaf_value_length !=
             fixture->account_evidence.account_leaf_value_length ||
         memcmp(account.account_leaf_value,
                fixture->account_evidence.account_leaf_value,
                account.account_leaf_value_length) != 0 ||
         memcmp(account.resulting_state_root,
                fixture->kernel.current_state_root, 32U) != 0))
        return 1;
    if (expected_records >= 3U &&
        (lxp_daemon_activity_evidence_lookup(
             store, fixture->activity_id[0], arena, &activity) != LXP_OK ||
         activity.canonical_activity.length !=
             fixture->canonical_activity_length[0] ||
         memcmp(activity.canonical_activity.bytes,
                fixture->canonical_activity[0],
                activity.canonical_activity.length) != 0 ||
         activity.canonical_receipt.length !=
             fixture->canonical_receipt_length[0] ||
         memcmp(activity.canonical_receipt.bytes,
                fixture->canonical_receipt[0],
                activity.canonical_receipt.length) != 0))
        return 1;
    if (expected_records >= 4U &&
        (lxp_daemon_finality_evidence_lookup(
             store, fixture->checkpoint_id, 0U,
             arena, &finality) != LXP_OK ||
         finality.checkpoint_payload.length !=
             fixture->checkpoint_payload_length ||
         finality.finality_proof.length !=
             fixture->finality_proof_length ||
         finality.finality_proof.length == 0U ||
         finality.resulting_registration_count != 1U ||
         store->registry.registration_count != 1U ||
         memcmp(finality.checkpoint_payload.bytes,
                fixture->checkpoint_payload,
                finality.checkpoint_payload.length) != 0 ||
         memcmp(finality.finality_proof.bytes,
                fixture->finality_proof,
                finality.finality_proof.length) != 0 ||
         (expected_finality_digest != NULL &&
          memcmp(finality.record_digest,
                 expected_finality_digest, 32U) != 0)))
        return 1;
    return 0;
}

static bool merkle_proof_equal(const lxp_merkle_proof *left,
                               const lxp_merkle_proof *right)
{
    return left != NULL && right != NULL &&
        left->leaf_index == right->leaf_index &&
        left->leaf_count == right->leaf_count &&
        left->depth == right->depth &&
        lxp_ct_memcmp(left->siblings, right->siblings,
                      (size_t)left->depth * 32U) == 0;
}

static int verify_multi_activity_records(
    const lxp_daemon_evidence_store *store,
    const lxp_daemon_receipt_authority_store *receipt_authority,
    const test_fixture *fixture, lxp_arena *arena)
{
    size_t index;
    if (store->record_count != 2U ||
        receipt_authority->record_count != 2U)
        return 1;
    for (index = 0U; index < 2U; ++index) {
        lxp_daemon_activity_evidence activity;
        lxp_daemon_receipt_evidence receipt;
        size_t mark = lxp_arena_mark(arena);
        if (lxp_daemon_activity_evidence_lookup(
                store, fixture->activity_id[index], arena,
                &activity) != LXP_OK ||
            lxp_daemon_receipt_authority_lookup(
                receipt_authority, fixture->receipt_digest[index], arena,
                &receipt) != LXP_OK ||
            activity.global_sequence != TEST_FIRST_SEQUENCE + index ||
            activity.batch_number != TEST_BATCH_NUMBER ||
            activity.canonical_activity.length !=
                fixture->canonical_activity_length[index] ||
            lxp_ct_memcmp(activity.canonical_activity.bytes,
                          fixture->canonical_activity[index],
                          activity.canonical_activity.length) != 0 ||
            activity.canonical_receipt.length !=
                fixture->canonical_receipt_length[index] ||
            lxp_ct_memcmp(activity.canonical_receipt.bytes,
                          fixture->canonical_receipt[index],
                          activity.canonical_receipt.length) != 0 ||
            !merkle_proof_equal(&activity.activity_proof,
                                &fixture->activity_proof[index]) ||
            !merkle_proof_equal(&activity.receipt_proof,
                                &fixture->receipt_proof[index]) ||
            activity.activity_proof.leaf_count != 2U ||
            activity.activity_proof.leaf_index != index ||
            activity.receipt_proof.leaf_count != 2U ||
            activity.receipt_proof.leaf_index != index ||
            activity.signed_header.canonical_header.length !=
                sizeof(fixture->canonical_header) ||
            lxp_ct_memcmp(activity.signed_header.canonical_header.bytes,
                          fixture->canonical_header,
                          sizeof(fixture->canonical_header)) != 0 ||
            lxp_ct_memcmp(activity.signed_header.signature,
                          fixture->header_signature, 64U) != 0 ||
            receipt.canonical_header.length !=
                sizeof(fixture->canonical_header) ||
            lxp_ct_memcmp(receipt.canonical_header.bytes,
                          activity.signed_header.canonical_header.bytes,
                          sizeof(fixture->canonical_header)) != 0 ||
            lxp_ct_memcmp(receipt.header_signature,
                          activity.signed_header.signature, 64U) != 0 ||
            !merkle_proof_equal(&receipt.receipt_proof,
                                &activity.receipt_proof) ||
            lxp_ct_memcmp(
                activity.signed_header.authorization.sequencer_id,
                receipt_authority->authorization.sequencer_id, 32U) != 0 ||
            lxp_ct_memcmp(
                activity.signed_header.authorization.public_key,
                receipt_authority->authorization.public_key, 32U) != 0) {
            (void)lxp_arena_reset(arena, mark);
            return 1;
        }
        if (lxp_arena_reset(arena, mark) != LXP_OK) return 1;
    }
    return 0;
}

static int partial_multi_activity_child(
    const char *canonical_path, const char *authority_path,
    const char *evidence_path, test_fixture *fixture,
    finality_authority *authority)
{
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_arena arena;
    lxp_log canonical_log;
    lxp_log authority_log;
    lxp_log evidence_log;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_daemon_evidence_store evidence_store;
    uint8_t digest[32];
    size_t index;
    lxp_result status;
    if (arena_memory == NULL ||
        lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_log_open(&canonical_log, canonical_path) != LXP_OK)
        return 1;
    status = LXP_OK;
    for (index = 0U; status == LXP_OK && index < 2U; ++index) {
        status = lxp_log_append(
            &canonical_log, LXP_LOG_ACTIVITY,
            TEST_FIRST_SEQUENCE + index,
            fixture->canonical_activity[index],
            (uint32_t)fixture->canonical_activity_length[index], NULL);
        if (status == LXP_OK)
            status = lxp_log_append(
                &canonical_log, LXP_LOG_RECEIPT,
                TEST_FIRST_SEQUENCE + index,
                fixture->canonical_receipt[index],
                (uint32_t)fixture->canonical_receipt_length[index], NULL);
    }
    if (status == LXP_OK)
        status = lxp_log_write_boundary(&canonical_log);
    if (status == LXP_OK)
        status = lxp_log_open(&authority_log, authority_path);
    if (status == LXP_OK)
        status = lxp_daemon_receipt_authority_open(
            &receipt_authority, &authority_log, &fixture->authorization);
    for (index = 0U; status == LXP_OK && index < 2U; ++index)
        status = lxp_daemon_receipt_authority_append(
            &receipt_authority, fixture->canonical_receipt[index],
            fixture->canonical_receipt_length[index],
            fixture->canonical_header, sizeof(fixture->canonical_header),
            fixture->header_signature, &fixture->receipt_proof[index],
            &arena);
    if (status == LXP_OK)
        status = lxp_log_open(&evidence_log, evidence_path);
    if (status == LXP_OK)
        status = lxp_daemon_evidence_open(
            &evidence_store, &evidence_log, TEST_NETWORK_ID,
            &fixture->authorization, fixture->initial_anchor, true,
            verify_finality_authority, authority, &arena);
    if (status == LXP_OK)
        status = lxp_daemon_activity_evidence_publish(
            &evidence_store,
            (lxp_byte_span){fixture->canonical_activity[0],
                fixture->canonical_activity_length[0]},
            &fixture->activity_proof[0],
            (lxp_byte_span){fixture->canonical_receipt[0],
                fixture->canonical_receipt_length[0]},
            &fixture->receipt_proof[0], &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena, digest);
    return status == LXP_OK ? 0 : 1;
}

static int recover_multi_activity_batch(
    test_fixture *fixture, finality_authority *authority)
{
    char canonical_directory[] = "/tmp/lxp-evidence-canonical-XXXXXX";
    char authority_directory[] = "/tmp/lxp-evidence-authority-XXXXXX";
    char evidence_directory[] = "/tmp/lxp-evidence-batch-XXXXXX";
    char canonical_path[160] = {0};
    char authority_path[160] = {0};
    char evidence_path[160] = {0};
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_arena arena;
    lxp_log canonical_log;
    lxp_log authority_log;
    lxp_log evidence_log;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_daemon_evidence_store evidence_store;
    uint64_t recovered_end;
    pid_t child;
    int child_status;
    bool canonical_open = false;
    bool authority_open = false;
    bool evidence_open = false;
    int result = 1;
    if (arena_memory == NULL ||
        mkdtemp(canonical_directory) == NULL ||
        mkdtemp(authority_directory) == NULL ||
        mkdtemp(evidence_directory) == NULL ||
        snprintf(canonical_path, sizeof(canonical_path), "%s/%020u.lxp",
                 canonical_directory, 0U) < 0 ||
        snprintf(authority_path, sizeof(authority_path), "%s/%020u.lxp",
                 authority_directory, 0U) < 0 ||
        snprintf(evidence_path, sizeof(evidence_path), "%s/%020u.lxp",
                 evidence_directory, 0U) < 0 ||
        lxp_log_segment_create(&canonical_log, canonical_directory, 0U,
                               TEST_LOG_BYTES) != LXP_OK ||
        lxp_log_close(&canonical_log) != LXP_OK ||
        lxp_log_segment_create(&authority_log, authority_directory, 0U,
                               TEST_LOG_BYTES) != LXP_OK ||
        lxp_log_close(&authority_log) != LXP_OK ||
        lxp_log_segment_create(&evidence_log, evidence_directory, 0U,
                               TEST_LOG_BYTES) != LXP_OK ||
        lxp_log_close(&evidence_log) != LXP_OK)
        goto cleanup;
    child = fork();
    if (child < 0) goto cleanup;
    if (child == 0)
        _exit(partial_multi_activity_child(
            canonical_path, authority_path, evidence_path, fixture,
            authority) == 0 ? 93 : 1);
    if (waitpid(child, &child_status, 0) != child ||
        !WIFEXITED(child_status) || WEXITSTATUS(child_status) != 93)
        goto cleanup;
    if (lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_log_open(&canonical_log, canonical_path) != LXP_OK)
        goto cleanup;
    canonical_open = true;
    if (lxp_log_recover_complete_records(
            &canonical_log, NULL, NULL) != LXP_OK ||
        lxp_log_open(&authority_log, authority_path) != LXP_OK)
        goto cleanup;
    authority_open = true;
    if (lxp_daemon_receipt_authority_open(
            &receipt_authority, &authority_log,
            &fixture->authorization) != LXP_OK ||
        lxp_log_open(&evidence_log, evidence_path) != LXP_OK)
        goto cleanup;
    evidence_open = true;
    if (lxp_daemon_evidence_open(
            &evidence_store, &evidence_log, TEST_NETWORK_ID,
            &fixture->authorization, fixture->initial_anchor, false,
            verify_finality_authority, authority, &arena) != LXP_OK ||
        evidence_store.record_count != 1U ||
        lxp_daemon_activity_evidence_recover_batch(
            &evidence_store, &canonical_log, &receipt_authority,
            &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena) != LXP_OK ||
        verify_multi_activity_records(
            &evidence_store, &receipt_authority, fixture, &arena) != 0)
        goto cleanup;
    recovered_end = evidence_log.write_offset;
    if (lxp_daemon_activity_evidence_recover_batch(
            &evidence_store, &canonical_log, &receipt_authority,
            &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena) != LXP_OK ||
        evidence_store.record_count != 2U ||
        evidence_log.write_offset != recovered_end ||
        lxp_log_close(&evidence_log) != LXP_OK ||
        lxp_log_close(&authority_log) != LXP_OK ||
        lxp_log_close(&canonical_log) != LXP_OK)
        goto cleanup;
    evidence_open = false;
    authority_open = false;
    canonical_open = false;
    if (lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_log_open(&canonical_log, canonical_path) != LXP_OK)
        goto cleanup;
    canonical_open = true;
    if (lxp_log_recover_complete_records(
            &canonical_log, NULL, NULL) != LXP_OK ||
        lxp_log_open(&authority_log, authority_path) != LXP_OK)
        goto cleanup;
    authority_open = true;
    if (lxp_daemon_receipt_authority_open(
            &receipt_authority, &authority_log,
            &fixture->authorization) != LXP_OK ||
        lxp_log_open(&evidence_log, evidence_path) != LXP_OK)
        goto cleanup;
    evidence_open = true;
    if (lxp_daemon_evidence_open(
            &evidence_store, &evidence_log, TEST_NETWORK_ID,
            &fixture->authorization, fixture->initial_anchor, false,
            verify_finality_authority, authority, &arena) != LXP_OK ||
        evidence_log.write_offset != recovered_end ||
        verify_multi_activity_records(
            &evidence_store, &receipt_authority, fixture, &arena) != 0)
        goto cleanup;
    result = 0;

cleanup:
    if (evidence_open) (void)lxp_log_close(&evidence_log);
    if (authority_open) (void)lxp_log_close(&authority_log);
    if (canonical_open) (void)lxp_log_close(&canonical_log);
    if (evidence_path[0] != '\0') (void)unlink(evidence_path);
    if (authority_path[0] != '\0') (void)unlink(authority_path);
    if (canonical_path[0] != '\0') (void)unlink(canonical_path);
    if (evidence_directory[0] != '\0') (void)rmdir(evidence_directory);
    if (authority_directory[0] != '\0') (void)rmdir(authority_directory);
    if (canonical_directory[0] != '\0') (void)rmdir(canonical_directory);
    free(arena_memory);
    return result;
}

static int checkpoint_account_roundtrip(
    const char *path, test_fixture *fixture,
    finality_authority *authority)
{
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    uint8_t *expected_value = NULL;
    uint8_t *expected_proof = NULL;
    size_t expected_value_length = 0U;
    size_t expected_proof_length = 0U;
    lxp_log log;
    bool log_open = false;
    unsigned pass;
    int result = 1;
    if (arena_memory == NULL) return 1;
    for (pass = 0U; pass < 2U; ++pass) {
        lxp_arena arena;
        lxp_daemon_evidence_store store;
        lxp_daemon_account_evidence account;
        lxp_daemon_finality_evidence checkpoint;
        lxp_kernel changed_latest;
        lxp_state_store changed_latest_state;
        lxp_byte_span value;
        lxp_byte_span proof;
        uint8_t changed_id[32];
        if (open_store(path, fixture, authority, false,
                       &log, &store, &arena, arena_memory) != LXP_OK)
            goto cleanup;
        log_open = true;
        if (lxp_daemon_account_evidence_lookup_batch(
                &store, fixture->account_id, TEST_BATCH_NUMBER,
                &arena, &account) != LXP_OK ||
            lxp_daemon_finality_evidence_lookup(
                &store, fixture->checkpoint_id, 0U,
                &arena, &checkpoint) != LXP_OK ||
            lxp_ct_memcmp(account.resulting_state_root,
                          checkpoint.resulting_state_root, 32U) != 0 ||
            lxp_daemon_account_evidence_wire_encode(
                &store, NULL, NULL, TEST_NETWORK_ID,
                fixture->account_id, 3U, 0U,
                fixture->checkpoint_id, &arena,
                &value, &proof) != LXP_OK || proof.length == 0U)
            goto cleanup;
        if (pass == 0U) {
            expected_value = (uint8_t *)malloc(value.length);
            expected_proof = (uint8_t *)malloc(proof.length);
            if (expected_value == NULL || expected_proof == NULL)
                goto cleanup;
            expected_value_length = value.length;
            expected_proof_length = proof.length;
            (void)memcpy(expected_value, value.bytes, value.length);
            (void)memcpy(expected_proof, proof.bytes, proof.length);
        } else if (value.length != expected_value_length ||
                   proof.length != expected_proof_length ||
                   lxp_ct_memcmp(value.bytes, expected_value,
                                 value.length) != 0 ||
                   lxp_ct_memcmp(proof.bytes, expected_proof,
                                 proof.length) != 0)
            goto cleanup;
        if (lxp_daemon_account_evidence_wire_encode(
                &store, &account, &fixture->kernel, TEST_NETWORK_ID,
                fixture->account_id, 1U, 0U, NULL, &arena,
                &value, &proof) != LXP_OK)
            goto cleanup;
        changed_latest = fixture->kernel;
        (void)memset(&changed_latest_state, 0,
                     sizeof(changed_latest_state));
        changed_latest.state = &changed_latest_state;
        changed_latest_state.next_sequence =
            fixture->state.next_sequence + 1U;
        changed_latest.current_state_root[0] ^= 1U;
        if (lxp_daemon_account_evidence_wire_encode(
                &store, &account, &changed_latest, TEST_NETWORK_ID,
                fixture->account_id, 1U, 0U, NULL, &arena,
                &value, &proof) == LXP_OK)
            goto cleanup;
        (void)memcpy(changed_id, fixture->checkpoint_id, 32U);
        changed_id[0] ^= 1U;
        if (lxp_daemon_account_evidence_wire_encode(
                &store, NULL, NULL, TEST_NETWORK_ID,
                fixture->account_id, 3U, 0U, changed_id, &arena,
                &value, &proof) == LXP_OK)
            goto cleanup;
        if (lxp_daemon_account_evidence_wire_encode(
                &store, NULL, NULL, TEST_NETWORK_ID,
                fixture->account_id, 2U, TEST_BATCH_NUMBER + 1U,
                NULL, &arena,
                &value, &proof) == LXP_OK)
            goto cleanup;
        (void)memcpy(changed_id, fixture->account_id, 32U);
        changed_id[0] ^= 1U;
        if (lxp_daemon_account_evidence_wire_encode(
                &store, NULL, NULL, TEST_NETWORK_ID, changed_id,
                3U, 0U, fixture->checkpoint_id, &arena,
                &value, &proof) == LXP_OK ||
            lxp_log_close(&log) != LXP_OK)
            goto cleanup;
        log_open = false;
    }
    result = 0;

cleanup:
    if (log_open) (void)lxp_log_close(&log);
    free(expected_proof);
    free(expected_value);
    free(arena_memory);
    return result;
}

static int refuse_mismatched_checkpoint_account(
    test_fixture *fixture, bool change_resulting_root)
{
    char directory[] = "/tmp/lxp-mismatched-checkpoint-XXXXXX";
    char path[160] = {0};
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    test_fixture *alternative = calloc(1U, sizeof(*alternative));
    lxp_arena arena;
    lxp_log log;
    lxp_daemon_evidence_store store;
    lxp_daemon_finality_evidence finality;
    lxp_batch_header header;
    lxp_byte_span encoded;
    lxp_byte_span value;
    lxp_byte_span proof;
    finality_authority authority;
    uint8_t digest[32];
    size_t mark;
    bool log_open = false;
    int result = 1;
    if (arena_memory == NULL || alternative == NULL ||
        lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_batch_header_decode(
            fixture->canonical_header, sizeof(fixture->canonical_header),
            &header) != LXP_OK)
        goto cleanup;
    if (change_resulting_root)
        header.resulting_state_root[0] ^= 1U;
    else
        header.oracle_root[0] ^= 1U;
    mark = lxp_arena_mark(&arena);
    if (lxp_batch_header_encode(&header, &arena, &encoded) != LXP_OK ||
        encoded.length != sizeof(alternative->canonical_header))
        goto cleanup;
    (void)memcpy(alternative->canonical_header, encoded.bytes,
                 encoded.length);
    if (lxp_arena_reset(&arena, mark) != LXP_OK ||
        build_finality(alternative, &arena) != 0 ||
        mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/%020u.lxp",
                 directory, 0U) < 0 ||
        lxp_log_segment_create(&log, directory, 0U,
                               TEST_LOG_BYTES) != LXP_OK ||
        lxp_log_close(&log) != LXP_OK)
        goto cleanup;
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.initial_anchor,
                 fixture->initial_anchor, 32U);
    authority.independently_finalized = alternative->settlement;
    if (open_store(path, fixture, &authority, true, &log, &store,
                   &arena, arena_memory) != LXP_OK)
        goto cleanup;
    log_open = true;
    if (lxp_daemon_account_evidence_publish(
            &store, &fixture->account_evidence,
            &arena, digest) != LXP_OK ||
        lxp_daemon_finality_evidence_register(
            &store,
            (lxp_byte_span){alternative->checkpoint_payload,
                alternative->checkpoint_payload_length},
            (lxp_byte_span){alternative->finality_proof,
                alternative->finality_proof_length},
            &arena, &finality) != LXP_OK ||
        finality.resulting_registration_count != 1U ||
        store.registry.registration_count != 1U ||
        (change_resulting_root &&
         lxp_ct_memcmp(finality.resulting_state_root,
                       fixture->account_evidence.resulting_state_root,
                       32U) == 0) ||
        (!change_resulting_root &&
         lxp_ct_memcmp(finality.resulting_state_root,
                       fixture->account_evidence.resulting_state_root,
                       32U) != 0) ||
        lxp_daemon_account_evidence_wire_encode(
            &store, NULL, NULL, TEST_NETWORK_ID,
            fixture->account_id, 3U, 0U,
            alternative->checkpoint_id, &arena,
            &value, &proof) == LXP_OK)
        goto cleanup;
    result = 0;

cleanup:
    if (log_open) (void)lxp_log_close(&log);
    if (path[0] != '\0') (void)unlink(path);
    if (directory[0] != '\0') (void)rmdir(directory);
    free(alternative);
    free(arena_memory);
    return result;
}

static int recover_torn_tail(
    const char *path, test_fixture *fixture,
    finality_authority *authority, uint64_t durable_end)
{
    static const uint8_t partial[] = {'L', 'X', 'P', 'L', 1U, 0U, 0U};
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_log log;
    lxp_daemon_evidence_store store;
    lxp_arena arena;
    uint8_t after = 0xffU;
    if (arena_memory == NULL || lxp_log_open(&log, path) != LXP_OK ||
        pwrite(log.descriptor, partial, sizeof(partial),
               (off_t)durable_end) != (ssize_t)sizeof(partial) ||
        ftruncate(log.descriptor,
                  (off_t)(durable_end + sizeof(partial))) != 0 ||
        lxp_log_close(&log) != LXP_OK ||
        open_store(path, fixture, authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        store.log->write_offset != durable_end ||
        pread(log.descriptor, &after, 1U, (off_t)durable_end) != 1 ||
        after != 0U || verify_recovered(&store, fixture, &arena,
                                        4U, NULL) != 0 ||
        lxp_log_close(&log) != LXP_OK) {
        free(arena_memory);
        return 1;
    }
    free(arena_memory);
    return 0;
}

static int refuse_corrupt_committed_tail(
    const char *path, test_fixture *fixture,
    finality_authority *authority, uint64_t finality_offset)
{
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_log log;
    lxp_daemon_evidence_store store;
    lxp_arena arena;
    struct stat before;
    struct stat after;
    uint8_t byte;
    lxp_result status;
    if (arena_memory == NULL || lxp_log_open(&log, path) != LXP_OK ||
        pread(log.descriptor, &byte, 1U,
              (off_t)(finality_offset + LXP_LOG_HEADER_BYTES + 60U)) != 1)
        return 1;
    byte ^= 1U;
    if (pwrite(log.descriptor, &byte, 1U,
               (off_t)(finality_offset + LXP_LOG_HEADER_BYTES + 60U)) != 1 ||
        fstat(log.descriptor, &before) != 0 ||
        lxp_log_close(&log) != LXP_OK ||
        lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_log_open(&log, path) != LXP_OK)
        return 1;
    status = lxp_daemon_evidence_open(
        &store, &log, TEST_NETWORK_ID, &fixture->authorization,
        fixture->initial_anchor, false, verify_finality_authority,
        authority, &arena);
    if (status != LXP_ERR_LOG_CORRUPT ||
        fstat(log.descriptor, &after) != 0 ||
        before.st_size != after.st_size ||
        lxp_log_close(&log) != LXP_OK) {
        free(arena_memory);
        return 1;
    }
    free(arena_memory);
    return 0;
}

static int refuse_foreign_sequencer_recovery(
    const test_fixture *trusted_fixture)
{
    test_fixture *foreign = calloc(1U, sizeof(*foreign));
    uint8_t *fixture_arena_memory = malloc(TEST_ARENA_BYTES);
    uint8_t *arena_memory = malloc(TEST_ARENA_BYTES);
    lxp_arena fixture_arena;
    finality_authority authority;
    unsigned kind;
    int result = 1;
    if (foreign == NULL || fixture_arena_memory == NULL ||
        arena_memory == NULL ||
        lxp_arena_init(&fixture_arena, fixture_arena_memory,
                       TEST_ARENA_BYTES) != LXP_OK ||
        build_account_and_batch(foreign, &fixture_arena, 0x17U) != 0)
        goto cleanup;
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.initial_anchor, foreign->initial_anchor, 32U);
    for (kind = LXP_DAEMON_EVIDENCE_ACCOUNT;
         kind <= LXP_DAEMON_EVIDENCE_ACTIVITY; ++kind) {
        char directory[] = "/tmp/lxp-foreign-authority-XXXXXX";
        char path[160];
        lxp_arena arena;
        lxp_log log;
        lxp_daemon_evidence_store store;
        uint8_t digest[32];
        lxp_result status;
        if (mkdtemp(directory) == NULL ||
            snprintf(path, sizeof(path), "%s/%020u.lxp",
                     directory, 0U) < 0 ||
            lxp_log_segment_create(&log, directory, 0U,
                                   TEST_LOG_BYTES) != LXP_OK ||
            lxp_log_close(&log) != LXP_OK ||
            open_store(path, foreign, &authority, true, &log, &store,
                       &arena, arena_memory) != LXP_OK)
            goto cleanup;
        if (kind == LXP_DAEMON_EVIDENCE_ACCOUNT)
            status = lxp_daemon_account_evidence_publish(
                &store, &foreign->account_evidence, &arena, digest);
        else
            status = lxp_daemon_activity_evidence_publish(
                &store,
                (lxp_byte_span){foreign->canonical_activity[0],
                    foreign->canonical_activity_length[0]},
                &foreign->activity_proof[0],
                (lxp_byte_span){foreign->canonical_receipt[0],
                    foreign->canonical_receipt_length[0]},
                &foreign->receipt_proof[0], &foreign->authorization,
                (lxp_byte_span){foreign->canonical_header,
                    sizeof(foreign->canonical_header)},
                foreign->header_signature, &arena, digest);
        if (status != LXP_OK || lxp_log_close(&log) != LXP_OK ||
            lxp_arena_init(&arena, arena_memory,
                           TEST_ARENA_BYTES) != LXP_OK ||
            lxp_log_open(&log, path) != LXP_OK)
            goto cleanup;
        status = lxp_daemon_evidence_open(
            &store, &log, TEST_NETWORK_ID,
            &trusted_fixture->authorization, foreign->initial_anchor,
            false, verify_finality_authority, &authority, &arena);
        if (status != LXP_ERR_LOG_CORRUPT ||
            lxp_log_close(&log) != LXP_OK ||
            unlink(path) != 0 || rmdir(directory) != 0)
            goto cleanup;
    }
    result = 0;

cleanup:
    if (foreign != NULL && foreign->state_initialized)
        (void)lxp_state_store_destroy(&foreign->state);
    free(arena_memory);
    free(fixture_arena_memory);
    free(foreign);
    return result;
}

int main(void)
{
    char directory[] = "/tmp/lxp-evidence-recovery-XXXXXX";
    char path[160] = {0};
    uint8_t *fixture_arena_memory = NULL;
    uint8_t *arena_memory = NULL;
    lxp_arena fixture_arena;
    lxp_arena arena;
    test_fixture *fixture = NULL;
    finality_authority authority;
    lxp_log log;
    lxp_daemon_evidence_store store;
    lxp_daemon_finality_evidence finality;
    uint8_t first_finality_digest[32];
    uint8_t digest[32];
    uint64_t offset_before;
    uint64_t finality_offset;
    uint64_t durable_end;
    int result = 1;
    fixture = calloc(1U, sizeof(*fixture));
    fixture_arena_memory = malloc(TEST_ARENA_BYTES);
    arena_memory = malloc(TEST_ARENA_BYTES);
    if (fixture == NULL || fixture_arena_memory == NULL ||
        arena_memory == NULL) {
        report_stage_failure("fixture allocation");
        goto cleanup;
    }
    if (lxp_arena_init(&fixture_arena, fixture_arena_memory,
                       TEST_ARENA_BYTES) != LXP_OK) {
        report_stage_failure("fixture arena initialization");
        goto cleanup;
    }
    if (build_account_and_batch(fixture, &fixture_arena, 0x13U) != 0) {
        report_stage_failure("account and batch fixture");
        goto cleanup;
    }
    if (build_finality(fixture, &fixture_arena) != 0) {
        report_stage_failure("finality fixture");
        goto cleanup;
    }
    if (mkdtemp(directory) == NULL ||
        snprintf(path, sizeof(path), "%s/%020u.lxp",
                 directory, 0U) < 0 ||
        lxp_log_segment_create(&log, directory, 0U,
                               TEST_LOG_BYTES) != LXP_OK ||
        lxp_log_close(&log) != LXP_OK) {
        report_stage_failure("primary evidence log initialization");
        goto cleanup;
    }
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.initial_anchor,
                 fixture->initial_anchor, 32U);
    authority.independently_finalized = fixture->settlement;

    if (publish_then_crash(path, fixture, &authority,
                           LXP_DAEMON_EVIDENCE_ACCOUNT) != 0 ||
        open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        verify_recovered(&store, fixture, &arena, 1U, NULL) != 0 ||
        lxp_daemon_account_evidence_publish_batch(
            &store, &fixture->kernel,
            (lxp_byte_span){fixture->canonical_receipt[1],
                fixture->canonical_receipt_length[1]},
            &fixture->receipt_proof[1], &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena) != LXP_OK ||
        store.record_count != 2U ||
        lxp_log_close(&log) != LXP_OK ||
        open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        verify_recovered(&store, fixture, &arena, 2U, NULL) != 0 ||
        reject_account_mutations(&store, fixture, &arena) != 0 ||
        lxp_log_close(&log) != LXP_OK) {
        report_stage_failure("account evidence crash recovery");
        goto cleanup;
    }

    if (publish_then_crash(path, fixture, &authority,
                           LXP_DAEMON_EVIDENCE_ACTIVITY) != 0 ||
        open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        verify_recovered(&store, fixture, &arena, 3U, NULL) != 0 ||
        reject_activity_mutations(&store, fixture, &arena) != 0 ||
        lxp_log_close(&log) != LXP_OK) {
        report_stage_failure("activity evidence crash recovery");
        goto cleanup;
    }

    if (open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        reject_finality_mutations(&store, fixture, &authority, &arena) != 0) {
        (void)lxp_log_close(&log);
        report_stage_failure("finality evidence mutation rejection");
        goto cleanup;
    }
    finality_offset = log.write_offset;
    if (lxp_log_close(&log) != LXP_OK ||
        lxp_arena_init(&arena, arena_memory, TEST_ARENA_BYTES) != LXP_OK ||
        lxp_log_open(&log, path) != LXP_OK ||
        lxp_daemon_evidence_open(
            &store, &log, TEST_NETWORK_ID, &fixture->authorization,
            fixture->initial_anchor, false, NULL, NULL, &arena) != LXP_OK ||
        lxp_daemon_finality_evidence_register(
            &store,
            (lxp_byte_span){fixture->checkpoint_payload,
                fixture->checkpoint_payload_length},
            (lxp_byte_span){fixture->finality_proof,
                fixture->finality_proof_length},
            &arena, &finality) == LXP_OK ||
        store.record_count != 3U || log.write_offset != finality_offset ||
        lxp_log_close(&log) != LXP_OK ||
        publish_then_crash(path, fixture, &authority,
                           LXP_DAEMON_EVIDENCE_FINALITY) != 0 ||
        open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK ||
        verify_recovered(&store, fixture, &arena, 4U, NULL) != 0 ||
        lxp_daemon_finality_evidence_lookup(
            &store, fixture->checkpoint_id, 0U,
            &arena, &finality) != LXP_OK) {
        report_stage_failure("finality authority and crash recovery");
        goto cleanup;
    }
    (void)memcpy(first_finality_digest,
                 finality.record_digest, 32U);
    if (lxp_log_close(&log) != LXP_OK ||
        checkpoint_account_roundtrip(
            path, fixture, &authority) != 0 ||
        open_store(path, fixture, &authority, false, &log, &store,
                   &arena, arena_memory) != LXP_OK) {
        report_stage_failure("checkpoint-selected account recovery");
        goto cleanup;
    }
    offset_before = log.write_offset;
    if (lxp_daemon_account_evidence_publish(
            &store, &fixture->account_evidence,
            &arena, digest) != LXP_OK ||
        lxp_daemon_activity_evidence_publish(
            &store,
            (lxp_byte_span){fixture->canonical_activity[0],
                fixture->canonical_activity_length[0]},
            &fixture->activity_proof[0],
            (lxp_byte_span){fixture->canonical_receipt[0],
                fixture->canonical_receipt_length[0]},
            &fixture->receipt_proof[0], &fixture->authorization,
            (lxp_byte_span){fixture->canonical_header,
                sizeof(fixture->canonical_header)},
            fixture->header_signature, &arena, digest) != LXP_OK ||
        lxp_daemon_finality_evidence_register(
            &store,
            (lxp_byte_span){fixture->checkpoint_payload,
                fixture->checkpoint_payload_length},
            (lxp_byte_span){fixture->finality_proof,
                fixture->finality_proof_length},
            &arena, &finality) != LXP_OK ||
        store.record_count != 4U || log.write_offset != offset_before ||
        memcmp(first_finality_digest,
               finality.record_digest, 32U) != 0) {
        report_stage_failure("idempotent evidence republication");
        goto cleanup;
    }
    durable_end = log.write_offset;
    if (lxp_log_close(&log) != LXP_OK) {
        report_stage_failure("durable evidence close");
        goto cleanup;
    }
    if (recover_torn_tail(path, fixture, &authority, durable_end) != 0) {
        report_stage_failure("torn evidence tail recovery");
        goto cleanup;
    }
    if (refuse_corrupt_committed_tail(
            path, fixture, &authority, finality_offset) != 0) {
        report_stage_failure("corrupt committed evidence tail rejection");
        goto cleanup;
    }
    if (refuse_foreign_sequencer_recovery(fixture) != 0) {
        report_stage_failure("foreign sequencer evidence rejection");
        goto cleanup;
    }
    if (refuse_mismatched_checkpoint_account(fixture, true) != 0) {
        report_stage_failure("mismatched checkpoint state root rejection");
        goto cleanup;
    }
    if (refuse_mismatched_checkpoint_account(fixture, false) != 0) {
        report_stage_failure("mismatched checkpoint header rejection");
        goto cleanup;
    }
    if (recover_multi_activity_batch(fixture, &authority) != 0) {
        report_stage_failure("multi-activity batch evidence recovery");
        goto cleanup;
    }
    result = 0;

cleanup:
    if (fixture != NULL && fixture->state_initialized)
        (void)lxp_state_store_destroy(&fixture->state);
    if (path[0] != '\0') (void)unlink(path);
    if (directory[0] != '\0') (void)rmdir(directory);
    free(arena_memory);
    free(fixture_arena_memory);
    free(fixture);
    return result;
}
