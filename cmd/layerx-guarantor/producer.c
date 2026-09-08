#define _POSIX_C_SOURCE 200809L
#define OPENSSL_API_COMPAT 0x10100000L
#include "producer.h"
#include "layerx/lxp_crypto.h"
#include <errno.h>
#include <fcntl.h>
#include <openssl/ec.h>
#include <openssl/obj_mac.h>
#include <openssl/pem.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

static uint64_t get(const uint8_t *p, size_t n)
{
    uint64_t v = 0U;
    for (size_t i = 0U; i < n; ++i)
        v = (v << 8U) | p[i];
    return v;
}
static void put(uint8_t *p, uint64_t v, size_t n)
{
    for (size_t i = 0U; i < n; ++i)
        p[n - 1U - i] = (uint8_t)(v >> (i * 8U));
}
lxp_result gp_attestation_encode(const lxp_guarantor_attestation *a,
                                 uint8_t out[GP_ATTESTATION_BYTES])
{
    if (a == NULL || out == NULL)
        return LXP_ERR_NON_CANONICAL;
    put(out, a->protocol_version, 2U);
    put(out + 2U, a->network_id, 4U);
    put(out + 6U, a->paxeer_chain_id, 8U);
    memcpy(out + 14U, a->paxeer_settlement_contract, 20U);
    put(out + 34U, a->epoch, 8U);
    memcpy(out + 42U, a->checkpoint_id, 32U);
    memcpy(out + 74U, a->checkpoint_hash, 32U);
    memcpy(out + 106U, a->guarantor_id, 32U);
    put(out + 138U, a->batch_number, 8U);
    memcpy(out + 146U, a->data_availability_root, 32U);
    out[178] = a->replayed ? 1U : 0U;
    out[179] = a->da_possessed ? 1U : 0U;
    out[180] = a->availability_class_mask;
    put(out + 181U, a->attested_at_ms, 8U);
    memcpy(out + 189U, a->signer, 20U);
    memcpy(out + 209U, a->signature, 64U);
    out[273] = a->signature_v;
    return LXP_OK;
}
lxp_result gp_attestation_decode(const uint8_t *p, size_t n, lxp_guarantor_attestation *a)
{
    if (p == NULL || a == NULL || n != GP_ATTESTATION_BYTES || p[178] > 1U || p[179] > 1U)
        return LXP_ERR_NON_CANONICAL;
    memset(a, 0, sizeof(*a));
    a->protocol_version = (uint16_t)get(p, 2U);
    a->network_id = (uint32_t)get(p + 2U, 4U);
    a->paxeer_chain_id = get(p + 6U, 8U);
    memcpy(a->paxeer_settlement_contract, p + 14U, 20U);
    a->epoch = get(p + 34U, 8U);
    memcpy(a->checkpoint_id, p + 42U, 32U);
    memcpy(a->checkpoint_hash, p + 74U, 32U);
    memcpy(a->guarantor_id, p + 106U, 32U);
    a->batch_number = get(p + 138U, 8U);
    memcpy(a->data_availability_root, p + 146U, 32U);
    a->replayed = p[178] == 1U;
    a->da_possessed = p[179] == 1U;
    a->availability_class_mask = p[180];
    a->attested_at_ms = get(p + 181U, 8U);
    memcpy(a->signer, p + 189U, 20U);
    memcpy(a->signature, p + 209U, 64U);
    a->signature_v = p[273];
    return LXP_OK;
}
lxp_result gp_key_load(const char *path, lxp_guarantor_ctx *ctx)
{
    struct stat st;
    int fd;
    FILE *file;
    EVP_PKEY *key;
    EC_KEY *ec;
    const EC_GROUP *group;
    const BIGNUM *secret;
    lxp_result result = LXP_ERR_BAD_SIGNATURE;
    if (path == NULL || ctx == NULL)
        return LXP_ERR_NON_CANONICAL;
    fd = open(path, O_RDONLY | O_NOFOLLOW | O_CLOEXEC);
    if (fd < 0)
        return LXP_ERR_IO;
    if (fstat(fd, &st) != 0 || !S_ISREG(st.st_mode) || (st.st_mode & 0007) != 0 ||
        st.st_size > 8192) {
        (void)close(fd);
        return LXP_ERR_AUTH_SCOPE;
    }
    file = fdopen(fd, "r");
    if (file == NULL) {
        (void)close(fd);
        return LXP_ERR_IO;
    }
    key = PEM_read_PrivateKey(file, NULL, NULL, NULL);
    (void)fclose(file);
    ec = key == NULL ? NULL : EVP_PKEY_get1_EC_KEY(key);
    group = ec == NULL ? NULL : EC_KEY_get0_group(ec);
    secret = ec == NULL ? NULL : EC_KEY_get0_private_key(ec);
    if (group != NULL && secret != NULL && EC_GROUP_get_curve_name(group) == NID_secp256k1 &&
        EC_KEY_check_key(ec) == 1 && BN_bn2binpad(secret, ctx->paxeer_private_key, 32) == 32 &&
        EC_POINT_point2oct(group, EC_KEY_get0_public_key(ec), POINT_CONVERSION_COMPRESSED,
                           ctx->paxeer_public_key, 33U, NULL) == 33U)
        result = LXP_OK;
    EC_KEY_free(ec);
    EVP_PKEY_free(key);
    if (result != LXP_OK)
        lxp_secure_zero(ctx->paxeer_private_key, 32U);
    return result;
}
lxp_result gp_file_write(const char *path, const uint8_t *bytes, size_t length)
{
    char temporary[4096], directory[4096];
    char *slash;
    int fd, parent;
    size_t offset = 0U;
    lxp_result status = LXP_OK;
    int size;
    if (path == NULL || (bytes == NULL && length != 0U) || strlen(path) >= sizeof(directory))
        return LXP_ERR_NON_CANONICAL;
    size = snprintf(temporary, sizeof(temporary), "%s.tmp.XXXXXX", path);
    if (size < 0 || (size_t)size >= sizeof(temporary))
        return LXP_ERR_LENGTH_LIMIT;
    strcpy(directory, path);
    slash = strrchr(directory, '/');
    if (slash == NULL)
        strcpy(directory, ".");
    else if (slash == directory)
        slash[1] = '\0';
    else
        *slash = '\0';
    parent = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (parent < 0)
        return LXP_ERR_IO;
    fd = mkstemp(temporary);
    if (fd < 0) {
        (void)close(parent);
        return LXP_ERR_IO;
    }
    while (offset < length) {
        ssize_t n = write(fd, bytes + offset, length - offset);
        if (n < 0 && errno == EINTR)
            continue;
        if (n <= 0) {
            status = LXP_ERR_IO;
            break;
        }
        offset += (size_t)n;
    }
    if (status == LXP_OK && fsync(fd) != 0)
        status = LXP_ERR_IO;
    if (close(fd) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK && rename(temporary, path) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK && fsync(parent) != 0)
        status = LXP_ERR_IO;
    (void)close(parent);
    if (status != LXP_OK)
        (void)unlink(temporary);
    return status;
}
lxp_result gp_verify_replay(lxp_guarantor_ctx *ctx, const lxp_da_bundle *bundle,
                            const lxp_batch_header *header, const uint8_t signature[64],
                            const lxp_da_store *store, lxp_arena *arena, const char **field)
{
    lxp_batch_body body;
    lxp_replay_batch_result replay = {0};
    lxp_batch_roots roots;
    lxp_result status;
    if (ctx == NULL || bundle == NULL || header == NULL || signature == NULL || store == NULL ||
        arena == NULL || field == NULL)
        return LXP_ERR_NON_CANONICAL;
    ctx->ready_to_sign = false;
    ctx->possesses_availability = false;
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_NONE;
    *field = "availability";
    status = lxp_da_bundle_body(bundle, header, arena, &body);
    if (status != LXP_OK)
        return status;
    memcpy(body.sequencer_signature, signature, 64U);
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_DOWNLOADED;
    *field = "signature/authority";
    status = lxp_guarantor_verify_signatures(ctx, &body, arena);
    if (status != LXP_OK)
        return status;
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_SIGNATURES;
    *field = "replay/state_diff/recovery_metadata";
    status = lxp_replay_batch_publication(ctx->replay_engine, &body, ctx->independent_state_root,
                                          arena, &replay);
    if (status != LXP_OK) {
        if (!lxp_ct_is_zero(replay.resulting_state_root, 32U)) {
            if (lxp_ct_memcmp(replay.resulting_state_root, header->resulting_state_root, 32U) != 0)
                *field = "resulting_state_root";
            else if (lxp_ct_memcmp(replay.roots.activity_merkle_root, header->activity_merkle_root,
                                   32U) != 0)
                *field = "activity_merkle_root";
            else if (lxp_ct_memcmp(replay.roots.receipt_merkle_root, header->receipt_merkle_root,
                                   32U) != 0)
                *field = "receipt_merkle_root";
            else if (lxp_ct_memcmp(replay.roots.event_merkle_root, header->event_merkle_root,
                                   32U) != 0)
                *field = "event_merkle_root";
            else if (lxp_ct_memcmp(replay.roots.oracle_root, header->oracle_root, 32U) != 0)
                *field = "oracle_root";
            else if (lxp_ct_memcmp(replay.roots.data_availability_root,
                                   header->data_availability_root, 32U) != 0)
                *field = "data_availability_root";
        }
        return status;
    }
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_REPLAYED;
    *field = "resulting_state_root";
    if (lxp_ct_memcmp(replay.resulting_state_root, header->resulting_state_root, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
#define CHECK_ROOT(name)                                                                           \
    do {                                                                                           \
        *field = #name;                                                                            \
        if (lxp_ct_memcmp(replay.roots.name, header->name, 32U) != 0)                              \
            return LXP_ERR_ROOT_MISMATCH;                                                          \
    } while (0)
    CHECK_ROOT(activity_merkle_root);
    CHECK_ROOT(receipt_merkle_root);
    CHECK_ROOT(event_merkle_root);
    CHECK_ROOT(oracle_root);
    CHECK_ROOT(data_availability_root);
#undef CHECK_ROOT
    status = lxp_guarantor_recompute_roots(&body, &replay, arena, &roots);
    if (status != LXP_OK)
        return status;
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_ROOTS;
    *field = "durable_availability";
    status = lxp_da_store_bundle(store, bundle, arena);
    if (status != LXP_OK)
        return status;
    ctx->possesses_availability = true;
    ctx->ready_to_sign = true;
    ctx->last_completed_duty = LXP_GUARANTOR_DUTY_READY_TO_SIGN;
    memcpy(ctx->independent_state_root, replay.resulting_state_root, 32U);
    return LXP_OK;
}
lxp_result gp_verify_attest(lxp_guarantor_ctx *ctx, const lxp_da_bundle *bundle,
                            const lxp_batch_header *header, const uint8_t signature[64],
                            const lxp_da_store *store, uint64_t timestamp, lxp_arena *arena,
                            lxp_guarantor_attestation *attestation, const char **field)
{
    lxp_checkpoint_certificate checkpoint;
    lxp_result status;
    if (attestation == NULL || header == NULL || field == NULL)
        return LXP_ERR_NON_CANONICAL;
    memset(attestation, 0, sizeof(*attestation));
    *field = "header.timestamp_ms";
    if (timestamp < header->timestamp_ms ||
        timestamp - header->timestamp_ms > lxp_checkpoint_maximum_attestation_delay_ms())
        return LXP_ERR_CONTEXT_MISMATCH;
    status = gp_verify_replay(ctx, bundle, header, signature, store, arena, field);
    if (status != LXP_OK)
        return status;
    checkpoint = (lxp_checkpoint_certificate){*header, {NULL, 0U}};
    *field = "attestation";
    return lxp_guarantor_attest(ctx, &checkpoint, true, true, timestamp, arena, attestation);
}

lxp_result gp_attestation_accept(const lxp_checkpoint_certificate *checkpoint, uint64_t chain,
                                 const uint8_t settlement[20], const lxp_guarantor_set *set,
                                 const lxp_guarantor_attestation *incoming,
                                 const lxp_guarantor_attestation *previous, size_t count,
                                 const char *evidence_directory, lxp_arena *arena)
{
    uint8_t id[32], key[33];
    const lxp_guarantor_bond_state *member = NULL;
    lxp_result status;
    if (checkpoint == NULL || settlement == NULL || set == NULL || incoming == NULL ||
        arena == NULL || (previous == NULL && count != 0U) ||
        count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < set->count; ++i)
        if (memcmp(set->records[i].guarantor_id, incoming->guarantor_id, 32U) == 0)
            member = &set->records[i];
    if (member == NULL || !member->active || member->jailed || member->unresolved_slashing ||
        member->joined_epoch > incoming->epoch ||
        (member->removed_epoch != 0U && member->removed_epoch <= incoming->epoch))
        return LXP_ERR_ATTESTATION_THRESHOLD;
    status = lxp_guarantor_signer_at_epoch(member, incoming->epoch, key);
    if (status == LXP_OK)
        status = lxp_guarantor_attestation_verify(incoming, key);
    if (status != LXP_OK)
        return status;
    for (size_t i = 0U; i < count; ++i) {
        lxp_equivocation_evidence evidence;
        if (lxp_equivocation_detect(LXP_EQUIVOCATION_GUARANTOR, &previous[i], incoming, key,
                                    sizeof(key), &evidence) == LXP_OK) {
            lxp_byte_span encoded;
            char path[4096], identity[65];
            int n;
            if (evidence_directory == NULL)
                return LXP_ERR_IO;
            status = lxp_equivocation_verify(&evidence, arena);
            if (status == LXP_OK)
                status = lxp_equivocation_encode(&evidence, arena, &encoded);
            for (size_t j = 0U; j < 32U; ++j)
                (void)snprintf(identity + 2U * j, 3U, "%02x", incoming->guarantor_id[j]);
            n = snprintf(path, sizeof(path), "%s/%llu-%llu-%s-kind1.bin", evidence_directory,
                         (unsigned long long)incoming->epoch,
                         (unsigned long long)incoming->batch_number, identity);
            if (n < 0 || (size_t)n >= sizeof(path))
                return LXP_ERR_LENGTH_LIMIT;
            if (status == LXP_OK)
                status = gp_file_write(path, encoded.bytes, encoded.length);
            return status == LXP_OK ? LXP_ERR_ROOT_MISMATCH : status;
        }
    }
    status = lxp_checkpoint_certificate_hash(checkpoint, arena, id);
    if (status != LXP_OK)
        return status;
    if (incoming->protocol_version != checkpoint->header.protocol_version ||
        incoming->network_id != checkpoint->header.network_id ||
        incoming->paxeer_chain_id != chain ||
        memcmp(incoming->paxeer_settlement_contract, settlement, 20U) != 0 ||
        memcmp(id, incoming->checkpoint_id, 32U) != 0 ||
        incoming->epoch != checkpoint->header.epoch ||
        incoming->batch_number != checkpoint->header.batch_number ||
        memcmp(incoming->data_availability_root, checkpoint->header.data_availability_root, 32U) !=
            0 ||
        incoming->attested_at_ms < checkpoint->header.timestamp_ms ||
        incoming->attested_at_ms - checkpoint->header.timestamp_ms >
            lxp_checkpoint_maximum_attestation_delay_ms())
        return LXP_ERR_CONTEXT_MISMATCH;
    return LXP_OK;
}

lxp_result gp_checkpoint_requirements(const lxp_batch_header *header, uint64_t observed,
                                      size_t threshold, lxp_u128 minimum_bond,
                                      lxp_finalisation_requirements *requirements)
{
    uint64_t delay = lxp_checkpoint_maximum_attestation_delay_ms();
    if (header == NULL || requirements == NULL || observed == 0U || threshold == 0U ||
        threshold > LXP_MAX_GUARANTOR_ATTESTATIONS || header->timestamp_ms > UINT64_MAX - delay)
        return LXP_ERR_NON_CANONICAL;
    memset(requirements, 0, sizeof(*requirements));
    requirements->checkpoint_epoch = header->epoch;
    requirements->challenge_window_end_ms = observed;
    requirements->checkpoint_deadline_ms = header->timestamp_ms + delay;
    requirements->now_ms = observed;
    requirements->threshold = threshold;
    requirements->minimum_bond = minimum_bond;
    requirements->availability_challenges_answered = true;
    return LXP_OK;
}
