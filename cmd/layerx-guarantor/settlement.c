#define _POSIX_C_SOURCE 200809L
#include "settlement.h"
#include "layerx/lxp_crypto.h"
#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/wait.h>
#include <unistd.h>

enum {
    GP_SETTLEMENT_PATH = 4096,
    GP_MEMBERSHIP_RECORD = 85,
    GP_MEMBERSHIP_PREFIX = 40,
    GP_REGISTRATION_WIRE = 57
};
typedef struct gp_files {
    char directory[GP_SETTLEMENT_PATH];
    char input[GP_SETTLEMENT_PATH];
    char output[GP_SETTLEMENT_PATH];
    char wire[GP_SETTLEMENT_PATH];
} gp_files;
static void hex(FILE *file, const uint8_t *bytes, size_t length)
{
    size_t i;
    (void)fputs("\"0x", file);
    for (i = 0U; i < length; ++i)
        (void)fprintf(file, "%02x", bytes[i]);
    (void)fputc('"', file);
}
static void quoted(FILE *file, const char *value)
{
    const unsigned char *p = (const unsigned char *)value;
    (void)fputc('"', file);
    while (*p != 0U) {
        if (*p == '"' || *p == '\\')
            (void)fputc('\\', file);
        if (*p < 32U)
            (void)fprintf(file, "\\u%04x", *p);
        else
            (void)fputc(*p, file);
        ++p;
    }
    (void)fputc('"', file);
}
static uint64_t read64(const uint8_t *p)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i)
        value = (value << 8U) | p[i];
    return value;
}
static uint32_t read32(const uint8_t *p)
{
    return ((uint32_t)p[0] << 24U) | ((uint32_t)p[1] << 16U) | ((uint32_t)p[2] << 8U) | p[3];
}
static void cleanup(gp_files *files)
{
    (void)unlink(files->input);
    (void)unlink(files->output);
    (void)unlink(files->wire);
    (void)rmdir(files->directory);
}
static int valid_config(const gp_settlement_config *config)
{
    return config != NULL && config->python != NULL && config->helper != NULL &&
           config->state_dir != NULL && config->rpc_url != NULL &&
           config->submitter_key_file != NULL && config->chain_id != 0U &&
           config->member_count > 0U && config->member_count <= LXP_MAX_GUARANTOR_ATTESTATIONS;
}
static lxp_result begin_files(const gp_settlement_config *config, gp_files *files, FILE **output)
{
    int length, fd;
    if (config == NULL || files == NULL || output == NULL || config->python == NULL ||
        config->helper == NULL || config->state_dir == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(files, 0, sizeof(*files));
    length = snprintf(files->directory, sizeof(files->directory), "%s/settlement-XXXXXX",
                      config->state_dir);
    if (length < 0 || (size_t)length >= sizeof(files->directory) - 16U)
        return LXP_ERR_LENGTH_LIMIT;
    if (mkdtemp(files->directory) == NULL)
        return LXP_ERR_IO;
    (void)memcpy(files->input, files->directory, (size_t)length);
    (void)memcpy(files->input + (size_t)length, "/request.json", 14U);
    (void)memcpy(files->output, files->directory, (size_t)length);
    (void)memcpy(files->output + (size_t)length, "/result.json", 13U);
    (void)memcpy(files->wire, files->directory, (size_t)length);
    (void)memcpy(files->wire + (size_t)length, "/result.wire", 13U);
    fd = open(files->input, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) {
        cleanup(files);
        return LXP_ERR_IO;
    }
    *output = fdopen(fd, "w");
    if (*output == NULL) {
        (void)close(fd);
        cleanup(files);
        return LXP_ERR_IO;
    }
    (void)fputs("{\"wire_output\":", *output);
    quoted(*output, files->wire);
    return LXP_OK;
}
static lxp_result begin(const gp_settlement_config *config, gp_files *files, FILE **output)
{
    lxp_result status;
    if (!valid_config(config))
        return LXP_ERR_NON_CANONICAL;
    status = begin_files(config, files, output);
    if (status != LXP_OK)
        return status;
    (void)fputs(",\"rpc_url\":", *output);
    quoted(*output, config->rpc_url);
    (void)fprintf(*output, ",\"chain_id\":%" PRIu64 ",\"settlement_contract\":", config->chain_id);
    hex(*output, config->settlement_contract, 20U);
    (void)fputs(",\"checkpoint_registry\":", *output);
    hex(*output, config->checkpoint_registry, 20U);
    (void)fputs(",\"submitter_key_file\":", *output);
    quoted(*output, config->submitter_key_file);
    if (config->submitter_lock_file != NULL) {
        (void)fputs(",\"submitter_lock_file\":", *output);
        quoted(*output, config->submitter_lock_file);
    }
    return LXP_OK;
}
static lxp_result execute(const gp_settlement_config *config, const char *mode, gp_files *files,
                          FILE *input, uint8_t *wire, size_t capacity, size_t *length)
{
    pid_t pid, waited;
    int status, fd;
    ssize_t got;
    bool io_failed;
    (void)fputs("}\n", input);
    io_failed = ferror(input) != 0 || fflush(input) != 0;
    if (fclose(input) != 0)
        io_failed = true;
    if (io_failed)
        return LXP_ERR_IO;
    pid = fork();
    if (pid < 0)
        return LXP_ERR_IO;
    if (pid == 0) {
        execlp(config->python, config->python, config->helper, mode, files->input, files->output,
               (char *)NULL);
        _exit(127);
    }
    do {
        waited = waitpid(pid, &status, 0);
    } while (waited < 0 && errno == EINTR);
    if (waited != pid || !WIFEXITED(status) || WEXITSTATUS(status) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    fd = open(files->wire, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (fd < 0)
        return LXP_ERR_IO;
    *length = 0U;
    while (*length < capacity) {
        got = read(fd, wire + *length, capacity - *length);
        if (got < 0 && errno == EINTR)
            continue;
        if (got < 0) {
            (void)close(fd);
            return LXP_ERR_IO;
        }
        if (got == 0)
            break;
        *length += (size_t)got;
    }
    {
        uint8_t extra;
        got = read(fd, &extra, 1U);
        (void)close(fd);
        if (got != 0)
            return LXP_ERR_LENGTH_LIMIT;
    }
    return LXP_OK;
}
lxp_result gp_settlement_membership(const gp_settlement_config *config, uint64_t epoch,
                                    lxp_guarantor_set *set, size_t *threshold,
                                    uint64_t *maximum_delay, lxp_u128 *minimum_bond)
{
    gp_files files;
    FILE *input;
    uint8_t wire[GP_MEMBERSHIP_PREFIX + GP_MEMBERSHIP_RECORD * LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t i, length;
    lxp_guarantor_set result;
    lxp_u128 minimum;
    lxp_result status;
    if (epoch == 0U || set == NULL || threshold == NULL || maximum_delay == NULL ||
        minimum_bond == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = begin(config, &files, &input);
    if (status != LXP_OK)
        return status;
    (void)fprintf(input, ",\"epoch\":%" PRIu64 ",\"guarantors\":[", epoch);
    for (i = 0U; i < config->member_count; ++i) {
        uint8_t signer[20];
        status = lxp_secp256k1_address(config->members[i].public_key, 33U, signer);
        if (status != LXP_OK) {
            (void)fclose(input);
            cleanup(&files);
            return status;
        }
        (void)fputs(i == 0U ? "{\"guarantor_id\":" : ",{\"guarantor_id\":", input);
        hex(input, config->members[i].guarantor_id, 32U);
        (void)fputs(",\"signer\":", input);
        hex(input, signer, 20U);
        (void)fputc('}', input);
    }
    (void)fputc(']', input);
    status = execute(config, "membership", &files, input, wire, sizeof(wire), &length);
    cleanup(&files);
    if (status != LXP_OK)
        return status;
    if (length != GP_MEMBERSHIP_PREFIX + config->member_count * GP_MEMBERSHIP_RECORD ||
        read32(wire + 20U) != config->member_count || read64(wire) == 0U ||
        read32(wire + 8U) == 0U || read32(wire + 8U) > config->member_count)
        return LXP_ERR_CONTEXT_MISMATCH;
    (void)memset(&result, 0, sizeof(result));
    result.version = read64(wire);
    result.count = config->member_count;
    for (i = 0U; i < result.count; ++i) {
        const uint8_t *record = wire + GP_MEMBERSHIP_PREFIX + GP_MEMBERSHIP_RECORD * i;
        lxp_guarantor_bond_state *bond = &result.records[i];
        uint8_t signer[20];
        status = lxp_secp256k1_address(config->members[i].public_key, 33U, signer);
        if (status != LXP_OK || memcmp(record, config->members[i].guarantor_id, 32U) != 0 ||
            memcmp(record + 32U, signer, 20U) != 0 || record[52] != 1U)
            return LXP_ERR_CONTEXT_MISMATCH;
        (void)memcpy(bond->guarantor_id, record, 32U);
        (void)memcpy(bond->public_key, config->members[i].public_key, 33U);
        status = lxp_u128_from_be(record + 53U, &bond->bond_amount);
        if (status != LXP_OK)
            return status;
        bond->joined_epoch = read64(record + 69U);
        bond->active = true;
        bond->signer_authorization_count = 1U;
        (void)memcpy(bond->signer_authorizations[0].public_key, bond->public_key, 33U);
        bond->signer_authorizations[0].active_from_epoch = bond->joined_epoch;
        bond->signer_authorizations[0].set_version = read64(record + 77U);
    }
    status = lxp_guarantor_set_validate(&result);
    if (status != LXP_OK)
        return status;
    status = lxp_u128_from_be(wire + 24U, &minimum);
    if (status != LXP_OK)
        return status;
    *set = result;
    *threshold = read32(wire + 8U);
    *maximum_delay = read64(wire + 12U);
    *minimum_bond = minimum;
    return LXP_OK;
}
static void header_json(FILE *file, const lxp_batch_header *h)
{
    const uint8_t *hashes[] = {h->previous_state_root,  h->resulting_state_root,
                               h->activity_merkle_root, h->receipt_merkle_root,
                               h->event_merkle_root,    h->data_availability_root,
                               h->oracle_root};
    size_t i;
    (void)fprintf(file, "[%u,%" PRIu32 ",%" PRIu64 ",%" PRIu64 ",%" PRIu64 ",%" PRIu64,
                  (unsigned)h->protocol_version, h->network_id, h->epoch, h->batch_number,
                  h->first_sequence, h->last_sequence);
    for (i = 0U; i < 7U; ++i) {
        (void)fputc(',', file);
        hex(file, hashes[i], 32U);
    }
    (void)fprintf(file, ",%" PRIu64 ",", h->timestamp_ms);
    hex(file, h->sequencer_id, 32U);
    (void)fputc(']', file);
}
static void attestation_json(FILE *file, const lxp_guarantor_attestation *a)
{
    (void)fprintf(file, "[%u,%" PRIu32 ",%" PRIu64 ",", (unsigned)a->protocol_version,
                  a->network_id, a->paxeer_chain_id);
    hex(file, a->paxeer_settlement_contract, 20U);
    (void)fprintf(file, ",%" PRIu64 ",", a->epoch);
    hex(file, a->checkpoint_id, 32U);
    (void)fputc(',', file);
    hex(file, a->checkpoint_hash, 32U);
    (void)fputc(',', file);
    hex(file, a->guarantor_id, 32U);
    (void)fprintf(file, ",%" PRIu64 ",", a->batch_number);
    hex(file, a->data_availability_root, 32U);
    (void)fprintf(file, ",%s,%s,%u,%" PRIu64 ",", a->replayed ? "true" : "false",
                  a->da_possessed ? "true" : "false", (unsigned)a->availability_class_mask,
                  a->attested_at_ms);
    hex(file, a->signer, 20U);
    (void)fputc(',', file);
    hex(file, a->signature, 32U);
    (void)fputc(',', file);
    hex(file, a->signature + 32U, 32U);
    (void)fprintf(file, ",%u]", (unsigned)a->signature_v);
}
lxp_result gp_settlement_register(const gp_settlement_config *config,
                                  const lxp_guarantor_cert *certificate, gp_runtime *runtime,
                                  lxp_daemon_settlement_registration_evidence *registration,
                                  bool *already_registered, uint64_t *registered_set_version)
{
    gp_files files;
    FILE *input;
    uint8_t wire[GP_REGISTRATION_WIRE], checkpoint_id[32];
    uint8_t *memory;
    lxp_arena arena;
    size_t i, length;
    lxp_result status;
    lxp_daemon_settlement_registration_evidence result;
    if (certificate == NULL || registration == NULL || already_registered == NULL ||
        registered_set_version == NULL || certificate->attestation_count == 0U ||
        certificate->attestation_count > LXP_MAX_GUARANTOR_ATTESTATIONS)
        return LXP_ERR_NON_CANONICAL;
    memory = malloc(LXP_MAX_VALIDITY_PROOF_BYTES + 4096U);
    if (memory == NULL)
        return LXP_ERR_IO;
    status = lxp_arena_init(&arena, memory, LXP_MAX_VALIDITY_PROOF_BYTES + 4096U);
    if (status == LXP_OK)
        status = lxp_checkpoint_certificate_hash(&certificate->checkpoint, &arena, checkpoint_id);
    free(memory);
    if (status != LXP_OK)
        return status;
    status = begin(config, &files, &input);
    if (status != LXP_OK)
        return status;
    (void)fputs(",\"header\":", input);
    header_json(input, &certificate->checkpoint.header);
    (void)fputs(",\"validity_proof\":", input);
    hex(input, certificate->checkpoint.validity_proof.bytes,
        certificate->checkpoint.validity_proof.length);
    (void)fputs(",\"checkpoint_id\":", input);
    hex(input, checkpoint_id, 32U);
    (void)fputs(",\"attestations\":[", input);
    for (i = 0U; i < certificate->attestation_count; ++i) {
        if (i != 0U)
            (void)fputc(',', input);
        attestation_json(input, &certificate->attestations[i]);
    }
    (void)fputc(']', input);
    if (runtime != NULL) {
        (void)fputs(",\"publication_state_dir\":", input);
        quoted(input, config->state_dir);
        if (config->publication_inputs_dir != NULL) {
            (void)fputs(",\"publication_inputs_dir\":", input);
            quoted(input, config->publication_inputs_dir);
        }
        (void)fputs(",\"native_facts\":", input);
        status = gp_runtime_settlement_facts(runtime, input);
        if (status != LXP_OK) { (void)fclose(input); cleanup(&files); return status; }
    }
    status = execute(config, "register", &files, input, wire, sizeof(wire), &length);
    cleanup(&files);
    if (status != LXP_OK)
        return status;
    if (length != GP_REGISTRATION_WIRE || wire[0] > 1U || lxp_ct_is_zero(wire + 1U, 32U) ||
        read64(wire + 33U) == 0U || read64(wire + 41U) == 0U || read64(wire + 49U) == 0U)
        return LXP_ERR_CONTEXT_MISMATCH;
    (void)memset(&result, 0, sizeof(result));
    result.paxeer_chain_id = config->chain_id;
    (void)memcpy(result.settlement_contract, config->settlement_contract, 20U);
    (void)memcpy(result.checkpoint_id, checkpoint_id, 32U);
    (void)memcpy(result.transaction_id, wire + 1U, 32U);
    result.observed_block_number = read64(wire + 33U);
    result.observed_at_ms = read64(wire + 41U);
    *registration = result;
    *already_registered = wire[0] != 0U;
    *registered_set_version = read64(wire + 49U);
    return LXP_OK;
}
lxp_result gp_settlement_config_from_env(gp_settlement_config *config, const char *state_dir)
{
    const char *file = getenv("LAYERX_GUARANTOR_SETTLEMENT_FILE");
    const char *domain = getenv("LAYERX_GUARANTOR_SETTLEMENT_DOMAIN");
    const char *address = getenv("LAYERX_NODE_PAXEER_RPC_ADDRESS");
    const char *port = getenv("LAYERX_NODE_PAXEER_RPC_PORT");
    gp_files files;
    FILE *input;
    uint8_t wire[56U + 65U * LXP_MAX_GUARANTOR_ATTESTATIONS];
    size_t length, i;
    unsigned long parsed_port;
    char *end = NULL;
    int written;
    lxp_result status;
    if (config == NULL || state_dir == NULL || file == NULL || address == NULL ||
        strcmp(address, "127.0.0.1") != 0 || port == NULL || *port == '\0')
        return LXP_ERR_NON_CANONICAL;
    errno = 0;
    parsed_port = strtoul(port, &end, 10);
    if (errno != 0 || end == port || *end != '\0' || parsed_port == 0U || parsed_port > UINT16_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(config, 0, sizeof(*config));
    config->state_dir = state_dir;
    config->python = getenv("LAYERX_GUARANTOR_PYTHON");
    config->helper = getenv("LAYERX_GUARANTOR_SETTLEMENT_HELPER");
    config->submitter_key_file = getenv("LAYERX_GUARANTOR_SUBMITTER_KEY_FILE");
    config->submitter_lock_file = getenv("LAYERX_GUARANTOR_SUBMITTER_LOCK_FILE");
    config->publication_inputs_dir = getenv("LAYERX_GUARANTOR_PUBLICATION_INPUTS_DIR");
    if (config->python == NULL)
        config->python = "python3";
    if (config->helper == NULL)
        config->helper = "/opt/layerx/guarantor/settlement.py";
    if (domain == NULL)
        domain = "beta";
    written = snprintf(config->rpc_url_storage, sizeof(config->rpc_url_storage),
                       "http://127.0.0.1:%lu", parsed_port);
    if (written < 0 || (size_t)written >= sizeof(config->rpc_url_storage))
        return LXP_ERR_LENGTH_LIMIT;
    config->rpc_url = config->rpc_url_storage;
    status = begin_files(config, &files, &input);
    if (status != LXP_OK)
        return status;
    (void)fputs(",\"settlement_file\":", input);
    quoted(input, file);
    (void)fputs(",\"settlement_domain\":", input);
    quoted(input, domain);
    status = execute(config, "config", &files, input, wire, sizeof(wire), &length);
    cleanup(&files);
    if (status != LXP_OK)
        return status;
    if (length < 56U || read32(wire + 52U) == 0U ||
        read32(wire + 52U) > LXP_MAX_GUARANTOR_ATTESTATIONS ||
        length != 56U + (size_t)read32(wire + 52U) * 65U || read64(wire) == 0U ||
        read32(wire + 8U) == 0U)
        return LXP_ERR_CONTEXT_MISMATCH;
    config->chain_id = read64(wire);
    config->network_id = read32(wire + 8U);
    (void)memcpy(config->settlement_contract, wire + 12U, 20U);
    (void)memcpy(config->checkpoint_registry, wire + 32U, 20U);
    config->member_count = read32(wire + 52U);
    for (i = 0U; i < config->member_count; ++i) {
        (void)memcpy(config->members[i].guarantor_id, wire + 56U + 65U * i, 32U);
        (void)memcpy(config->members[i].public_key, wire + 88U + 65U * i, 33U);
    }
    return LXP_OK;
}
