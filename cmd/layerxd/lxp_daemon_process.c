#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_snapshot.h"

#include <errno.h>
#include <dirent.h>
#include <fcntl.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <time.h>
#include <unistd.h>

enum {
    NODE_EXECUTION_ARENA_BYTES = LXP_MAX_ACTIVITY_BYTES * 3U,
    NODE_SNAPSHOT_ARENA_BYTES = LXP_MAX_ACTIVITY_BYTES * 4U
};

typedef struct lxp_daemon_process {
    lxp_daemon daemon;
    lxp_daemon_protocol_owner owner;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lx_account_registry accounts;
    lxp_transfer_asset_state assets[LX_ACCOUNT_REGISTRY_CAPACITY];
    size_t asset_count;
    lx_programs_transfer_runtime programs;
    lxp_identity_store identities;
    lxp_fee_params fees;
    lxp_log feed_log;
    lxp_log canonical_log;
    lxp_log authority_log;
    lxp_log batch_log;
    lxp_history history;
    lxp_verified_receipt_index verified_receipts;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_sequencer_authorization sequencer_authorization;
    uint8_t sequencer_private_key[32];
    uint8_t authority_replica_id[32];
    uint8_t authority_replica_token[LXP_DAEMON_BEARER_MAX_BYTES];
    size_t authority_replica_token_length;
    const char *authority_replica_address;
    uint16_t authority_replica_port;
    uint8_t *owner_scratch_bytes;
    lxp_arena owner_scratch;
    uint8_t *execution_arena_bytes;
    lxp_arena execution_arena;
    uint8_t *checkpoint_arena_bytes;
    lxp_arena checkpoint_arena;
    const char *checkpoint_directory;
    uint64_t next_batch;
    uint32_t parameter_version;
    uint32_t network_id;
    bool state_open;
    bool history_open;
    bool feed_open;
    bool canonical_open;
    bool authority_open;
    bool batch_open;
    bool daemon_started;
    bool checkpoint_selected;
} lxp_daemon_process;

static lxp_result resume_batch_number(lxp_daemon_process *process);

static volatile sig_atomic_t stop_requested;

static void request_stop(int signal_number)
{
    (void)signal_number;
    stop_requested = 1;
}

static const char *required_environment(const char *name)
{
    const char *value = getenv(name);
    return value != NULL && value[0] != '\0' ? value : NULL;
}

static lxp_result parse_u64_text(const char *text, uint64_t *value)
{
    char *end = NULL;
    unsigned long long parsed;
    if (text == NULL || value == NULL || *text == '\0')
        return LXP_ERR_NON_CANONICAL;
    errno = 0;
    parsed = strtoull(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0')
        return LXP_ERR_NON_CANONICAL;
    *value = (uint64_t)parsed;
    return LXP_OK;
}

static bool checkpoint_name(const char *name, uint64_t *sequence)
{
    char digits[21];
    size_t index;
    if (name == NULL || sequence == NULL || strlen(name) != 24U ||
        strcmp(name + 20U, ".lxs") != 0)
        return false;
    for (index = 0U; index < 20U; ++index)
        if (name[index] < '0' || name[index] > '9') return false;
    (void)memcpy(digits, name, 20U);
    digits[20] = '\0';
    return parse_u64_text(digits, sequence) == LXP_OK;
}

static lxp_result latest_snapshot_path(
    const char *directory, const char *bootstrap, char output[4096],
    bool *checkpoint_selected)
{
    DIR *stream;
    struct dirent *entry;
    uint64_t latest = 0U;
    bool found = false;
    int length;
    if (directory == NULL || bootstrap == NULL || output == NULL ||
        checkpoint_selected == NULL)
        return LXP_ERR_NON_CANONICAL;
    stream = opendir(directory);
    if (stream == NULL) return LXP_ERR_IO;
    errno = 0;
    while ((entry = readdir(stream)) != NULL) {
        uint64_t sequence;
        if (checkpoint_name(entry->d_name, &sequence) &&
            (!found || sequence > latest)) {
            latest = sequence;
            found = true;
        }
    }
    if (errno != 0) {
        (void)closedir(stream);
        return LXP_ERR_IO;
    }
    if (closedir(stream) != 0) return LXP_ERR_IO;
    length = found ?
        snprintf(output, 4096U, "%s/%020llu.lxs", directory,
                 (unsigned long long)latest) :
        snprintf(output, 4096U, "%s", bootstrap);
    if (length < 0 || length >= 4096) return LXP_ERR_LENGTH_LIMIT;
    *checkpoint_selected = found;
    return LXP_OK;
}

static void write_u16_be(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static uint16_t read_u16_be(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint64_t read_u64_be(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static int hex_nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

static lxp_result decode_hex(const char *text, uint8_t *output,
                             size_t output_length)
{
    size_t index;
    if (text == NULL || output == NULL || strlen(text) != output_length * 2U)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < output_length; ++index) {
        int high = hex_nibble(text[index * 2U]);
        int low = hex_nibble(text[index * 2U + 1U]);
        if (high < 0 || low < 0) return LXP_ERR_NON_CANONICAL;
        output[index] = (uint8_t)(((unsigned int)high << 4U) |
                                 (unsigned int)low);
    }
    return LXP_OK;
}

static lxp_result load_identities(const char *path,
                                  lxp_identity_store *identities)
{
    FILE *file;
    char line[4096];
    lxp_result status = LXP_OK;
    if (path == NULL || identities == NULL) return LXP_ERR_NON_CANONICAL;
    file = fopen(path, "rb");
    if (file == NULL) return LXP_ERR_IO;
    (void)memset(identities, 0, sizeof(*identities));
    while (status == LXP_OK && fgets(line, sizeof(line), file) != NULL) {
        char *key_separator = strchr(line, ':');
        char *sequence_separator;
        char *end;
        uint8_t did[LXP_MAX_DID_LENGTH];
        uint8_t key[32];
        size_t did_length;
        uint64_t next_sequence;
        lxp_identity *identity;
        if (key_separator == NULL) { status = LXP_ERR_NON_CANONICAL; break; }
        sequence_separator = strchr(key_separator + 1, ':');
        if (sequence_separator == NULL ||
            strchr(sequence_separator + 1, ':') != NULL) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        end = strchr(sequence_separator + 1, '\n');
        if (end != NULL) *end = '\0';
        *key_separator = '\0';
        *sequence_separator = '\0';
        if ((size_t)(key_separator - line) == 0U ||
            ((size_t)(key_separator - line) & 1U) != 0U ||
            (size_t)(key_separator - line) / 2U > sizeof(did)) {
            status = LXP_ERR_NON_CANONICAL;
            break;
        }
        did_length = (size_t)(key_separator - line) / 2U;
        status = decode_hex(line, did, did_length);
        if (status == LXP_OK) status = decode_hex(key_separator + 1, key, 32U);
        if (status == LXP_OK)
            status = parse_u64_text(sequence_separator + 1, &next_sequence);
        if (status == LXP_OK)
            status = lxp_identity_register(identities, did, did_length,
                                           key, &identity);
        if (status == LXP_OK) identity->next_sequence = next_sequence;
    }
    if (status == LXP_OK && ferror(file)) status = LXP_ERR_IO;
    if (status == LXP_OK && identities->count == 0U)
        status = LXP_ERR_UNKNOWN_DID;
    if (fclose(file) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result collect_assets(lxp_daemon_process *process)
{
    size_t account_index;
    process->asset_count = 0U;
    for (account_index = 0U; account_index < process->accounts.count;
         ++account_index) {
        lx_account *account = &process->accounts.accounts[account_index];
        size_t asset_index;
        if (!account->has_asset) continue;
        for (asset_index = 0U; asset_index < process->asset_count;
             ++asset_index)
            if (lxp_ct_memcmp(process->assets[asset_index].asset_id,
                              account->asset_id, 32U) == 0)
                break;
        if (asset_index != process->asset_count) continue;
        if (process->asset_count == LX_ACCOUNT_REGISTRY_CAPACITY)
            return LXP_ERR_LENGTH_LIMIT;
        (void)memcpy(process->assets[process->asset_count].asset_id,
                     account->asset_id, 32U);
        process->assets[process->asset_count].registered = true;
        process->assets[process->asset_count].paused = false;
        ++process->asset_count;
    }
    return process->asset_count == 0U ? LXP_ERR_ASSET_MISMATCH : LXP_OK;
}

static lxp_result occupancy_parameters(
    void *context, uint32_t recorded_fee_schedule_version,
    lx_programs_fee_schedule *schedule, uint8_t occupancy_asset_id[32])
{
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    if (process == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_programs_fee_governance_resolve_runtime(
        &process->kernel, recorded_fee_schedule_version, schedule,
        occupancy_asset_id);
}

static lx_account *principal_account(lxp_daemon_process *process,
                                     const uint8_t key[32])
{
    size_t index;
    lx_account *match = NULL;
    for (index = 0U; index < process->accounts.count; ++index) {
        lx_account *account = &process->accounts.accounts[index];
        if (account->kind != LX_ACCOUNT_AGENT_MAIN ||
            !account->has_authority_key ||
            lxp_ct_memcmp(account->authority_key, key, 32U) != 0)
            continue;
        if (match != NULL) return NULL;
        match = account;
    }
    return match;
}

static lxp_result current_time_ms(uint64_t *timestamp)
{
    struct timespec now;
    if (timestamp == NULL || clock_gettime(CLOCK_REALTIME, &now) != 0 ||
        now.tv_sec < 0 || now.tv_nsec < 0)
        return LXP_ERR_IO;
    *timestamp = (uint64_t)now.tv_sec * UINT64_C(1000) +
                 (uint64_t)now.tv_nsec / UINT64_C(1000000);
    return LXP_OK;
}

static void write_u64_be(uint8_t bytes[8], uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static lxp_result write_file_bytes(int descriptor, const uint8_t *bytes,
                                   size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t count = write(descriptor, bytes + offset, length - offset);
        if (count > 0) offset += (size_t)count;
        else if (count < 0 && errno == EINTR) continue;
        else return LXP_ERR_IO;
    }
    return LXP_OK;
}

static lxp_result file_matches(const char *path, const uint8_t *bytes,
                               size_t length, bool *present)
{
    struct stat information;
    uint8_t buffer[4096];
    size_t offset = 0U;
    int descriptor;
    lxp_result status = LXP_OK;
    if (path == NULL || bytes == NULL || present == NULL)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    descriptor = open(path, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0)
        return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
    *present = true;
    if (fstat(descriptor, &information) != 0 || information.st_size < 0 ||
        (uint64_t)information.st_size != (uint64_t)length)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    while (status == LXP_OK && offset < length) {
        size_t wanted = length - offset < sizeof(buffer) ?
                            length - offset : sizeof(buffer);
        ssize_t count = read(descriptor, buffer, wanted);
        if (count > 0) {
            if (lxp_ct_memcmp(buffer, bytes + offset, (size_t)count) != 0)
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            offset += (size_t)count;
        } else if (count < 0 && errno == EINTR) {
            continue;
        } else {
            status = LXP_ERR_IO;
        }
    }
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    lxp_secure_zero(buffer, sizeof(buffer));
    return status;
}

static lxp_result identity_checkpoint_write(
    const lxp_daemon_process *process, uint64_t global_sequence)
{
    static const uint8_t magic[4] = {'L', 'X', 'I', '1'};
    const size_t record_bytes = 72U;
    size_t body_length;
    size_t length;
    size_t offset = 0U;
    size_t index;
    uint8_t *bytes;
    uint8_t digest[32];
    char temporary[4096];
    char final[4096];
    int descriptor = -1;
    int directory_descriptor = -1;
    int path_length;
    bool present = false;
    lxp_result status;
    if (process == NULL || process->checkpoint_directory == NULL ||
        global_sequence == UINT64_MAX ||
        process->identities.count == 0U ||
        process->identities.count > UINT16_MAX)
        return LXP_ERR_NON_CANONICAL;
    if (process->identities.count > (SIZE_MAX - 14U - 32U) / record_bytes)
        return LXP_ERR_LENGTH_LIMIT;
    body_length = 14U + process->identities.count * record_bytes;
    length = body_length + 32U;
    bytes = (uint8_t *)malloc(length);
    if (bytes == NULL) return LXP_ERR_IO;
    (void)memcpy(bytes + offset, magic, sizeof(magic)); offset += sizeof(magic);
    write_u64_be(bytes + offset, global_sequence); offset += 8U;
    write_u16_be(bytes + offset, (uint16_t)process->identities.count);
    offset += 2U;
    for (index = 0U; index < process->identities.count; ++index) {
        const lxp_identity *identity = &process->identities.identities[index];
        (void)memcpy(bytes + offset, identity->did_id, 32U); offset += 32U;
        (void)memcpy(bytes + offset, identity->primary_key, 32U); offset += 32U;
        write_u64_be(bytes + offset, identity->next_sequence); offset += 8U;
    }
    status = offset == body_length ?
        lxp_hash_sha256(bytes, body_length, digest) : LXP_FATAL_INVARIANT;
    if (status == LXP_OK) (void)memcpy(bytes + offset, digest, 32U);
    path_length = snprintf(final, sizeof(final), "%s/%020llu.lxi",
                           process->checkpoint_directory,
                           (unsigned long long)global_sequence);
    if (status == LXP_OK &&
        (path_length < 0 || (size_t)path_length >= sizeof(final)))
        status = LXP_ERR_LENGTH_LIMIT;
    path_length = status == LXP_OK ?
        snprintf(temporary, sizeof(temporary), "%s.tmp", final) : -1;
    if (status == LXP_OK &&
        (path_length < 0 || (size_t)path_length >= sizeof(temporary)))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK)
        status = file_matches(final, bytes, length, &present);
    if (status == LXP_OK && present) {
        lxp_secure_zero(bytes, length);
        free(bytes);
        return LXP_OK;
    }
    if (status == LXP_OK && unlink(temporary) != 0 && errno != ENOENT)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        descriptor = open(temporary,
                          O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
        if (descriptor < 0) status = LXP_ERR_IO;
    }
    if (status == LXP_OK) status = write_file_bytes(descriptor, bytes, length);
    if (status == LXP_OK && fdatasync(descriptor) != 0) status = LXP_ERR_IO;
    if (descriptor >= 0 && close(descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    descriptor = -1;
    if (status == LXP_OK && rename(temporary, final) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        directory_descriptor = open(process->checkpoint_directory,
                                    O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        if (directory_descriptor < 0 || fsync(directory_descriptor) != 0)
            status = LXP_ERR_IO;
    }
    if (directory_descriptor >= 0) (void)close(directory_descriptor);
    if (status != LXP_OK) (void)unlink(temporary);
    lxp_secure_zero(bytes, length);
    free(bytes);
    return status;
}

static lxp_result identity_checkpoint_load(
    const char *snapshot_path, uint64_t global_sequence,
    lxp_identity_store *identities)
{
    struct stat information;
    char path[4096];
    uint8_t *bytes;
    uint8_t digest[32];
    size_t path_length;
    size_t length;
    size_t body_length;
    size_t offset = 14U;
    size_t index;
    uint16_t count;
    bool seen[LXP_IDENTITY_STORE_CAPACITY] = {false};
    int descriptor;
    lxp_result status = LXP_OK;
    if (snapshot_path == NULL || identities == NULL ||
        strlen(snapshot_path) < 4U ||
        strcmp(snapshot_path + strlen(snapshot_path) - 4U, ".lxs") != 0)
        return LXP_ERR_NON_CANONICAL;
    path_length = strlen(snapshot_path);
    if (path_length >= sizeof(path)) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(path, snapshot_path, path_length + 1U);
    (void)memcpy(path + path_length - 4U, ".lxi", 5U);
    descriptor = open(path, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size < 0) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    length = (size_t)information.st_size;
    if ((off_t)length != information.st_size || length < 46U) {
        (void)close(descriptor);
        return LXP_ERR_LOG_CORRUPT;
    }
    bytes = (uint8_t *)malloc(length);
    if (bytes == NULL) { (void)close(descriptor); return LXP_ERR_IO; }
    {
        size_t read_offset = 0U;
        while (status == LXP_OK && read_offset < length) {
            ssize_t result = read(descriptor, bytes + read_offset,
                                  length - read_offset);
            if (result > 0) read_offset += (size_t)result;
            else if (result < 0 && errno == EINTR) continue;
            else status = LXP_ERR_IO;
        }
    }
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    count = status == LXP_OK ? read_u16_be(bytes + 12U) : 0U;
    body_length = 14U + (size_t)count * 72U;
    if (status == LXP_OK &&
        (memcmp(bytes, "LXI1", 4U) != 0 ||
         read_u64_be(bytes + 4U) != global_sequence ||
         count != identities->count || length != body_length + 32U))
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK) status = lxp_hash_sha256(bytes, body_length, digest);
    if (status == LXP_OK &&
        lxp_ct_memcmp(digest, bytes + body_length, 32U) != 0)
        status = LXP_ERR_LOG_CORRUPT;
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        size_t identity_index;
        lxp_identity *match = NULL;
        for (identity_index = 0U; identity_index < identities->count;
             ++identity_index) {
            lxp_identity *candidate = &identities->identities[identity_index];
            if (lxp_ct_memcmp(candidate->did_id, bytes + offset, 32U) == 0) {
                if (match != NULL) { status = LXP_ERR_LOG_CORRUPT; break; }
                match = candidate;
            }
        }
        if (status != LXP_OK) break;
        if (match == NULL || seen[(size_t)(match - identities->identities)] ||
            lxp_ct_memcmp(match->primary_key,
                          bytes + offset + 32U, 32U) != 0)
            status = LXP_ERR_LOG_CORRUPT;
        else {
            seen[(size_t)(match - identities->identities)] = true;
            match->next_sequence = read_u64_be(bytes + offset + 64U);
        }
        offset += 72U;
    }
    lxp_secure_zero(bytes, length);
    free(bytes);
    return status;
}

static lxp_result persist_state_checkpoint(lxp_daemon_process *process,
                                           uint64_t global_sequence)
{
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    size_t mark;
    char temporary[4096];
    int length;
    lxp_result status;
    if (process == NULL || process->checkpoint_directory == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(&process->checkpoint_arena);
    status = lxp_snapshot_write(&process->kernel, global_sequence,
                                &process->checkpoint_arena, &snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest(
            snapshot.bytes, snapshot.length, global_sequence,
            process->kernel.current_state_root, &manifest);
    if (status == LXP_OK)
        status = identity_checkpoint_write(process, global_sequence);
    length = status == LXP_OK ?
        snprintf(temporary, sizeof(temporary), "%s/%020llu.lxs.tmp",
                 process->checkpoint_directory,
                 (unsigned long long)global_sequence) : -1;
    if (status == LXP_OK &&
        (length < 0 || (size_t)length >= sizeof(temporary)))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK && unlink(temporary) != 0 && errno != ENOENT)
        status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_snapshot_store_write(
            process->checkpoint_directory, &manifest,
            snapshot.bytes, snapshot.length);
    (void)lxp_arena_reset(&process->checkpoint_arena, mark);
    return status;
}

static lxp_result replay_execute_activity(
    lxp_daemon_process *process, uint64_t global_sequence,
    const uint8_t *canonical_activity, size_t activity_length,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const lxp_receipt *expected, uint64_t timestamp,
    uint64_t batch_number, lxp_activity *activity, lxp_receipt *receipt)
{
    lxp_identity *identity;
    lx_account *principal;
    lxp_authority_scope scope;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_byte_span encoded_receipt;
    uint8_t batch_preimage[32U + 32U + 8U + 8U];
    uint8_t activity_id[32];
    uint8_t grant_id[32] = {0};
    lxp_result status;
    if (process == NULL || canonical_activity == NULL ||
        canonical_receipt == NULL || activity == NULL || receipt == NULL ||
        expected == NULL ||
        activity_length == 0U || receipt_length == 0U || timestamp == 0U ||
        batch_number <
            process->sequencer_authorization.first_batch_number ||
        batch_number > process->sequencer_authorization.last_batch_number ||
        global_sequence != process->state.next_sequence ||
        expected->global_sequence != global_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    if (expected->module_id != LXP_MODULE_PROGRAMS ||
        expected->module_version == 0U ||
        expected->parameter_version != process->parameter_version ||
        process->fees.version != expected->parameter_version ||
        process->programs.fee_schedule.version !=
            expected->parameter_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_activity_decode(canonical_activity, activity_length, activity);
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(activity, process->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK &&
        lxp_activity_module_id(activity->activity_type) != LXP_MODULE_PROGRAMS)
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK)
        status = lxp_identity_resolve(&process->identities,
                                      activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK &&
        (activity->authority.length != 32U ||
         !lxp_identity_key_valid(identity, activity->authority.bytes,
                                 timestamp, global_sequence)))
        status = LXP_ERR_BAD_SIGNATURE;
    principal = status == LXP_OK ?
        principal_account(process, activity->authority.bytes) : NULL;
    if (status == LXP_OK && principal == NULL)
        status = LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
    if (status != LXP_OK) return status;
    (void)memset(&scope, 0, sizeof(scope));
    scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    scope.activity_ordinal_min = 1U;
    scope.activity_ordinal_max = 8U;
    scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    scope.maximum_total = (lxp_u128){UINT64_MAX, UINT64_MAX};
    scope.maximum_per_period = (lxp_u128){UINT64_MAX, UINT64_MAX};
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.actor, identity->did_id, 32U);
    (void)memcpy(authority.principal, principal->id, 32U);
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, activity->authority.bytes, 32U);
    authority.scope = &scope;
    status = lxp_authority_hash(authority.kind, grant_id,
                                authority.verified_key,
                                authority.authority_hash);
    if (status != LXP_OK) return status;
    (void)memcpy(batch_preimage, process->kernel.current_state_root, 32U);
    (void)memcpy(batch_preimage + 32U, activity_id, 32U);
    write_u64_be(batch_preimage + 64U, global_sequence);
    write_u64_be(batch_preimage + 72U, batch_number);
    (void)memset(&execution, 0, sizeof(execution));
    status = lxp_hash_context_value(batch_preimage, sizeof(batch_preimage),
                                    execution.batch_id);
    if (status != LXP_OK) return status;
    execution.network_id = process->network_id;
    execution.batch_number = batch_number;
    execution.batch_timestamp_ms = timestamp;
    execution.maximum_timestamp_window = UINT64_C(300000);
    execution.epoch = process->kernel.epoch;
    execution.global_sequence = global_sequence;
    execution.recorded_module_version = expected->module_version;
    execution.recorded_metering_schedule_version =
        expected->program_outcome.present ?
            expected->program_outcome.metering_schedule_version : 0U;
    execution.recorded_fee_schedule_version =
        expected->program_outcome.present ?
            expected->program_outcome.fee_schedule_version : 0U;
    execution.parameter_version = expected->parameter_version;
    execution.signature_valid = true;
    execution.identities = &process->identities;
    execution.authority = &authority;
    execution.fee_parameters = &process->fees;
    execution.fee_balance = principal->balance;
    execution.gas_limit = UINT64_MAX;
    execution.arena = &process->execution_arena;
    execution.sequencer_private_key = process->sequencer_private_key;
    execution.verified_receipts = &process->verified_receipts;
    (void)memset(receipt, 0, sizeof(*receipt));
    status = lxp_kernel_execute_activity(&process->kernel, activity,
                                         &execution, receipt);
    if (status == LXP_OK)
        status = lxp_receipt_encode(receipt, true,
                                    &process->execution_arena,
                                    &encoded_receipt);
    if (status == LXP_OK &&
        (encoded_receipt.length != receipt_length ||
         lxp_ct_memcmp(encoded_receipt.bytes, canonical_receipt,
                       receipt_length) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    return status;
}

static lxp_result ensure_batch_record(
    lxp_daemon_process *process, const lxp_batch_header *expected,
    const uint8_t *canonical_header, size_t header_length)
{
    uint64_t offset = 0U;
    uint64_t prior_batch = 0U;
    uint64_t prior_sequence = 0U;
    bool found = false;
    lxp_result status = LXP_OK;
    while (status == LXP_OK && offset < process->batch_log.write_offset) {
        lxp_log_record_header record;
        uint8_t body[LXP_BATCH_HEADER_ENCODED_SIZE];
        lxp_batch_header header;
        status = lxp_log_read(&process->batch_log, offset, &record, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (record.record_kind != (uint8_t)LXP_LOG_BATCH_HEADER ||
            record.body_length != sizeof(body))
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_log_read(&process->batch_log, offset, &record, body,
                              sizeof(body));
        if (status == LXP_OK)
            status = lxp_batch_header_decode(body, sizeof(body), &header);
        if (status == LXP_OK &&
            ((prior_batch != 0U &&
              (prior_batch == UINT64_MAX || prior_sequence == UINT64_MAX ||
               header.batch_number != prior_batch + 1U ||
               header.first_sequence != prior_sequence + 1U)) ||
             header.first_sequence != header.last_sequence ||
             header.last_sequence != record.global_sequence))
            status = LXP_ERR_BATCH_GAP;
        if (status == LXP_OK &&
            header.batch_number == expected->batch_number) {
            if (found || header_length != sizeof(body) ||
                lxp_ct_memcmp(body, canonical_header, sizeof(body)) != 0)
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            else
                found = true;
        }
        if (status == LXP_OK) {
            prior_batch = header.batch_number;
            prior_sequence = header.last_sequence;
            offset += LXP_LOG_HEADER_BYTES + record.body_length;
        }
    }
    if (status != LXP_OK || found) return status;
    if ((prior_batch != 0U &&
         (prior_batch == UINT64_MAX || prior_sequence == UINT64_MAX ||
          expected->batch_number != prior_batch + 1U ||
          expected->first_sequence != prior_sequence + 1U)) ||
        expected->first_sequence != expected->last_sequence)
        return LXP_ERR_BATCH_GAP;
    status = lxp_log_append(&process->batch_log, LXP_LOG_BATCH_HEADER,
                            expected->last_sequence, canonical_header,
                            (uint32_t)header_length, NULL);
    if (status == LXP_OK) status = lxp_log_write_boundary(&process->batch_log);
    return status;
}

static lxp_result replay_publish_evidence(
    lxp_daemon_process *process, const uint8_t *canonical_activity,
    size_t activity_length, const uint8_t *canonical_receipt,
    size_t receipt_length, const lxp_activity *activity,
    const lxp_receipt *receipt, uint64_t batch_number)
{
    lxp_byte_span activities[1];
    lxp_byte_span receipts[1];
    lxp_batch_roots roots;
    lxp_batch_header header;
    lxp_byte_span canonical_header;
    lxp_merkle_proof proof;
    lxp_daemon_receipt_evidence existing;
    uint8_t signature[64];
    uint8_t digest[32];
    bool exists = false;
    size_t mark = lxp_arena_mark(&process->execution_arena);
    lxp_result status;
    if (batch_number <
            process->sequencer_authorization.first_batch_number ||
        batch_number > process->sequencer_authorization.last_batch_number) {
        (void)lxp_arena_reset(&process->execution_arena, mark);
        return LXP_ERR_AUTH_SCOPE;
    }
    activities[0] = (lxp_byte_span){canonical_activity, activity_length};
    receipts[0] = (lxp_byte_span){canonical_receipt, receipt_length};
    status = lxp_batch_roots_compute(
        &(lxp_batch_root_inputs){activities, 1U, receipts, 1U,
                                 NULL, 0U, NULL, 0U, NULL, 0U},
        &process->execution_arena, &roots);
    if (status == LXP_OK) {
        (void)memset(&header, 0, sizeof(header));
        header.protocol_version = activity->protocol_version;
        header.network_id = process->network_id;
        header.epoch = process->kernel.epoch;
        header.batch_number = batch_number;
        header.first_sequence = receipt->global_sequence;
        header.last_sequence = receipt->global_sequence;
        (void)memcpy(header.previous_state_root,
                     receipt->previous_state_root, 32U);
        (void)memcpy(header.resulting_state_root,
                     receipt->resulting_state_root, 32U);
        (void)memcpy(header.activity_merkle_root,
                     roots.activity_merkle_root, 32U);
        (void)memcpy(header.receipt_merkle_root,
                     roots.receipt_merkle_root, 32U);
        (void)memcpy(header.event_merkle_root,
                     roots.event_merkle_root, 32U);
        (void)memcpy(header.data_availability_root,
                     roots.data_availability_root, 32U);
        (void)memcpy(header.oracle_root, roots.oracle_root, 32U);
        header.timestamp_ms = receipt->timestamp;
        (void)memcpy(header.sequencer_id,
                     process->sequencer_authorization.sequencer_id, 32U);
        status = lxp_batch_header_encode(&header,
                                         &process->execution_arena,
                                         &canonical_header);
    }
    if (status == LXP_OK)
        status = ensure_batch_record(process, &header,
                                     canonical_header.bytes,
                                     canonical_header.length);
    if (status == LXP_OK)
        status = lxp_batch_sign(
            &header, process->sequencer_private_key,
            &process->sequencer_authorization, signature,
            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_receipt_digest(receipt, &process->execution_arena,
                                    digest);
    if (status == LXP_OK) {
        size_t lookup_mark = lxp_arena_mark(&process->execution_arena);
        status = lxp_daemon_receipt_authority_lookup(
            &process->receipt_authority, digest,
            &process->execution_arena, &existing);
        if (status == LXP_OK) {
            exists = true;
            if (existing.canonical_receipt.length != receipt_length ||
                lxp_ct_memcmp(existing.canonical_receipt.bytes,
                              canonical_receipt, receipt_length) != 0 ||
                existing.canonical_header.length != canonical_header.length ||
                lxp_ct_memcmp(existing.canonical_header.bytes,
                              canonical_header.bytes,
                              canonical_header.length) != 0 ||
                lxp_ct_memcmp(existing.header_signature,
                              signature, 64U) != 0)
                status = LXP_FATAL_REPLAY_DIVERGENCE;
        } else if (status == LXP_ERR_UNKNOWN_ACTIVITY) {
            status = LXP_OK;
        }
        (void)lxp_arena_reset(&process->execution_arena, lookup_mark);
    }
    (void)memset(&proof, 0, sizeof(proof));
    proof.leaf_count = 1U;
    if (status == LXP_OK && !exists)
        status = lxp_daemon_receipt_authority_append(
            &process->receipt_authority, canonical_receipt, receipt_length,
            canonical_header.bytes, canonical_header.length, signature,
            &proof, &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_daemon_authority_replica_publish(
            process->authority_replica_address,
            process->authority_replica_port,
            process->authority_replica_token,
            process->authority_replica_token_length,
            process->authority_replica_id, canonical_receipt, receipt_length,
            canonical_header.bytes, canonical_header.length, signature,
            &proof);
    (void)lxp_arena_reset(&process->execution_arena, mark);
    return status;
}

static lxp_result replay_canonical_group(
    lxp_daemon_process *process, const uint8_t *canonical_activity,
    size_t activity_length, const uint8_t *canonical_receipt,
    size_t receipt_length)
{
    lxp_activity activity;
    lxp_receipt expected;
    lxp_receipt replayed;
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header existing_header;
    uint8_t digest[32];
    uint64_t batch_number = 0U;
    size_t mark = lxp_arena_mark(&process->execution_arena);
    lxp_result status = lxp_receipt_decode(
        canonical_receipt, receipt_length, true, &expected);
    if (status == LXP_OK)
        status = lxp_receipt_verify(
            &expected, process->sequencer_authorization.public_key,
            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_receipt_digest(&expected,
                                    &process->execution_arena, digest);
    if (status == LXP_OK) {
        size_t lookup_mark = lxp_arena_mark(&process->execution_arena);
        status = lxp_daemon_receipt_authority_lookup(
            &process->receipt_authority, digest,
            &process->execution_arena, &evidence);
        if (status == LXP_OK)
            status = lxp_batch_header_decode(
                evidence.canonical_header.bytes,
                evidence.canonical_header.length, &existing_header);
        if (status == LXP_OK)
            batch_number = existing_header.batch_number;
        else if (status == LXP_ERR_UNKNOWN_ACTIVITY) {
            if (process->receipt_authority.record_count == 0U)
                batch_number =
                    process->sequencer_authorization.first_batch_number;
            else if (process->receipt_authority.last_global_sequence !=
                         UINT64_MAX &&
                     process->receipt_authority.last_batch_number !=
                         UINT64_MAX &&
                     expected.global_sequence ==
                         process->receipt_authority.last_global_sequence + 1U)
                batch_number =
                    process->receipt_authority.last_batch_number + 1U;
            else
                status = LXP_ERR_BATCH_GAP;
            if (batch_number != 0U) status = LXP_OK;
        }
        (void)lxp_arena_reset(&process->execution_arena, lookup_mark);
    }
    if (status == LXP_OK &&
        (expected.global_sequence != process->state.next_sequence ||
         lxp_ct_memcmp(expected.previous_state_root,
                       process->kernel.current_state_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK && expected.program_outcome.present) {
        lx_programs_metering_schedule metering_schedule;
        status = lxp_programs_metering_schedule_at(
            &process->kernel,
            expected.program_outcome.metering_schedule_version,
            batch_number,
            &metering_schedule);
    }
    if (status == LXP_OK)
        status = replay_execute_activity(
            process, expected.global_sequence, canonical_activity,
            activity_length, canonical_receipt, receipt_length,
            &expected, expected.timestamp, batch_number, &activity,
            &replayed);
    if (status == LXP_OK)
        status = persist_state_checkpoint(process,
                                          expected.global_sequence);
    if (status == LXP_OK)
        status = replay_publish_evidence(
            process, canonical_activity, activity_length,
            canonical_receipt, receipt_length, &activity, &replayed,
            batch_number);
    (void)lxp_arena_reset(&process->execution_arena, mark);
    return status;
}

static lxp_result reconcile_snapshot_evidence(lxp_daemon_process *process)
{
    static const uint8_t pending_magic[5] = {'L', 'X', 'P', 'P', '1'};
    static const uint8_t complete_magic[5] = {'L', 'X', 'P', 'C', '1'};
    uint64_t target;
    uint64_t offset = 0U;
    uint8_t *activity_bytes = NULL;
    uint8_t *receipt_bytes = NULL;
    uint8_t pending_activity_id[32] = {0};
    uint8_t pending_previous_root[32] = {0};
    uint8_t pending_resulting_root[32] = {0};
    uint8_t complete_receipt_digest[32] = {0};
    uint8_t complete_resulting_root[32] = {0};
    size_t activity_length = 0U;
    size_t receipt_length = 0U;
    bool pending = false;
    bool complete = false;
    lxp_activity activity;
    lxp_receipt receipt;
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header existing_header;
    uint8_t digest[32];
    uint64_t batch_number = 0U;
    size_t arena_mark;
    lxp_result status = LXP_OK;
    if (!process->checkpoint_selected) return LXP_OK;
    if (process->state.next_sequence <= 1U) return LXP_ERR_SEQUENCE_GAP;
    arena_mark = lxp_arena_mark(&process->execution_arena);
    target = process->state.next_sequence - 1U;
    while (status == LXP_OK && offset < process->canonical_log.write_offset) {
        lxp_log_record_header header;
        uint8_t *body = NULL;
        status = lxp_log_read(&process->canonical_log, offset,
                              &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.body_length > LXP_MAX_ACTIVITY_BYTES) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        if (header.body_length != 0U) {
            body = (uint8_t *)malloc(header.body_length);
            if (body == NULL) {
                status = LXP_ERR_IO;
                break;
            }
            status = lxp_log_read(&process->canonical_log, offset,
                                  &header, body, header.body_length);
        }
        if (status != LXP_OK) {
            free(body);
            break;
        }
        if (header.global_sequence == target &&
            header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
            header.body_length == 109U &&
            memcmp(body, pending_magic, sizeof(pending_magic)) == 0) {
            if (pending || complete || read_u64_be(body + 5U) != target)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                pending = true;
                (void)memcpy(pending_activity_id, body + 13U, 32U);
                (void)memcpy(pending_previous_root, body + 45U, 32U);
                (void)memcpy(pending_resulting_root, body + 77U, 32U);
            }
        } else if (header.global_sequence == target &&
                   header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
            if (!pending || activity_bytes != NULL || receipt_bytes != NULL)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                activity_bytes = body;
                activity_length = header.body_length;
                body = NULL;
            }
        } else if (header.global_sequence == target &&
                   header.record_kind == (uint8_t)LXP_LOG_RECEIPT) {
            if (!pending || activity_bytes == NULL || receipt_bytes != NULL)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                receipt_bytes = body;
                receipt_length = header.body_length;
                body = NULL;
            }
        } else if (header.global_sequence == target &&
                   header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
                   header.body_length == 77U &&
                   memcmp(body, complete_magic, sizeof(complete_magic)) == 0) {
            if (!pending || activity_bytes == NULL || receipt_bytes == NULL ||
                complete || read_u64_be(body + 5U) != target)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                complete = true;
                (void)memcpy(complete_receipt_digest, body + 13U, 32U);
                (void)memcpy(complete_resulting_root, body + 45U, 32U);
            }
        }
        free(body);
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    if (status == LXP_OK &&
        (!pending || !complete || activity_bytes == NULL ||
         receipt_bytes == NULL))
        status = LXP_ERR_PROJECTION_STALE;
    if (status == LXP_OK)
        status = lxp_activity_decode(activity_bytes, activity_length,
                                     &activity);
    if (status == LXP_OK)
        status = lxp_activity_id(activity_bytes, activity_length, digest);
    if (status == LXP_OK)
        status = lxp_receipt_decode(receipt_bytes, receipt_length,
                                    true, &receipt);
    if (status == LXP_OK)
        status = lxp_receipt_verify(
            &receipt, process->sequencer_authorization.public_key,
            &process->execution_arena);
    if (status == LXP_OK &&
        (receipt.global_sequence != target ||
         lxp_ct_memcmp(digest, pending_activity_id, 32U) != 0 ||
         lxp_ct_memcmp(receipt.activity_id,
                       pending_activity_id, 32U) != 0 ||
         lxp_ct_memcmp(receipt.previous_state_root,
                       pending_previous_root, 32U) != 0 ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       pending_resulting_root, 32U) != 0 ||
         lxp_ct_memcmp(receipt.resulting_state_root,
                       process->kernel.current_state_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt,
                                    &process->execution_arena, digest);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(digest, complete_receipt_digest, 32U) != 0 ||
         lxp_ct_memcmp(pending_resulting_root,
                       complete_resulting_root, 32U) != 0))
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK) {
        size_t mark = lxp_arena_mark(&process->execution_arena);
        status = lxp_daemon_receipt_authority_lookup(
            &process->receipt_authority, digest,
            &process->execution_arena, &evidence);
        if (status == LXP_OK)
            status = lxp_batch_header_decode(
                evidence.canonical_header.bytes,
                evidence.canonical_header.length, &existing_header);
        if (status == LXP_OK)
            batch_number = existing_header.batch_number;
        else if (status == LXP_ERR_UNKNOWN_ACTIVITY) {
            if (process->receipt_authority.record_count == 0U)
                batch_number =
                    process->sequencer_authorization.first_batch_number;
            else if (process->receipt_authority.last_global_sequence !=
                         UINT64_MAX &&
                     process->receipt_authority.last_batch_number !=
                         UINT64_MAX &&
                     target ==
                         process->receipt_authority.last_global_sequence + 1U)
                batch_number =
                    process->receipt_authority.last_batch_number + 1U;
            else
                status = LXP_ERR_BATCH_GAP;
            if (batch_number != 0U) status = LXP_OK;
        }
        (void)lxp_arena_reset(&process->execution_arena, mark);
    }
    if (status == LXP_OK)
        status = replay_publish_evidence(
            process, activity_bytes, activity_length,
            receipt_bytes, receipt_length, &activity, &receipt,
            batch_number);
    free(activity_bytes);
    free(receipt_bytes);
    (void)lxp_arena_reset(&process->execution_arena, arena_mark);
    return status;
}

static lxp_result replay_canonical_after_snapshot(
    void *context, lxp_daemon_protocol_owner *owner)
{
    static const uint8_t pending_magic[5] = {'L', 'X', 'P', 'P', '1'};
    static const uint8_t complete_magic[5] = {'L', 'X', 'P', 'C', '1'};
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    uint64_t offset = 0U;
    uint64_t scan_end;
    uint64_t expected_sequence;
    uint64_t pending_sequence = 0U;
    uint8_t pending_activity_id[32] = {0};
    uint8_t pending_previous_root[32] = {0};
    uint8_t pending_resulting_root[32] = {0};
    uint8_t *activity_bytes = NULL;
    uint8_t *receipt_bytes = NULL;
    size_t activity_length = 0U;
    size_t receipt_length = 0U;
    bool pending = false;
    lxp_result status = LXP_OK;
    if (process == NULL || owner == NULL || owner->kernel != &process->kernel ||
        owner->feed_store.canonical_log != &process->canonical_log)
        return LXP_ERR_NON_CANONICAL;
    status = reconcile_snapshot_evidence(process);
    if (status != LXP_OK) return status;
    expected_sequence = process->state.next_sequence;
    scan_end = process->canonical_log.write_offset;
    while (status == LXP_OK && offset < scan_end) {
        lxp_log_record_header header;
        uint8_t *body = NULL;
        status = lxp_log_read(&process->canonical_log, offset,
                              &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.body_length > LXP_MAX_ACTIVITY_BYTES ||
            offset + LXP_LOG_HEADER_BYTES + header.body_length > scan_end) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        if (header.body_length != 0U) {
            body = (uint8_t *)malloc(header.body_length);
            if (body == NULL) {
                status = LXP_ERR_IO;
                break;
            }
            status = lxp_log_read(&process->canonical_log, offset,
                                  &header, body, header.body_length);
        }
        if (status != LXP_OK) {
            free(body);
            break;
        }
        if (header.global_sequence < expected_sequence) {
            free(body);
            offset += LXP_LOG_HEADER_BYTES + header.body_length;
            continue;
        }
        if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
            header.body_length == 109U &&
            memcmp(body, pending_magic, sizeof(pending_magic)) == 0) {
            pending_sequence = read_u64_be(body + 5U);
            if (pending || activity_bytes != NULL || receipt_bytes != NULL ||
                pending_sequence != expected_sequence ||
                pending_sequence != header.global_sequence ||
                lxp_ct_is_zero(body + 13U, 32U) ||
                lxp_ct_is_zero(body + 45U, 32U) ||
                lxp_ct_is_zero(body + 77U, 32U))
                status = LXP_ERR_LOG_CORRUPT;
            else {
                pending = true;
                (void)memcpy(pending_activity_id, body + 13U, 32U);
                (void)memcpy(pending_previous_root, body + 45U, 32U);
                (void)memcpy(pending_resulting_root, body + 77U, 32U);
            }
        } else if (header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
            if (!pending || activity_bytes != NULL || receipt_bytes != NULL ||
                header.global_sequence != pending_sequence)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                activity_bytes = body;
                activity_length = header.body_length;
                body = NULL;
            }
        } else if (header.record_kind == (uint8_t)LXP_LOG_RECEIPT) {
            lxp_receipt receipt;
            uint8_t activity_id[32];
            if (!pending || activity_bytes == NULL || receipt_bytes != NULL ||
                header.global_sequence != pending_sequence)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                status = lxp_activity_id(activity_bytes, activity_length,
                                         activity_id);
                if (status == LXP_OK)
                    status = lxp_receipt_decode(body, header.body_length,
                                                true, &receipt);
                if (status == LXP_OK &&
                    (receipt.global_sequence != pending_sequence ||
                     lxp_ct_memcmp(activity_id, pending_activity_id, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.activity_id,
                                   pending_activity_id, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.previous_state_root,
                                   pending_previous_root, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.resulting_state_root,
                                   pending_resulting_root, 32U) != 0))
                    status = LXP_ERR_LOG_CORRUPT;
                if (status == LXP_OK) {
                    receipt_bytes = body;
                    receipt_length = header.body_length;
                    body = NULL;
                }
            }
        } else if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
                   header.body_length == 77U &&
                   memcmp(body, complete_magic, sizeof(complete_magic)) == 0) {
            lxp_receipt receipt;
            uint8_t digest[32];
            size_t mark = lxp_arena_mark(&process->execution_arena);
            if (!pending || activity_bytes == NULL || receipt_bytes == NULL ||
                read_u64_be(body + 5U) != pending_sequence ||
                header.global_sequence != pending_sequence)
                status = LXP_ERR_LOG_CORRUPT;
            if (status == LXP_OK)
                status = lxp_receipt_decode(receipt_bytes, receipt_length,
                                            true, &receipt);
            if (status == LXP_OK)
                status = lxp_receipt_digest(
                    &receipt, &process->execution_arena, digest);
            if (status == LXP_OK &&
                (lxp_ct_memcmp(body + 13U, digest, 32U) != 0 ||
                 lxp_ct_memcmp(body + 45U,
                               pending_resulting_root, 32U) != 0))
                status = LXP_ERR_LOG_CORRUPT;
            (void)lxp_arena_reset(&process->execution_arena, mark);
            if (status == LXP_OK)
                status = replay_canonical_group(
                    process, activity_bytes, activity_length,
                    receipt_bytes, receipt_length);
            if (status == LXP_OK) {
                free(activity_bytes);
                free(receipt_bytes);
                activity_bytes = NULL;
                receipt_bytes = NULL;
                activity_length = 0U;
                receipt_length = 0U;
                pending = false;
                if (expected_sequence == UINT64_MAX)
                    status = LXP_ERR_OVERFLOW;
                else
                    ++expected_sequence;
            }
        } else {
            status = LXP_ERR_LOG_CORRUPT;
        }
        free(body);
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    if (status == LXP_OK && pending && activity_bytes != NULL &&
        receipt_bytes != NULL)
        status = replay_canonical_group(
            process, activity_bytes, activity_length,
            receipt_bytes, receipt_length);
    if (status == LXP_OK &&
        ((pending && (activity_bytes == NULL || receipt_bytes == NULL)) ||
         process->state.next_sequence !=
             (owner->feed_store.scanned_through_sequence == 0U ?
                  owner->feed_store.baseline_next_sequence :
                  owner->feed_store.scanned_through_sequence + 1U) ||
         lxp_ct_memcmp(process->kernel.current_state_root,
                       owner->feed_store.scanned_through_sequence == 0U ?
                           owner->feed_store.baseline_state_root :
                           owner->feed_store.head_state_root,
                       32U) != 0))
        status = LXP_ERR_PROJECTION_STALE;
    free(activity_bytes);
    free(receipt_bytes);
    if (status == LXP_OK) status = resume_batch_number(process);
    return status;
}

static lxp_result apply_canonical_activity(
    void *context, uint64_t global_sequence,
    const uint8_t *canonical_activity, size_t activity_length)
{
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    lxp_activity activity;
    lxp_identity *identity;
    lx_account *principal;
    lxp_authority_scope scope;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_receipt receipt;
    lxp_byte_span canonical_receipt;
    lxp_byte_span canonical_header;
    lxp_byte_span activities[1];
    lxp_byte_span receipts[1];
    lxp_batch_roots roots;
    lxp_batch_header header;
    lxp_batch_seal_input seal;
    lxp_merkle_proof proof;
    uint8_t header_signature[64];
    uint8_t batch_preimage[32U + 32U + 8U + 8U];
    uint8_t activity_id[32];
    uint8_t grant_id[32] = {0};
    uint64_t timestamp;
    size_t mark;
    lxp_result status;
    if (process == NULL || canonical_activity == NULL ||
        activity_length == 0U || global_sequence != process->state.next_sequence ||
        process->kernel.publication_poisoned || process->next_batch == 0U ||
        process->next_batch <
            process->sequencer_authorization.first_batch_number ||
        process->next_batch >
            process->sequencer_authorization.last_batch_number)
        return LXP_ERR_SEQUENCE_GAP;
    if (pthread_mutex_lock(&process->owner.mutex) != 0) return LXP_ERR_IO;
    process->state.writer = pthread_self();
    mark = lxp_arena_mark(&process->execution_arena);
    status = lxp_activity_decode(canonical_activity, activity_length, &activity);
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(&activity, process->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK &&
        lxp_activity_module_id(activity.activity_type) != LXP_MODULE_PROGRAMS)
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK) status = current_time_ms(&timestamp);
    if (status == LXP_OK)
        status = lxp_identity_resolve(&process->identities,
                                      activity.actor_did.bytes,
                                      activity.actor_did.length, &identity);
    if (status == LXP_OK &&
        (activity.authority.length != 32U ||
         !lxp_identity_key_valid(identity, activity.authority.bytes,
                                 timestamp,
                                 global_sequence)))
        status = LXP_ERR_BAD_SIGNATURE;
    principal = status == LXP_OK ?
        principal_account(process, activity.authority.bytes) : NULL;
    if (status == LXP_OK && principal == NULL)
        status = LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
    if (status != LXP_OK) goto finish;
    (void)memset(&scope, 0, sizeof(scope));
    scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    scope.activity_ordinal_min = 1U;
    scope.activity_ordinal_max = 8U;
    scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    scope.maximum_total = (lxp_u128){UINT64_MAX, UINT64_MAX};
    scope.maximum_per_period = (lxp_u128){UINT64_MAX, UINT64_MAX};
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.actor, identity->did_id, 32U);
    (void)memcpy(authority.principal, principal->id, 32U);
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.verified_key, activity.authority.bytes, 32U);
    authority.scope = &scope;
    status = lxp_authority_hash(authority.kind, grant_id,
                                authority.verified_key,
                                authority.authority_hash);
    if (status != LXP_OK) goto finish;
    (void)memcpy(batch_preimage, process->kernel.current_state_root, 32U);
    (void)memcpy(batch_preimage + 32U, activity_id, 32U);
    write_u64_be(batch_preimage + 64U, global_sequence);
    write_u64_be(batch_preimage + 72U, process->next_batch);
    (void)memset(&execution, 0, sizeof(execution));
    status = lxp_hash_context_value(batch_preimage, sizeof(batch_preimage),
                                    execution.batch_id);
    if (status != LXP_OK) goto finish;
    execution.network_id = process->network_id;
    execution.batch_number = process->next_batch;
    execution.batch_timestamp_ms = timestamp;
    execution.maximum_timestamp_window = UINT64_C(300000);
    execution.epoch = process->kernel.epoch;
    execution.global_sequence = global_sequence;
    execution.recorded_module_version = LX_PROGRAMS_ACCOUNT_ABI_VERSION;
    execution.recorded_fee_schedule_version = 0U;
    execution.parameter_version = process->parameter_version;
    execution.signature_valid = true;
    execution.identities = &process->identities;
    execution.authority = &authority;
    execution.fee_parameters = &process->fees;
    execution.fee_balance = principal->balance;
    execution.gas_limit = UINT64_MAX;
    execution.arena = &process->execution_arena;
    execution.sequencer_private_key = process->sequencer_private_key;
    execution.verified_receipts = &process->verified_receipts;
    (void)memset(&receipt, 0, sizeof(receipt));
    status = lxp_kernel_execute_activity(&process->kernel, &activity,
                                         &execution, &receipt);
    if (status != LXP_OK || process->kernel.publication_poisoned) {
        if (status == LXP_OK) status = LXP_FATAL_INVARIANT;
        goto finish;
    }
    status = persist_state_checkpoint(process, global_sequence);
    if (status != LXP_OK) goto finish;
    status = lxp_receipt_encode(&receipt, true, &process->execution_arena,
                                &canonical_receipt);
    if (status != LXP_OK) goto finish;
    activities[0] = (lxp_byte_span){canonical_activity, activity_length};
    receipts[0] = canonical_receipt;
    status = lxp_batch_roots_compute(
        &(lxp_batch_root_inputs){activities, 1U, receipts, 1U,
                                 NULL, 0U, NULL, 0U, NULL, 0U},
        &process->execution_arena, &roots);
    if (status != LXP_OK) goto finish;
    (void)memset(&seal, 0, sizeof(seal));
    seal.protocol_version = activity.protocol_version;
    seal.network_id = process->network_id;
    seal.epoch = process->kernel.epoch;
    seal.batch_number = process->next_batch;
    seal.first_sequence = global_sequence;
    seal.last_sequence = global_sequence;
    (void)memcpy(seal.previous_state_root,
                 receipt.previous_state_root, 32U);
    (void)memcpy(seal.resulting_state_root,
                 receipt.resulting_state_root, 32U);
    seal.timestamp_ms = timestamp;
    (void)memcpy(seal.sequencer_id,
                 process->sequencer_authorization.sequencer_id, 32U);
    status = lxp_batch_seal(&header, &seal, &roots, &process->batch_log,
                            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_batch_sign(
            &header, process->sequencer_private_key,
            &process->sequencer_authorization, header_signature,
            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(&header,
                                         &process->execution_arena,
                                         &canonical_header);
    if (status != LXP_OK) goto finish;
    (void)memset(&proof, 0, sizeof(proof));
    proof.leaf_index = 0U;
    proof.leaf_count = 1U;
    status = lxp_daemon_protocol_publish_receipt(
        &process->owner, canonical_receipt.bytes, canonical_receipt.length,
        canonical_header.bytes, canonical_header.length, header_signature,
        &proof);
    if (status == LXP_OK)
        status = lxp_daemon_authority_replica_publish(
            process->authority_replica_address,
            process->authority_replica_port,
            process->authority_replica_token,
            process->authority_replica_token_length,
            process->authority_replica_id,
            canonical_receipt.bytes, canonical_receipt.length,
            canonical_header.bytes, canonical_header.length,
            header_signature, &proof);
    if (status == LXP_OK)
        process->next_batch = process->next_batch ==
                                      process->sequencer_authorization
                                          .last_batch_number ?
                                  0U : process->next_batch + 1U;
    if (status == LXP_OK && process->next_batch == 0U) {
        if (pthread_mutex_lock(&process->daemon.mutex) != 0)
            status = LXP_FATAL_INVARIANT;
        else {
            process->daemon.accepting = false;
            process->daemon.failure = LXP_ERR_AUTH_SCOPE;
            process->daemon.queue_count = 0U;
            (void)pthread_cond_broadcast(&process->daemon.queue_changed);
            if (pthread_mutex_unlock(&process->daemon.mutex) != 0)
                status = LXP_FATAL_INVARIANT;
        }
    }
finish:
    (void)lxp_arena_reset(&process->execution_arena, mark);
    if (pthread_mutex_unlock(&process->owner.mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result open_log(lxp_log *log, const char *environment,
                           bool *opened)
{
    const char *path = required_environment(environment);
    lxp_result status = path == NULL ? LXP_ERR_NON_CANONICAL :
                                      lxp_log_open(log, path);
    if (status == LXP_OK) *opened = true;
    return status;
}

static lxp_result resume_batch_number(lxp_daemon_process *process)
{
    uint64_t next = process->sequencer_authorization.first_batch_number;
    uint64_t offset = 0U;
    bool present = true;
    lxp_result status = LXP_OK;
    while (status == LXP_OK && present) {
        lxp_daemon_receipt_evidence evidence;
        lxp_batch_header header;
        size_t mark = lxp_arena_mark(&process->owner_scratch);
        status = lxp_daemon_receipt_authority_scan(
            &process->receipt_authority, &offset,
            &process->owner_scratch, &evidence, &present);
        if (status == LXP_OK && present)
            status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                                             evidence.canonical_header.length,
                                             &header);
        if (status == LXP_OK && present) {
            if (next == 0U || header.batch_number < next)
                status = LXP_ERR_BATCH_GAP;
            else
                next = header.batch_number ==
                               process->sequencer_authorization.last_batch_number ?
                           0U : header.batch_number + 1U;
        }
        (void)lxp_arena_reset(&process->owner_scratch, mark);
    }
    if (status == LXP_OK && next != 0U &&
        (next < process->sequencer_authorization.first_batch_number ||
         next > process->sequencer_authorization.last_batch_number))
        status = LXP_ERR_BATCH_GAP;
    if (status == LXP_OK) process->next_batch = next;
    return status;
}

static lxp_result replicate_authority_history(lxp_daemon_process *process)
{
    uint64_t offset = 0U;
    bool present = true;
    lxp_result status = LXP_OK;
    while (status == LXP_OK && present) {
        lxp_daemon_receipt_evidence evidence;
        size_t mark = lxp_arena_mark(&process->owner_scratch);
        status = lxp_daemon_receipt_authority_scan(
            &process->receipt_authority, &offset,
            &process->owner_scratch, &evidence, &present);
        if (status == LXP_OK && present)
            status = lxp_daemon_authority_replica_publish(
                process->authority_replica_address,
                process->authority_replica_port,
                process->authority_replica_token,
                process->authority_replica_token_length,
                process->authority_replica_id,
                evidence.canonical_receipt.bytes,
                evidence.canonical_receipt.length,
                evidence.canonical_header.bytes,
                evidence.canonical_header.length,
                evidence.header_signature, &evidence.receipt_proof);
        (void)lxp_arena_reset(&process->owner_scratch, mark);
    }
    return status;
}

static lxp_result load_schedule(lxp_daemon_process *process)
{
    uint64_t parameter_version;
    lxp_result status = parse_u64_text(
        required_environment("LAYERX_NODE_PARAMETER_VERSION"),
        &parameter_version);
    if (status != LXP_OK || parameter_version == 0U ||
        parameter_version > UINT16_MAX)
        return LXP_ERR_VERSION_UNSUPPORTED;
    process->parameter_version = (uint32_t)parameter_version;
    process->fees = (lxp_fee_params){
        (uint16_t)parameter_version, {0U, 0U}, {0U, 0U}, {0U, 0U},
        {0U, 0U}, {0U, 0U}, 10000U
    };
    return LXP_OK;
}

static lxp_result project_bootstrap_metering(
    lxp_daemon_process *process, const lxp_snapshot_manifest_record *snapshot)
{
    const char *path = required_environment("LAYERX_NODE_GENESIS_MANIFEST");
    lxp_genesis_manifest *genesis;
    lxp_kernel *candidate;
    uint8_t *bytes;
    uint8_t projected_root[32];
    FILE *file;
    long length;
    lxp_result status;
    if (process == NULL || snapshot == NULL || path == NULL)
        return LXP_ERR_NON_CANONICAL;
    file = fopen(path, "rb");
    if (file == NULL || fseek(file, 0L, SEEK_END) != 0 ||
        (length = ftell(file)) <= 0 || fseek(file, 0L, SEEK_SET) != 0) {
        if (file != NULL) (void)fclose(file);
        return LXP_ERR_IO;
    }
    bytes = (uint8_t *)malloc((size_t)length);
    genesis = (lxp_genesis_manifest *)malloc(sizeof(*genesis));
    candidate = (lxp_kernel *)malloc(sizeof(*candidate));
    if (bytes == NULL || genesis == NULL || candidate == NULL) {
        free(bytes);
        free(genesis);
        free(candidate);
        (void)fclose(file);
        return LXP_ERR_IO;
    }
    if (fread(bytes, 1U, (size_t)length, file) != (size_t)length ||
        fclose(file) != 0)
        status = LXP_ERR_IO;
    else
        status = lxp_genesis_parse(bytes, (size_t)length,
                                   LXP_GENESIS_INPUT_MANIFEST, genesis);
    if (status == LXP_OK && lxp_ct_memcmp(
            genesis->genesis_state_root, snapshot->state_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) {
        *candidate = process->kernel;
        status = lxp_programs_metering_genesis_project(
            genesis, &process->owner_scratch, candidate);
        if (status == LXP_OK)
            status = lxp_programs_fee_genesis_project(
                genesis, &process->owner_scratch, candidate);
    }
    if (status == LXP_OK)
        status = lxp_state_root(candidate, projected_root);
    if (status == LXP_OK && lxp_ct_memcmp(
            projected_root, snapshot->state_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) {
        process->kernel.module_kv_count = candidate->module_kv_count;
        (void)memcpy(process->kernel.module_kv, candidate->module_kv,
                     candidate->module_kv_count *
                         sizeof(candidate->module_kv[0]));
    }
    free(bytes);
    free(genesis);
    free(candidate);
    return status;
}

static void close_process(lxp_daemon_process *process)
{
    if (process->daemon_started) {
        (void)lxp_daemon_shutdown(&process->daemon);
        process->daemon_started = false;
    }
    if (process->owner.attached)
        (void)lxp_daemon_protocol_owner_detach(&process->owner);
    if (process->history_open) (void)lxp_history_close(&process->history);
    if (process->batch_open) (void)lxp_log_close(&process->batch_log);
    if (process->authority_open) (void)lxp_log_close(&process->authority_log);
    if (process->canonical_open) (void)lxp_log_close(&process->canonical_log);
    if (process->feed_open) (void)lxp_log_close(&process->feed_log);
    if (process->state_open) (void)lxp_state_store_destroy(&process->state);
    lxp_secure_zero(process->sequencer_private_key, 32U);
    lxp_secure_zero(process->authority_replica_token,
                    sizeof(process->authority_replica_token));
    free(process->checkpoint_arena_bytes);
    free(process->execution_arena_bytes);
    free(process->owner_scratch_bytes);
}

static lxp_result open_process(lxp_daemon_process *process,
                               const char *configuration_path,
                               lxp_daemon_configuration *configuration,
                               const char **activity_fifo,
                               const char **listener_address,
                               uint16_t *listener_port)
{
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    lxp_arena snapshot_arena;
    uint8_t *snapshot_bytes;
    char snapshot_path[4096];
    bool checkpoint_selected = false;
    uint64_t value;
    const char *bearer;
    const char *replica_token;
    lxp_result status;
    (void)memset(process, 0, sizeof(*process));
    process->owner_scratch_bytes =
        (uint8_t *)malloc(LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    process->execution_arena_bytes =
        (uint8_t *)malloc(NODE_EXECUTION_ARENA_BYTES);
    process->checkpoint_arena_bytes =
        (uint8_t *)malloc(NODE_SNAPSHOT_ARENA_BYTES);
    snapshot_bytes = (uint8_t *)malloc(NODE_SNAPSHOT_ARENA_BYTES);
    if (process->owner_scratch_bytes == NULL ||
        process->execution_arena_bytes == NULL ||
        process->checkpoint_arena_bytes == NULL || snapshot_bytes == NULL) {
        free(snapshot_bytes);
        return LXP_ERR_IO;
    }
    status = lxp_daemon_config_load(configuration_path, configuration);
    if (status == LXP_OK) process->network_id = configuration->network_id;
    if (status == LXP_OK)
        status = lxp_arena_init(&process->owner_scratch,
            process->owner_scratch_bytes,
            LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    if (status == LXP_OK)
        status = lxp_arena_init(&process->execution_arena,
            process->execution_arena_bytes, NODE_EXECUTION_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_arena_init(&snapshot_arena, snapshot_bytes,
                                NODE_SNAPSHOT_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_arena_init(&process->checkpoint_arena,
            process->checkpoint_arena_bytes, NODE_SNAPSHOT_ARENA_BYTES);
    if (status == LXP_OK) status = lx_account_registry_init(&process->accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(&process->state, 1U);
        process->state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(&process->state,
                                               &process->accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(&process->kernel, &process->state,
                                   &process->journal, configuration, 1U);
    if (status == LXP_OK)
        status = lxp_kernel_register_module(
            &process->kernel, programs_module_registration_v2());
    if (status == LXP_OK)
        status = lxp_kernel_set_capabilities(
            &process->kernel, NULL, lxp_kernel_canonical_ledger_apply);
    process->checkpoint_directory = required_environment(
        "LAYERX_NODE_CHECKPOINT_DIRECTORY");
    if (status == LXP_OK)
        status = latest_snapshot_path(
            process->checkpoint_directory,
            required_environment("LAYERX_NODE_SNAPSHOT"), snapshot_path,
            &checkpoint_selected);
    if (status == LXP_OK) process->checkpoint_selected = checkpoint_selected;
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(snapshot_path, &snapshot_arena,
                                         &manifest, &snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_load(snapshot.bytes, snapshot.length,
                                   &manifest, manifest.state_root,
                                   &process->kernel);
    if (status == LXP_OK && !checkpoint_selected)
        status = project_bootstrap_metering(process, &manifest);
    free(snapshot_bytes);
    if (status == LXP_OK &&
        configuration->start_sequence > process->state.next_sequence)
        status = LXP_ERR_SEQUENCE_GAP;
    if (status == LXP_OK)
        configuration->start_sequence = process->state.next_sequence;
    if (status == LXP_OK) status = collect_assets(process);
    if (status == LXP_OK) status = load_schedule(process);
    if (status == LXP_OK) {
        process->programs.accounts = &process->accounts;
        process->programs.assets = process->assets;
        process->programs.asset_count = process->asset_count;
        process->programs.resolve_occupancy_parameters = occupancy_parameters;
        process->programs.occupancy_parameter_context = process;
        process->programs.resolve_metering_schedule =
            lxp_programs_metering_resolve_runtime;
        process->programs.metering_schedule_context = &process->kernel;
    }
    if (status == LXP_OK)
        status = load_identities(
            required_environment("LAYERX_NODE_IDENTITIES"),
            &process->identities);
    if (status == LXP_OK && checkpoint_selected)
        status = identity_checkpoint_load(snapshot_path,
                                           manifest.global_sequence,
                                           &process->identities);
    if (status == LXP_OK) status = open_log(
        &process->feed_log, "LAYERX_NODE_PROGRAM_FEED_LOG",
        &process->feed_open);
    if (status == LXP_OK) status = open_log(
        &process->canonical_log, "LAYERX_NODE_CANONICAL_LOG",
        &process->canonical_open);
    if (status == LXP_OK) status = open_log(
        &process->authority_log, "LAYERX_NODE_RECEIPT_AUTHORITY_LOG",
        &process->authority_open);
    if (status == LXP_OK) status = open_log(
        &process->batch_log, "LAYERX_NODE_BATCH_LOG", &process->batch_open);
    if (status == LXP_OK)
        status = lxp_history_open(
            &process->history, &process->canonical_log,
            required_environment("LAYERX_NODE_HISTORY_DATABASE"),
            required_environment("LAYERX_NODE_HISTORY_MIGRATIONS"));
    process->history_open = status == LXP_OK;
    if (status == LXP_OK)
        status = lxp_verified_receipt_index_init(
            &process->verified_receipts);
    if (status == LXP_OK)
        status = decode_hex(
            required_environment("LAYERX_NODE_SEQUENCER_ID"),
            process->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = decode_hex(
            required_environment("LAYERX_NODE_SEQUENCER_PUBLIC_KEY"),
            process->sequencer_authorization.public_key, 32U);
    if (status == LXP_OK)
        status = decode_hex(
            required_environment("LAYERX_NODE_SEQUENCER_PRIVATE_KEY"),
            process->sequencer_private_key, 32U);
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_FIRST_BATCH"), &value);
    if (status == LXP_OK) process->sequencer_authorization.first_batch_number = value;
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_LAST_BATCH"), &value);
    if (status == LXP_OK) process->sequencer_authorization.last_batch_number = value;
    process->sequencer_authorization.authorized = 1U;
    if (status == LXP_OK)
        status = lxp_daemon_receipt_authority_open(
            &process->receipt_authority, &process->authority_log,
            &process->sequencer_authorization);
    process->authority_replica_address = required_environment(
        "LAYERX_NODE_AUTHORITY_REPLICA_ADDRESS");
    if (status == LXP_OK &&
        (process->authority_replica_address == NULL ||
         strcmp(process->authority_replica_address, "127.0.0.1") != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = parse_u64_text(required_environment(
            "LAYERX_NODE_AUTHORITY_REPLICA_PORT"), &value);
    if (status == LXP_OK && (value == 0U || value > UINT16_MAX))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) process->authority_replica_port = (uint16_t)value;
    if (status == LXP_OK)
        status = decode_hex(required_environment(
            "LAYERX_NODE_AUTHORITY_REPLICA_ID"),
            process->authority_replica_id, 32U);
    if (status == LXP_OK &&
        lxp_ct_is_zero(process->authority_replica_id, 32U))
        status = LXP_ERR_NON_CANONICAL;
    replica_token = required_environment(
        "LAYERX_NODE_AUTHORITY_REPLICA_BEARER_TOKEN");
    if (status == LXP_OK &&
        (replica_token == NULL || strlen(replica_token) < 32U ||
         strlen(replica_token) > sizeof(process->authority_replica_token)))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        process->authority_replica_token_length = strlen(replica_token);
        (void)memcpy(process->authority_replica_token, replica_token,
                     process->authority_replica_token_length);
    }
    if (status == LXP_OK) status = resume_batch_number(process);
    if (status == LXP_OK) {
        lx_programs_metering_schedule metering_schedule;
        status = lxp_programs_metering_schedule_current(
            &process->kernel,
            process->next_batch != 0U ? process->next_batch :
                process->sequencer_authorization.last_batch_number,
            &metering_schedule);
    }
    if (status == LXP_OK) {
        lx_programs_fee_schedule fee_schedule;
        uint8_t occupancy_asset_id[32];
        status = lxp_programs_fee_governance_resolve_runtime(
            &process->kernel, 0U, &fee_schedule, occupancy_asset_id);
    }
    if (status == LXP_OK) status = replicate_authority_history(process);
    bearer = required_environment("LAYERX_NODE_PROGRAM_BEARER_TOKEN");
    if (status == LXP_OK)
        status = lxp_daemon_protocol_owner_attach(
            &process->owner, &process->kernel, &process->programs,
            &process->feed_log, &process->canonical_log, &process->history,
            &process->verified_receipts, &process->receipt_authority,
            &process->owner_scratch, replay_canonical_after_snapshot,
            process, (const uint8_t *)bearer,
            bearer == NULL ? 0U : strlen(bearer));
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_PROGRAM_PORT"), &value);
    if (status == LXP_OK && (value == 0U || value > UINT16_MAX))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) *listener_port = (uint16_t)value;
    *listener_address = required_environment("LAYERX_NODE_PROGRAM_ADDRESS");
    *activity_fifo = required_environment("LAYERX_NODE_ACTIVITY_FIFO");
    if (status == LXP_OK &&
        (*listener_address == NULL || *activity_fifo == NULL ||
         (strcmp(*listener_address, process->authority_replica_address) == 0 &&
          *listener_port == process->authority_replica_port) ||
         (bearer != NULL && strlen(bearer) ==
              process->authority_replica_token_length &&
          lxp_ct_memcmp(bearer, process->authority_replica_token,
                        process->authority_replica_token_length) == 0) ||
         (process->next_batch != 0U &&
          process->sequencer_authorization.last_batch_number <
              process->next_batch)))
        status = LXP_ERR_NON_CANONICAL;
    return status;
}

static lxp_result read_frame(FILE *input, uint8_t bytes[4])
{
    size_t count = fread(bytes, 1U, 4U, input);
    if (count == 4U) return LXP_OK;
    return ferror(input) ? LXP_ERR_IO : LXP_ERR_TRUNCATED;
}

lxp_result lxp_daemon_serve(const char *configuration_path)
{
    lxp_daemon_process *process;
    lxp_daemon_configuration configuration;
    const char *activity_fifo = NULL;
    const char *listener_address = NULL;
    uint16_t listener_port = 0U;
    lxp_result status;
    process = (lxp_daemon_process *)calloc(1U, sizeof(*process));
    if (process == NULL) return LXP_ERR_IO;
    status = open_process(process, configuration_path, &configuration,
                          &activity_fifo, &listener_address, &listener_port);
    if (status == LXP_OK)
        status = lxp_daemon_start_protocol(
            &process->daemon, &configuration, apply_canonical_activity,
            process, &process->owner, listener_address, listener_port);
    process->daemon_started = status == LXP_OK;
    if (status == LXP_OK && process->next_batch == 0U) {
        if (pthread_mutex_lock(&process->daemon.mutex) != 0)
            status = LXP_ERR_IO;
        else {
            process->daemon.accepting = false;
            process->daemon.failure = LXP_ERR_AUTH_SCOPE;
            if (pthread_mutex_unlock(&process->daemon.mutex) != 0)
                status = LXP_ERR_IO;
        }
    }
    if (status == LXP_OK) {
        struct sigaction action;
        (void)memset(&action, 0, sizeof(action));
        action.sa_handler = request_stop;
        (void)sigemptyset(&action.sa_mask);
        if (sigaction(SIGINT, &action, NULL) != 0 ||
            sigaction(SIGTERM, &action, NULL) != 0)
            status = LXP_ERR_IO;
    }
    while (status == LXP_OK && !stop_requested) {
        FILE *input = fopen(activity_fifo, "rb");
        if (input == NULL) { status = LXP_ERR_IO; break; }
        while (status == LXP_OK && !stop_requested) {
            uint8_t length_bytes[4];
            uint8_t activity[LXP_DAEMON_ACTIVITY_BYTES];
            uint32_t length;
            status = read_frame(input, length_bytes);
            if (status == LXP_ERR_TRUNCATED && feof(input)) {
                status = LXP_OK;
                break;
            }
            if (status != LXP_OK) break;
            length = ((uint32_t)length_bytes[0] << 24U) |
                     ((uint32_t)length_bytes[1] << 16U) |
                     ((uint32_t)length_bytes[2] << 8U) |
                     length_bytes[3];
            if (length == 0U || length > sizeof(activity) ||
                fread(activity, 1U, length, input) != length) {
                status = LXP_ERR_TRUNCATED;
                break;
            }
            status = lxp_daemon_submit(&process->daemon, activity, length);
        }
        if (fclose(input) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    }
    close_process(process);
    free(process);
    return status;
}
