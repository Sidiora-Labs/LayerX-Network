#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_handover.h"
#include "../layerxd/lxp_daemon_artifact.h"
#include "../layerxd/lxp_daemon_finality_authority.h"
#include "../layerxd/lxp_daemon_handover_history.h"

#include <openssl/evp.h>

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum { HANDOVER_ARENA_BYTES = 128 * 1024 * 1024 };

typedef struct handover_issuer {
    lxp_handover_trust_chain chain;
    lxp_daemon_finality_authority authority;
    lxp_finalisation_state finalisation;
    lxp_arena arena;
    bool enabled;
    const lxp_log *history_log;
} handover_issuer;

static lxp_result hex_bytes(const char *text, uint8_t *bytes, size_t length)
{
    if (text == NULL || strlen(text) != length * 2U) return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < length; ++i) {
        unsigned int value = 0U;
        for (size_t j = 0U; j < 2U; ++j) {
            char digit = text[i * 2U + j];
            if (digit >= '0' && digit <= '9') value = value * 16U + (unsigned int)(digit - '0');
            else if (digit >= 'a' && digit <= 'f') value = value * 16U + (unsigned int)(digit - 'a') + 10U;
            else return LXP_ERR_NON_CANONICAL;
        }
        bytes[i] = (uint8_t)value;
    }
    return LXP_OK;
}

static lxp_result decimal_u64(const char *text, uint64_t *value)
{
    uint64_t result = 0U;
    if (text == NULL || text[0] == '\0' || (text[0] == '0' && text[1] != '\0'))
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; text[i] != '\0'; ++i) {
        unsigned int digit;
        if (text[i] < '0' || text[i] > '9') return LXP_ERR_NON_CANONICAL;
        digit = (unsigned int)(text[i] - '0');
        if (result > (UINT64_MAX - digit) / 10U) return LXP_ERR_LENGTH_LIMIT;
        result = result * 10U + digit;
    }
    *value = result;
    return LXP_OK;
}

static lxp_result finality_verify(void *context,
    const lxp_batch_header *predecessor, const uint8_t signature[64],
    const lxp_handover_evidence *evidence, lxp_arena *arena)
{
    handover_issuer *issuer = context;
    return lxp_daemon_handover_finality_verify(&issuer->authority,
        &issuer->finalisation, predecessor, signature, evidence, arena);
}

static lxp_result authorize_wal(void *context, const lxp_daemon_batch_wal_input *input)
{
    handover_issuer *issuer = context;
    return lxp_daemon_handover_wal_verify(&issuer->chain, issuer->history_log,
        input, finality_verify, issuer, &issuer->arena);
}

static lxp_result load_history(handover_issuer *issuer,
    const char *manifest_path, const char *log_path)
{
    lxp_genesis_manifest *manifest = NULL;
    lxp_log log;
    uint8_t *bytes = NULL;
    size_t length = 0U;
    char checkpoint_directory[PATH_MAX];
    const char *slash = strrchr(log_path, '/');
    size_t directory_length = slash == NULL ? 1U : slash == log_path ? 1U : (size_t)(slash - log_path);
    bool opened = false;
    struct stat information;
    lxp_result status;
    if (directory_length >= sizeof(checkpoint_directory)) return LXP_ERR_LENGTH_LIMIT;
    if (slash == NULL) checkpoint_directory[0] = '.';
    else (void)memcpy(checkpoint_directory, log_path, directory_length);
    checkpoint_directory[directory_length] = '\0';
    status = lxp_daemon_artifact_read(manifest_path,
        LXP_GENESIS_MAX_ENCODED_BYTES, 0U, &bytes, &length);
    if (status == LXP_OK) {
        manifest = malloc(sizeof(*manifest));
        if (manifest == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
    }
    if (status == LXP_OK) status = lxp_genesis_parse(bytes, length,
        LXP_GENESIS_INPUT_MANIFEST, manifest);
    if (status == LXP_OK) status = lxp_genesis_verify_signature(manifest, &issuer->arena);
    if (status == LXP_OK) status = lxp_handover_genesis_authority(manifest,
        issuer->chain.governance_public_key, &issuer->enabled);
    if (status == LXP_OK && !issuer->enabled) {
        (void)memcpy(issuer->chain.current_authorization.public_key, manifest->signer_public_key, 32U);
        free(bytes);
        free(manifest);
        return LXP_OK;
    }
    if (status == LXP_OK) status = lxp_handover_trust_initialize(&issuer->chain, manifest);
    if (status == LXP_OK) status = lxp_daemon_finality_authority_init_pins(&issuer->authority);
    if (status == LXP_OK && lstat(log_path, &information) != 0) {
        if (errno != ENOENT) status = LXP_ERR_IO;
    } else if (status == LXP_OK) {
        if (!S_ISREG(information.st_mode) || information.st_nlink != 1)
            status = LXP_ERR_AUTH_SCOPE;
        if (status == LXP_OK) status = lxp_log_open(&log, log_path);
        if (status == LXP_OK) opened = true;
        if (status == LXP_OK) status = lxp_daemon_handover_history_load(&issuer->chain,
            &log, checkpoint_directory, finality_verify, issuer, &issuer->arena);
        if (status == LXP_OK) {
            lxp_daemon_batch_wal_record *record = NULL;
            bool present = false;
            issuer->history_log = &log;
            status = lxp_daemon_batch_wal_read_authorized(checkpoint_directory,
                authorize_wal, issuer, &record, &present);
            if (status == LXP_OK && present) {
                const lxp_daemon_batch_wal_input *input = lxp_daemon_batch_wal_view(record);
                if (input == NULL) status = LXP_ERR_LOG_CORRUPT;
                else if (input->batch_number > issuer->chain.predecessor.batch_number) {
                    lxp_batch_body body;
                    if (lxp_daemon_batch_wal_record_state(record) != LXP_DAEMON_BATCH_WAL_PREPARED)
                        status = LXP_ERR_LOG_CORRUPT;
                    if (status == LXP_OK) status = lxp_daemon_batch_wal_body(input, &issuer->arena, &body);
                    if (status == LXP_OK) status = lxp_handover_trust_accept(&issuer->chain,
                        &body, finality_verify, issuer, &issuer->arena);
                }
            }
            lxp_daemon_batch_wal_destroy(record);
            issuer->history_log = NULL;
        }
    }
    if (opened && lxp_log_close(&log) != LXP_OK && status == LXP_OK) status = LXP_ERR_IO;
    free(bytes);
    free(manifest);
    return status;
}

static lxp_result verify_candidate(handover_issuer *issuer,
    const lxp_handover_evidence *evidence)
{
    lxp_result status;
    if (issuer->chain.predecessor.last_sequence == UINT64_MAX)
        return LXP_ERR_SEQUENCE_EXHAUSTED;
    status = lxp_handover_evidence_verify_binding(evidence,
        issuer->chain.governance_public_key, &issuer->chain.current_authorization,
        issuer->chain.epoch, issuer->chain.predecessor.batch_number,
        issuer->chain.predecessor.last_sequence + 1U,
        issuer->chain.predecessor.resulting_state_root, &issuer->arena);
    if (status == LXP_OK) status = finality_verify(issuer, &issuer->chain.predecessor,
        issuer->chain.predecessor_signature, evidence, &issuer->arena);
    return status;
}

static lxp_result verify_key(handover_issuer *issuer, const char *public_key_text,
    const char *activity_path)
{
    uint8_t public_key[32];
    uint8_t *bytes = NULL;
    size_t length = 0U;
    lxp_activity activity;
    lxp_handover_evidence evidence;
    lxp_result status = hex_bytes(public_key_text, public_key, sizeof(public_key));
    if (status != LXP_OK) return status;
    if (memcmp(public_key, issuer->chain.current_authorization.public_key, 32U) == 0)
        return LXP_OK;
    if (!issuer->enabled || activity_path == NULL) return LXP_ERR_AUTH_SCOPE;
    status = lxp_daemon_artifact_read(activity_path, LXP_MAX_ACTIVITY_BYTES, 0U, &bytes, &length);
    if (status == LXP_OK) status = lxp_activity_decode(bytes, length, &activity);
    if (status == LXP_OK) status = lxp_activity_check_envelope(&activity, issuer->chain.network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK && (activity.activity_type != LXP_GOVERNANCE_HANDOVER ||
        activity.authority.length != 32U || memcmp(activity.authority.bytes,
            issuer->chain.governance_public_key, 32U) != 0)) status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK) status = lxp_handover_evidence_decode(activity.payload, &evidence);
    if (status == LXP_OK) status = verify_candidate(issuer, &evidence);
    if (status == LXP_OK && memcmp(public_key, evidence.certificate.new_public_key, 32U) != 0)
        status = LXP_ERR_AUTH_SCOPE;
    free(bytes);
    return status;
}

static lxp_result read_private_key(const char *path, uint8_t key[32])
{
    struct stat before, after;
    size_t offset = 0U;
    int descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    lxp_result status = LXP_OK;
    if (descriptor < 0) return LXP_ERR_IO;
    if (fstat(descriptor, &before) != 0 || !S_ISREG(before.st_mode) || before.st_nlink != 1 ||
        before.st_uid != geteuid() || (before.st_mode & 0777) != 0600 || before.st_size != 32)
        status = LXP_ERR_AUTH_SCOPE;
    while (status == LXP_OK && offset < 32U) {
        ssize_t count = read(descriptor, key + offset, 32U - offset);
        if (count > 0) offset += (size_t)count;
        else if (count < 0 && errno == EINTR) continue;
        else status = LXP_ERR_IO;
    }
    if (status == LXP_OK && (fstat(descriptor, &after) != 0 ||
        before.st_dev != after.st_dev || before.st_ino != after.st_ino ||
        before.st_size != after.st_size || before.st_mode != after.st_mode ||
        before.st_uid != after.st_uid || after.st_nlink != 1)) status = LXP_ERR_AUTH_SCOPE;
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status != LXP_OK) lxp_secure_zero(key, 32U);
    return status;
}

static lxp_result publish_activity(const char *path, lxp_byte_span activity)
{
    char temporary[PATH_MAX], directory[PATH_MAX];
    const char *slash = strrchr(path, '/');
    size_t parent_length = slash == NULL ? 1U : slash == path ? 1U : (size_t)(slash - path);
    size_t offset = 0U;
    int descriptor = -1, parent = -1;
    int length = snprintf(temporary, sizeof(temporary), "%s.tmp.XXXXXX", path);
    lxp_result status = LXP_OK;
    if (length < 0 || (size_t)length >= sizeof(temporary) || parent_length >= sizeof(directory))
        return LXP_ERR_LENGTH_LIMIT;
    if (slash == NULL) directory[0] = '.';
    else (void)memcpy(directory, path, parent_length);
    directory[parent_length] = '\0';
    parent = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (parent < 0) return LXP_ERR_IO;
    descriptor = mkstemp(temporary);
    if (descriptor < 0) status = LXP_ERR_IO;
    while (status == LXP_OK && offset < activity.length) {
        ssize_t count = write(descriptor, activity.bytes + offset, activity.length - offset);
        if (count > 0) offset += (size_t)count;
        else if (count < 0 && errno == EINTR) continue;
        else status = LXP_ERR_IO;
    }
    if (status == LXP_OK && fsync(descriptor) != 0) status = LXP_ERR_IO;
    if (descriptor >= 0 && close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status == LXP_OK && link(temporary, path) != 0) status = LXP_ERR_IO;
    if (descriptor >= 0 && unlink(temporary) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status == LXP_OK && fsync(parent) != 0) status = LXP_ERR_IO;
    if (close(parent) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result issue(handover_issuer *issuer, char **arguments)
{
    lxp_handover_evidence evidence = {0};
    lxp_handover_certificate *certificate = &evidence.certificate;
    lxp_activity activity = {0};
    lxp_byte_span encoded;
    uint8_t *checkpoint = NULL, *proof = NULL;
    uint8_t private_key[32] = {0}, public_key[32], signature[64], preimage[32];
    char did[76];
    size_t checkpoint_length = 0U, proof_length = 0U, public_length = 32U, signature_length = 64U;
    EVP_PKEY *key = NULL;
    EVP_MD_CTX *signing = NULL;
    lxp_result status;
    if (!issuer->enabled) return LXP_ERR_AUTH_SCOPE;
    if (issuer->chain.epoch == UINT64_MAX || issuer->chain.predecessor.batch_number == UINT64_MAX ||
        issuer->chain.predecessor.last_sequence == UINT64_MAX)
        return LXP_ERR_SEQUENCE_EXHAUSTED;
    if (issuer->chain.predecessor.batch_number == 0U) return LXP_ERR_AUTH_SCOPE;
    status = lxp_daemon_artifact_read(arguments[4], LXP_HANDOVER_MAX_EVIDENCE_BYTES,
        0U, &checkpoint, &checkpoint_length);
    if (status == LXP_OK) status = lxp_daemon_artifact_read(arguments[5],
        LXP_HANDOVER_MAX_EVIDENCE_BYTES, 0U, &proof, &proof_length);
    if (status == LXP_OK) status = read_private_key(arguments[6], private_key);
    if (status == LXP_OK) {
        key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, private_key, 32U);
        if (key == NULL || EVP_PKEY_get_raw_public_key(key, public_key, &public_length) != 1 ||
            public_length != 32U || memcmp(public_key, issuer->chain.governance_public_key, 32U) != 0)
            status = LXP_ERR_AUTH_SCOPE;
    }
    certificate->network_id = issuer->chain.network_id;
    certificate->protocol_version = issuer->chain.predecessor.protocol_version;
    certificate->old_epoch = issuer->chain.epoch;
    certificate->new_epoch = issuer->chain.epoch + 1U;
    certificate->predecessor_batch = issuer->chain.predecessor.batch_number;
    certificate->predecessor_last_sequence = issuer->chain.predecessor.last_sequence;
    certificate->activation_batch = issuer->chain.predecessor.batch_number + 1U;
    (void)memcpy(certificate->old_public_key, issuer->chain.current_authorization.public_key, 32U);
    (void)memcpy(certificate->old_sequencer_id, issuer->chain.current_authorization.sequencer_id, 32U);
    (void)memcpy(certificate->predecessor_state_root, issuer->chain.predecessor.resulting_state_root, 32U);
    (void)memcpy(evidence.predecessor_signature, issuer->chain.predecessor_signature, 64U);
    evidence.checkpoint_payload = (lxp_byte_span){checkpoint, checkpoint_length};
    evidence.finality_proof = (lxp_byte_span){proof, proof_length};
    if (status == LXP_OK) status = hex_bytes(arguments[7], certificate->new_public_key, 32U);
    if (status == LXP_OK) status = lxp_handover_sequencer_id(certificate->new_public_key,
        certificate->new_sequencer_id);
    if (status == LXP_OK) status = hex_bytes(arguments[8], certificate->predecessor_checkpoint_id, 32U);
    if (status == LXP_OK) status = lxp_batch_header_hash(&issuer->chain.predecessor,
        &issuer->arena, certificate->predecessor_header_hash);
    if (status == LXP_OK) status = lxp_batch_header_encode(&issuer->chain.predecessor,
        &issuer->arena, &evidence.predecessor_header);
    if (status == LXP_OK) status = lxp_handover_finality_digest(evidence.checkpoint_payload,
        evidence.finality_proof, certificate->finality_evidence_digest);
    if (status == LXP_OK) status = lxp_handover_certificate_sign(certificate, private_key);
    if (status == LXP_OK) status = verify_candidate(issuer, &evidence);
    if (status == LXP_OK) status = lxp_handover_evidence_encode(&evidence, &issuer->arena, &activity.payload);
    activity.protocol_version = certificate->protocol_version;
    activity.network_id = certificate->network_id;
    activity.activity_type = LXP_GOVERNANCE_HANDOVER;
    (void)memcpy(did, "did:layerx:", 11U);
    for (size_t i = 0U; i < 32U; ++i) {
        static const char digits[] = "0123456789abcdef";
        did[11U + i * 2U] = digits[issuer->chain.governance_public_key[i] >> 4U];
        did[12U + i * 2U] = digits[issuer->chain.governance_public_key[i] & 15U];
    }
    did[75] = '\0';
    activity.actor_did = (lxp_byte_span){(const uint8_t *)did, 75U};
    activity.authority = (lxp_byte_span){issuer->chain.governance_public_key, 32U};
    if (status == LXP_OK) status = decimal_u64(arguments[9], &activity.account_sequence);
    if (status == LXP_OK) status = decimal_u64(arguments[10], &activity.timestamp_bound.not_before);
    if (status == LXP_OK) status = decimal_u64(arguments[11], &activity.timestamp_bound.not_after);
    if (status == LXP_OK) status = decimal_u64(arguments[12], &activity.fee_limit.lo);
    if (status == LXP_OK) status = hex_bytes(arguments[13], activity.idempotency_key, 32U);
    if (status == LXP_OK) status = lxp_hash_payload(activity.payload.bytes,
        activity.payload.length, activity.payload_hash);
    if (status == LXP_OK) status = lxp_activity_signing_preimage(&activity, preimage);
    if (status == LXP_OK) {
        signing = EVP_MD_CTX_new();
        if (signing == NULL || EVP_DigestSignInit(signing, NULL, NULL, NULL, key) != 1 ||
            EVP_DigestSign(signing, signature, &signature_length, preimage, sizeof(preimage)) != 1 ||
            signature_length != 64U) status = LXP_ERR_BAD_SIGNATURE;
    }
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    if (status == LXP_OK) status = lxp_activity_check_envelope(&activity, issuer->chain.network_id);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK) status = lxp_activity_encode(&activity, &issuer->arena, &encoded);
    if (status == LXP_OK) status = publish_activity(arguments[14], encoded);
    EVP_MD_CTX_free(signing);
    EVP_PKEY_free(key);
    lxp_secure_zero(private_key, sizeof(private_key));
    free(checkpoint);
    free(proof);
    return status;
}

int main(int argc, char **argv)
{
    handover_issuer *issuer;
    uint8_t *memory;
    bool issuing = argc == 15 && strcmp(argv[1], "--issue") == 0;
    bool verifying = (argc == 5 || argc == 6) && strcmp(argv[1], "--verify-key") == 0;
    lxp_result status;
    if (!issuing && !verifying) {
        (void)fprintf(stderr, "usage: layerx-handover --verify-key MANIFEST DA_LOG PUBLIC_KEY [ACTIVITY]\n"
            "       layerx-handover --issue MANIFEST DA_LOG CHECKPOINT FINALITY GOVERNANCE_KEY NEW_PUBLIC_KEY CHECKPOINT_ID IDENTITY_SEQUENCE NOT_BEFORE NOT_AFTER FEE_LIMIT IDEMPOTENCY_KEY OUTPUT\n");
        return 2;
    }
    issuer = calloc(1U, sizeof(*issuer));
    memory = malloc(HANDOVER_ARENA_BYTES);
    status = issuer == NULL || memory == NULL ? LXP_ERR_ARENA_EXHAUSTED :
        lxp_arena_init(&issuer->arena, memory, HANDOVER_ARENA_BYTES);
    if (status == LXP_OK) status = load_history(issuer, argv[2], argv[3]);
    if (status == LXP_OK) status = issuing ? issue(issuer, argv) :
        verify_key(issuer, argv[4], argc == 6 ? argv[5] : NULL);
    if (status != LXP_OK) (void)fprintf(stderr, "layerx-handover: refused with result %d\n", (int)status);
    free(memory);
    free(issuer);
    return status == LXP_OK ? 0 : 1;
}
