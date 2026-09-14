#define _POSIX_C_SOURCE 200809L
#include "../../cmd/layerx-guarantor/producer.h"
#include "../../cmd/layerx-guarantor/settlement.h"
#include "layerx/lxp_arena.h"
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

enum { MATERIAL_MAX = 2 * 1024 * 1024, ARENA_BYTES = 16 * 1024 * 1024 };
struct input { const uint8_t *bytes; size_t length, offset; };
struct material {
    gp_settlement_config config;
    lxp_guarantor_cert certificate;
    lxp_daemon_settlement_registration_evidence registration;
    uint64_t first_batch, set_version;
};

static int take(struct input *input, void *destination, size_t count)
{
    if (count > input->length - input->offset)
        return 0;
    memcpy(destination, input->bytes + input->offset, count);
    input->offset += count;
    return 1;
}
static int number(struct input *input, size_t count, uint64_t *value)
{
    uint8_t bytes[8];
    if (count > sizeof(bytes) || !take(input, bytes, count))
        return 0;
    *value = 0U;
    for (size_t i = 0U; i < count; ++i)
        *value = (*value << 8U) | bytes[i];
    return 1;
}
static lxp_result decode(struct input *input, struct material *material)
{
    uint8_t magic[8], header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint64_t length, count;
    gp_settlement_config *config = &material->config;
    lxp_guarantor_cert *certificate = &material->certificate;
    lxp_daemon_settlement_registration_evidence *registration = &material->registration;
    if (!take(input, magic, sizeof(magic)) || memcmp(magic, "LXBFIN1\0", 8U) != 0 ||
        !number(input, 8U, &config->chain_id) || config->chain_id == 0U ||
        !take(input, config->settlement_contract, 20U) ||
        !take(input, config->checkpoint_registry, 20U) ||
        !number(input, 8U, &material->set_version) ||
        !take(input, registration->checkpoint_id, 32U) ||
        !take(input, registration->transaction_id, 32U) ||
        !number(input, 8U, &registration->observed_block_number) ||
        !number(input, 8U, &registration->observed_at_ms) ||
        !number(input, 8U, &material->first_batch) || material->first_batch == 0U ||
        !take(input, header, sizeof(header)))
        return LXP_ERR_NON_CANONICAL;
    lxp_result status = lxp_batch_header_decode(header, sizeof(header), &certificate->checkpoint.header);
    if (status != LXP_OK)
        return status;
    config->network_id = certificate->checkpoint.header.network_id;
    if (certificate->checkpoint.header.protocol_version != 3U || config->network_id != 77U ||
        certificate->checkpoint.header.batch_number < material->first_batch ||
        !number(input, 4U, &length) || length > LXP_MAX_VALIDITY_PROOF_BYTES ||
        length > input->length - input->offset)
        return LXP_ERR_NON_CANONICAL;
    certificate->checkpoint.validity_proof = (lxp_byte_span){input->bytes + input->offset, (size_t)length};
    input->offset += (size_t)length;
    if (!number(input, 4U, &count) || count == 0U || count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_NON_CANONICAL;
    config->member_count = certificate->attestation_count = (size_t)count;
    for (size_t i = 0U; i < (size_t)count; ++i) {
        uint8_t encoded[GP_ATTESTATION_BYTES];
        if (!take(input, config->members[i].guarantor_id, 32U) ||
            !take(input, config->members[i].public_key, 33U) ||
            !take(input, encoded, sizeof(encoded)))
            return LXP_ERR_NON_CANONICAL;
        status = gp_attestation_decode(encoded, sizeof(encoded), &certificate->attestations[i]);
        if (status != LXP_OK)
            return status;
        if (memcmp(config->members[i].guarantor_id, certificate->attestations[i].guarantor_id, 32U) != 0 ||
            (i != 0U && memcmp(config->members[i - 1U].guarantor_id, config->members[i].guarantor_id, 32U) >= 0))
            return LXP_ERR_NON_CANONICAL;
    }
    registration->paxeer_chain_id = config->chain_id;
    memcpy(registration->settlement_contract, config->settlement_contract, 20U);
    return input->offset == input->length ? LXP_OK : LXP_ERR_TRAILING_BYTES;
}
static lxp_result load(const char *path, uint8_t *bytes, size_t *length)
{
    struct stat info;
    int fd = open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC);
    if (fd < 0)
        return LXP_ERR_IO;
    lxp_result status = LXP_OK;
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) || info.st_uid != geteuid() ||
        info.st_nlink != 1 || (info.st_mode & 0077) != 0 || info.st_size <= 0 || info.st_size > MATERIAL_MAX)
        status = LXP_ERR_AUTH_SCOPE;
    *length = 0U;
    while (status == LXP_OK && *length < (size_t)info.st_size) {
        ssize_t got = read(fd, bytes + *length, (size_t)info.st_size - *length);
        if (got < 0 && errno == EINTR)
            continue;
        if (got <= 0) {
            status = LXP_ERR_IO;
            break;
        }
        *length += (size_t)got;
    }
    uint8_t trailing;
    if (status == LXP_OK && read(fd, &trailing, 1U) != 0)
        status = LXP_ERR_NON_CANONICAL;
    if (close(fd) != 0)
        status = LXP_ERR_IO;
    return status;
}
static lxp_result emit(struct material *material, lxp_arena *arena, const char *checkpoint_path,
                       const char *finality_path)
{
    gp_settlement_membership_view membership = {0};
    lxp_finalisation_requirements requirements = {0};
    lxp_byte_span checkpoint, finality;
    lxp_guarantor_cert *certificate = &material->certificate;
    gp_settlement_config *config = &material->config;
    lxp_result status = gp_settlement_membership(config, certificate->checkpoint.header.epoch, &membership);
    if (status != LXP_OK)
        return status;
    if (membership.set.version != material->set_version ||
        membership.observed_block_number < material->registration.observed_block_number ||
        membership.maximum_delay != lxp_checkpoint_maximum_attestation_delay_ms() ||
        membership.threshold > certificate->attestation_count)
        return LXP_ERR_CONTEXT_MISMATCH;
    for (size_t i = 0U; status == LXP_OK && i < certificate->attestation_count; ++i)
        status = gp_attestation_accept(&certificate->checkpoint, config->chain_id,
            config->settlement_contract, &membership.set, &certificate->attestations[i],
            certificate->attestations, i, config->state_dir, arena);
    certificate->threshold = membership.threshold;
    certificate->bonded_economic_guarantee = true;
    certificate->validity_proof_present = certificate->checkpoint.validity_proof.length != 0U;
    if (status == LXP_OK)
        status = gp_checkpoint_requirements(&certificate->checkpoint.header,
            material->registration.observed_at_ms, membership.threshold, membership.minimum_bond, &requirements);
    if (status == LXP_OK)
        status = lxp_daemon_finality_evidence_encode(certificate, &membership.set, &requirements,
            certificate->checkpoint.header.batch_number - material->first_batch,
            &material->registration, arena, &checkpoint, &finality);
    if (status == LXP_OK)
        status = gp_file_write(checkpoint_path, checkpoint.bytes, checkpoint.length);
    if (status == LXP_OK)
        status = gp_file_write(finality_path, finality.bytes, finality.length);
    return status;
}
int main(int argc, char **argv)
{
    if (argc != 9) {
        fputs("expected material rpc-url submitter-key-path state-dir python settlement-helper checkpoint finality\n", stderr);
        return 2;
    }
    uint8_t *bytes = malloc(MATERIAL_MAX), *memory = malloc(ARENA_BYTES);
    struct material *material = calloc(1U, sizeof(*material));
    if (bytes == NULL || memory == NULL || material == NULL) {
        free(bytes); free(memory); free(material);
        return 2;
    }
    size_t length = 0U;
    lxp_arena arena;
    lxp_result status = load(argv[1], bytes, &length);
    struct input input = {bytes, length, 0U};
    if (status == LXP_OK)
        status = decode(&input, material);
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, memory, ARENA_BYTES);
    material->config.rpc_url = argv[2];
    material->config.submitter_key_file = argv[3];
    material->config.state_dir = argv[4];
    material->config.python = argv[5];
    material->config.helper = argv[6];
    if (status == LXP_OK)
        status = emit(material, &arena, argv[7], argv[8]);
    if (status != LXP_OK)
        fprintf(stderr, "native Budget finality material refused: %d\n", status);
    free(material); free(memory); free(bytes);
    return status == LXP_OK ? 0 : 1;
}
