#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L
#include "../../cmd/layerx-guarantor/lni.h"
#include "../../cmd/layerx-guarantor/producer.h"
#include "support/lxp_real_replay.h"
#include <fcntl.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <openssl/pem.h>
#include <sys/stat.h>
#include <unistd.h>
#define REQUIRE(x)                                                                                 \
    do {                                                                                           \
        if (!(x)) {                                                                                \
            fprintf(stderr, "guarantor core line %d: %s\n", __LINE__, #x);                         \
            return 1;                                                                              \
        }                                                                                          \
    } while (0)
static lxp_real_replay_fixture builder, verifier;
static lxp_result authority(void *context, const lxp_activity *activity, lxp_byte_span canonical,
                            lxp_guarantor_authority_verdict *verdict)
{
    lxp_real_replay_fixture *f = context;
    lxp_identity *identity;
    lxp_activity decoded;
    lxp_result status = lxp_activity_decode(canonical.bytes, canonical.length, &decoded);
    if (status == LXP_OK)
        status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK)
        status = lxp_identity_resolve(&f->identities, activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status != LXP_OK)
        return status;
    if (activity->authority.length != 32U ||
        !lxp_identity_key_valid(identity, activity->authority.bytes, 10U, 1U))
        return LXP_ERR_BAD_SIGNATURE;
    *verdict = (lxp_guarantor_authority_verdict){true, true, true, true};
    return LXP_OK;
}
static lxp_result oracle(void *context, lxp_byte_span bytes, bool *valid)
{
    (void)context;
    (void)bytes;
    *valid = false;
    return LXP_ERR_MODULE_DISABLED;
}
static void number(uint8_t *p, uint64_t value, size_t n)
{
    for (size_t i = 0U; i < n; ++i)
        p[n - i - 1U] = (uint8_t)(value >> (8U * i));
}
int main(void)
{
    uint8_t *memory = malloc(32U * 1024U * 1024U);
    lxp_arena arena;
    lxp_batch_body body;
    lxp_byte_span activity;
    lxp_da_bundle bundle;
    lxp_da_store store;
    lxp_sequencer_authorization sequencer = {0};
    lxp_guarantor_ctx ctx = {0};
    lxp_guarantor_attestation attestation, decoded, conflicting;
    lxp_checkpoint_certificate checkpoint;
    lxp_guarantor_set set;
    lxp_guarantor_bond_state member = {0};
    uint8_t signature[64], public_key[32], digest[32] = {0};
    uint8_t encoded[GP_ATTESTATION_BYTES], reencoded[GP_ATTESTATION_BYTES];
    char directory[] = "/tmp/guarantor-core-XXXXXX", path[256];
    const char *field;
    REQUIRE(memory != NULL && mkdtemp(directory) != NULL);
    REQUIRE(lxp_arena_init(&arena, memory, 32U * 1024U * 1024U) == LXP_OK);
    {
        EVP_PKEY_CTX *generation = EVP_PKEY_CTX_new_id(EVP_PKEY_EC, NULL);
        EVP_PKEY *generated = NULL;
        lxp_guarantor_ctx loaded = {0};
        REQUIRE(generation != NULL && EVP_PKEY_keygen_init(generation) == 1 &&
                EVP_PKEY_CTX_set_ec_paramgen_curve_nid(generation, NID_secp256k1) == 1 &&
                EVP_PKEY_keygen(generation, &generated) == 1);
        REQUIRE(snprintf(path, sizeof(path), "%s/key.pem", directory) > 0);
        int fd = open(path, O_CREAT | O_EXCL | O_WRONLY, 0600);
        REQUIRE(fd >= 0);
        FILE *file = fdopen(fd, "w");
        REQUIRE(file != NULL &&
                PEM_write_PrivateKey(file, generated, NULL, NULL, 0, NULL, NULL) == 1 &&
                fclose(file) == 0);
        REQUIRE(gp_key_load(path, &loaded) == LXP_OK);
        REQUIRE(lxp_secp256k1_sign(loaded.paxeer_private_key, LXP_DOMAIN_GUARANTOR_ATTESTATION,
                                   digest, sizeof(digest), signature) == LXP_OK);
        REQUIRE(lxp_secp256k1_verify(loaded.paxeer_public_key, 33U, signature,
                                     LXP_DOMAIN_GUARANTOR_ATTESTATION, digest,
                                     sizeof(digest)) == LXP_OK);
        REQUIRE(chmod(path, 0644) == 0 && gp_key_load(path, &loaded) != LXP_OK);
        REQUIRE(unlink(path) == 0);
        EVP_PKEY_free(generated);
        EVP_PKEY_CTX_free(generation);
        lxp_secure_zero(loaded.paxeer_private_key, 32U);
    }

    REQUIRE(lxp_real_replay_init(&builder) == 0 && lxp_real_replay_init(&verifier) == 0);
    REQUIRE(lxp_real_replay_activity(&builder, 0U, &arena, &activity) == 0);
    REQUIRE(lxp_real_replay_build(&builder, 1U, &activity, 1U, NULL, 0U, &arena, &body) == 0);
    REQUIRE(lxp_real_replay_sign(digest, signature, public_key) == 0);
    memcpy(sequencer.public_key, public_key, 32U);
    memcpy(sequencer.sequencer_id, public_key, 32U);
    sequencer.first_batch_number = 1U;
    sequencer.last_batch_number = 1U;
    sequencer.authorized = 1U;
    memcpy(body.header.sequencer_id, public_key, 32U);
    REQUIRE(lxp_batch_sign(&body.header, lxp_real_replay_seed, &sequencer, signature, &arena) ==
            LXP_OK);
    REQUIRE(lxp_da_bundle_build(&body, LXP_DA_CANONICAL_CHUNK_BYTES, &arena, &bundle) == LXP_OK);
    REQUIRE(lxp_da_store_init(&store, directory) == LXP_OK);
    REQUIRE(lxp_da_store_bundle(&store, &bundle, &arena) == LXP_OK);
    for (size_t i = 0U; i < bundle.chunk_count; ++i) {
        lxp_byte_span bytes, proof;
        lxp_da_chunk chunk;
        uint32_t count;
        REQUIRE(lxp_da_serve_chunk_proof(&store, 1U, (uint32_t)i,
                                         body.header.data_availability_root, &arena, &bytes,
                                         &proof) == LXP_OK);
        REQUIRE(lxp_guarantor_chunk_verify(bytes, proof, &body.header, (uint32_t)i, &chunk,
                                           &count) == LXP_OK);
        REQUIRE(count == bundle.chunk_count && chunk.length == bytes.length);
        uint8_t corrupt[2048];
        REQUIRE(proof.length <= sizeof(corrupt));
        memcpy(corrupt, proof.bytes, proof.length);
        corrupt[21] ^= 1U;
        REQUIRE(lxp_guarantor_chunk_verify(bytes, (lxp_byte_span){corrupt, proof.length},
                                           &body.header, (uint32_t)i, &chunk, &count) != LXP_OK);
        memcpy(corrupt, proof.bytes, proof.length);
        number(corrupt, 2U, 8U);
        REQUIRE(lxp_guarantor_chunk_verify(bytes, (lxp_byte_span){corrupt, proof.length},
                                           &body.header, (uint32_t)i, &chunk, &count) != LXP_OK);
        REQUIRE(lxp_guarantor_chunk_verify(bytes, proof, &body.header, (uint32_t)i + 1U, &chunk,
                                           &count) != LXP_OK);
        REQUIRE(lxp_guarantor_chunk_verify(bytes, (lxp_byte_span){proof.bytes, proof.length - 1U},
                                           &body.header, (uint32_t)i, &chunk, &count) != LXP_OK);
    }
    ctx.protocol_version = body.header.protocol_version;
    ctx.network_id = body.header.network_id;
    ctx.paxeer_chain_id = 31337U;
    ctx.paxeer_settlement_contract[0] = 1U;
    ctx.guarantor_id[31] = 1U;
    ctx.paxeer_private_key[31] = 1U;
    static const uint8_t key[33] = {0x02, 0x79, 0xbe, 0x66, 0x7e, 0xf9, 0xdc, 0xbb, 0xac,
                                    0x55, 0xa0, 0x62, 0x95, 0xce, 0x87, 0x0b, 0x07, 0x02,
                                    0x9b, 0xfc, 0xdb, 0x2d, 0xce, 0x28, 0xd9, 0x59, 0xf2,
                                    0x81, 0x5b, 0x16, 0xf8, 0x17, 0x98};
    memcpy(ctx.paxeer_public_key, key, sizeof(key));
    ctx.bond_view.bonded = true;
    ctx.replay_engine = &verifier.engine;
    ctx.sequencer_authorization = &sequencer;
    ctx.verify_authority = authority;
    ctx.authority_context = &verifier;
    ctx.verify_oracle = oracle;
    memcpy(ctx.independent_state_root, verifier.kernel.current_state_root, 32U);
    verifier.execution.batch_number = 1U;
    REQUIRE(gp_verify_attest(&ctx, &bundle, &body.header, signature, &store, 11U, &arena,
                             &attestation, &field) == LXP_OK);
    REQUIRE(lxp_guarantor_attestation_verify(&attestation, key) == LXP_OK);
    REQUIRE(gp_attestation_encode(&attestation, encoded) == LXP_OK);
    REQUIRE(gp_attestation_decode(encoded, sizeof(encoded), &decoded) == LXP_OK);
    REQUIRE(gp_attestation_encode(&decoded, reencoded) == LXP_OK &&
            memcmp(encoded, reencoded, sizeof(encoded)) == 0);
    REQUIRE(gp_attestation_decode(encoded, sizeof(encoded) - 1U, &decoded) != LXP_OK);
    encoded[178] = 2U;
    REQUIRE(gp_attestation_decode(encoded, sizeof(encoded), &decoded) != LXP_OK);
    encoded[178] = 1U;
    checkpoint = (lxp_checkpoint_certificate){body.header, {NULL, 0U}};
    lxp_finalisation_requirements requirements;
    REQUIRE(gp_checkpoint_requirements(&body.header, 9U, 1U, (lxp_u128){0U, 100U}, &requirements) ==
            LXP_OK);
    REQUIRE(requirements.checkpoint_deadline_ms ==
            body.header.timestamp_ms + lxp_checkpoint_maximum_attestation_delay_ms());
    REQUIRE(requirements.checkpoint_deadline_ms >= attestation.attested_at_ms &&
            requirements.minimum_bond.lo == 100U);
    lxp_batch_header overflow = body.header;
    overflow.timestamp_ms = UINT64_MAX;
    REQUIRE(gp_checkpoint_requirements(&overflow, 9U, 1U, (lxp_u128){0U, 100U}, &requirements) !=
            LXP_OK);

    REQUIRE(lxp_guarantor_set_init(&set) == LXP_OK);
    memcpy(member.guarantor_id, ctx.guarantor_id, 32U);
    memcpy(member.public_key, key, 33U);
    member.active = true;
    member.joined_epoch = body.header.epoch;
    member.bond_amount.lo = 1000U;
    REQUIRE(lxp_guarantor_set_apply(&set, 1U, true, &member) == LXP_OK);
    REQUIRE(gp_attestation_accept(&checkpoint, ctx.paxeer_chain_id, ctx.paxeer_settlement_contract,
                                  &set, &attestation, NULL, 0U, directory, &arena) == LXP_OK);
    set.records[0].active = false;
    REQUIRE(gp_attestation_accept(&checkpoint, ctx.paxeer_chain_id, ctx.paxeer_settlement_contract,
                                  &set, &attestation, NULL, 0U, directory, &arena) != LXP_OK);
    set.records[0].active = true;
    checkpoint.header.resulting_state_root[0] ^= 1U;
    REQUIRE(lxp_guarantor_attest(&ctx, &checkpoint, true, true, 11U, &arena, &conflicting) ==
            LXP_OK);
    checkpoint.header = body.header;
    REQUIRE(gp_attestation_accept(&checkpoint, ctx.paxeer_chain_id, ctx.paxeer_settlement_contract,
                                  &set, &conflicting, &attestation, 1U, directory,
                                  &arena) != LXP_OK);
    REQUIRE(
        snprintf(
            path, sizeof(path),
            "%s/%llu-1-0000000000000000000000000000000000000000000000000000000000000001-kind1.bin",
            directory, (unsigned long long)body.header.epoch) > 0);
    struct stat info;
    REQUIRE(stat(path, &info) == 0 && info.st_size > 0);
    REQUIRE(unlink(path) == 0);
    REQUIRE(lxp_state_store_destroy(&verifier.state) == LXP_OK);
    memset(&verifier, 0, sizeof(verifier));
    REQUIRE(lxp_real_replay_init(&verifier) == 0);
    verifier.execution.batch_number = 1U;
    memcpy(ctx.independent_state_root, verifier.kernel.current_state_root, 32U);
    body.header.resulting_state_root[0] ^= 1U;
    REQUIRE(lxp_batch_sign(&body.header, lxp_real_replay_seed, &sequencer, signature, &arena) ==
            LXP_OK);
    REQUIRE(gp_verify_attest(&ctx, &bundle, &body.header, signature, &store, 11U, &arena, &decoded,
                             &field) != LXP_OK);
    REQUIRE(lxp_ct_is_zero(decoded.signature, sizeof(decoded.signature)) && !ctx.ready_to_sign);
    REQUIRE(strcmp(field, "resulting_state_root") == 0);
    REQUIRE(gp_verify_attest(&ctx, &bundle, &body.header, signature, &store,
                             10U + lxp_checkpoint_maximum_attestation_delay_ms() + 1U, &arena,
                             &decoded, &field) != LXP_OK);
    REQUIRE(lxp_ct_is_zero(decoded.signature, sizeof(decoded.signature)));
    REQUIRE(lxp_state_store_destroy(&verifier.state) == LXP_OK);
    REQUIRE(lxp_state_store_destroy(&builder.state) == LXP_OK);
    REQUIRE(snprintf(path, sizeof(path), "%s/%020u.lxda", directory, 1U) > 0);
    REQUIRE(unlink(path) == 0 && rmdir(directory) == 0);
    free(memory);
    puts("guarantor replay, chunk proofs, signing, codec, membership, equivocation and root "
         "refusal passed");
    return 0;
}
