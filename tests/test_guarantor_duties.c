#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "support/lxp_real_replay.h"
#include "support/lxp_pay1_replay.h"

#include <fcntl.h>
#include <openssl/evp.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

typedef struct verifier_state {
    bool reject_delegation;
} verifier_state;

typedef struct file_source {
    const char *path;
} file_source;

static lxp_result sign_raw(const uint8_t private_key[32], const uint8_t *message,
                           size_t message_length, uint8_t signature[64],
                           uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  private_key, 32U);
    EVP_MD_CTX *context = key == NULL ? NULL : EVP_MD_CTX_new();
    size_t public_length = 32U;
    size_t signature_length = 64U;
    int ok = context != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &public_length) == 1 &&
        public_length == 32U &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length, message,
                       message_length) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? LXP_OK : LXP_ERR_BAD_SIGNATURE;
}

static lxp_result verify_authority(
    void *context, const lxp_activity *activity,
    lxp_byte_span canonical_activity,
    lxp_guarantor_authority_verdict *verdict)
{
    verifier_state *state = (verifier_state *)context;
    uint8_t preimage[32];
    (void)canonical_activity;
    if (activity->authority.length != 32U ||
        activity->signature.length != 64U ||
        lxp_activity_signing_preimage(activity, preimage) != LXP_OK ||
        lxp_ed25519_verify_raw(activity->authority.bytes,
                               activity->signature.bytes, preimage,
                               sizeof(preimage)) != LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    verdict->actor_signature = true;
    verdict->session_key = true;
    verdict->capability_grant = true;
    verdict->delegated_authority = !state->reject_delegation;
    return LXP_OK;
}

static lxp_result verify_oracle(void *context, lxp_byte_span canonical_oracle,
                                bool *valid)
{
    (void)context;
    if (canonical_oracle.length < 96U) return LXP_ERR_NON_CANONICAL;
    *valid = lxp_ed25519_verify_raw(canonical_oracle.bytes,
        canonical_oracle.bytes + 32U, canonical_oracle.bytes + 96U,
        canonical_oracle.length - 96U) == LXP_OK;
    return LXP_OK;
}

static lxp_result download_file(void *context, uint64_t batch_number,
                                lxp_arena *arena,
                                lxp_byte_span *canonical_body)
{
    const file_source *source = (const file_source *)context;
    struct stat information;
    void *memory;
    int descriptor;
    ssize_t count;
    if (batch_number != 1U) return LXP_ERR_BATCH_GAP;
    descriptor = open(source->path, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size <= 0 || information.st_size > INT32_MAX) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    if (lxp_arena_alloc(arena, (size_t)information.st_size, 1U, &memory) !=
        LXP_OK) {
        (void)close(descriptor);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    count = read(descriptor, memory, (size_t)information.st_size);
    if (close(descriptor) != 0 || count != information.st_size)
        return LXP_ERR_IO;
    canonical_body->bytes = (const uint8_t *)memory;
    canonical_body->length = (size_t)information.st_size;
    return LXP_OK;
}

static lxp_result store_file(void *context, uint64_t batch_number,
                             const uint8_t *canonical_body,
                             size_t body_length)
{
    const file_source *destination = (const file_source *)context;
    int descriptor;
    ssize_t count;
    if (batch_number != 1U || body_length > INT32_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    descriptor = open(destination->path,
                      O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0600);
    if (descriptor < 0) return LXP_ERR_IO;
    count = write(descriptor, canonical_body, body_length);
    if (count != (ssize_t)body_length || fdatasync(descriptor) != 0 ||
        close(descriptor) != 0) return LXP_ERR_IO;
    return LXP_OK;
}

static int write_exact_file(const char *path, const uint8_t *bytes,
                            size_t length)
{
    int descriptor = open(path, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC,
                          0600);
    if (descriptor < 0 || write(descriptor, bytes, length) != (ssize_t)length ||
        fdatasync(descriptor) != 0 || close(descriptor) != 0) {
        if (descriptor >= 0) (void)close(descriptor);
        return 1;
    }
    return 0;
}

int main(void)
{
    uint8_t *storage = malloc(8U * 1024U * 1024U);
    uint8_t oracle_private[32] = {2U};
    uint8_t sequencer_private[32] = {3U};
    uint8_t oracle_public[32];
    uint8_t oracle_signature[64];
    uint8_t oracle_item[99];
    uint8_t payload[] = {7U, 8U, 9U};
    uint8_t body_copy[16384];
    uint8_t stored_copy[16384];
    lxp_byte_span activity_item;
    lxp_byte_span oracle_span;
    lxp_batch_body body;
    static lxp_real_replay_fixture builder, verifier_kernel;
    lxp_sequencer_authorization sequencer_authorization;
    lxp_guarantor_ctx guarantor;
    lxp_arena arena;
    lxp_byte_span canonical_body;
    verifier_state verifier = {false};
    file_source source;
    file_source destination;
    bool ready = false;
    char directory[] = "/tmp/lxp-guarantor-XXXXXX";
    char source_path[160] = {0};
    char stored_path[160] = {0};
    struct stat information;
    int descriptor;
    int result = 1;
    size_t public_length = 32U;
    EVP_PKEY *sequencer_key = NULL;

    if (pay1_guarantor_replay() != 0) goto cleanup;
    if (storage == NULL || mkdtemp(directory) == NULL ||
        snprintf(source_path, sizeof(source_path), "%s/batch.lxb", directory) < 0 ||
        snprintf(stored_path, sizeof(stored_path), "%s/stored.lxb", directory) < 0 ||
        lxp_arena_init(&arena, storage, 8U * 1024U * 1024U) != LXP_OK ||
        lxp_real_replay_init(&builder) != 0 ||
        lxp_real_replay_init(&verifier_kernel) != 0)
        goto cleanup;
    if (lxp_real_replay_activity(&builder, 0U, &arena, &activity_item) != 0)
        goto cleanup;
    if (sign_raw(oracle_private, payload, sizeof(payload), oracle_signature,
                 oracle_public) != LXP_OK) goto cleanup;
    (void)memcpy(oracle_item, oracle_public, 32U);
    (void)memcpy(oracle_item + 32U, oracle_signature, 64U);
    (void)memcpy(oracle_item + 96U, payload, sizeof(payload));
    oracle_span = (lxp_byte_span){oracle_item, sizeof(oracle_item)};
    if (lxp_real_replay_build(&builder, 1U, &activity_item, 1U, &oracle_span, 1U,
                              &arena, &body) != 0) goto cleanup;
    verifier_kernel.execution.batch_number = 1U;
    (void)memset(&sequencer_authorization, 0, sizeof(sequencer_authorization));
    sequencer_key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                 sequencer_private, 32U);
    if (sequencer_key == NULL || EVP_PKEY_get_raw_public_key(
            sequencer_key, sequencer_authorization.public_key,
            &public_length) != 1 || public_length != 32U) goto cleanup;
    EVP_PKEY_free(sequencer_key);
    sequencer_key = NULL;
    (void)memcpy(sequencer_authorization.sequencer_id,
                 sequencer_authorization.public_key, 32U);
    (void)memcpy(body.header.sequencer_id,
                 sequencer_authorization.sequencer_id, 32U);
    sequencer_authorization.first_batch_number = 1U;
    sequencer_authorization.last_batch_number = 1U;
    sequencer_authorization.authorized = 1U;
    if (lxp_batch_sign(&body.header, sequencer_private,
                       &sequencer_authorization, body.sequencer_signature,
                       &arena) != LXP_OK ||
        lxp_batch_body_encode(&body, &arena, &canonical_body) != LXP_OK ||
        canonical_body.length > sizeof(body_copy)) goto cleanup;
    (void)memcpy(body_copy, canonical_body.bytes, canonical_body.length);
    if (write_exact_file(source_path, body_copy, canonical_body.length) != 0)
        goto cleanup;
    source.path = source_path;
    destination.path = stored_path;
    (void)memset(&guarantor, 0, sizeof(guarantor));
    guarantor.guarantor_id[0] = 4U;
    guarantor.paxeer_public_key[0] = 2U;
    guarantor.protocol_version = body.header.protocol_version;
    guarantor.network_id = body.header.network_id;
    guarantor.bond_view.bonded = true;
    guarantor.bond_view.bonded_amount = (lxp_u128){0U, 100U};
    guarantor.replay_engine = &verifier_kernel.engine;
    (void)memcpy(guarantor.independent_state_root, verifier_kernel.kernel.current_state_root, 32U);
    guarantor.sequencer_authorization = &sequencer_authorization;
    guarantor.download = download_file;
    guarantor.download_context = &source;
    guarantor.verify_authority = verify_authority;
    guarantor.authority_context = &verifier;
    guarantor.verify_oracle = verify_oracle;
    guarantor.store_availability = store_file;
    guarantor.storage_context = &destination;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 1U, &arena, &ready) != LXP_OK ||
        !ready || !guarantor.ready_to_sign ||
        !guarantor.possesses_availability ||
        guarantor.last_completed_duty != LXP_GUARANTOR_DUTY_READY_TO_SIGN)
        goto cleanup;
    descriptor = open(stored_path, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size != (off_t)canonical_body.length ||
        read(descriptor, stored_copy, sizeof(stored_copy)) !=
            information.st_size || close(descriptor) != 0 ||
        memcmp(stored_copy, body_copy, canonical_body.length) != 0)
        goto cleanup;
    verifier.reject_delegation = true;
    if (lxp_state_store_destroy(&verifier_kernel.state) != LXP_OK) goto cleanup;
    (void)memset(&verifier_kernel, 0, sizeof(verifier_kernel));
    if (lxp_real_replay_init(&verifier_kernel) != 0) goto cleanup;
    verifier_kernel.execution.batch_number = 1U;
    (void)memcpy(guarantor.independent_state_root, verifier_kernel.kernel.current_state_root, 32U);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 1U, &arena, &ready) !=
            LXP_ERR_BAD_SIGNATURE || ready || guarantor.ready_to_sign ||
        guarantor.last_completed_duty != LXP_GUARANTOR_DUTY_DOWNLOADED)
        goto cleanup;
    verifier.reject_delegation = false;
    destination.path = directory;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 1U, &arena, &ready) !=
            LXP_ERR_IO || ready || guarantor.ready_to_sign ||
        guarantor.possesses_availability ||
        guarantor.last_completed_duty != LXP_GUARANTOR_DUTY_ROOTS)
        goto cleanup;
    result = 0;

cleanup:
    EVP_PKEY_free(sequencer_key);
    (void)unlink(stored_path);
    (void)unlink(source_path);
    (void)rmdir(directory);
    free(storage);
    return result;
}
