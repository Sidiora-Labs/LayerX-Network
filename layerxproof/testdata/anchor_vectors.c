/*
 * Emits layerxproof/testdata/anchor_vectors.json from the C implementation.
 *
 * Every hash, signature, canonical header and checkpoint payload below is
 * produced by liblayerx and the layerxd evidence encoder
 * (lxp_batch_header_encode, lxp_guarantor_attest, lxp_guarantor_cert_assemble,
 * lxp_guarantor_cert_verify, lxp_checkpoint_certificate_hash,
 * lxp_daemon_finality_evidence_encode). Refused payloads are byte edits of an
 * accepted payload. Guarantor signatures use OpenSSL's randomised ECDSA nonce,
 * so regenerating replaces the signatures.
 *
 * Build, from the repository root after the C tree is built into build/:
 *   cc -O1 -Iinclude -Icmd/layerxd -Isrc/paxeer \
 *      layerxproof/testdata/anchor_vectors.c \
 *      build/obj/cmd/layerxd/lxp_daemon_evidence.o \
 *      build/obj/cmd/layerxd/lxp_daemon_receipt_authority.o \
 *      build/liblayerx.a \
 *      $PROGRAMS_TARGET/debug/liblayerx_programs_runtime.a \
 *      $PROGRAMS_TARGET/debug/liblayerx_programs_sandbox.a \
 *      -lcrypto -lm -lpthread -ldl -o build/anchor_vectors
 *   build/anchor_vectors > layerxproof/testdata/anchor_vectors.json
 */
#define OPENSSL_API_COMPAT 0x10100000L

#include "layerx/lxp_arena.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_guarantor.h"

#include <openssl/bn.h>
#include <openssl/ec.h>
#include <openssl/evp.h>
#include <openssl/obj_mac.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { ATTESTATION_WIRE = 274, GUARANTORS = 4, CHAIN_ID = 125 };

static const uint8_t SETTLEMENT[20] = {0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                                       0, 0, 0, 0, 0, 0, 0, 0, 0x10, 0x14};
static lxp_arena arena;
static lxp_guarantor_ctx guarantors[GUARANTORS];
static int first_field;
static int first_case = 1;

static void fail(const char *what, int status)
{
    fprintf(stderr, "anchor_vectors: %s failed (%d)\n", what, status);
    exit(1);
}

static void hex(const char *key, const uint8_t *bytes, size_t length)
{
    size_t i;
    printf("%s\"%s\":\"", first_field ? "" : ",", key);
    for (i = 0U; i < length; ++i) printf("%02x", bytes[i]);
    printf("\"");
    first_field = 0;
}

static void number(const char *key, uint64_t value)
{
    printf("%s\"%s\":\"%llu\"", first_field ? "" : ",", key,
           (unsigned long long)value);
    first_field = 0;
}

static void open_case(const char *name, int valid)
{
    printf("%s{\"name\":\"%s\",\"valid\":%s", first_case ? "" : ",", name,
           valid ? "true" : "false");
    first_case = 0;
    first_field = 0;
}

static void close_case(void) { printf("}\n"); }

static void fill(uint8_t *out, size_t length, uint8_t seed)
{
    size_t i;
    for (i = 0U; i < length; ++i) out[i] = (uint8_t)(seed + i);
}

static void secp_public(const uint8_t private_key[32], uint8_t out[33])
{
    EC_KEY *key = EC_KEY_new_by_curve_name(NID_secp256k1);
    BIGNUM *scalar = BN_bin2bn(private_key, 32, NULL);
    const EC_GROUP *group = EC_KEY_get0_group(key);
    EC_POINT *point = EC_POINT_new(group);
    if (EC_POINT_mul(group, point, scalar, NULL, NULL, NULL) != 1 ||
        EC_POINT_point2oct(group, point, POINT_CONVERSION_COMPRESSED, out, 33,
                           NULL) != 33)
        fail("secp256k1 public key", 0);
    EC_POINT_free(point);
    BN_free(scalar);
    EC_KEY_free(key);
}

static void ed25519_keypair(const uint8_t seed[32], EVP_PKEY **key,
                            uint8_t public_key[32])
{
    size_t length = 32U;
    *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    if (*key == NULL ||
        EVP_PKEY_get_raw_public_key(*key, public_key, &length) != 1)
        fail("ed25519 key", 0);
}

static void sign_header(EVP_PKEY *key, const uint8_t public_key[32],
                        const lxp_batch_header *header, uint8_t encoded[354],
                        uint8_t signature[64], int check)
{
    lxp_byte_span span;
    uint8_t digest[32];
    size_t length = 64U;
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    lxp_result status = lxp_batch_header_encode(header, &arena, &span);
    if (status != LXP_OK || span.length != 354U) fail("header encode", status);
    memcpy(encoded, span.bytes, 354U);
    status = lxp_batch_header_hash(header, &arena, digest);
    if (status != LXP_OK) fail("header hash", status);
    if (context == NULL ||
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
        EVP_DigestSign(context, signature, &length, digest, 32U) != 1)
        fail("header sign", 0);
    EVP_MD_CTX_free(context);
    if (check) {
        status = lxp_ed25519_verify(public_key, signature,
                                    LXP_DOMAIN_BATCH_HEADER, encoded, 354U);
        if (status != LXP_OK) fail("header verify", status);
    }
}

static lxp_batch_header make_header(uint64_t batch, uint64_t first,
                                    uint64_t last, uint8_t previous_seed,
                                    uint8_t result_seed,
                                    const uint8_t sequencer_id[32])
{
    lxp_batch_header header;
    memset(&header, 0, sizeof(header));
    header.protocol_version = LXP_PROTOCOL_VERSION;
    header.network_id = 7U;
    header.epoch = 3U;
    header.batch_number = batch;
    header.first_sequence = first;
    header.last_sequence = last;
    fill(header.previous_state_root, 32U, previous_seed);
    fill(header.resulting_state_root, 32U, result_seed);
    fill(header.activity_merkle_root, 32U, (uint8_t)(result_seed + 1U));
    fill(header.receipt_merkle_root, 32U, (uint8_t)(result_seed + 2U));
    fill(header.event_merkle_root, 32U, (uint8_t)(result_seed + 3U));
    fill(header.data_availability_root, 32U, (uint8_t)(result_seed + 4U));
    fill(header.oracle_root, 32U, (uint8_t)(result_seed + 5U));
    header.timestamp_ms = UINT64_C(1790000000000) + batch * 1000U;
    memcpy(header.sequencer_id, sequencer_id, 32U);
    return header;
}

static void write_wire_attestation(const lxp_guarantor_attestation *a,
                                   const uint8_t *payload,
                                   size_t payload_length, uint8_t out[274])
{
    /* The wire form is lifted from the daemon payload, not re-encoded. */
    size_t offset = 2U + 4U + 354U + 4U + 1U;
    for (; offset + ATTESTATION_WIRE <= payload_length;
         offset += ATTESTATION_WIRE)
        if (memcmp(payload + offset + 106U, a->guarantor_id, 32U) == 0) {
            memcpy(out, payload + offset, ATTESTATION_WIRE);
            return;
        }
    fail("attestation not in payload", 0);
}

static size_t certificate(const lxp_batch_header *header, const size_t *signers,
                          size_t signer_count, size_t threshold,
                          uint64_t delay_ms, uint8_t **payload,
                          uint8_t checkpoint_id[32],
                          lxp_guarantor_attestation *attestations_out)
{
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_attestation attestations[GUARANTORS];
    lxp_guarantor_key_record keys[GUARANTORS];
    lxp_guarantor_cert *cert = calloc(1U, sizeof(*cert));
    lxp_guarantor_set *set = calloc(1U, sizeof(*set));
    lxp_finalisation_requirements requirements;
    lxp_daemon_settlement_registration_evidence registration;
    lxp_byte_span encoded;
    lxp_byte_span proof;
    size_t valid = 0U;
    size_t i;
    lxp_result status;
    if (cert == NULL || set == NULL) fail("allocation", 0);
    memset(&checkpoint, 0, sizeof(checkpoint));
    checkpoint.header = *header;
    status = lxp_checkpoint_certificate_hash(&checkpoint, &arena,
                                              checkpoint_id);
    if (status != LXP_OK) fail("checkpoint hash", status);
    for (i = 0U; i < signer_count; ++i) {
        status = lxp_guarantor_attest(&guarantors[signers[i]], &checkpoint,
                                      true, true,
                                      header->timestamp_ms + delay_ms, &arena,
                                      &attestations[i]);
        if (status != LXP_OK) fail("attest", status);
    }
    status = lxp_guarantor_cert_assemble(&checkpoint, attestations,
                                         signer_count, threshold, cert);
    if (status != LXP_OK) fail("assemble", status);
    memset(keys, 0, sizeof(keys));
    set->version = 1U;
    set->count = GUARANTORS;
    for (i = 0U; i < GUARANTORS; ++i) {
        lxp_guarantor_bond_state *record = &set->records[i];
        memcpy(keys[i].guarantor_id, guarantors[i].guarantor_id, 32U);
        memcpy(keys[i].public_key, guarantors[i].paxeer_public_key, 33U);
        keys[i].bonded = true;
        memcpy(record->guarantor_id, guarantors[i].guarantor_id, 32U);
        memcpy(record->public_key, guarantors[i].paxeer_public_key, 33U);
        record->bond_amount.lo = 1000000U;
        record->joined_epoch = 1U;
        record->active = true;
        record->signer_authorization_count = 1U;
        memcpy(record->signer_authorizations[0].public_key,
               guarantors[i].paxeer_public_key, 33U);
        record->signer_authorizations[0].active_from_epoch = 1U;
        record->signer_authorizations[0].set_version = 1U;
    }
    if (delay_ms <= lxp_checkpoint_maximum_attestation_delay_ms()) {
        status = lxp_guarantor_cert_verify(cert, keys, GUARANTORS, &arena,
                                           &valid);
        if (status != LXP_OK || valid != signer_count)
            fail("certificate verify", status);
    }
    memset(&requirements, 0, sizeof(requirements));
    requirements.checkpoint_epoch = header->epoch;
    requirements.challenge_window_end_ms = header->timestamp_ms;
    requirements.checkpoint_deadline_ms = header->timestamp_ms + 3600000U;
    requirements.now_ms = header->timestamp_ms + 1U;
    requirements.threshold = threshold;
    requirements.minimum_bond.lo = 1U;
    requirements.availability_challenges_answered = true;
    memset(&registration, 0, sizeof(registration));
    registration.paxeer_chain_id = CHAIN_ID;
    memcpy(registration.settlement_contract, SETTLEMENT, 20U);
    memcpy(registration.checkpoint_id, checkpoint_id, 32U);
    fill(registration.transaction_id, 32U, 0x91U);
    registration.observed_block_number = 42U;
    registration.observed_at_ms = header->timestamp_ms + 2000U;
    status = lxp_daemon_finality_evidence_encode(
        cert, set, &requirements, 0U, &registration, &arena, &encoded, &proof);
    if (status != LXP_OK) fail("payload encode", status);
    *payload = malloc(encoded.length);
    if (*payload == NULL) fail("allocation", 0);
    memcpy(*payload, encoded.bytes, encoded.length);
    if (attestations_out != NULL)
        memcpy(attestations_out, cert->attestations,
               signer_count * sizeof(*attestations_out));
    free(cert);
    free(set);
    return encoded.length;
}

static void emit_checkpoint(const char *name, int valid, const char *refusal,
                            const uint8_t header[354],
                            const uint8_t signature[64],
                            const uint8_t *payload, size_t payload_length,
                            const uint8_t checkpoint_id[32],
                            const lxp_batch_header *decoded, size_t signers,
                            size_t threshold)
{
    open_case(name, valid);
    printf(",\"refusal\":\"%s\"", refusal);
    hex("header", header, 354U);
    hex("header_signature", signature, 64U);
    hex("certificate", payload, payload_length);
    hex("checkpoint_id", checkpoint_id, 32U);
    number("batch_number", decoded->batch_number);
    number("first_sequence", decoded->first_sequence);
    number("last_sequence", decoded->last_sequence);
    hex("resulting_state_root", decoded->resulting_state_root, 32U);
    hex("receipt_root", decoded->receipt_merkle_root, 32U);
    number("signers", signers);
    number("threshold", threshold);
    close_case();
}

int main(void)
{
    static uint8_t memory[8U * 1024U * 1024U];
    const size_t all[3] = {0U, 1U, 2U};
    const size_t pair[2] = {0U, 1U};
    const size_t one[1] = {0U};
    const size_t stranger[2] = {0U, 3U};
    uint8_t seed[32];
    uint8_t other_seed[32];
    uint8_t sequencer_key[32];
    uint8_t other_key[32];
    uint8_t sequencer_id[32];
    uint8_t header_bytes[354];
    uint8_t signature[64];
    uint8_t checkpoint_id[32];
    uint8_t wire[ATTESTATION_WIRE];
    uint8_t *payload;
    uint8_t *edited;
    size_t length;
    size_t i;
    size_t first_attestation = 2U + 4U + 354U + 4U + 1U;
    EVP_PKEY *sequencer;
    EVP_PKEY *other;
    lxp_batch_header header;
    lxp_guarantor_attestation attestations[GUARANTORS];
    if (lxp_arena_init(&arena, memory, sizeof(memory)) != LXP_OK)
        fail("arena", 0);
    fill(seed, 32U, 0x11U);
    fill(other_seed, 32U, 0x55U);
    fill(sequencer_id, 32U, 0xa0U);
    ed25519_keypair(seed, &sequencer, sequencer_key);
    ed25519_keypair(other_seed, &other, other_key);
    for (i = 0U; i < GUARANTORS; ++i) {
        lxp_guarantor_ctx *ctx = &guarantors[i];
        memset(ctx, 0, sizeof(*ctx));
        fill(ctx->guarantor_id, 32U, (uint8_t)(0x20U + 0x10U * i));
        fill(ctx->paxeer_private_key, 32U, (uint8_t)(0x31U + 7U * i));
        secp_public(ctx->paxeer_private_key, ctx->paxeer_public_key);
        ctx->protocol_version = LXP_PROTOCOL_VERSION;
        ctx->network_id = 7U;
        ctx->paxeer_chain_id = CHAIN_ID;
        memcpy(ctx->paxeer_settlement_contract, SETTLEMENT, 20U);
        ctx->bond_view.bonded = true;
        ctx->bond_view.bonded_amount.lo = 1000000U;
        ctx->bond_view.epoch = 3U;
        ctx->possesses_availability = true;
        ctx->ready_to_sign = true;
    }

    printf("{\"generator\":\"layerxproof/testdata/anchor_vectors.c\",\n");
    printf("\"context\":[\n");
    open_case("context", 1);
    hex("sequencer_id", sequencer_id, 32U);
    hex("sequencer_public_key", sequencer_key, 32U);
    number("paxeer_chain_id", CHAIN_ID);
    hex("settlement_contract", SETTLEMENT, 20U);
    number("network_id", 7U);
    number("maximum_attestation_delay_ms",
           lxp_checkpoint_maximum_attestation_delay_ms());
    close_case();
    printf("],\n\"guarantors\":[\n");
    first_case = 1;
    for (i = 0U; i < GUARANTORS; ++i) {
        uint8_t address[20];
        char name[16];
        if (lxp_secp256k1_address(guarantors[i].paxeer_public_key, 33U,
                                  address) != LXP_OK)
            fail("address", 0);
        snprintf(name, sizeof(name), "guarantor_%zu", i);
        open_case(name, 1);
        hex("guarantor_id", guarantors[i].guarantor_id, 32U);
        hex("public_key", guarantors[i].paxeer_public_key, 33U);
        hex("signer", address, 20U);
        close_case();
    }
    printf("],\n\"checkpoints\":[\n");
    first_case = 1;

    header = make_header(1U, 1U, 10U, 0x01U, 0x40U, sequencer_id);
    sign_header(sequencer, sequencer_key, &header, header_bytes, signature, 1);
    length = certificate(&header, all, 3U, 2U, 5000U, &payload, checkpoint_id,
                         attestations);
    emit_checkpoint("batch_1_quorum", 1, "", header_bytes, signature, payload,
                    length, checkpoint_id, &header, 3U, 2U);

    edited = malloc(length);
    memcpy(edited, payload, length);
    memcpy(edited + first_attestation, payload + first_attestation +
           ATTESTATION_WIRE, ATTESTATION_WIRE);
    memcpy(edited + first_attestation + ATTESTATION_WIRE,
           payload + first_attestation, ATTESTATION_WIRE);
    emit_checkpoint("batch_1_unsorted", 0, "unsorted", header_bytes, signature,
                    edited, length, checkpoint_id, &header, 3U, 2U);
    memcpy(edited, payload, length);
    memcpy(edited + first_attestation + ATTESTATION_WIRE,
           payload + first_attestation, ATTESTATION_WIRE);
    emit_checkpoint("batch_1_duplicate", 0, "duplicate", header_bytes,
                    signature, edited, length, checkpoint_id, &header, 3U, 2U);
    memcpy(edited, payload, length);
    edited[first_attestation + 209U + 5U] ^= 0x01U;
    emit_checkpoint("batch_1_bad_signature", 0, "signature", header_bytes,
                    signature, edited, length, checkpoint_id, &header, 3U, 2U);
    free(edited);
    {
        uint8_t wrong[64];
        uint8_t ignored[354];
        sign_header(other, other_key, &header, ignored, wrong, 0);
        emit_checkpoint("batch_1_wrong_sequencer", 0, "sequencer",
                        header_bytes, wrong, payload, length, checkpoint_id,
                        &header, 3U, 2U);
    }
    free(payload);

    length = certificate(&header, one, 1U, 1U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_1_below_threshold", 1, "", header_bytes, signature,
                    payload, length, checkpoint_id, &header, 1U, 1U);
    free(payload);
    length = certificate(&header, pair, 2U, 2U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_1_pair", 1, "", header_bytes, signature, payload,
                    length, checkpoint_id, &header, 2U, 2U);
    free(payload);
    length = certificate(&header, stranger, 2U, 2U, 5000U, &payload,
                         checkpoint_id, NULL);
    emit_checkpoint("batch_1_unregistered_guarantor", 0, "guarantor",
                    header_bytes, signature, payload, length, checkpoint_id,
                    &header, 2U, 2U);
    free(payload);
    length = certificate(&header, pair, 2U, 2U, 3600001U, &payload,
                         checkpoint_id, NULL);
    emit_checkpoint("batch_1_stale_attestation", 0, "freshness", header_bytes,
                    signature, payload, length, checkpoint_id, &header, 2U,
                    2U);
    free(payload);

    header = make_header(2U, 11U, 20U, 0x40U, 0x60U, sequencer_id);
    sign_header(sequencer, sequencer_key, &header, header_bytes, signature, 1);
    length = certificate(&header, all, 3U, 2U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_2_quorum", 1, "", header_bytes, signature, payload,
                    length, checkpoint_id, &header, 3U, 2U);
    free(payload);

    header = make_header(2U, 15U, 20U, 0x40U, 0x60U, sequencer_id);
    sign_header(sequencer, sequencer_key, &header, header_bytes, signature, 1);
    length = certificate(&header, all, 3U, 2U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_2_sequence_gap", 0, "continuity", header_bytes,
                    signature, payload, length, checkpoint_id, &header, 3U,
                    2U);
    free(payload);

    header = make_header(2U, 11U, 20U, 0x41U, 0x60U, sequencer_id);
    sign_header(sequencer, sequencer_key, &header, header_bytes, signature, 1);
    length = certificate(&header, all, 3U, 2U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_2_wrong_previous_root", 0, "continuity",
                    header_bytes, signature, payload, length, checkpoint_id,
                    &header, 3U, 2U);
    free(payload);

    header = make_header(9U, 11U, 20U, 0x40U, 0x60U, sequencer_id);
    sign_header(sequencer, sequencer_key, &header, header_bytes, signature, 1);
    length = certificate(&header, all, 3U, 2U, 5000U, &payload, checkpoint_id,
                         NULL);
    emit_checkpoint("batch_9_outside_authorization", 0, "authorization",
                    header_bytes, signature, payload, length, checkpoint_id,
                    &header, 3U, 2U);
    free(payload);
    printf("],\n\"attestations\":[\n");
    first_case = 1;

    header = make_header(1U, 1U, 10U, 0x01U, 0x40U, sequencer_id);
    length = certificate(&header, all, 3U, 2U, 6000U, &payload, checkpoint_id,
                         attestations);
    for (i = 0U; i < 3U; ++i) {
        char name[32];
        write_wire_attestation(&attestations[i], payload, length, wire);
        snprintf(name, sizeof(name), "batch_1_attestation_%zu", i);
        open_case(name, 1);
        hex("attestation", wire, ATTESTATION_WIRE);
        hex("guarantor_id", attestations[i].guarantor_id, 32U);
        hex("signer", attestations[i].signer, 20U);
        hex("checkpoint_id", checkpoint_id, 32U);
        number("batch_number", 1U);
        close_case();
    }
    free(payload);
    header = make_header(1U, 1U, 10U, 0x01U, 0x70U, sequencer_id);
    length = certificate(&header, all, 3U, 2U, 6000U, &payload, checkpoint_id,
                         attestations);
    for (i = 0U; i < 3U; ++i) {
        char name[40];
        write_wire_attestation(&attestations[i], payload, length, wire);
        snprintf(name, sizeof(name), "batch_1_conflicting_attestation_%zu", i);
        open_case(name, 1);
        hex("attestation", wire, ATTESTATION_WIRE);
        hex("guarantor_id", attestations[i].guarantor_id, 32U);
        hex("signer", attestations[i].signer, 20U);
        hex("checkpoint_id", checkpoint_id, 32U);
        number("batch_number", 1U);
        close_case();
    }
    free(payload);
    printf("]}\n");
    EVP_PKEY_free(sequencer);
    EVP_PKEY_free(other);
    return 0;
}
