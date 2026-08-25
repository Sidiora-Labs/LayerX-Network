#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_guarantor.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

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

static lxp_result parameter_version(void *context, uint64_t epoch,
                                    uint32_t *version)
{
    (void)context;
    if (epoch > UINT32_MAX) return LXP_ERR_OVERFLOW;
    *version = (uint32_t)epoch;
    return LXP_OK;
}

static lxp_result transition(void *context, uint16_t transition_version,
                             uint32_t parameter, uint64_t timestamp,
                             uint64_t sequence, lxp_byte_span activity,
                             const uint8_t previous_root[32], lxp_arena *arena,
                             lxp_replay_activity_output *output)
{
    uint8_t *input;
    void *memory;
    size_t length = 54U + activity.length;
    size_t i;
    lxp_result status = lxp_arena_alloc(arena, length, 1U, &memory);
    (void)context;
    if (status != LXP_OK) return status;
    input = (uint8_t *)memory;
    (void)memcpy(input, previous_root, 32U);
    input[32] = (uint8_t)(transition_version >> 8U);
    input[33] = (uint8_t)transition_version;
    for (i = 0U; i < 4U; ++i)
        input[34U + 3U - i] = (uint8_t)(parameter >> (i * 8U));
    for (i = 0U; i < 8U; ++i)
        input[38U + 7U - i] = (uint8_t)(timestamp >> (i * 8U));
    for (i = 0U; i < 8U; ++i)
        input[46U + 7U - i] = (uint8_t)(sequence >> (i * 8U));
    (void)memcpy(input + 54U, activity.bytes, activity.length);
    status = lxp_hash_sha256(input, length, output->resulting_state_root);
    if (status != LXP_OK) return status;
    output->result_code = LXP_OK;
    output->fee_charged = (lxp_u128){0U, 1U};
    output->effects = activity;
    output->resulting_balance = (lxp_byte_span){
        output->resulting_state_root, 16U
    };
    output->canonical_receipt = (lxp_byte_span){
        output->resulting_state_root, 32U
    };
    output->canonical_events = activity;
    return LXP_OK;
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
    if (batch_number != 4U) return LXP_ERR_BATCH_GAP;
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
    if (batch_number != 4U || body_length > INT32_MAX)
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
    uint8_t actor_private[32] = {1U};
    uint8_t oracle_private[32] = {2U};
    uint8_t sequencer_private[32] = {3U};
    uint8_t actor_public[32];
    uint8_t oracle_public[32];
    uint8_t oracle_signature[64];
    uint8_t oracle_item[99];
    uint8_t actor_signature[64];
    uint8_t preimage[32];
    uint8_t payload[] = {7U, 8U, 9U};
    uint8_t did[] = {'d','i','d',':','l','x',':','g'};
    uint8_t activity_copy[2048];
    uint8_t body_copy[16384];
    uint8_t stored_copy[16384];
    lxp_activity activity;
    lxp_byte_span activity_encoded;
    lxp_byte_span activity_item;
    lxp_byte_span oracle_span;
    lxp_byte_span section;
    lxp_byte_span empty;
    lxp_batch_body body;
    lxp_replay_batch_result replay;
    lxp_replay_engine engine;
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

    if (storage == NULL || mkdtemp(directory) == NULL ||
        snprintf(source_path, sizeof(source_path), "%s/batch.lxb", directory) < 0 ||
        snprintf(stored_path, sizeof(stored_path), "%s/stored.lxb", directory) < 0 ||
        lxp_arena_init(&arena, storage, 8U * 1024U * 1024U) != LXP_OK ||
        lxp_replay_engine_init(&engine, parameter_version, NULL) != LXP_OK ||
        lxp_replay_engine_register(&engine, 1U, transition) != LXP_OK)
        goto cleanup;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = 1U;
    activity.network_id = 9U;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){did, sizeof(did)};
    activity.account_sequence = 1U;
    activity.timestamp_bound.not_before = 1U;
    activity.timestamp_bound.not_after = 2000U;
    activity.idempotency_key[0] = 1U;
    activity.fee_limit = (lxp_u128){0U, 10U};
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK ||
        sign_raw(actor_private, payload, 0U, actor_signature, actor_public) != LXP_OK)
        goto cleanup;
    activity.authority = (lxp_byte_span){actor_public, sizeof(actor_public)};
    if (lxp_activity_signing_preimage(&activity, preimage) != LXP_OK ||
        sign_raw(actor_private, preimage, sizeof(preimage), actor_signature,
                 actor_public) != LXP_OK)
        goto cleanup;
    activity.signature = (lxp_byte_span){actor_signature,
                                         sizeof(actor_signature)};
    if (lxp_activity_encode(&activity, &arena, &activity_encoded) != LXP_OK ||
        activity_encoded.length > sizeof(activity_copy)) goto cleanup;
    (void)memcpy(activity_copy, activity_encoded.bytes, activity_encoded.length);
    activity_item = (lxp_byte_span){activity_copy, activity_encoded.length};
    if (sign_raw(oracle_private, payload, sizeof(payload), oracle_signature,
                 oracle_public) != LXP_OK) goto cleanup;
    (void)memcpy(oracle_item, oracle_public, 32U);
    (void)memcpy(oracle_item + 32U, oracle_signature, 64U);
    (void)memcpy(oracle_item + 96U, payload, sizeof(payload));
    oracle_span = (lxp_byte_span){oracle_item, sizeof(oracle_item)};
    if (lxp_replay_section_encode(&activity_item, 1U, &arena, &section) != LXP_OK)
        goto cleanup;
    (void)memset(&body, 0, sizeof(body));
    body.header.protocol_version = 1U;
    body.header.network_id = 9U;
    body.header.epoch = 2U;
    body.header.batch_number = 4U;
    body.header.first_sequence = 1U;
    body.header.last_sequence = 1U;
    body.header.timestamp_ms = 100U;
    body.activities = section;
    if (lxp_replay_section_encode(&oracle_span, 1U, &arena,
                                  &body.oracle_inputs) != LXP_OK ||
        lxp_replay_section_encode(NULL, 0U, &arena, &empty) != LXP_OK)
        goto cleanup;
    body.state_diff = empty;
    body.recovery_metadata = empty;
    if (lxp_replay_batch(&engine, &body, body.header.previous_state_root,
                         &arena, &replay) != LXP_OK) goto cleanup;
    body.receipts = replay.canonical_receipt_section;
    body.events = replay.canonical_event_section;
    (void)memcpy(body.header.resulting_state_root,
                 replay.resulting_state_root, 32U);
    (void)memcpy(body.header.activity_merkle_root,
                 replay.roots.activity_merkle_root, 32U);
    (void)memcpy(body.header.receipt_merkle_root,
                 replay.roots.receipt_merkle_root, 32U);
    (void)memcpy(body.header.event_merkle_root,
                 replay.roots.event_merkle_root, 32U);
    (void)memcpy(body.header.oracle_root, replay.roots.oracle_root, 32U);
    (void)memcpy(body.header.data_availability_root,
                 replay.roots.data_availability_root, 32U);
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
    sequencer_authorization.first_batch_number = 4U;
    sequencer_authorization.last_batch_number = 4U;
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
    guarantor.protocol_version = 1U;
    guarantor.network_id = 9U;
    guarantor.bond_view.bonded = true;
    guarantor.bond_view.bonded_amount = (lxp_u128){0U, 100U};
    guarantor.replay_engine = &engine;
    guarantor.sequencer_authorization = &sequencer_authorization;
    guarantor.download = download_file;
    guarantor.download_context = &source;
    guarantor.verify_authority = verify_authority;
    guarantor.authority_context = &verifier;
    guarantor.verify_oracle = verify_oracle;
    guarantor.store_availability = store_file;
    guarantor.storage_context = &destination;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 4U, &arena, &ready) != LXP_OK ||
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
    (void)memset(guarantor.independent_state_root, 0, 32U);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 4U, &arena, &ready) !=
            LXP_ERR_BAD_SIGNATURE || ready || guarantor.ready_to_sign ||
        guarantor.last_completed_duty != LXP_GUARANTOR_DUTY_DOWNLOADED)
        goto cleanup;
    verifier.reject_delegation = false;
    destination.path = directory;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_guarantor_process_batch(&guarantor, 4U, &arena, &ready) !=
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
