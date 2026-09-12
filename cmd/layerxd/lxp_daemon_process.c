#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_daemon.h"

#include "layerx/lxp_activity.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_batch_identity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_fee.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_bridge_credit.h"
#include "layerx/lxp_snapshot.h"
#include "lxp_daemon_artifact.h"
#include "lxp_daemon_batch_wal.h"
#include "lxp_daemon_lni_internal.h"
#include "lxp_daemon_finality_authority.h"

#include <openssl/evp.h>

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

static uint64_t pay_timing_us(void)
{
    struct timespec now;
    return clock_gettime(CLOCK_MONOTONIC, &now) == 0 ?
        (uint64_t)now.tv_sec * 1000000U + (uint64_t)now.tv_nsec / 1000U : 0U;
}

enum {
    NODE_EXECUTION_ARENA_BYTES = LXP_MAX_ACTIVITY_BYTES * 3U,
    NODE_SNAPSHOT_ARENA_BYTES = LXP_MAX_ACTIVITY_BYTES * 4U
};

struct postcommit_job;

typedef struct lxp_daemon_process {
    lxp_daemon daemon;
    lxp_daemon_lni_server lni;
    lxp_daemon_protocol_owner owner;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    uint16_t protocol_version;
    bool custody_credit_enabled;
    lx_account_registry accounts;
    lxp_transfer_asset_state assets[LX_ASSET_REGISTRY_CAPACITY];
    lx_asset_record send_assets[LX_ASSET_REGISTRY_CAPACITY];
    lx_asset_runtime asset_runtime;
    size_t asset_count;
    lx_programs_transfer_runtime programs;
    lxp_identity_store identities;
    uint8_t admitted_identity_digest[32];
    lxp_fee_params fees;
    lxp_log feed_log;
    lxp_log canonical_log;
    lxp_log authority_log;
    lxp_log batch_log;
    lxp_log availability_log;
    bool availability_log_open;
    lxp_log evidence_log;
    lxp_history history;
    lxp_verified_receipt_index verified_receipts;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_daemon_evidence_store evidence_store;
    lxp_da_store availability_store;
    lxp_batch_body prepared_availability_body;
    uint64_t availability_retain_batches;
    lxp_daemon_finality_authority finality_authority;
    lxp_sequencer_authorization sequencer_authorization;
    uint8_t sequencer_private_key[32];
    uint8_t authority_replica_id[32];
    uint8_t authority_replica_token[LXP_DAEMON_BEARER_MAX_BYTES];
    size_t authority_replica_token_length;
    const char *authority_replica_address;
    uint16_t authority_replica_port;
    uint8_t *owner_scratch_bytes;
    lxp_arena owner_scratch;
    uint8_t *availability_scratch_bytes;
    lxp_arena availability_scratch;
    uint8_t *execution_arena_bytes;
    lxp_arena execution_arena;
    uint8_t *checkpoint_arena_bytes;
    lxp_arena checkpoint_arena;
    const char *checkpoint_directory;
    uint64_t next_batch;
    uint32_t parameter_version;
    uint32_t network_id;
    uint64_t bootstrap_sealed_timestamp;
    bool state_open;
    bool history_open;
    bool feed_open;
    bool canonical_open;
    bool authority_open;
    bool batch_open;
    bool evidence_open;
    bool daemon_started;
    bool lni_started;
    bool checkpoint_selected;
    pthread_mutex_t postcommit_mutex;
    pthread_mutex_t publication_mutex;
    pthread_cond_t postcommit_changed;
    pthread_t postcommit_thread;
    struct postcommit_job *postcommit_head;
    struct postcommit_job *postcommit_tail;
    uint64_t postcommit_local_started_batch;
    lxp_result postcommit_status;
    bool postcommit_initialized;
    bool postcommit_started;
    bool postcommit_stopping;
} lxp_daemon_process;

static lxp_result resume_batch_number(lxp_daemon_process *process);
static lxp_result initialized_genesis_marker_identity(
    lxp_daemon_process *process, bool create, bool *present,
    const uint8_t identity_digest[32]);

static lxp_result availability_prune(lxp_daemon_process *process, uint64_t head)
{
    uint64_t low = head >= process->availability_retain_batches ?
        head - process->availability_retain_batches + 1U : 0U;
    uint64_t checkpoint = process->evidence_store.latest_finalized_batch;
    DIR *directory;
    struct dirent *entry;
    int descriptor;
    bool changed = false;
    lxp_result status = LXP_OK;
    if (checkpoint == 0U) return LXP_OK;
    if (checkpoint < low) low = checkpoint;
    descriptor = open(process->availability_store.directory,
                      O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) return LXP_ERR_IO;
    directory = fdopendir(descriptor);
    if (directory == NULL) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    while (status == LXP_OK) {
        uint64_t batch = 0U;
        size_t i;
        struct stat information;
        errno = 0;
        entry = readdir(directory);
        if (entry == NULL) {
            if (errno != 0) status = LXP_ERR_IO;
            break;
        }
        if (strlen(entry->d_name) != 25U || strcmp(entry->d_name + 20U, ".lxda") != 0)
            continue;
        for (i = 0U; i < 20U; ++i) {
            unsigned digit = (unsigned)(entry->d_name[i] - '0');
            if (digit > 9U || batch > (UINT64_MAX - digit) / 10U) break;
            batch = batch * 10U + digit;
        }
        if (i != 20U || batch >= low) continue;
        if (fstatat(descriptor, entry->d_name, &information, AT_SYMLINK_NOFOLLOW) != 0 ||
            !S_ISREG(information.st_mode) || information.st_nlink != 1) {
            status = LXP_ERR_IO;
            break;
        }
        if (unlinkat(descriptor, entry->d_name, 0) != 0) status = LXP_ERR_IO;
        else changed = true;
    }
    if (changed && !lxp_durability_group_defer_descriptor(descriptor) &&
        fsync(descriptor) != 0)
        status = LXP_ERR_IO;
    if (closedir(directory) != 0) status = LXP_ERR_IO;
    return status;
}

static lxp_result availability_store_body_with_arena(
    lxp_daemon_process *process, const lxp_batch_body *body,
    lxp_arena *arena)
{
    lxp_da_bundle bundle;
    uint8_t root[32];
    size_t mark;
    lxp_result status;
    if (process == NULL || body == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_batch_availability_root(body, arena, root);
    if (status == LXP_OK &&
        lxp_ct_memcmp(root, body->header.data_availability_root, 32U) != 0)
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_da_bundle_build(body, LXP_DA_CANONICAL_CHUNK_BYTES,
                                     arena, &bundle);
    if (status == LXP_OK)
        status = lxp_da_store_bundle(&process->availability_store, &bundle,
                                     arena);
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static lxp_result availability_store_body(lxp_daemon_process *process,
                                         const lxp_batch_body *body)
{
    return availability_store_body_with_arena(
        process, body, &process->owner_scratch);
}

typedef struct availability_store_job {
    lxp_daemon_process *process;
    const lxp_batch_body *body;
    lxp_result status;
    uint64_t started_us;
    uint64_t finished_us;
} availability_store_job;

static void *availability_store_worker(void *context)
{
    availability_store_job *job = (availability_store_job *)context;
    job->started_us = pay_timing_us();
    job->status = availability_store_body_with_arena(
        job->process, job->body, &job->process->availability_scratch);
    job->finished_us = pay_timing_us();
    return NULL;
}

static void availability_verify_retained(lxp_daemon_process *process)
{
    uint64_t head = process->receipt_authority.last_batch_number;
    uint64_t low = head >= process->availability_retain_batches ?
        head - process->availability_retain_batches + 1U :
        process->sequencer_authorization.first_batch_number;
    uint64_t checkpoint = process->evidence_store.latest_finalized_batch;
    uint64_t offset = 0U, seen = 0U;
    size_t mark = lxp_arena_mark(&process->owner_scratch);
    lxp_result status = LXP_OK;
    if (checkpoint == 0U) low = process->sequencer_authorization.first_batch_number;
    else if (checkpoint < low) low = checkpoint;
    if (low < process->sequencer_authorization.first_batch_number)
        low = process->sequencer_authorization.first_batch_number;
    process->owner.availability_store = &process->availability_store;
    process->owner.availability_ready = false;
    while (status == LXP_OK) {
        lxp_daemon_receipt_evidence evidence;
        lxp_batch_header header;
        lxp_da_bundle bundle;
        bool present = false;
        status = lxp_daemon_receipt_authority_scan(&process->receipt_authority,
            &offset, &process->owner_scratch, &evidence, &present);
        if (status != LXP_OK || !present) break;
        status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                                         evidence.canonical_header.length, &header);
        if (status == LXP_OK && header.batch_number >= low && header.batch_number != seen) {
            if ((seen == 0U && header.batch_number != low) ||
                (seen != 0U && (seen == UINT64_MAX || header.batch_number != seen + 1U)))
                status = LXP_ERR_BATCH_GAP;
            if (status == LXP_OK)
                status = lxp_da_store_read_verified(&process->availability_store,
                    header.batch_number, header.data_availability_root,
                    &process->owner_scratch, &bundle);
            if (status == LXP_OK) seen = header.batch_number;
        }
        (void)lxp_arena_reset(&process->owner_scratch, mark);
    }
    (void)lxp_arena_reset(&process->owner_scratch, mark);
    process->owner.availability_ready = status == LXP_OK && seen == head;
}
static lxp_result recover_ranged_batch_authorities(
    lxp_daemon_process *process);
static lxp_result recover_prepared_batch_wal(
    lxp_daemon_process *process, lxp_daemon_protocol_owner *owner);

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
    uint64_t before_sequence, bool *checkpoint_selected)
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
        if (checkpoint_name(entry->d_name, &sequence) && sequence == 0U) {
            (void)closedir(stream);
            return LXP_ERR_SNAPSHOT_MISMATCH;
        }
        if (checkpoint_name(entry->d_name, &sequence) &&
            (before_sequence == 0U || sequence < before_sequence) &&
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

static lxp_result remove_pending_checkpoint_pair(
    const char *directory, uint64_t sequence)
{
    static const char *const suffixes[2] = {".lxs", ".lxi"};
    int directory_descriptor;
    size_t index;
    bool changed = false;
    lxp_result status = LXP_OK;
    if (directory == NULL || sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    directory_descriptor = open(directory,
        O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
    if (directory_descriptor < 0) return LXP_ERR_IO;
    for (index = 0U; status == LXP_OK && index < 2U; ++index) {
        char name[32];
        struct stat information;
        int length = snprintf(name, sizeof(name), "%020llu%s",
                              (unsigned long long)sequence,
                              suffixes[index]);
        if (length < 0 || (size_t)length >= sizeof(name)) {
            status = LXP_ERR_LENGTH_LIMIT;
            break;
        }
        if (fstatat(directory_descriptor, name, &information,
                    AT_SYMLINK_NOFOLLOW) != 0) {
            if (errno == ENOENT) continue;
            status = LXP_ERR_IO;
            break;
        }
        if (!S_ISREG(information.st_mode) || information.st_nlink != 1U ||
            information.st_uid != geteuid() ||
            (information.st_mode & 0777U) != 0600U) {
            status = LXP_ERR_AUTH_SCOPE;
            break;
        }
        if (unlinkat(directory_descriptor, name, 0) != 0)
            status = LXP_ERR_IO;
        else
            changed = true;
    }
    if (status == LXP_OK && changed && fsync(directory_descriptor) != 0)
        status = LXP_ERR_IO;
    if (close(directory_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    return status;
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

static lxp_result read_identities(FILE *file, lxp_identity_store *identities)
{
    char line[4096];
    lxp_result status = LXP_OK;
    if (file == NULL || identities == NULL) return LXP_ERR_NON_CANONICAL;
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
    return status;
}

static lxp_result load_identities(const char *path, lxp_identity_store *identities)
{
    if (path == NULL || identities == NULL) return LXP_ERR_NON_CANONICAL;
    FILE *file = fopen(path, "rb");
    if (file == NULL) return LXP_ERR_IO;
    lxp_result status = read_identities(file, identities);
    if (fclose(file) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result admit_provisioned_identities(lxp_daemon_process *process)
{
    lxp_result status = LXP_OK;
    if (pthread_mutex_lock(&process->owner.mutex) != 0) return LXP_ERR_IO;
    if (process->protocol_version != 3U || process->state.next_sequence != 1U)
        goto finish;
    const char *path = required_environment("LAYERX_NODE_IDENTITIES");
    int fd = path == NULL ? -1 : open(path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    struct stat info;
    if (fd < 0) { status = LXP_ERR_IO; goto finish; }
    if (fstat(fd, &info) != 0 || !S_ISREG(info.st_mode) ||
        info.st_uid != geteuid() || (info.st_mode & 0777U) != 0600U ||
        info.st_nlink != 1 || info.st_size <= 0 || info.st_size > 1048576) {
        (void)close(fd);
        status = LXP_ERR_NON_CANONICAL;
        goto finish;
    }
    FILE *file = fdopen(fd, "rb");
    if (file == NULL) { (void)close(fd); status = LXP_ERR_IO; goto finish; }
    lxp_identity_store *next = malloc(sizeof(*next));
    status = next == NULL ? LXP_ERR_IO : read_identities(file, next);
    uint8_t digest[32];
    uint8_t *bytes = NULL;
    if (status == LXP_OK && next->count != process->identities.count) {
        bytes = malloc((size_t)info.st_size);
        if (bytes == NULL || fseek(file, 0, SEEK_SET) != 0 ||
            fread(bytes, 1U, (size_t)info.st_size, file) != (size_t)info.st_size)
            status = LXP_ERR_IO;
        if (status == LXP_OK)
            status = lxp_hash_sha256(bytes, (size_t)info.st_size, digest);
        free(bytes);
    }
    if (fclose(file) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status == LXP_OK && next->count < process->identities.count)
        status = LXP_ERR_AUTH_SCOPE;
    for (size_t i = 0U; status == LXP_OK && i < process->identities.count; ++i)
        if (memcmp(&next->identities[i], &process->identities.identities[i],
                   sizeof(lxp_identity)) != 0) status = LXP_ERR_AUTH_SCOPE;
    for (size_t i = process->identities.count; status == LXP_OK && i < next->count; ++i)
        if (next->identities[i].next_sequence != 0U ||
            !lxp_ed25519_pubkey_is_canonical(next->identities[i].primary_key))
            status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK && next->count != process->identities.count) {
        bool present = false;
        status = initialized_genesis_marker_identity(process, false, &present,
                                                      process->admitted_identity_digest);
        if (status == LXP_OK && !present) status = LXP_ERR_ROOT_MISMATCH;
        if (status == LXP_OK)
            status = initialized_genesis_marker_identity(process, true, &present, digest);
        if (status == LXP_OK) {
            (void)memcpy(process->admitted_identity_digest, digest, sizeof(digest));
            process->identities = *next;
        }
    }
    free(next);
finish:
    if (pthread_mutex_unlock(&process->owner.mutex) != 0) status = LXP_ERR_IO;
    return status;
}

static bool asset_activity_supported(uint32_t activity_type)
{
    switch (activity_type) {
    case LX_ASSET_REGISTER:
    case LX_ASSET_PAUSE:
    case LX_ASSET_UNPAUSE:
    case LX_ASSET_ACCOUNT_OPEN:
    case LX_ASSET_SEND:
    case LX_ASSET_RECEIVE:
    case LX_ASSET_GRANT_ISSUE:
    case LX_ASSET_GRANT_REVOKE:
    case LX_ASSET_WITHDRAW:
    case LX_ASSET_MINT:
    case LX_ASSET_BURN:
        return true;
    default:
        return false;
    }
}

static lxp_result collect_assets(lxp_daemon_process *process)
{
    lxp_result status = lx_asset_committed_records(&process->kernel,
        process->send_assets, LX_ASSET_REGISTRY_CAPACITY, &process->asset_count);
    if (status != LXP_OK) return status;
    if (process->asset_count == 0U) return LXP_ERR_ASSET_MISMATCH;
    for (size_t i = 0U; i < process->accounts.count; ++i) {
        const lx_account *account = &process->accounts.accounts[i];
        size_t asset;
        if (!account->has_asset) continue;
        for (asset = 0U; asset < process->asset_count; ++asset)
            if (memcmp(account->asset_id, process->send_assets[asset].asset_id, 32U) == 0) break;
        if (asset == process->asset_count) return LXP_ERR_ASSET_MISMATCH;
    }
    for (size_t asset = 0U; asset < process->asset_count; ++asset) {
        const lx_asset_record *record = &process->send_assets[asset];
        lxp_u128 circulating = {0U, 0U};
        lxp_u128 initial = lxp_u128_is_zero(record->supply_cap) ?
            (lxp_u128){UINT64_MAX, UINT64_MAX} : record->supply_cap;
        uint8_t name[LX_ASSET_ISSUANCE_NAME_BYTES], id[32];
        size_t issuance_count = 0U;
        bool native_record = false;
        status = lx_asset_issuance_name(record->asset_id, name, id);
        if (status != LXP_OK) return status;
        for (size_t i = 0U; i < process->kernel.module_kv_count; ++i) {
            const lxp_module_kv_entry *entry = &process->kernel.module_kv[i];
            if (entry->module_id == LXP_MODULE_ASSET && entry->key_length == 38U &&
                memcmp(entry->key, "asset:", 6U) == 0 &&
                memcmp(entry->key + 6U, record->asset_id, 32U) == 0) native_record = true;
        }
        for (size_t i = 0U; i < process->accounts.count; ++i) {
            const lx_account *account = &process->accounts.accounts[i];
            lxp_u128 issued;
            if (!account->has_asset || memcmp(account->asset_id, record->asset_id, 32U) != 0) continue;
            if (account->kind == LX_ACCOUNT_MODULE_VALUE && memcmp(account->id, id, 32U) == 0) {
                if (account->name_length != sizeof(name) || memcmp(account->name, name, sizeof(name)) != 0 ||
                    lx_account_validate_canonical(account) != LXP_OK ||
                    lxp_u128_sub(initial, account->balance, &issued) != LXP_OK ||
                    lxp_u128_cmp(issued, record->total_units) != 0) return LXP_FATAL_SUPPLY_MISMATCH;
                ++issuance_count;
            } else if (lxp_u128_add(circulating, account->balance, &circulating) != LXP_OK)
                return LXP_FATAL_SUPPLY_MISMATCH;
        }
        if (issuance_count != (native_record ? 1U : 0U) ||
            lxp_u128_cmp(circulating, record->total_units) != 0) return LXP_FATAL_SUPPLY_MISMATCH;
        status = lx_asset_transfer_state(record, &process->assets[asset]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
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

static lxp_result principal_authority(
    lxp_daemon_process *process, const lxp_activity *activity,
    const uint8_t account_key[32], uint8_t principal_id[32],
    lxp_u128 *fee_balance)
{
    static const uint8_t prefix[] = "agent:";
    static const uint8_t suffix[] = ":main";
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t account_id[32];
    size_t length;
    size_t index;
    lxp_result status;
    if (process == NULL || activity == NULL || account_key == NULL ||
        principal_id == NULL ||
        fee_balance == NULL || activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U || activity->authority.length != 32U ||
        activity->actor_did.length > sizeof(name) - sizeof(prefix) - sizeof(suffix) + 2U)
        return LXP_ERR_NON_CANONICAL;
    length = sizeof(prefix) - 1U;
    (void)memcpy(name, prefix, length);
    (void)memcpy(name + length, activity->actor_did.bytes, activity->actor_did.length);
    length += activity->actor_did.length;
    (void)memcpy(name + length, suffix, sizeof(suffix) - 1U);
    length += sizeof(suffix) - 1U;
    status = lx_account_id_from_string(name, length, account_id);
    if (status != LXP_OK) return status;
    *fee_balance = (lxp_u128){0U, 0U};
    for (index = 0U; index < process->accounts.count; ++index) {
        const lx_account *account = &process->accounts.accounts[index];
        if (lxp_ct_memcmp(account->id, account_id, 32U) != 0) continue;
        if (account->kind != LX_ACCOUNT_AGENT_MAIN ||
            !account->has_authority_key ||
            lxp_ct_memcmp(account->authority_key, account_key, 32U) != 0)
            return LXP_ERR_BAD_SIGNATURE;
        *fee_balance = account->balance;
        break;
    }
    if (activity->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT &&
        lxp_activity_module_id(activity->activity_type) == LXP_MODULE_PROGRAMS)
        return lxp_did_id_derive(activity->actor_did.bytes,
                                 activity->actor_did.length, principal_id);
    (void)memcpy(principal_id, account_id, 32U);
    return LXP_OK;
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
    if (status == LXP_OK &&
        !lxp_durability_group_defer_descriptor(descriptor) &&
        fdatasync(descriptor) != 0)
        status = LXP_ERR_IO;
    if (descriptor >= 0 && close(descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    descriptor = -1;
    if (status == LXP_OK && rename(temporary, final) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        directory_descriptor = open(process->checkpoint_directory,
                                    O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        if (directory_descriptor < 0 ||
            (!lxp_durability_group_defer_descriptor(directory_descriptor) &&
             fsync(directory_descriptor) != 0))
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
    lxp_kernel_batch_boundary boundary;
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    size_t mark;
    char temporary[4096];
    int length;
    lxp_result status;
    if (process == NULL || process->checkpoint_directory == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(&process->checkpoint_arena);
    status = lxp_kernel_batch_boundary_read(&process->kernel, &boundary);
    if (status == LXP_OK &&
        (boundary.next_sequence == 0U ||
         boundary.next_sequence - 1U != global_sequence))
        status = LXP_ERR_SEQUENCE_MISMATCH;
    if (status == LXP_OK)
        status = lxp_snapshot_write(&process->kernel, global_sequence,
                                    &process->checkpoint_arena, &snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest(
            snapshot.bytes, snapshot.length, global_sequence,
            boundary.canonical_state_root, boundary.receipt_state_root,
            &manifest);
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

static lxp_result recover_batch_account_evidence(
    lxp_daemon_process *process, const lxp_batch_header *header,
    lxp_byte_span canonical_header, const uint8_t header_signature[64],
    lxp_byte_span canonical_head_receipt,
    const lxp_merkle_proof *head_receipt_proof, bool maintenance)
{
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    lxp_state_store *state = NULL;
    lxp_state_journal *journal = NULL;
    lxp_kernel *kernel = NULL;
    lx_account_registry *accounts = NULL;
    char path[4096];
    size_t mark;
    bool state_open = false;
    int length;
    lxp_result status;
    if (process == NULL || header == NULL || canonical_header.bytes == NULL ||
        header_signature == NULL || canonical_head_receipt.bytes == NULL ||
        head_receipt_proof == NULL || process->checkpoint_directory == NULL)
        return LXP_ERR_NON_CANONICAL;
    length = snprintf(path, sizeof(path), "%s/%020llu.lxs",
                      process->checkpoint_directory,
                      (unsigned long long)header->last_sequence);
    if (length < 0 || (size_t)length >= sizeof(path))
        return LXP_ERR_LENGTH_LIMIT;
    state = (lxp_state_store *)malloc(sizeof(*state));
    journal = (lxp_state_journal *)malloc(sizeof(*journal));
    kernel = (lxp_kernel *)calloc(1U, sizeof(*kernel));
    accounts = (lx_account_registry *)malloc(sizeof(*accounts));
    if (state == NULL || journal == NULL || kernel == NULL ||
        accounts == NULL) {
        status = LXP_ERR_IO;
        goto done;
    }
    (void)memset(journal, 0, sizeof(*journal));
    status = lx_account_registry_init(accounts);
    if (status == LXP_OK) {
        status = lxp_state_store_init(state, 1U);
        state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(state, accounts);
    if (status == LXP_OK) {
        *kernel = process->kernel;
        kernel->state = state;
        kernel->journal = journal;
        (void)memset(kernel->blobs, 0, sizeof(kernel->blobs));
        kernel->blob_count = 0U;
        kernel->blob_total_bytes = 0U;
    }
    mark = lxp_arena_mark(&process->checkpoint_arena);
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(
            path, &process->checkpoint_arena, &manifest, &snapshot);
    if (status == LXP_OK &&
        (manifest.global_sequence != header->last_sequence ||
         lxp_ct_memcmp(manifest.receipt_state_root,
                       header->resulting_state_root, 32U) != 0))
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_snapshot_load(snapshot.bytes, snapshot.length,
                                   &manifest, kernel);
    if (status == LXP_OK)
        status = (maintenance ?
            lxp_daemon_account_evidence_publish_batch_maintenance :
            lxp_daemon_account_evidence_publish_batch)(
            &process->evidence_store, kernel, canonical_head_receipt,
            head_receipt_proof, &process->sequencer_authorization,
            canonical_header, header_signature,
            &process->checkpoint_arena);
    while (kernel->blob_count != 0U)
        free(kernel->blobs[--kernel->blob_count].bytes);
    (void)lxp_arena_reset(&process->checkpoint_arena, mark);
done:
    if (state_open) {
        lxp_result close_status = lxp_state_store_destroy(state);
        if (status == LXP_OK && close_status != LXP_OK) status = close_status;
    }
    lx_account_registry_release(accounts);
    if (accounts != NULL) lxp_secure_zero(accounts, sizeof(*accounts));
    if (kernel != NULL) lxp_secure_zero(kernel, sizeof(*kernel));
    if (journal != NULL) lxp_secure_zero(journal, sizeof(*journal));
    free(accounts);
    free(kernel);
    free(journal);
    free(state);
    return status;
}

static bool terminal_rejection_module_supported(
    const lxp_daemon_process *process, uint32_t activity_type)
{
    uint16_t module_id = lxp_activity_module_id(activity_type);
    return module_id == LXP_MODULE_PROGRAMS ||
           module_id == LXP_MODULE_ASSET ||
           module_id == LXP_MODULE_GOVERNANCE ||
           (process->custody_credit_enabled &&
            module_id == LXP_MODULE_BRIDGE);
}

static uint16_t recorded_module_version_for(uint32_t activity_type)
{
    return (uint16_t)((activity_type == LXP_BRIDGE_CREDIT ||
                       lxp_governance_activity(activity_type)) ? 1U :
                      (asset_activity_supported(activity_type) ?
                       lx_asset_module_iface()->abi_version :
                       LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION));
}

static lxp_result persist_prepared_batch_checkpoint(
    void *context, const lxp_kernel_batch_boundary *settled)
{
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    lxp_kernel_batch_boundary live;
    lxp_result status;
    if (process == NULL || settled == NULL || settled->next_sequence <= 1U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_kernel_batch_boundary_read(&process->kernel, &live);
    if (status == LXP_OK &&
        (live.next_sequence != settled->next_sequence ||
         lxp_ct_memcmp(live.receipt_state_root,
                       settled->receipt_state_root, 32U) != 0 ||
         lxp_ct_memcmp(live.canonical_state_root,
                       settled->canonical_state_root, 32U) != 0))
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = persist_state_checkpoint(
            process, settled->next_sequence - 1U);
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
    uint8_t principal_id[32];
    lxp_u128 principal_balance = {0U, 0U};
    lxp_authority_grant grant;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_byte_span encoded_receipt;
    uint8_t activity_id[32];
    bool decoded = false;
    lxp_result status;
    uint8_t fee_wire[LXP_FEE_PARAMS_V2_BYTES];
    size_t fee_wire_length;
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
    if ((expected->module_id != LXP_MODULE_PROGRAMS &&
         expected->module_id != LXP_MODULE_ASSET &&
         expected->module_id != LXP_MODULE_GOVERNANCE &&
         !(process->custody_credit_enabled && expected->module_id == LXP_MODULE_BRIDGE)) ||
        expected->module_version == 0U ||
        expected->parameter_version != process->parameter_version ||
        process->programs.fee_schedule.version !=
            expected->parameter_version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    status = lxp_fee_params_encode(&process->fees, fee_wire, sizeof(fee_wire), &fee_wire_length);
    if (status != LXP_OK) return status;
    status = lxp_activity_decode(canonical_activity, activity_length, activity);
    if (status == LXP_OK &&
        activity->protocol_version != process->protocol_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) decoded = true;
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(activity, process->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK &&
        lxp_activity_module_id(activity->activity_type) != LXP_MODULE_PROGRAMS &&
        !asset_activity_supported(activity->activity_type) &&
        !lxp_governance_activity(activity->activity_type) &&
        !(process->custody_credit_enabled && activity->activity_type == LXP_BRIDGE_CREDIT))
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK &&
        (expected->module_id != lxp_activity_module_id(activity->activity_type) ||
         (asset_activity_supported(activity->activity_type) &&
          expected->module_version != lx_asset_module_iface()->abi_version) ||
         (activity->activity_type == LXP_BRIDGE_CREDIT && expected->module_version != 1U)))
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = lxp_identity_resolve(&process->identities,
                                      activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK) status = lxp_governance_identity_refresh(&process->kernel, identity);
    if (status == LXP_OK && activity->authority.length != 32U)
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_OK)
        status = lxp_authority_resolve_activity(
            &process->kernel, identity, activity,
            lxp_identity_key_valid(identity, activity->authority.bytes,
                                   timestamp, global_sequence),
            true, timestamp, UINT64_C(300000), global_sequence, &grant,
            &authority);
    if (status == LXP_OK)
        status = principal_authority(process, activity,
            grant.kind == LXP_AUTHORITY_OWNER ? grant.key :
                identity->primary_key, principal_id, &principal_balance);
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
    if (status != LXP_OK) {
        lxp_result refusal = status;
        lxp_byte_span offered = {canonical_activity, activity_length};
        lxp_batch_roots bound_roots;
        uint8_t bound_batch_id[32];
        if (!decoded || !lxp_terminal_rejection_applies(refusal) ||
            !terminal_rejection_module_supported(process,
                                                 activity->activity_type) ||
            expected->result_code != refusal ||
            expected->timestamp != timestamp ||
            expected->module_id !=
                lxp_activity_module_id(activity->activity_type) ||
            expected->module_version !=
                recorded_module_version_for(activity->activity_type) ||
            expected->program_outcome.present)
            return refusal;
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
        if (status != LXP_OK) return refusal;
        (void)memset(&execution, 0, sizeof(execution));
        if (lxp_protocol_version_uses_occupancy(process->protocol_version))
            status = lxp_daemon_batch_bind_prefix(
                &offered, 1U, process->kernel.current_state_root,
                global_sequence, batch_number, &process->execution_arena,
                &execution, &bound_roots, bound_batch_id);
        else
            status = lxp_batch_identity_activity(
                process->kernel.current_state_root, activity_id,
                global_sequence, batch_number, execution.batch_id);
        if (status != LXP_OK) return status;
        execution.network_id = process->network_id;
        execution.batch_number = batch_number;
        execution.batch_timestamp_ms = timestamp;
        execution.maximum_timestamp_window = UINT64_C(300000);
        execution.epoch = process->kernel.epoch;
        execution.global_sequence = global_sequence;
        execution.recorded_module_version = expected->module_version;
        execution.parameter_version = expected->parameter_version;
        execution.signature_valid = true;
        execution.identities = &process->identities;
        execution.fee_parameters = &process->fees;
        execution.gas_limit = UINT64_MAX;
        execution.arena = &process->execution_arena;
        execution.sequencer_private_key = process->sequencer_private_key;
        execution.verified_receipts = &process->verified_receipts;
        (void)memset(receipt, 0, sizeof(*receipt));
        status = lxp_kernel_terminal_rejection(&process->kernel, activity,
                                               &execution, refusal, receipt);
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
    (void)memcpy(authority.principal, principal_id, 32U);
    (void)memset(&execution, 0, sizeof(execution));
    status = lxp_batch_identity_activity(
        process->kernel.current_state_root, activity_id, global_sequence,
        batch_number, execution.batch_id);
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
    execution.fee_balance = principal_balance;
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

static lxp_result check_batch_record(
    lxp_daemon_process *process, const lxp_batch_header *expected,
    const uint8_t *canonical_header, size_t header_length,
    bool append_missing)
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
        if (found && !append_missing)
            return LXP_FATAL_REPLAY_DIVERGENCE;
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
             header.first_sequence == 0U ||
             header.last_sequence < header.first_sequence ||
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
        expected->first_sequence == 0U ||
        expected->last_sequence < expected->first_sequence)
        return LXP_ERR_BATCH_GAP;
    if (prior_batch == 0U &&
        expected->batch_number !=
            process->sequencer_authorization.first_batch_number)
        return LXP_ERR_BATCH_GAP;
    if (!append_missing) return LXP_OK;
    status = lxp_log_append(&process->batch_log, LXP_LOG_BATCH_HEADER,
                            expected->last_sequence, canonical_header,
                            (uint32_t)header_length, NULL);
    if (status == LXP_OK) status = lxp_log_write_boundary(&process->batch_log);
    return status;
}

static lxp_result ensure_batch_record(
    lxp_daemon_process *process, const lxp_batch_header *expected,
    const uint8_t *canonical_header, size_t header_length)
{
    return check_batch_record(process, expected, canonical_header,
                              header_length, true);
}

static lxp_result replay_publish_evidence(
    lxp_daemon_process *process, const uint8_t *canonical_activity,
    size_t activity_length, const uint8_t *canonical_receipt,
    size_t receipt_length, const lxp_activity *activity,
    const lxp_receipt *receipt, uint64_t batch_number)
{
    lxp_byte_span activities[1];
    lxp_byte_span receipts[1];
    lxp_byte_span events[1];
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
    status = lxp_programs_project_receipt_events(
        receipt, &process->execution_arena, &events[0]);
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(
        &(lxp_batch_root_inputs){activities, 1U, receipts, 1U,
                                 events, 1U, NULL, 0U, NULL, 0U},
        &process->execution_arena, &roots);
    if (status == LXP_OK) {
        lxp_batch_body body;
        status = lxp_da_log_read_body(&process->availability_log, batch_number,
                                      &process->execution_arena, &body);
        if (status == LXP_OK)
            status = lxp_replay_section_encode(activities, 1U, &process->execution_arena,
                                               &body.activities);
        if (status == LXP_OK)
            status = lxp_da_receipt_section_encode(receipts, 1U, events, 1U,
                                                   &process->execution_arena, &body.receipts);
        if (status == LXP_OK)
            status = lxp_batch_availability_root(&body, &process->execution_arena,
                                                 roots.data_availability_root);
    }
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
        status = lxp_daemon_activity_evidence_publish(
            &process->evidence_store,
            (lxp_byte_span){canonical_activity, activity_length}, &proof,
            (lxp_byte_span){canonical_receipt, receipt_length}, &proof,
            &process->sequencer_authorization, canonical_header, signature,
            &process->execution_arena, NULL);
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
    bool authority_exists = false;
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
        if (status == LXP_OK) {
            batch_number = existing_header.batch_number;
            authority_exists = true;
        }
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
    if (status == LXP_OK && !authority_exists)
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
    bool maintenance_present = false;
    uint64_t maintenance_timestamp = 0U;
    lxp_activity activity;
    lxp_receipt receipt;
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header existing_header;
    uint8_t digest[32];
    uint64_t batch_number = 0U;
    bool authority_exists = false;
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
            header.body_length > 13U && memcmp(body, "LXPM1", 5U) == 0) {
            if (pending || complete || maintenance_present || receipt_bytes != NULL)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                maintenance_present = true;
                maintenance_timestamp = read_u64_be(body + 5U);
                receipt_length = header.body_length - 13U;
                (void)memmove(body, body + 13U, receipt_length);
                receipt_bytes = body;
                body = NULL;
            }
        } else if (header.global_sequence == target &&
            header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
            header.body_length == 109U &&
            memcmp(body, pending_magic, sizeof(pending_magic)) == 0) {
            if (pending || complete || maintenance_present || read_u64_be(body + 5U) != target)
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
    if (status == LXP_OK && maintenance_present) {
        lxp_programs_occupancy_receipt record;
        status = lxp_programs_occupancy_receipt_decode(receipt_bytes, receipt_length, &record);
        if (status == LXP_OK &&
            (!lxp_protocol_version_uses_occupancy(process->protocol_version) ||
             record.global_sequence != target || maintenance_timestamp == 0U ||
             lxp_ct_memcmp(record.resulting_state_root, process->kernel.current_state_root, 32U) != 0))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_hash_sha256(receipt_bytes, receipt_length, digest);
        if (status == LXP_OK)
            status = lxp_daemon_receipt_authority_lookup(&process->receipt_authority,
                digest, &process->execution_arena, &evidence);
        if (status == LXP_OK)
            status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                evidence.canonical_header.length, &existing_header);
        if (status == LXP_OK &&
            (existing_header.batch_number != record.batch_number ||
             existing_header.last_sequence != target ||
             existing_header.timestamp_ms != maintenance_timestamp ||
             evidence.canonical_receipt.length != receipt_length ||
             lxp_ct_memcmp(evidence.canonical_receipt.bytes, receipt_bytes, receipt_length) != 0))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        free(activity_bytes);
        free(receipt_bytes);
        (void)lxp_arena_reset(&process->execution_arena, arena_mark);
        return status;
    }
    if (status == LXP_OK &&
        (!pending || !complete || activity_bytes == NULL ||
         receipt_bytes == NULL))
        status = LXP_ERR_PROJECTION_STALE;
    if (status == LXP_OK)
        status = lxp_activity_decode(activity_bytes, activity_length,
                                     &activity);
    if (status == LXP_OK &&
        activity.protocol_version != process->protocol_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
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
        if (status == LXP_OK) {
            batch_number = existing_header.batch_number;
            authority_exists = true;
        }
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
    if (status == LXP_OK && !authority_exists)
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
    status = recover_prepared_batch_wal(process, owner);
    if (status == LXP_OK) status = reconcile_snapshot_evidence(process);
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
            header.body_length > 13U && memcmp(body, "LXPM1", 5U) == 0) {
            lxp_programs_occupancy_receipt recorded;
            lxp_programs_occupancy_receipt replayed;
            lxp_byte_span encoded;
            lxp_daemon_receipt_evidence authority;
            uint8_t digest[32];
            uint64_t timestamp = read_u64_be(body + 5U);
            size_t maintenance_mark = lxp_arena_mark(&process->execution_arena);
            if (pending || activity_bytes != NULL || receipt_bytes != NULL ||
                header.global_sequence != expected_sequence || expected_sequence == UINT64_MAX)
                status = LXP_ERR_LOG_CORRUPT;
            if (status == LXP_OK)
                status = lxp_programs_occupancy_receipt_decode(body + 13U,
                    header.body_length - 13U, &recorded);
            if (status == LXP_OK &&
                (recorded.global_sequence != expected_sequence ||
                 lxp_ct_memcmp(recorded.previous_state_root, process->kernel.current_state_root, 32U) != 0))
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            if (status == LXP_OK)
                status = lxp_hash_sha256(body + 13U, header.body_length - 13U, digest);
            if (status == LXP_OK)
                status = lxp_daemon_receipt_authority_lookup(&process->receipt_authority,
                    digest, &process->execution_arena, &authority);
            if (status == LXP_OK &&
                (authority.canonical_receipt.length != header.body_length - 13U ||
                 lxp_ct_memcmp(authority.canonical_receipt.bytes, body + 13U,
                     header.body_length - 13U) != 0))
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            if (status == LXP_OK)
                status = lxp_programs_finalize_occupancy_batch_selected(&process->kernel,
                    process->protocol_version, recorded.schedule_version,
                    recorded.batch_number, timestamp, recorded.global_sequence,
                    recorded.parameter_version, &process->execution_arena, &replayed, &encoded);
            if (status == LXP_OK &&
                (encoded.length != header.body_length - 13U ||
                 lxp_ct_memcmp(encoded.bytes, body + 13U, encoded.length) != 0))
                status = LXP_FATAL_REPLAY_DIVERGENCE;
            if (status == LXP_OK)
                status = persist_state_checkpoint(process, expected_sequence);
            if (status == LXP_OK) ++expected_sequence;
            (void)lxp_arena_reset(&process->execution_arena, maintenance_mark);
        } else if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
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
    if (status == LXP_OK)
        status = recover_ranged_batch_authorities(process);
    if (status == LXP_OK) status = resume_batch_number(process);
    return status;
}

typedef struct pay_publication_timing {
    uint64_t setup_us;
    uint64_t proofs_us;
    uint64_t authority_us;
    uint64_t evidence_us;
    uint64_t replica_us;
    uint64_t account_evidence_us;
    uint64_t visibility_us;
    uint64_t prune_us;
    lxp_result replica_result;
} pay_publication_timing;

typedef struct authority_replica_job {
    lxp_daemon_process *process;
    lxp_daemon_batch_wal_record *record;
    const lxp_byte_span *receipts;
    const lxp_merkle_proof *receipt_proofs;
    size_t activity_count;
    lxp_byte_span maintenance;
    const lxp_merkle_proof *maintenance_proof;
    lxp_byte_span canonical_header;
    uint8_t header_signature[64];
    uint64_t batch_number;
    lxp_result status;
    uint64_t started_us;
    uint64_t finished_us;
} authority_replica_job;

static void *authority_replica_worker(void *context)
{
    authority_replica_job *job = (authority_replica_job *)context;
    size_t i;
    job->started_us = pay_timing_us();
    job->status = LXP_OK;
    for (i = 0U; job->status == LXP_OK && i < job->activity_count; ++i)
        job->status = lxp_daemon_authority_replica_publish(
            job->process->authority_replica_address,
            job->process->authority_replica_port,
            job->process->authority_replica_token,
            job->process->authority_replica_token_length,
            job->process->authority_replica_id,
            job->receipts[i].bytes, job->receipts[i].length,
            job->canonical_header.bytes, job->canonical_header.length,
            job->header_signature, &job->receipt_proofs[i]);
    if (job->status == LXP_OK && job->maintenance.length != 0U)
        job->status = lxp_daemon_authority_replica_publish_maintenance(
            job->process->authority_replica_address,
            job->process->authority_replica_port,
            job->process->authority_replica_token,
            job->process->authority_replica_token_length,
            job->process->authority_replica_id,
            job->maintenance.bytes, job->maintenance.length,
            job->canonical_header.bytes, job->canonical_header.length,
            job->header_signature, job->maintenance_proof);
    job->finished_us = pay_timing_us();
    if (job->batch_number != 0U && getenv("LAYERX_PAY_TIMING") != NULL)
        (void)fprintf(stderr,
            "pay-native-replica batch=%llu replica_us=%llu result=%d\n",
            (unsigned long long)job->batch_number,
            (unsigned long long)(job->finished_us - job->started_us),
            (int)job->status);
    return NULL;
}

static lxp_result publish_canonical_batch(
    lxp_daemon_process *process, const lxp_byte_span *activities,
    const lxp_byte_span *receipts, const lxp_receipt *decoded_receipts,
    size_t activity_count, const lxp_byte_span *events, size_t event_count,
    uint16_t protocol_version, uint64_t timestamp,
    bool checkpoint_persisted, lxp_byte_span maintenance,
    const lxp_kernel *base_kernel, uint64_t batch_number,
    bool publish_replica, bool publish_visibility, bool advance_batch,
    pay_publication_timing *timing)
{
    lxp_batch_roots roots;
    lxp_batch_header header;
    lxp_batch_seal_input seal;
    lxp_byte_span canonical_header;
    lxp_byte_span projected_events[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    const lxp_byte_span *root_events = NULL;
    size_t root_event_count = 0U;
    uint8_t header_signature[64];
    uint8_t activity_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES][32];
    uint8_t receipt_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U][32];
    lxp_merkle_proof activity_proofs[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_merkle_proof receipt_proofs[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_byte_span combined[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U];
    lxp_programs_occupancy_receipt maintenance_record;
    size_t receipt_count = activity_count + (maintenance.length != 0U ? 1U : 0U);
    lxp_merkle_proof head_receipt_proof;
    authority_replica_job replica_job;
    pthread_t replica_thread;
    bool replica_started = false;
    uint64_t stage_started = pay_timing_us();
    size_t i;
    lxp_result status;
    if (timing != NULL) (void)memset(timing, 0, sizeof(*timing));
    if (process == NULL || activities == NULL || receipts == NULL ||
        decoded_receipts == NULL || activity_count == 0U ||
        activity_count > LXP_DAEMON_MAX_BATCH_ACTIVITIES || timestamp == 0U ||
        ((events == NULL) != (event_count == 0U)) ||
        (activity_count > 1U && events == NULL) ||
        (events != NULL && event_count != activity_count) ||
        decoded_receipts[0].global_sequence == 0U ||
        decoded_receipts[activity_count - 1U].global_sequence <
            decoded_receipts[0].global_sequence)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < activity_count; ++i) combined[i] = receipts[i];
    if (maintenance.length != 0U) {
        status = lxp_programs_occupancy_receipt_decode(maintenance.bytes, maintenance.length, &maintenance_record);
        if (status != LXP_OK) return status;
        if (!checkpoint_persisted || maintenance_record.batch_number != batch_number ||
            maintenance_record.global_sequence == 0U ||
            maintenance_record.global_sequence - 1U != decoded_receipts[activity_count - 1U].global_sequence ||
            lxp_ct_memcmp(maintenance_record.previous_state_root,
                decoded_receipts[activity_count - 1U].resulting_state_root, 32U) != 0)
            return LXP_ERR_CONTEXT_MISMATCH;
        combined[activity_count] = maintenance;
    }
    for (i = 0U; i < activity_count; ++i) {
        lxp_activity activity;
        uint8_t activity_id[32] = {0};
        if (activities[i].bytes == NULL || activities[i].length == 0U ||
            receipts[i].bytes == NULL || receipts[i].length == 0U ||
            decoded_receipts[i].protocol_version != protocol_version ||
            decoded_receipts[i].timestamp != timestamp ||
            decoded_receipts[0].global_sequence > UINT64_MAX - i ||
            decoded_receipts[i].global_sequence !=
                decoded_receipts[0].global_sequence + i ||
            (i != 0U &&
             lxp_ct_memcmp(decoded_receipts[i - 1U].resulting_state_root,
                           decoded_receipts[i].previous_state_root,
                           32U) != 0))
            return LXP_ERR_NON_CANONICAL;
        status = lxp_activity_decode(activities[i].bytes,
                                     activities[i].length, &activity);
        if (status == LXP_OK)
            status = lxp_activity_id(activities[i].bytes,
                                     activities[i].length, activity_id);
        if (status != LXP_OK ||
            activity.protocol_version != protocol_version ||
            lxp_ct_memcmp(activity_id,
                          decoded_receipts[i].activity_id, 32U) != 0)
            return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
        if (events != NULL) {
            status = lxp_programs_project_receipt_events(
                &decoded_receipts[i], &process->execution_arena,
                &projected_events[i]);
            if (status != LXP_OK ||
                projected_events[i].length != events[i].length ||
                lxp_ct_memcmp(projected_events[i].bytes, events[i].bytes,
                              events[i].length) != 0)
                return status != LXP_OK ? status : LXP_ERR_NON_CANONICAL;
        }
    }
    if (events != NULL) {
        root_events = projected_events;
        root_event_count = activity_count;
    }
    if (lxp_ct_memcmp(
            maintenance.length != 0U ? maintenance_record.resulting_state_root :
                decoded_receipts[activity_count - 1U].resulting_state_root,
            process->kernel.current_state_root, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    status = LXP_OK;
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){activities, activity_count,
                                     combined, receipt_count,
                                     root_events, root_event_count,
                                     NULL, 0U, NULL, 0U},
            &process->execution_arena, &roots);
    if (status == LXP_OK && checkpoint_persisted)
        status = lxp_batch_availability_root(&process->prepared_availability_body,
            &process->execution_arena, roots.data_availability_root);
    (void)memset(&seal, 0, sizeof(seal));
    seal.protocol_version = protocol_version;
    seal.network_id = process->network_id;
    seal.epoch = process->kernel.epoch;
    seal.batch_number = batch_number;
    seal.first_sequence = decoded_receipts[0].global_sequence;
    seal.last_sequence = maintenance.length != 0U ? maintenance_record.global_sequence :
        decoded_receipts[activity_count - 1U].global_sequence;
    (void)memcpy(seal.previous_state_root,
                 decoded_receipts[0].previous_state_root, 32U);
    (void)memcpy(seal.resulting_state_root,
                 maintenance.length != 0U ? maintenance_record.resulting_state_root :
                     decoded_receipts[activity_count - 1U].resulting_state_root, 32U);
    seal.timestamp_ms = timestamp;
    (void)memcpy(seal.sequencer_id,
                 process->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK && !checkpoint_persisted) {
        lxp_batch_header availability_header = {0};
        availability_header.protocol_version = seal.protocol_version;
        availability_header.network_id = seal.network_id;
        availability_header.epoch = seal.epoch;
        availability_header.batch_number = seal.batch_number;
        availability_header.first_sequence = seal.first_sequence;
        availability_header.last_sequence = seal.last_sequence;
        availability_header.timestamp_ms = seal.timestamp_ms;
        (void)memcpy(availability_header.previous_state_root, seal.previous_state_root, 32U);
        (void)memcpy(availability_header.resulting_state_root, seal.resulting_state_root, 32U);
        (void)memcpy(availability_header.sequencer_id, seal.sequencer_id, 32U);
        (void)memcpy(availability_header.activity_merkle_root, roots.activity_merkle_root, 32U);
        (void)memcpy(availability_header.receipt_merkle_root, roots.receipt_merkle_root, 32U);
        (void)memcpy(availability_header.event_merkle_root, roots.event_merkle_root, 32U);
        (void)memcpy(availability_header.oracle_root, roots.oracle_root, 32U);
        status = lxp_da_body_from_kernels(&availability_header, base_kernel,
            &process->kernel, activities, activity_count, combined, receipt_count,
            root_events, root_event_count, NULL, 0U, &process->execution_arena,
            &process->prepared_availability_body);
        if (status == LXP_OK)
            status = lxp_batch_availability_root(&process->prepared_availability_body,
                &process->execution_arena, roots.data_availability_root);
        if (status == LXP_OK)
            status = availability_store_body(process, &process->prepared_availability_body);
    }
    if (status == LXP_OK && !checkpoint_persisted)
        status = lxp_batch_sign(&process->prepared_availability_body.header,
            process->sequencer_private_key, &process->sequencer_authorization,
            process->prepared_availability_body.sequencer_signature,
            &process->execution_arena);
    if (status == LXP_OK && !checkpoint_persisted)
        status = lxp_da_log_store_body(&process->availability_log,
            &process->prepared_availability_body, &process->execution_arena);
    if (status == LXP_OK && !checkpoint_persisted)
        status = persist_state_checkpoint(process,
            decoded_receipts[activity_count - 1U].global_sequence);
    if (status == LXP_OK)
        status = lxp_batch_seal(&header, &seal, &roots, &process->batch_log,
                                &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_batch_sign(
            &header, process->sequencer_private_key,
            &process->sequencer_authorization, header_signature,
            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(
            &header, &process->execution_arena, &canonical_header);
    if (status == LXP_OK) {
        process->prepared_availability_body.header = header;
        (void)memcpy(process->prepared_availability_body.sequencer_signature,
                     header_signature, 64U);
        status = lxp_da_log_store_body(&process->availability_log,
            &process->prepared_availability_body, &process->execution_arena);
    }
    if (timing != NULL) timing->setup_us = pay_timing_us() - stage_started;
    stage_started = pay_timing_us();
    (void)memset(&head_receipt_proof, 0, sizeof(head_receipt_proof));
    for (i = 0U; status == LXP_OK && i < activity_count; ++i)
        status = lxp_merkle_leaf_hash(activities[i].bytes,
                                      activities[i].length,
                                      activity_hashes[i]);
    for (i = 0U; status == LXP_OK && i < receipt_count; ++i)
        status = lxp_merkle_leaf_hash(combined[i].bytes, combined[i].length,
                                      receipt_hashes[i]);
    for (i = 0U; status == LXP_OK && i < activity_count; ++i) {
        uint8_t proof_root[32];
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, receipt_count, i,
            &process->execution_arena, &receipt_proofs[i], proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root, roots.receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK)
            status = lxp_merkle_proof_generate(
                (const uint8_t (*)[32])activity_hashes, activity_count, i,
                &process->execution_arena, &activity_proofs[i], proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root, roots.activity_merkle_root, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK && i + 1U == activity_count)
            head_receipt_proof = receipt_proofs[i];
    }
    if (status == LXP_OK && maintenance.length != 0U) {
        uint8_t root[32];
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, receipt_count,
            activity_count, &process->execution_arena,
            &head_receipt_proof, root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(root, roots.receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    if (timing != NULL) timing->proofs_us = pay_timing_us() - stage_started;
    stage_started = pay_timing_us();
    for (i = 0U; status == LXP_OK && i < activity_count; ++i)
        if (status == LXP_OK)
            status = lxp_daemon_receipt_authority_append_artifacts(
                &process->receipt_authority,
                receipts[i].bytes, receipts[i].length,
                canonical_header.bytes, canonical_header.length,
                header_signature, &receipt_proofs[i],
                &process->execution_arena,
                decoded_receipts[i].program_outcome.terminal_payload,
                decoded_receipts[i].program_outcome.call_graph_payload);
    if (status == LXP_OK && maintenance.length != 0U) {
        status = lxp_daemon_receipt_authority_append_maintenance(
            &process->receipt_authority, maintenance.bytes, maintenance.length,
            canonical_header.bytes, canonical_header.length, header_signature,
            &head_receipt_proof, &process->execution_arena);
    }
    if (timing != NULL) timing->authority_us = pay_timing_us() - stage_started;
    (void)memset(&replica_job, 0, sizeof(replica_job));
    replica_job.process = process;
    replica_job.receipts = receipts;
    replica_job.receipt_proofs = receipt_proofs;
    replica_job.activity_count = activity_count;
    replica_job.maintenance = maintenance;
    replica_job.maintenance_proof = &head_receipt_proof;
    replica_job.canonical_header = canonical_header;
    (void)memcpy(replica_job.header_signature, header_signature,
                 sizeof(replica_job.header_signature));
    if (status == LXP_OK && publish_replica && pthread_create(
            &replica_thread, NULL, authority_replica_worker,
            &replica_job) == 0)
        replica_started = true;
    else if (status == LXP_OK && publish_replica)
        (void)authority_replica_worker(&replica_job);
    stage_started = pay_timing_us();
    for (i = 0U; status == LXP_OK && i < activity_count; ++i) {
        status = lxp_daemon_activity_evidence_publish(
            &process->evidence_store, activities[i], &activity_proofs[i],
            receipts[i], &receipt_proofs[i],
            &process->sequencer_authorization, canonical_header,
            header_signature, &process->execution_arena, NULL);
        if (status == LXP_OK)
            status = lxp_verified_receipt_index_add(
                &process->verified_receipts, &decoded_receipts[i],
                process->sequencer_authorization.public_key,
                &process->execution_arena);
    }
    if (timing != NULL) timing->evidence_us = pay_timing_us() - stage_started;
    stage_started = pay_timing_us();
    if (status == LXP_OK && maintenance.length != 0U &&
        process->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = lxp_daemon_account_evidence_publish_batch_maintenance(
            &process->evidence_store, &process->kernel, maintenance,
            &head_receipt_proof, &process->sequencer_authorization,
            canonical_header, header_signature, &process->execution_arena);
    if (status == LXP_OK && maintenance.length == 0U &&
        process->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = lxp_daemon_account_evidence_publish_batch(
            &process->evidence_store, &process->kernel,
            receipts[activity_count - 1U], &head_receipt_proof,
            &process->sequencer_authorization, canonical_header,
            header_signature, &process->execution_arena);
    if (timing != NULL)
        timing->account_evidence_us = pay_timing_us() - stage_started;
    if (replica_started && pthread_join(replica_thread, NULL) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    if (timing != NULL && publish_replica &&
        replica_job.finished_us >= replica_job.started_us)
        timing->replica_us = replica_job.finished_us - replica_job.started_us;
    if (timing != NULL)
        timing->replica_result = publish_replica ? replica_job.status : LXP_OK;
    if (status == LXP_OK && publish_replica && replica_job.status != LXP_OK)
        status = replica_job.status;
    stage_started = pay_timing_us();
    if (status == LXP_OK && publish_visibility) {
        if (pthread_mutex_lock(&process->owner.receipt_mutex) != 0)
            status = LXP_ERR_IO;
        else {
            process->owner.published_receipt_log = *process->owner.history->log;
            process->owner.published_receipt_log.capacity =
                process->owner.published_receipt_log.write_offset;
            process->owner.published_batch_number =
                process->receipt_authority.last_batch_number;
            (void)memcpy(process->owner.published_checkpoint_id,
                         process->evidence_store.latest_checkpoint_id, 32U);
            if (pthread_mutex_unlock(&process->owner.receipt_mutex) != 0)
                status = LXP_FATAL_INVARIANT;
        }
    }
    if (timing != NULL) timing->visibility_us = pay_timing_us() - stage_started;
    stage_started = pay_timing_us();
    if (status == LXP_OK)
        status = lxp_daemon_lni_receipts_committed();
    if (status == LXP_OK)
        status = availability_prune(process, header.batch_number);
    if (timing != NULL) timing->prune_us = pay_timing_us() - stage_started;
    if (status == LXP_OK) {
        process->owner.latest_sealed_timestamp = timestamp;
        if (advance_batch)
            process->next_batch = batch_number ==
                                      process->sequencer_authorization
                                          .last_batch_number ?
                                  0U : batch_number + 1U;
    }
    return status;
}

typedef struct postcommit_job {
    lxp_daemon_process *process;
    lxp_daemon_batch_wal_record *record;
    struct postcommit_job *next;
} postcommit_job;

static lxp_result install_pending_receipts(
    lxp_daemon_process *process,
    const lxp_daemon_batch_wal_input *view)
{
    lxp_daemon_pending_receipt pending[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    size_t index;
    lxp_result status = LXP_OK;
    if (process == NULL || view == NULL || view->count == 0U ||
        view->count > LXP_DAEMON_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(pending, 0, sizeof(pending));
    for (index = 0U; status == LXP_OK && index < view->count; ++index) {
        lxp_activity activity;
        lxp_receipt receipt;
        status = lxp_activity_decode(view->activities[index].bytes,
                                     view->activities[index].length,
                                     &activity);
        if (status == LXP_OK)
            status = lxp_activity_id(view->activities[index].bytes,
                                     view->activities[index].length,
                                     pending[index].activity_id);
        if (status == LXP_OK)
            status = lxp_receipt_decode(view->receipts[index].bytes,
                                        view->receipts[index].length,
                                        true, &receipt);
        if (status == LXP_OK &&
            (lxp_ct_memcmp(pending[index].activity_id,
                           receipt.activity_id, 32U) != 0 ||
             receipt.global_sequence != view->first_sequence + index))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK) {
            pending[index].bytes = view->receipts[index].bytes;
            pending[index].length = view->receipts[index].length;
            pending[index].global_sequence = receipt.global_sequence;
            (void)memcpy(pending[index].idempotency_key,
                         activity.idempotency_key, 32U);
        }
    }
    if (status != LXP_OK) return status;
    if (pthread_mutex_lock(&process->owner.receipt_mutex) != 0)
        return LXP_ERR_IO;
    if (process->owner.pending_receipt_count != 0U)
        status = LXP_FATAL_INVARIANT;
    else {
        (void)memcpy(process->owner.pending_receipts, pending,
                     view->count * sizeof(pending[0]));
        process->owner.pending_receipt_count = view->count;
    }
    if (pthread_mutex_unlock(&process->owner.receipt_mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result publish_receipt_visibility(lxp_daemon_process *process)
{
    lxp_result status = LXP_OK;
    if (pthread_mutex_lock(&process->owner.receipt_mutex) != 0)
        return LXP_ERR_IO;
    process->owner.published_receipt_log = *process->owner.history->log;
    process->owner.published_receipt_log.capacity =
        process->owner.published_receipt_log.write_offset;
    process->owner.published_batch_number =
        process->receipt_authority.last_batch_number;
    (void)memcpy(process->owner.published_checkpoint_id,
                 process->evidence_store.latest_checkpoint_id, 32U);
    (void)memset(process->owner.pending_receipts, 0,
                 sizeof(process->owner.pending_receipts));
    process->owner.pending_receipt_count = 0U;
    if (pthread_mutex_unlock(&process->owner.receipt_mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static void fail_postcommit(lxp_daemon_process *process, lxp_result status)
{
    if (status == LXP_OK) return;
    if (pthread_mutex_lock(&process->daemon.mutex) == 0) {
        process->daemon.accepting = false;
        process->daemon.failure = LXP_FATAL_INVARIANT;
        (void)pthread_cond_broadcast(&process->daemon.queue_changed);
        (void)pthread_mutex_unlock(&process->daemon.mutex);
    }
}

static lxp_result finish_async_replica(
    authority_replica_job **job, pthread_t *thread)
{
    authority_replica_job *completed;
    lxp_result status;
    if (job == NULL || thread == NULL) return LXP_ERR_NON_CANONICAL;
    completed = *job;
    if (completed == NULL) return LXP_OK;
    status = pthread_join(*thread, NULL) == 0 ?
        completed->status : LXP_ERR_IO;
    lxp_daemon_batch_wal_destroy(completed->record);
    free(completed);
    *job = NULL;
    return status;
}

static void run_postcommit(postcommit_job *job,
                           authority_replica_job **inflight_replica,
                           pthread_t *replica_thread)
{
    lxp_daemon_process *process = job->process;
    lxp_daemon_batch_wal_record *record = job->record;
    const lxp_daemon_batch_wal_input *view =
        lxp_daemon_batch_wal_view(record);
    lxp_activity *activities = NULL;
    lxp_receipt *receipts = NULL;
    pay_publication_timing publication = {0};
    authority_replica_job *replica = NULL;
    lxp_durability_group durability;
    lxp_kernel_batch_boundary live;
    lxp_batch_body body;
    uint64_t started_us = pay_timing_us();
    uint64_t observer_us = 0U, checkpoint_us = 0U;
    uint64_t publish_us = 0U, durability_us = 0U;
    uint64_t stage_started;
    size_t mark = 0U;
    size_t index;
    bool owner_locked = false;
    bool publication_locked = false;
    bool group_active = false;
    lxp_result status = view == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
    if (status == LXP_OK) {
        activities = (lxp_activity *)calloc(view->count, sizeof(*activities));
        receipts = (lxp_receipt *)calloc(view->count, sizeof(*receipts));
        if (activities == NULL || receipts == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK &&
        pthread_mutex_lock(&process->publication_mutex) != 0)
        status = LXP_ERR_IO;
    else if (status == LXP_OK)
        publication_locked = true;
    if (status == LXP_OK && pthread_mutex_lock(&process->owner.mutex) != 0)
        status = LXP_ERR_IO;
    else if (status == LXP_OK)
        owner_locked = true;
    if (pthread_mutex_lock(&process->postcommit_mutex) == 0) {
        if (view != NULL && view->batch_number >
                                process->postcommit_local_started_batch)
            process->postcommit_local_started_batch = view->batch_number;
        if (status != LXP_OK && process->postcommit_status == LXP_OK)
            process->postcommit_status = status;
        (void)pthread_cond_broadcast(&process->postcommit_changed);
        (void)pthread_mutex_unlock(&process->postcommit_mutex);
    } else if (status == LXP_OK) {
        status = LXP_ERR_IO;
    }
    if (status == LXP_OK) {
        mark = lxp_arena_mark(&process->execution_arena);
        status = lxp_durability_group_begin(&durability);
        group_active = status == LXP_OK;
    }
    for (index = 0U; status == LXP_OK && index < view->count; ++index) {
        status = lxp_activity_decode(view->activities[index].bytes,
                                     view->activities[index].length,
                                     &activities[index]);
        if (status == LXP_OK)
            status = lxp_receipt_decode(view->receipts[index].bytes,
                                        view->receipts[index].length,
                                        true, &receipts[index]);
    }
    stage_started = pay_timing_us();
    if (status == LXP_OK)
        status = view->maintenance.length != 0U ?
            lxp_kernel_finalize_batch_publication_maintenance(
                &process->kernel, activities, receipts, view->count,
                view->maintenance, &view->base, &view->settled, view->events,
                view->publication_digest) :
            lxp_kernel_finalize_batch_publication_records(
                &process->kernel, activities, receipts, view->count,
                &view->base, &view->settled, view->events, view->publication_digest);
    observer_us = pay_timing_us() - stage_started;
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_body(
            view, &process->execution_arena, &body);
    if (status == LXP_OK) {
        process->prepared_availability_body = body;
        status = availability_store_body(
            process, &process->prepared_availability_body);
    }
    stage_started = pay_timing_us();
    if (status == LXP_OK)
        status = persist_prepared_batch_checkpoint(
            process, &view->settled);
    checkpoint_us = pay_timing_us() - stage_started;
    stage_started = pay_timing_us();
    if (status == LXP_OK)
        status = publish_canonical_batch(
            process, view->activities, view->receipts, receipts,
            view->count, view->events, view->count,
            view->protocol_version, view->timestamp_ms, true,
            view->maintenance, NULL, view->batch_number,
            false, false, false, &publication);
    publish_us = pay_timing_us() - stage_started;
    if (status == LXP_OK)
        status = lxp_kernel_batch_boundary_read(&process->kernel, &live);
    if (owner_locked) {
        (void)lxp_arena_reset(&process->execution_arena, mark);
        if (pthread_mutex_unlock(&process->owner.mutex) != 0 &&
            status == LXP_OK)
            status = LXP_FATAL_INVARIANT;
        owner_locked = false;
    }
    stage_started = pay_timing_us();
    if (status == LXP_OK)
        status = lxp_durability_group_commit(&durability);
    else if (group_active)
        lxp_durability_group_abort(&durability);
    group_active = false;
    durability_us = pay_timing_us() - stage_started;
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_transition(
            process->checkpoint_directory, record, &live,
            LXP_DAEMON_BATCH_WAL_COMMITTED);
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_retire(
            process->checkpoint_directory, record, &live);
    if (status == LXP_OK)
        status = publish_receipt_visibility(process);
    if (group_active) lxp_durability_group_abort(&durability);
    if (owner_locked) {
        (void)lxp_arena_reset(&process->execution_arena, mark);
        if (pthread_mutex_unlock(&process->owner.mutex) != 0 &&
            status == LXP_OK)
            status = LXP_FATAL_INVARIANT;
    }
    if (publication_locked &&
        pthread_mutex_unlock(&process->publication_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = finish_async_replica(inflight_replica, replica_thread);
    if (status == LXP_OK) {
        replica = (authority_replica_job *)calloc(1U, sizeof(*replica));
        if (replica == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK) {
        replica->process = process;
        replica->record = record;
        replica->receipts = view->receipts;
        replica->receipt_proofs = view->receipt_proofs;
        replica->activity_count = view->count;
        replica->maintenance = view->maintenance;
        replica->maintenance_proof = &view->maintenance_proof;
        replica->canonical_header = view->canonical_header;
        replica->batch_number = view->batch_number;
        (void)memcpy(replica->header_signature,
                     view->header_signature, 64U);
        if (pthread_create(replica_thread, NULL,
                           authority_replica_worker, replica) == 0) {
            *inflight_replica = replica;
            record = NULL;
            replica = NULL;
        } else {
            (void)authority_replica_worker(replica);
            status = replica->status;
        }
    }
    if (pthread_mutex_lock(&process->postcommit_mutex) == 0) {
        if (status != LXP_OK && process->postcommit_status == LXP_OK)
            process->postcommit_status = status;
        (void)pthread_cond_broadcast(&process->postcommit_changed);
        (void)pthread_mutex_unlock(&process->postcommit_mutex);
    } else if (status == LXP_OK) {
        status = LXP_ERR_IO;
    }
    fail_postcommit(process, status);
    if (getenv("LAYERX_PAY_TIMING") != NULL)
        (void)fprintf(stderr, "pay-native-postcommit batch=%llu observer_us=%llu checkpoint_us=%llu publication_us=%llu publication_setup_us=%llu proof_us=%llu authority_us=%llu evidence_us=%llu account_evidence_us=%llu visibility_us=%llu prune_us=%llu durability_sync_us=%llu replica_us=%llu total_us=%llu result=%d\n",
            (unsigned long long)(view == NULL ? 0U : view->batch_number),
            (unsigned long long)observer_us,
            (unsigned long long)checkpoint_us,
            (unsigned long long)publish_us,
            (unsigned long long)publication.setup_us,
            (unsigned long long)publication.proofs_us,
            (unsigned long long)publication.authority_us,
            (unsigned long long)publication.evidence_us,
            (unsigned long long)publication.account_evidence_us,
            (unsigned long long)publication.visibility_us,
            (unsigned long long)publication.prune_us,
            (unsigned long long)durability_us,
            (unsigned long long)0U,
            (unsigned long long)(pay_timing_us() - started_us), (int)status);
    if (replica != NULL) {
        lxp_daemon_batch_wal_destroy(replica->record);
        free(replica);
        record = NULL;
    }
    lxp_daemon_batch_wal_destroy(record);
    free(activities);
    free(receipts);
    free(job);
}

static void *postcommit_worker(void *context)
{
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    authority_replica_job *inflight_replica = NULL;
    pthread_t replica_thread;
    for (;;) {
        postcommit_job *job = NULL;
        if (pthread_mutex_lock(&process->postcommit_mutex) != 0) break;
        while (process->postcommit_head == NULL &&
               !process->postcommit_stopping)
            if (pthread_cond_wait(&process->postcommit_changed,
                                  &process->postcommit_mutex) != 0) {
                process->postcommit_status = LXP_ERR_IO;
                process->postcommit_stopping = true;
                break;
            }
        if (process->postcommit_head != NULL) {
            job = process->postcommit_head;
            process->postcommit_head = job->next;
            if (process->postcommit_head == NULL)
                process->postcommit_tail = NULL;
        }
        if (job == NULL && process->postcommit_stopping) {
            (void)pthread_mutex_unlock(&process->postcommit_mutex);
            break;
        }
        if (pthread_mutex_unlock(&process->postcommit_mutex) != 0) break;
        if (job != NULL)
            run_postcommit(job, &inflight_replica, &replica_thread);
    }
    {
        lxp_result status = finish_async_replica(
            &inflight_replica, &replica_thread);
        if (status != LXP_OK) {
            if (pthread_mutex_lock(&process->postcommit_mutex) == 0) {
                if (process->postcommit_status == LXP_OK)
                    process->postcommit_status = status;
                (void)pthread_cond_broadcast(&process->postcommit_changed);
                (void)pthread_mutex_unlock(&process->postcommit_mutex);
            }
            fail_postcommit(process, status);
        }
    }
    return NULL;
}

static lxp_result postcommit_start(lxp_daemon_process *process)
{
    lxp_result status = LXP_OK;
    if (pthread_mutex_init(&process->publication_mutex, NULL) != 0)
        return LXP_ERR_IO;
    if (pthread_mutex_init(&process->postcommit_mutex, NULL) != 0) {
        (void)pthread_mutex_destroy(&process->publication_mutex);
        return LXP_ERR_IO;
    }
    if (pthread_cond_init(&process->postcommit_changed, NULL) != 0) {
        (void)pthread_mutex_destroy(&process->postcommit_mutex);
        (void)pthread_mutex_destroy(&process->publication_mutex);
        return LXP_ERR_IO;
    }
    process->postcommit_initialized = true;
    process->postcommit_status = LXP_OK;
    if (pthread_create(&process->postcommit_thread, NULL,
                       postcommit_worker, process) != 0)
        status = LXP_ERR_IO;
    else
        process->postcommit_started = true;
    if (status != LXP_OK) {
        (void)pthread_cond_destroy(&process->postcommit_changed);
        (void)pthread_mutex_destroy(&process->postcommit_mutex);
        (void)pthread_mutex_destroy(&process->publication_mutex);
        process->postcommit_initialized = false;
    }
    return status;
}

static lxp_result postcommit_enqueue(
    lxp_daemon_process *process, postcommit_job *job)
{
    lxp_result status = LXP_OK;
    if (process == NULL || job == NULL || !process->postcommit_started)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&process->postcommit_mutex) != 0)
        return LXP_ERR_IO;
    if (process->postcommit_stopping || process->postcommit_status != LXP_OK)
        status = process->postcommit_status == LXP_OK ?
            LXP_ERR_IO : process->postcommit_status;
    else {
        job->next = NULL;
        if (process->postcommit_tail == NULL)
            process->postcommit_head = job;
        else
            process->postcommit_tail->next = job;
        process->postcommit_tail = job;
        (void)pthread_cond_broadcast(&process->postcommit_changed);
    }
    if (pthread_mutex_unlock(&process->postcommit_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    return status;
}

static lxp_result postcommit_wait_started(
    lxp_daemon_process *process, uint64_t batch_number)
{
    lxp_result status = LXP_OK;
    if (pthread_mutex_lock(&process->postcommit_mutex) != 0)
        return LXP_ERR_IO;
    while (process->postcommit_local_started_batch < batch_number &&
           process->postcommit_status == LXP_OK)
        if (pthread_cond_wait(&process->postcommit_changed,
                              &process->postcommit_mutex) != 0) {
            status = LXP_ERR_IO;
            break;
        }
    if (status == LXP_OK) status = process->postcommit_status;
    if (pthread_mutex_unlock(&process->postcommit_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    return status;
}

static lxp_result postcommit_stop(lxp_daemon_process *process)
{
    lxp_result status = LXP_OK;
    if (!process->postcommit_initialized) return LXP_OK;
    if (pthread_mutex_lock(&process->postcommit_mutex) != 0)
        return LXP_ERR_IO;
    process->postcommit_stopping = true;
    (void)pthread_cond_broadcast(&process->postcommit_changed);
    if (pthread_mutex_unlock(&process->postcommit_mutex) != 0)
        status = LXP_ERR_IO;
    if (process->postcommit_started &&
        pthread_join(process->postcommit_thread, NULL) != 0)
        status = LXP_ERR_IO;
    process->postcommit_started = false;
    if (status == LXP_OK) status = process->postcommit_status;
    if (pthread_cond_destroy(&process->postcommit_changed) != 0)
        status = LXP_ERR_IO;
    if (pthread_mutex_destroy(&process->postcommit_mutex) != 0)
        status = LXP_ERR_IO;
    if (pthread_mutex_destroy(&process->publication_mutex) != 0)
        status = LXP_ERR_IO;
    process->postcommit_initialized = false;
    return status;
}

static lxp_result apply_canonical_activity(
    void *context, uint64_t global_sequence,
    const uint8_t *canonical_activity, size_t activity_length)
{
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    lxp_kernel base_kernel;
    lxp_state_store base_state;
    lx_account_registry *base_accounts = NULL;
    lxp_activity activity;
    lxp_identity *identity;
    uint8_t principal_id[32];
    lxp_u128 principal_balance = {0U, 0U};
    lxp_authority_grant grant;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_receipt receipt;
    lxp_byte_span canonical_receipt;
    lxp_byte_span canonical_events;
    lxp_byte_span activities[1];
    lxp_byte_span receipts[1];
    uint8_t activity_id[32];
    uint64_t timestamp;
    size_t mark;
    bool decoded = false;
    const char *stage = "authorization";
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
    if (status == LXP_OK &&
        activity.protocol_version != process->protocol_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK) decoded = true;
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(&activity, process->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK &&
        lxp_activity_module_id(activity.activity_type) != LXP_MODULE_PROGRAMS &&
        !asset_activity_supported(activity.activity_type) &&
        !lxp_governance_activity(activity.activity_type) &&
        !(process->custody_credit_enabled && activity.activity_type == LXP_BRIDGE_CREDIT))
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK) status = current_time_ms(&timestamp);
    if (status == LXP_OK)
        status = lxp_identity_resolve(&process->identities,
                                      activity.actor_did.bytes,
                                      activity.actor_did.length, &identity);
    if (status == LXP_OK) status = lxp_governance_identity_refresh(&process->kernel, identity);
    if (status == LXP_OK && activity.authority.length != 32U)
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_OK)
        status = lxp_authority_resolve_activity(
            &process->kernel, identity, &activity,
            lxp_identity_key_valid(identity, activity.authority.bytes,
                                   timestamp, global_sequence),
            true, timestamp, UINT64_C(300000), global_sequence, &grant,
            &authority);
    if (status == LXP_OK)
        status = principal_authority(process, &activity,
            grant.kind == LXP_AUTHORITY_OWNER ? grant.key :
                identity->primary_key, principal_id, &principal_balance);
    if (status == LXP_OK)
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
    if (status != LXP_OK) {
        lxp_result refusal = status;
        if (!decoded || !lxp_terminal_rejection_applies(refusal) ||
            !terminal_rejection_module_supported(process,
                                                 activity.activity_type)) {
            status = refusal;
            goto finish;
        }
        stage = "terminal rejection";
        status = lxp_activity_id(canonical_activity, activity_length,
                                 activity_id);
        if (status == LXP_OK) status = current_time_ms(&timestamp);
        if (status == LXP_OK) {
            (void)memset(&execution, 0, sizeof(execution));
            status = lxp_batch_identity_activity(
                process->kernel.current_state_root, activity_id,
                global_sequence, process->next_batch, execution.batch_id);
        }
        if (status == LXP_OK) {
            base_accounts = malloc(sizeof(*base_accounts));
            if (base_accounts == NULL) status = LXP_ERR_IO;
            else {
                (void)memset(base_accounts, 0, sizeof(*base_accounts));
                status = lx_account_registry_copy(&process->accounts,
                                                  base_accounts);
            }
        }
        if (status != LXP_OK) {
            status = refusal;
            goto finish;
        }
        execution.network_id = process->network_id;
        execution.batch_number = process->next_batch;
        execution.batch_timestamp_ms = timestamp;
        execution.maximum_timestamp_window = UINT64_C(300000);
        execution.epoch = process->kernel.epoch;
        execution.global_sequence = global_sequence;
        execution.recorded_module_version =
            recorded_module_version_for(activity.activity_type);
        execution.recorded_fee_schedule_version = 0U;
        execution.parameter_version = process->parameter_version;
        execution.signature_valid = true;
        execution.identities = &process->identities;
        execution.fee_parameters = &process->fees;
        execution.gas_limit = UINT64_MAX;
        execution.arena = &process->execution_arena;
        execution.sequencer_private_key = process->sequencer_private_key;
        execution.verified_receipts = &process->verified_receipts;
        base_state = process->state;
        base_state.accounts = base_accounts;
        base_kernel = process->kernel;
        base_kernel.state = &base_state;
        (void)memset(&receipt, 0, sizeof(receipt));
        status = lxp_kernel_terminal_rejection(&process->kernel, &activity,
                                               &execution, refusal, &receipt);
        if (status != LXP_OK) {
            (void)fprintf(stderr,
                "layerxd: terminal rejection unavailable at sequence %llu for result %d with result %d\n",
                (unsigned long long)global_sequence, (int)refusal,
                (int)status);
            status = refusal;
            goto finish;
        }
        stage = "receipt encoding";
        status = lxp_receipt_encode(&receipt, true,
                                    &process->execution_arena,
                                    &canonical_receipt);
        if (status == LXP_OK) {
            stage = "event projection";
            status = lxp_programs_project_receipt_events(
                &receipt, &process->execution_arena, &canonical_events);
        }
        if (status != LXP_OK) goto finish;
        goto publish;
    }
    (void)memcpy(authority.principal, principal_id, 32U);
    (void)memset(&execution, 0, sizeof(execution));
    status = lxp_batch_identity_activity(
        process->kernel.current_state_root, activity_id, global_sequence,
        process->next_batch, execution.batch_id);
    if (status != LXP_OK) goto finish;
    execution.network_id = process->network_id;
    execution.batch_number = process->next_batch;
    execution.batch_timestamp_ms = timestamp;
    execution.maximum_timestamp_window = UINT64_C(300000);
    execution.epoch = process->kernel.epoch;
    execution.global_sequence = global_sequence;
    execution.recorded_module_version = (activity.activity_type == LXP_BRIDGE_CREDIT ||
        lxp_governance_activity(activity.activity_type)) ?
        1U : (asset_activity_supported(activity.activity_type) ?
        lx_asset_module_iface()->abi_version : LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION);
    execution.recorded_fee_schedule_version = 0U;
    execution.parameter_version = process->parameter_version;
    execution.signature_valid = true;
    execution.identities = &process->identities;
    execution.authority = &authority;
    execution.fee_parameters = &process->fees;
    execution.fee_balance = principal_balance;
    execution.gas_limit = UINT64_MAX;
    execution.arena = &process->execution_arena;
    execution.sequencer_private_key = process->sequencer_private_key;
    execution.verified_receipts = &process->verified_receipts;
    (void)memset(&receipt, 0, sizeof(receipt));
    base_accounts = malloc(sizeof(*base_accounts));
    if (base_accounts == NULL) { status = LXP_ERR_IO; goto finish; }
    (void)memset(base_accounts, 0, sizeof(*base_accounts));
    status = lx_account_registry_copy(&process->accounts, base_accounts);
    if (status != LXP_OK) goto finish;
    base_state = process->state;
    base_state.accounts = base_accounts;
    base_kernel = process->kernel;
    base_kernel.state = &base_state;
    stage = "kernel execution";
    status = lxp_kernel_execute_activity(&process->kernel, &activity,
                                         &execution, &receipt);
    if (status != LXP_OK || process->kernel.publication_poisoned) {
        if (status == LXP_OK) status = LXP_FATAL_INVARIANT;
        goto finish;
    }
    stage = "receipt encoding";
    status = lxp_receipt_encode(&receipt, true, &process->execution_arena,
                                &canonical_receipt);
    if (status != LXP_OK) goto finish;
    stage = "event projection";
    status = lxp_programs_project_receipt_events(
        &receipt, &process->execution_arena, &canonical_events);
    if (status != LXP_OK) goto finish;
publish:
    activities[0] = (lxp_byte_span){canonical_activity, activity_length};
    receipts[0] = canonical_receipt;
    stage = "batch publication";
    status = publish_canonical_batch(
        process, activities, receipts, &receipt, 1U,
        &canonical_events, 1U,
        activity.protocol_version, timestamp, false, (lxp_byte_span){NULL, 0U},
        &base_kernel, process->next_batch, true, true, true, NULL);
    if (status == LXP_OK && process->next_batch == 0U) {
        if (pthread_mutex_lock(&process->daemon.mutex) != 0)
            status = LXP_FATAL_INVARIANT;
        else {
            process->daemon.accepting = false;
            process->daemon.failure = LXP_ERR_AUTH_SCOPE;
            (void)pthread_cond_broadcast(&process->daemon.queue_changed);
            if (pthread_mutex_unlock(&process->daemon.mutex) != 0)
                status = LXP_FATAL_INVARIANT;
        }
    }
finish:
    lx_account_registry_release(base_accounts);
    free(base_accounts);
    if (status != LXP_OK)
        (void)fprintf(stderr, "layerxd: activity %s failed with result %d\n", stage, (int)status);
    (void)lxp_arena_reset(&process->execution_arena, mark);
    if (pthread_mutex_unlock(&process->owner.mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result commit_prepared_batch_wal(
    lxp_daemon_process *process, const lxp_activity *decoded_activities,
    const lxp_byte_span *activities, const lxp_byte_span *receipts,
    const lxp_byte_span *events, const lxp_receipt *decoded_receipts,
    size_t count, uint64_t timestamp,
    lxp_kernel_prepared_batch *owned_prepared,
    bool deferred,
    lxp_daemon_batch_wal_record **record,
    uint64_t *wal_prepare_us, uint64_t *wal_commit_us,
    uint64_t *availability_us)
{
    lxp_batch_roots roots;
    lxp_batch_header header;
    lxp_byte_span canonical_header;
    lxp_merkle_proof proofs[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U];
    uint8_t receipt_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U][32];
    lxp_byte_span combined[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U];
    lxp_byte_span maintenance = lxp_kernel_prepared_batch_maintenance(owned_prepared);
    size_t receipt_count = count + (maintenance.length != 0U ? 1U : 0U);
    uint8_t proof_root[32];
    uint8_t signature[64];
    lxp_daemon_batch_wal_input input;
    lxp_byte_span terminal_payloads[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_byte_span call_graphs[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    const lxp_kernel_batch_boundary *base;
    const lxp_kernel_batch_boundary *settled;
    availability_store_job availability_job;
    pthread_t availability_thread;
    uint64_t started_us;
    bool availability_started = false;
    lxp_daemon_batch_wal_timing deferred_timing = {0};
    size_t i;
    lxp_result status;
    if (record == NULL || wal_prepare_us == NULL || wal_commit_us == NULL ||
        availability_us == NULL)
        return LXP_ERR_NON_CANONICAL;
    *record = NULL;
    *wal_prepare_us = 0U;
    *wal_commit_us = 0U;
    *availability_us = 0U;
    started_us = pay_timing_us();
    if (count == 0U || count > LXP_DAEMON_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < count; ++i) combined[i] = receipts[i];
    if (maintenance.length != 0U) combined[count] = maintenance;
    status = lxp_batch_roots_compute(
        &(lxp_batch_root_inputs){activities, count, combined, receipt_count,
                                 events, count, NULL, 0U, NULL, 0U},
        &process->execution_arena, &roots);
    for (i = 0U; status == LXP_OK && i < receipt_count; ++i)
        status = lxp_merkle_leaf_hash(combined[i].bytes, combined[i].length,
                                      receipt_hashes[i]);
    for (i = 0U; status == LXP_OK && i < receipt_count; ++i) {
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, receipt_count, i,
            &process->execution_arena, &proofs[i], proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root, roots.receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    (void)memset(&header, 0, sizeof(header));
    header.protocol_version = decoded_activities[0].protocol_version;
    header.network_id = process->network_id;
    header.epoch = process->kernel.epoch;
    header.batch_number = process->next_batch;
    header.first_sequence = decoded_receipts[0].global_sequence;
    header.last_sequence = lxp_kernel_prepared_batch_final_boundary(owned_prepared)->next_sequence - 1U;
    (void)memcpy(header.previous_state_root,
                 decoded_receipts[0].previous_state_root, 32U);
    (void)memcpy(header.resulting_state_root,
                 lxp_kernel_prepared_batch_final_root(owned_prepared), 32U);
    (void)memcpy(header.activity_merkle_root,
                 roots.activity_merkle_root, 32U);
    (void)memcpy(header.receipt_merkle_root,
                 roots.receipt_merkle_root, 32U);
    (void)memcpy(header.event_merkle_root, roots.event_merkle_root, 32U);
    (void)memcpy(header.oracle_root, roots.oracle_root, 32U);
    (void)memcpy(header.data_availability_root,
                 roots.data_availability_root, 32U);
    header.timestamp_ms = timestamp;
    (void)memcpy(header.sequencer_id,
                 process->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = lxp_da_body_from_kernels(
            &header, lxp_kernel_prepared_batch_base_kernel(owned_prepared),
            lxp_kernel_prepared_batch_settled_kernel(owned_prepared),
            activities, count, combined, receipt_count, events, count,
            NULL, 0U, &process->execution_arena,
            &process->prepared_availability_body);
    if (status == LXP_OK)
        header = process->prepared_availability_body.header;
    if (status == LXP_OK)
        status = lxp_batch_sign(
            &header, process->sequencer_private_key,
            &process->sequencer_authorization, signature,
            &process->execution_arena);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(
            &header, &process->execution_arena, &canonical_header);
    base = lxp_kernel_prepared_batch_base_boundary(owned_prepared);
    settled = lxp_kernel_prepared_batch_final_boundary(owned_prepared);
    if (status == LXP_OK && (base == NULL || settled == NULL))
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    (void)memset(&input, 0, sizeof(input));
    input.protocol_version = header.protocol_version;
    input.network_id = header.network_id;
    input.epoch = header.epoch;
    input.batch_number = header.batch_number;
    input.timestamp_ms = header.timestamp_ms;
    input.parameter_version = decoded_receipts[0].parameter_version;
    input.fee_schedule_version = lxp_kernel_prepared_batch_fee_schedule_version(owned_prepared);
    input.metering_schedule_version = lxp_kernel_prepared_batch_metering_schedule_version(owned_prepared);
    input.first_sequence = header.first_sequence;
    input.last_sequence = header.last_sequence;
    input.count = count;
    input.base = *base;
    input.settled = *settled;
    (void)memcpy(input.publication_digest,
                 lxp_kernel_prepared_batch_publication_digest(owned_prepared),
                 32U);
    input.authorization = process->sequencer_authorization;
    input.canonical_header = canonical_header;
    (void)memcpy(input.header_signature, signature, 64U);
    input.activities = activities;
    input.receipts = receipts;
    input.events = events;
    for (i = 0U; i < count; ++i) {
        terminal_payloads[i] = decoded_receipts[i].program_outcome.terminal_payload;
        call_graphs[i] = decoded_receipts[i].program_outcome.call_graph_payload;
    }
    input.terminal_payloads = terminal_payloads;
    input.call_graphs = call_graphs;
    input.receipt_proofs = proofs;
    input.maintenance = maintenance;
    input.state_diff = process->prepared_availability_body.state_diff;
    input.recovery_metadata = process->prepared_availability_body.recovery_metadata;
    if (maintenance.length != 0U) input.maintenance_proof = proofs[count];
    process->prepared_availability_body.header = header;
    (void)memcpy(process->prepared_availability_body.sequencer_signature,
                 signature, sizeof(signature));
    availability_job = (availability_store_job){
        process, &process->prepared_availability_body, LXP_OK, 0U, 0U};
    *wal_prepare_us = pay_timing_us() - started_us;
    if (!deferred && pthread_create(&availability_thread, NULL,
                       availability_store_worker, &availability_job) == 0)
        availability_started = true;
    else if (!deferred)
        (void)availability_store_worker(&availability_job);
    started_us = pay_timing_us();
    if (deferred)
        status = lxp_daemon_batch_wal_commit_kernel_deferred(
            process->checkpoint_directory, &input,
            &process->kernel, &process->identities,
            owned_prepared, record, &deferred_timing);
    else if (availability_started || availability_job.status == LXP_OK)
        status = lxp_daemon_batch_wal_commit_kernel(
            process->checkpoint_directory, &input,
            &process->kernel, &process->identities,
            decoded_activities, owned_prepared,
            persist_prepared_batch_checkpoint, process, record);
    else
        status = availability_job.status;
    *wal_commit_us = pay_timing_us() - started_us;
    if (availability_started && pthread_join(availability_thread, NULL) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    if (!deferred &&
        availability_job.finished_us >= availability_job.started_us)
        *availability_us = availability_job.finished_us -
                           availability_job.started_us;
    if (!deferred && status == LXP_OK && availability_job.status != LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (deferred && getenv("LAYERX_PAY_TIMING") != NULL)
        (void)fprintf(stderr, "pay-native-wal batch=%llu pre_sync_us=%llu sync_us=%llu verify_us=%llu kernel_commit_us=%llu checkpoint_us=0 observer_us=0 total_us=%llu result=%d\n",
            (unsigned long long)input.batch_number,
            (unsigned long long)deferred_timing.pre_sync_us,
            (unsigned long long)deferred_timing.sync_us,
            (unsigned long long)deferred_timing.verify_us,
            (unsigned long long)deferred_timing.kernel_commit_us,
            (unsigned long long)deferred_timing.total_us, (int)status);
    return status;
}

static lxp_result apply_canonical_batch(
    void *context, uint64_t first_global_sequence,
    const lxp_daemon_activity *offered, size_t offered_count,
    size_t *consumed_count)
{
    uint64_t started_us = pay_timing_us(), prepared_us = 0U;
    uint64_t committed_us = 0U, published_us = 0U, finalized_us = 0U;
    uint64_t wal_prepare_us = 0U, wal_commit_us = 0U;
    uint64_t availability_us = 0U;
    pay_publication_timing publication_timing = {0};
    lxp_daemon_process *process = (lxp_daemon_process *)context;
    lxp_activity activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_kernel_execution executions[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_authority_grant grants[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_authority_resolved authorities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_byte_span canonical_activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_byte_span canonical_receipts[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_batch_roots scheduling_roots;
    uint8_t batch_id[32] = {0};
    uint64_t timestamp;
    size_t count = 0U;
    size_t retry_prefix_count = 0U;
    size_t kernel_consumed = 0U;
    size_t checked_count = 0U;
    bool timestamped = false;
    size_t mark;
    size_t i;
    uint32_t maximum_workers;
    lxp_kernel_prepared_batch *prepared_batch = NULL;
    lxp_daemon_batch_wal_record *wal_record = NULL;
    postcommit_job *post_job = NULL;
    const lxp_receipt *prepared_receipts = NULL;
    const lxp_byte_span *prepared_events = NULL;
    uint64_t handed_off_batch = 0U;
    bool deferred = false;
    bool wait_postcommit_start = false;
    bool publication_locked = false;
    bool live_committed = false;
    lxp_kernel_batch_boundary live_boundary;
    lxp_result status = LXP_OK;
    if (consumed_count == NULL) return LXP_ERR_NON_CANONICAL;
    *consumed_count = 0U;
    if (process != NULL) {
        if (pthread_mutex_lock(&process->owner.mutex) != 0)
            return LXP_ERR_IO;
        if (pthread_mutex_unlock(&process->owner.mutex) != 0)
            return LXP_ERR_IO;
    }
    if (process == NULL || offered == NULL || offered_count == 0U ||
        offered_count > LXP_DAEMON_MAX_BATCH_ACTIVITIES ||
        first_global_sequence != process->state.next_sequence ||
        process->kernel.publication_poisoned || process->next_batch == 0U ||
        process->next_batch <
            process->sequencer_authorization.first_batch_number ||
        process->next_batch >
            process->sequencer_authorization.last_batch_number)
        return LXP_ERR_SEQUENCE_GAP;
    while (count < offered_count) {
        status = lxp_activity_decode(offered[count].bytes,
                                     offered[count].length,
                                     &activities[count]);
        if (status == LXP_OK &&
            activities[count].protocol_version != process->protocol_version)
            status = LXP_ERR_VERSION_UNSUPPORTED;
        if (status != LXP_OK) {
            if (count == 0U) return status;
            status = LXP_OK;
            break;
        }
        if (activities[count].activity_type != LX_PROGRAMS_CALL) {
            if (count == 0U && lxp_protocol_version_uses_occupancy(process->protocol_version))
                count = 1U;
            break;
        }
        if (count != 0U && activities[count].protocol_version !=
                               activities[0].protocol_version)
            break;
        ++count;
    }
    if (count == 0U) {
        status = apply_canonical_activity(
            context, first_global_sequence,
            offered[0].bytes, offered[0].length);
        if (status == LXP_OK) *consumed_count = 1U;
        return status;
    }
    deferred = activities[0].activity_type != LX_PROGRAMS_CALL;
    if (first_global_sequence > UINT64_MAX - (count - 1U))
        return LXP_ERR_OVERFLOW;
    for (i = 0U; i < count; ++i)
        canonical_activities[i] =
            (lxp_byte_span){offered[i].bytes, offered[i].length};
    if (pthread_mutex_lock(&process->owner.mutex) != 0)
        return LXP_ERR_IO;
    process->state.writer = pthread_self();
    mark = lxp_arena_mark(&process->execution_arena);
    status = current_time_ms(&timestamp);
    if (status == LXP_OK) timestamped = true;
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_identity *identity;
        uint8_t principal_id[32];
        lxp_u128 principal_balance = {0U, 0U};
        uint64_t sequence = first_global_sequence + i;
        status = lxp_activity_check_envelope(&activities[i],
                                             process->network_id);
        if (status == LXP_OK)
            status = lxp_activity_verify_payload_hash(&activities[i]);
        if (status == LXP_OK)
            status = lxp_activity_verify_signature(&activities[i]);
        if (status == LXP_OK)
            status = lxp_identity_resolve(
                &process->identities, activities[i].actor_did.bytes,
                activities[i].actor_did.length, &identity);
        if (status == LXP_OK) status = lxp_governance_identity_refresh(&process->kernel, identity);
        if (status == LXP_OK && activities[i].authority.length != 32U)
            status = LXP_ERR_BAD_SIGNATURE;
        if (status == LXP_OK &&
            lxp_activity_module_id(activities[i].activity_type) != LXP_MODULE_PROGRAMS &&
            !asset_activity_supported(activities[i].activity_type) &&
            !lxp_governance_activity(activities[i].activity_type) &&
            !(process->custody_credit_enabled && activities[i].activity_type == LXP_BRIDGE_CREDIT))
            status = LXP_ERR_UNKNOWN_ACTIVITY;
        (void)memset(&grants[i], 0, sizeof(grants[i]));
        (void)memset(&authorities[i], 0, sizeof(authorities[i]));
        if (status == LXP_OK)
            status = lxp_authority_resolve_activity(
                &process->kernel, identity, &activities[i],
                lxp_identity_key_valid(identity,
                                       activities[i].authority.bytes,
                                       timestamp, sequence),
                true, timestamp, UINT64_C(300000), sequence, &grants[i],
                &authorities[i]);
        if (status == LXP_OK)
            status = principal_authority(process, &activities[i],
                grants[i].kind == LXP_AUTHORITY_OWNER ? grants[i].key :
                    identity->primary_key, principal_id, &principal_balance);
        if (status == LXP_OK)
            (void)memcpy(authorities[i].principal, principal_id, 32U);
        (void)memset(&executions[i], 0, sizeof(executions[i]));
        executions[i].network_id = process->network_id;
        executions[i].batch_number = process->next_batch;
        executions[i].batch_timestamp_ms = timestamp;
        executions[i].maximum_timestamp_window = UINT64_C(300000);
        executions[i].epoch = process->kernel.epoch;
        executions[i].global_sequence = sequence;
        executions[i].recorded_module_version = (activities[i].activity_type == LXP_BRIDGE_CREDIT ||
            lxp_governance_activity(activities[i].activity_type)) ?
            1U : (asset_activity_supported(activities[i].activity_type) ?
            lx_asset_module_iface()->abi_version : LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION);
        executions[i].parameter_version = process->parameter_version;
        executions[i].signature_valid = true;
        executions[i].identities = &process->identities;
        executions[i].authority = &authorities[i];
        executions[i].fee_parameters = &process->fees;
        executions[i].fee_balance = principal_balance;
        executions[i].gas_limit = UINT64_MAX;
        executions[i].arena = &process->execution_arena;
        executions[i].sequencer_private_key =
            process->sequencer_private_key;
        executions[i].verified_receipts = &process->verified_receipts;
        ++checked_count;
    }
    if (status == LXP_OK)
        status = lxp_daemon_batch_bind_prefix(
            canonical_activities, count,
            process->kernel.current_state_root,
            first_global_sequence, process->next_batch,
            &process->execution_arena, executions,
            &scheduling_roots, batch_id);
    maximum_workers = process->daemon.config.serial_execution ? 1U :
        (uint32_t)process->daemon.config.verify_workers;
    if (maximum_workers == 0U) maximum_workers = 1U;
    while (status == LXP_OK) {
        retry_prefix_count = 0U;
        if (activities[0].activity_type == LX_PROGRAMS_CALL)
            status = lxp_kernel_prepare_activity_batch(
                &process->kernel, activities, executions, count,
                maximum_workers, &prepared_batch, &retry_prefix_count);
        else
            status = lxp_kernel_prepare_serial_activity_batch(
                &process->kernel, &activities[0], &executions[0], &prepared_batch);
        if (status == LXP_OK) break;
        if (prepared_batch != NULL || retry_prefix_count == 0U)
            break;
        if (retry_prefix_count >= count) {
            status = LXP_FATAL_INVARIANT;
            break;
        }
        count = retry_prefix_count;
        status = lxp_daemon_batch_bind_prefix(
            canonical_activities, count,
            process->kernel.current_state_root,
            first_global_sequence, process->next_batch,
            &process->execution_arena, executions,
            &scheduling_roots, batch_id);
    }
    if (status != LXP_OK && prepared_batch == NULL && timestamped &&
        (checked_count == 0U || (checked_count == count && count == 1U)) &&
        lxp_terminal_rejection_applies(status) &&
        terminal_rejection_module_supported(process,
                                            activities[0].activity_type)) {
        lxp_result refusal = status;
        count = 1U;
        status = lxp_daemon_batch_bind_prefix(
            canonical_activities, count,
            process->kernel.current_state_root,
            first_global_sequence, process->next_batch,
            &process->execution_arena, executions,
            &scheduling_roots, batch_id);
        if (status == LXP_OK)
            status = lxp_kernel_prepare_terminal_rejection(
                &process->kernel, &activities[0], &executions[0], refusal,
                &prepared_batch);
        if (status != LXP_OK) {
            (void)fprintf(stderr,
                "layerxd: terminal rejection unavailable at sequence %llu for result %d with result %d\n",
                (unsigned long long)first_global_sequence, (int)refusal,
                (int)status);
            status = refusal;
        }
    }
    if (status == LXP_OK && lxp_protocol_version_uses_occupancy(process->protocol_version))
        status = lxp_kernel_prepare_batch_maintenance(prepared_batch, activities, executions);
    if (status == LXP_OK && lxp_protocol_version_uses_occupancy(process->protocol_version))
        status = lxp_daemon_reserve_batch_maintenance(&process->daemon, count);
    if (status == LXP_OK) {
        kernel_consumed = lxp_kernel_prepared_batch_count(prepared_batch);
        prepared_receipts =
            lxp_kernel_prepared_batch_receipts(prepared_batch);
        prepared_events = lxp_kernel_prepared_batch_events(prepared_batch);
    }
    if (status == LXP_OK &&
        (kernel_consumed != count || prepared_receipts == NULL ||
         prepared_events == NULL))
        status = LXP_FATAL_INVARIANT;
    for (i = 0U; status == LXP_OK && i < count; ++i)
        status = lxp_receipt_encode(
            &prepared_receipts[i], true, &process->execution_arena,
            &canonical_receipts[i]);
    prepared_us = pay_timing_us();
    if (status == LXP_OK) {
        if (pthread_mutex_lock(&process->publication_mutex) != 0)
            status = LXP_ERR_IO;
        else
            publication_locked = true;
    }
    if (status == LXP_OK)
        status = commit_prepared_batch_wal(
            process, activities, canonical_activities, canonical_receipts,
            prepared_events, prepared_receipts, count, timestamp,
            prepared_batch, deferred, &wal_record,
            &wal_prepare_us, &wal_commit_us,
            &availability_us);
    committed_us = pay_timing_us();
    if (status == LXP_OK) live_committed = true;
    if (status == LXP_OK && deferred)
        status = install_pending_receipts(
            process, lxp_daemon_batch_wal_view(wal_record));
    if (status == LXP_OK && deferred) {
        handed_off_batch = process->next_batch;
        post_job = (postcommit_job *)calloc(1U, sizeof(*post_job));
        if (post_job == NULL)
            status = LXP_ERR_IO;
        else {
            post_job->process = process;
            post_job->record = wal_record;
            process->next_batch = handed_off_batch ==
                    process->sequencer_authorization.last_batch_number ?
                0U : handed_off_batch + 1U;
            status = postcommit_enqueue(process, post_job);
            if (status == LXP_OK) {
                wal_record = NULL;
                post_job = NULL;
                wait_postcommit_start = true;
            }
        }
    }
    if (status == LXP_OK && !deferred)
        status = publish_canonical_batch(
            process, canonical_activities, canonical_receipts,
            prepared_receipts, count, prepared_events, count,
            activities[0].protocol_version, timestamp, true,
            lxp_kernel_prepared_batch_maintenance(prepared_batch), NULL,
            process->next_batch, true, true, true, &publication_timing);
    published_us = pay_timing_us();
    if (status == LXP_OK && !deferred)
        status = lxp_kernel_batch_boundary_read(
            &process->kernel, &live_boundary);
    if (status == LXP_OK && !deferred)
        status = lxp_daemon_batch_wal_transition(
            process->checkpoint_directory, wal_record,
            &live_boundary,
            LXP_DAEMON_BATCH_WAL_COMMITTED);
    if (status == LXP_OK && !deferred)
        status = lxp_daemon_batch_wal_retire(
            process->checkpoint_directory, wal_record, &live_boundary);
    finalized_us = pay_timing_us();
    if (status == LXP_OK && process->next_batch == 0U) {
        if (pthread_mutex_lock(&process->daemon.mutex) != 0)
            status = LXP_FATAL_INVARIANT;
        else {
            process->daemon.accepting = false;
            process->daemon.failure = LXP_ERR_AUTH_SCOPE;
            (void)pthread_cond_broadcast(&process->daemon.queue_changed);
            if (pthread_mutex_unlock(&process->daemon.mutex) != 0)
                status = LXP_FATAL_INVARIANT;
        }
    }
    if (live_committed && status != LXP_OK) status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) *consumed_count = count;
    free(post_job);
    lxp_daemon_batch_wal_destroy(wal_record);
    lxp_kernel_prepared_batch_destroy(prepared_batch);
    (void)lxp_arena_reset(&process->execution_arena, mark);
    if (pthread_mutex_unlock(&process->owner.mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (publication_locked &&
        pthread_mutex_unlock(&process->publication_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (wait_postcommit_start && status == LXP_OK)
        status = postcommit_wait_started(process, handed_off_batch);
    if (getenv("LAYERX_PAY_TIMING") != NULL)
        (void)fprintf(stderr, "pay-native sequence=%llu prepare_us=%llu wal_prepare_us=%llu wal_commit_us=%llu availability_us=%llu publication_us=%llu publication_setup_us=%llu proof_us=%llu authority_us=%llu evidence_us=%llu replica_us=%llu replica_result=%d account_evidence_us=%llu visibility_us=%llu prune_us=%llu wal_finalize_us=%llu total_us=%llu result=%d\n",
            (unsigned long long)first_global_sequence,
            (unsigned long long)(prepared_us - started_us),
            (unsigned long long)wal_prepare_us,
            (unsigned long long)wal_commit_us,
            (unsigned long long)availability_us,
            (unsigned long long)(published_us - committed_us),
            (unsigned long long)publication_timing.setup_us,
            (unsigned long long)publication_timing.proofs_us,
            (unsigned long long)publication_timing.authority_us,
            (unsigned long long)publication_timing.evidence_us,
            (unsigned long long)publication_timing.replica_us,
            (int)publication_timing.replica_result,
            (unsigned long long)publication_timing.account_evidence_us,
            (unsigned long long)publication_timing.visibility_us,
            (unsigned long long)publication_timing.prune_us,
            (unsigned long long)(finalized_us - published_us),
            (unsigned long long)(pay_timing_us() - started_us), (int)status);
    return status;
}

static lxp_result open_log(lxp_log *log, const char *environment,
                           bool *opened)
{
    const char *path = required_environment(environment);
    lxp_result status = path == NULL ? LXP_ERR_NON_CANONICAL :
        lxp_log_open_or_create(log, path, UINT64_C(64) * 1024U * 1024U);
    if (status == LXP_OK) *opened = true;
    return status;
}

static lxp_result require_distinct_logs(lxp_log *const *logs, size_t count)
{
    struct stat identities[5];
    size_t i;
    size_t prior;
    if (logs == NULL || count == 0U || count > 5U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < count; ++i) {
        if (logs[i] == NULL || logs[i]->descriptor < 0 ||
            fstat(logs[i]->descriptor, &identities[i]) != 0 ||
            !S_ISREG(identities[i].st_mode) || identities[i].st_nlink != 1)
            return LXP_ERR_AUTH_SCOPE;
        for (prior = 0U; prior < i; ++prior)
            if (identities[prior].st_dev == identities[i].st_dev &&
                identities[prior].st_ino == identities[i].st_ino)
                return LXP_ERR_CONTEXT_MISMATCH;
    }
    return LXP_OK;
}

static void free_batch_spans(lxp_byte_span *spans, size_t count)
{
    size_t i;
    for (i = 0U; i < count; ++i) free((void *)spans[i].bytes);
}

static lxp_result redo_prepared_batch_wal(
    lxp_daemon_process *process,
    const lxp_daemon_batch_wal_input *view,
    const lxp_batch_header *header)
{
    lxp_activity activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_receipt *expected;
    lxp_kernel_execution executions[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_authority_grant grants[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_authority_resolved authorities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_batch_roots roots;
    uint8_t batch_id[32];
    size_t retry_count = 0U;
    size_t mark;
    lxp_result status = LXP_OK;
    if (process == NULL || view == NULL || header == NULL ||
        view->count == 0U || view->count > LXP_DAEMON_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    expected = calloc(view->count, sizeof(*expected));
    if (expected == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    mark = lxp_arena_mark(&process->execution_arena);
    (void)memset(activities, 0, sizeof(activities));
    (void)memset(executions, 0, sizeof(executions));
    (void)memset(grants, 0, sizeof(grants));
    (void)memset(authorities, 0, sizeof(authorities));
    for (size_t i = 0U; status == LXP_OK && i < view->count; ++i) {
        lxp_identity *identity = NULL;
        uint8_t principal[32];
        lxp_u128 balance = {0U, 0U};
        status = lxp_activity_decode(view->activities[i].bytes,
            view->activities[i].length, &activities[i]);
        if (status == LXP_OK)
            status = lxp_receipt_decode(view->receipts[i].bytes,
                view->receipts[i].length, true, &expected[i]);
        if (status == LXP_OK)
            status = lxp_receipt_verify(&expected[i],
                process->sequencer_authorization.public_key, &process->execution_arena);
        if (status == LXP_OK &&
            (activities[i].protocol_version != process->protocol_version ||
             expected[i].protocol_version != process->protocol_version ||
             expected[i].module_id != lxp_activity_module_id(activities[i].activity_type) ||
             expected[i].global_sequence != view->first_sequence + i ||
             expected[i].timestamp != view->timestamp_ms ||
             expected[i].parameter_version != view->parameter_version ||
             expected[i].parameter_version != process->parameter_version ||
             (view->count != 1U && activities[i].activity_type != LX_PROGRAMS_CALL)))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_activity_check_envelope(&activities[i], process->network_id);
        if (status == LXP_OK) status = lxp_activity_verify_payload_hash(&activities[i]);
        if (status == LXP_OK) status = lxp_activity_verify_signature(&activities[i]);
        if (status == LXP_OK)
            status = lxp_identity_resolve(&process->identities, activities[i].actor_did.bytes,
                activities[i].actor_did.length, &identity);
        if (status == LXP_OK) status = lxp_governance_identity_refresh(&process->kernel, identity);
        if (status == LXP_OK && activities[i].authority.length != 32U)
            status = LXP_ERR_BAD_SIGNATURE;
        if (status == LXP_OK)
            status = lxp_authority_resolve_activity(&process->kernel, identity, &activities[i],
                lxp_identity_key_valid(identity, activities[i].authority.bytes,
                    view->timestamp_ms, view->first_sequence + i),
                true, view->timestamp_ms, UINT64_C(300000), view->first_sequence + i,
                &grants[i], &authorities[i]);
        if (status == LXP_OK)
            status = principal_authority(process, &activities[i],
                grants[i].kind == LXP_AUTHORITY_OWNER ? grants[i].key : identity->primary_key,
                principal, &balance);
        if (status == LXP_OK) (void)memcpy(authorities[i].principal, principal, 32U);
        executions[i].network_id = process->network_id;
        executions[i].batch_number = view->batch_number;
        executions[i].batch_timestamp_ms = view->timestamp_ms;
        executions[i].maximum_timestamp_window = UINT64_C(300000);
        executions[i].epoch = view->epoch;
        executions[i].global_sequence = view->first_sequence + i;
        executions[i].recorded_module_version = expected[i].module_version;
        executions[i].recorded_metering_schedule_version = view->metering_schedule_version;
        executions[i].recorded_fee_schedule_version = view->fee_schedule_version;
        executions[i].parameter_version = view->parameter_version;
        executions[i].signature_valid = true;
        executions[i].identities = &process->identities;
        executions[i].authority = &authorities[i];
        executions[i].fee_parameters = &process->fees;
        executions[i].fee_balance = balance;
        executions[i].gas_limit = UINT64_MAX;
        executions[i].arena = &process->execution_arena;
        executions[i].sequencer_private_key = process->sequencer_private_key;
        executions[i].verified_receipts = &process->verified_receipts;
    }
    if (status == LXP_OK)
        status = lxp_daemon_batch_bind_prefix(view->activities, view->count,
            process->kernel.current_state_root, view->first_sequence, view->batch_number,
            &process->execution_arena, executions, &roots, batch_id);
    if (status == LXP_OK) {
        if (activities[0].activity_type == LX_PROGRAMS_CALL)
            status = lxp_kernel_prepare_activity_batch(&process->kernel, activities,
                executions, view->count, 1U, &prepared, &retry_count);
        else
            status = lxp_kernel_prepare_serial_activity_batch(&process->kernel,
                &activities[0], &executions[0], &prepared);
    }
    if (status != LXP_OK && prepared == NULL && view->count == 1U &&
        status == expected[0].result_code && lxp_terminal_rejection_applies(status) &&
        terminal_rejection_module_supported(process, activities[0].activity_type)) {
        lxp_result refusal = status;
        status = lxp_daemon_batch_bind_prefix(view->activities, 1U,
            process->kernel.current_state_root, view->first_sequence, view->batch_number,
            &process->execution_arena, executions, &roots, batch_id);
        if (status == LXP_OK)
            status = lxp_kernel_prepare_terminal_rejection(&process->kernel,
                &activities[0], &executions[0], refusal, &prepared);
    }
    if (status == LXP_OK && view->maintenance.length != 0U)
        status = lxp_kernel_prepare_batch_maintenance(prepared, activities, executions);
    if (status == LXP_OK &&
        (lxp_kernel_prepared_batch_count(prepared) != view->count ||
         lxp_ct_memcmp(lxp_kernel_prepared_batch_publication_digest(prepared),
             view->publication_digest, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_kernel_commit_prepared_batch(&process->kernel,
            &process->identities, prepared, view->publication_digest);
    lxp_kernel_prepared_batch_destroy(prepared);
    (void)lxp_arena_reset(&process->execution_arena, mark);
    free(expected);
    return status;
}

static lxp_result recover_prepared_batch_wal(
    lxp_daemon_process *process, lxp_daemon_protocol_owner *owner)
{
    lxp_daemon_batch_wal_record *record = NULL;
    const lxp_daemon_batch_wal_input *view;
    lxp_kernel_batch_boundary live;
    lxp_daemon_batch_wal_recovery recovery;
    lxp_batch_header header;
    lxp_activity *activities = NULL;
    lxp_receipt *receipts = NULL;
    bool present = false;
    size_t i;
    lxp_result status;
    if (process == NULL || owner == NULL || owner != &process->owner)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_batch_wal_load(
        process->checkpoint_directory, &process->sequencer_authorization,
        &record, &present);
    if (status != LXP_OK) goto done;
    if (!present) {
        status = lxp_kernel_batch_boundary_read(&process->kernel, &live);
        if (status == LXP_OK && !owner->feed_store.baseline_present)
            status = lxp_programs_state_feed_store_anchor(
                &owner->feed_store, live.next_sequence,
                live.receipt_state_root);
        goto done;
    }
    view = lxp_daemon_batch_wal_view(record);
    if (view == NULL || view->count == 0U ||
        view->count > LXP_DAEMON_MAX_BATCH_ACTIVITIES) {
        status = LXP_ERR_LOG_CORRUPT;
        goto done;
    }
    status = lxp_batch_header_decode(view->canonical_header.bytes,
                                     view->canonical_header.length,
                                     &header);
    if (status == LXP_OK &&
        (view->network_id != process->network_id ||
         view->epoch != process->kernel.epoch ||
         view->batch_number <
             process->sequencer_authorization.first_batch_number ||
         view->batch_number >
             process->sequencer_authorization.last_batch_number ||
         view->first_sequence != view->base.next_sequence ||
         view->last_sequence == UINT64_MAX ||
         view->settled.next_sequence != view->last_sequence + 1U ||
         header.protocol_version != view->protocol_version ||
         header.network_id != view->network_id ||
         header.epoch != view->epoch ||
         header.batch_number != view->batch_number ||
         header.first_sequence != view->first_sequence ||
         header.last_sequence != view->last_sequence ||
         header.timestamp_ms != view->timestamp_ms ||
         lxp_ct_memcmp(header.sequencer_id,
                       process->sequencer_authorization.sequencer_id,
                       32U) != 0 ||
         lxp_ct_memcmp(header.previous_state_root,
                       view->base.receipt_state_root, 32U) != 0 ||
         lxp_ct_memcmp(header.resulting_state_root,
                       view->settled.receipt_state_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status != LXP_OK) goto done;
    status = lxp_kernel_batch_boundary_read(&process->kernel, &live);
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_classify(record, &live, &recovery);
    if (status != LXP_OK) goto done;
    status = check_batch_record(
        process, &header, view->canonical_header.bytes,
        view->canonical_header.length, false);
    if (status != LXP_OK) goto done;
    if (!owner->feed_store.baseline_present) {
        const lxp_kernel_batch_boundary *anchor =
            recovery == LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED ||
                    recovery == LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED ?
                &view->base : &live;
        status = lxp_programs_state_feed_store_anchor(
            &owner->feed_store, anchor->next_sequence,
            anchor->receipt_state_root);
        if (status != LXP_OK) goto done;
    }
    if (recovery == LXP_DAEMON_BATCH_WAL_DISCARD_BASE) {
        if (lxp_daemon_batch_wal_record_state(record) !=
                LXP_DAEMON_BATCH_WAL_PREPARED) {
            status = LXP_FATAL_REPLAY_DIVERGENCE;
            goto done;
        }
        status = redo_prepared_batch_wal(process, view, &header);
        if (status == LXP_OK)
            status = lxp_kernel_batch_boundary_read(
                &process->kernel, &live);
        if (status == LXP_OK)
            status = lxp_daemon_batch_wal_classify(
                record, &live, &recovery);
        if (status != LXP_OK ||
            recovery != LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED) {
            if (status == LXP_OK) status = LXP_FATAL_REPLAY_DIVERGENCE;
            goto done;
        }
        status = persist_state_checkpoint(
            process, view->settled.next_sequence - 1U);
        if (status != LXP_OK) goto done;
    }
    if (recovery == LXP_DAEMON_BATCH_WAL_ALREADY_ABORTED) {
        status = lxp_daemon_batch_wal_retire(
            process->checkpoint_directory, record, &live);
        goto done;
    }
    if (recovery != LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED &&
        recovery != LXP_DAEMON_BATCH_WAL_ALREADY_COMMITTED) {
        status = LXP_FATAL_REPLAY_DIVERGENCE;
        goto done;
    }
    if (view->state_diff.length != 0U) {
        lxp_batch_body body;
        size_t mark = lxp_arena_mark(&process->execution_arena);
        status = lxp_daemon_batch_wal_body(view, &process->execution_arena, &body);
        if (status == LXP_OK)
            status = lxp_da_recovery_verify_kernel(&process->kernel,
                header.last_sequence, header.last_sequence,
                body.recovery_metadata, &process->execution_arena);
        if (status == LXP_OK) status = availability_store_body(process, &body);
        if (status == LXP_OK)
            status = lxp_da_log_store_body(&process->availability_log, &body, &process->execution_arena);
        (void)lxp_arena_reset(&process->execution_arena, mark);
        if (status != LXP_OK) goto done;
    }
    activities = calloc(view->count, sizeof(*activities));
    receipts = calloc(view->count, sizeof(*receipts));
    if (activities == NULL || receipts == NULL) {
        status = LXP_ERR_IO;
        goto done;
    }
    for (i = 0U; status == LXP_OK && i < view->count; ++i) {
        status = lxp_activity_decode(view->activities[i].bytes,
                                     view->activities[i].length,
                                     &activities[i]);
        if (status == LXP_OK &&
            activities[i].protocol_version != process->protocol_version)
            status = LXP_ERR_VERSION_UNSUPPORTED;
        if (status == LXP_OK)
            status = lxp_receipt_decode(view->receipts[i].bytes,
                                        view->receipts[i].length, true,
                                        &receipts[i]);
    }
    if (status == LXP_OK && !process->kernel.batch_publication_pending)
        status = (view->maintenance.length != 0U ?
            lxp_kernel_restore_batch_publication_pending_maintenance :
            lxp_kernel_restore_batch_publication_pending)(
            &process->kernel, view->publication_digest,
            receipts[0].batch_id, view->base.receipt_state_root,
            view->settled.receipt_state_root, view->first_sequence,
            view->last_sequence, 0U);
    if (status == LXP_OK)
        status = view->maintenance.length != 0U ?
            lxp_kernel_finalize_batch_publication_maintenance(
                &process->kernel, activities, receipts, view->count,
                view->maintenance, &view->base, &view->settled, view->events,
                view->publication_digest) :
            lxp_kernel_finalize_batch_publication_records(
                &process->kernel, activities, receipts, view->count,
                &view->base, &view->settled, view->events, view->publication_digest);
    if (status == LXP_OK)
        status = ensure_batch_record(
            process, &header, view->canonical_header.bytes,
            view->canonical_header.length);
    for (i = 0U; status == LXP_OK && i < view->count; ++i) {
        if (view->terminal_payloads != NULL && view->call_graphs != NULL)
            status = lxp_daemon_receipt_authority_append_artifacts(
                &process->receipt_authority,
                view->receipts[i].bytes, view->receipts[i].length,
                view->canonical_header.bytes, view->canonical_header.length,
                view->header_signature, &view->receipt_proofs[i],
                &process->owner_scratch, view->terminal_payloads[i],
                view->call_graphs[i]);
        else
            status = lxp_daemon_receipt_authority_append(
                &process->receipt_authority,
                view->receipts[i].bytes, view->receipts[i].length,
                view->canonical_header.bytes, view->canonical_header.length,
                view->header_signature, &view->receipt_proofs[i],
                &process->owner_scratch);
        if (status == LXP_OK)
            status = lxp_verified_receipt_index_add(
                &process->verified_receipts, &receipts[i],
                process->sequencer_authorization.public_key,
                &process->owner_scratch);
        if (status == LXP_OK)
            status = lxp_daemon_authority_replica_publish(
                process->authority_replica_address,
                process->authority_replica_port,
                process->authority_replica_token,
                process->authority_replica_token_length,
                process->authority_replica_id,
                view->receipts[i].bytes, view->receipts[i].length,
                view->canonical_header.bytes, view->canonical_header.length,
                view->header_signature, &view->receipt_proofs[i]);
    }
    if (status == LXP_OK && view->maintenance.length != 0U) {
        status = lxp_daemon_receipt_authority_append_maintenance(
            &process->receipt_authority, view->maintenance.bytes, view->maintenance.length,
            view->canonical_header.bytes, view->canonical_header.length,
            view->header_signature, &view->maintenance_proof, &process->owner_scratch);
        if (status == LXP_OK)
            status = lxp_daemon_authority_replica_publish_maintenance(
                process->authority_replica_address, process->authority_replica_port,
                process->authority_replica_token, process->authority_replica_token_length,
                process->authority_replica_id, view->maintenance.bytes, view->maintenance.length,
                view->canonical_header.bytes, view->canonical_header.length,
                view->header_signature, &view->maintenance_proof);
        if (status == LXP_OK && process->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
            status = lxp_daemon_account_evidence_publish_batch_maintenance(
                &process->evidence_store, &process->kernel, view->maintenance,
                &view->maintenance_proof, &process->sequencer_authorization,
                view->canonical_header, view->header_signature, &process->owner_scratch);
    }
    if (status == LXP_OK &&
        lxp_daemon_batch_wal_record_state(record) ==
            LXP_DAEMON_BATCH_WAL_PREPARED)
        status = lxp_daemon_batch_wal_transition(
            process->checkpoint_directory, record,
            &live,
            LXP_DAEMON_BATCH_WAL_COMMITTED);
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_retire(
            process->checkpoint_directory, record, &live);
done:
    free(activities);
    free(receipts);
    lxp_daemon_batch_wal_destroy(record);
    return status;
}

static lxp_result recover_ranged_batch_authority(
    lxp_daemon_process *process, const lxp_batch_header *header,
    const uint8_t *canonical_header, size_t header_length)
{
    lxp_byte_span activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    lxp_byte_span receipts[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U] = {{0}};
    lxp_byte_span events[LXP_DAEMON_MAX_BATCH_ACTIVITIES] = {{0}};
    lxp_receipt *decoded;
    uint8_t receipt_hashes[LXP_DAEMON_MAX_BATCH_ACTIVITIES + 1U][32];
    lxp_programs_occupancy_receipt maintenance;
    bool has_maintenance = false;
    size_t receipt_count;
    uint8_t signature[64];
    lxp_batch_roots roots;
    lxp_merkle_proof head_receipt_proof;
    uint64_t offset = 0U;
    size_t count;
    size_t i;
    size_t mark;
    lxp_result status = LXP_OK;
    if (header->first_sequence == 0U ||
        header->last_sequence < header->first_sequence ||
        header->last_sequence - header->first_sequence >
            LXP_DAEMON_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_LENGTH_LIMIT;
    count = (size_t)(header->last_sequence - header->first_sequence + 1U);
    receipt_count = count;
    decoded = calloc(count, sizeof(*decoded));
    if (decoded == NULL) return LXP_ERR_IO;
    while (status == LXP_OK && offset < process->canonical_log.write_offset) {
        lxp_log_record_header record;
        uint8_t *body = NULL;
        size_t index;
        status = lxp_log_read(&process->canonical_log, offset,
                              &record, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (status == LXP_ERR_LENGTH_LIMIT) status = LXP_OK;
        if (record.global_sequence >= header->first_sequence &&
            record.global_sequence <= header->last_sequence &&
            (record.record_kind == (uint8_t)LXP_LOG_ACTIVITY ||
             record.record_kind == (uint8_t)LXP_LOG_RECEIPT ||
             (record.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
              record.body_length > 109U))) {
            if (record.body_length == 0U ||
                record.body_length > LXP_MAX_ACTIVITY_BYTES) {
                status = LXP_ERR_LOG_CORRUPT;
                break;
            }
            body = (uint8_t *)malloc(record.body_length);
            if (body == NULL) {
                status = LXP_ERR_IO;
                break;
            }
            status = lxp_log_read(&process->canonical_log, offset,
                                  &record, body, record.body_length);
            index = (size_t)(record.global_sequence -
                             header->first_sequence);
            if (status == LXP_OK &&
                record.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
                if (index >= LXP_DAEMON_MAX_BATCH_ACTIVITIES ||
                    activities[index].bytes != NULL)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    activities[index] =
                        (lxp_byte_span){body, record.body_length};
                    body = NULL;
                }
            } else if (status == LXP_OK &&
                       record.record_kind == (uint8_t)LXP_LOG_CHECKPOINT) {
                if (index + 1U != receipt_count || has_maintenance ||
                    receipts[index].bytes != NULL || record.body_length <= 13U ||
                    memcmp(body, "LXPM1", 5U) != 0 ||
                    read_u64_be(body + 5U) != header->timestamp_ms ||
                    !lxp_protocol_version_uses_occupancy(header->protocol_version)) {
                    status = LXP_ERR_LOG_CORRUPT;
                } else {
                    status = lxp_programs_occupancy_receipt_decode(
                        body + 13U, record.body_length - 13U, &maintenance);
                    if (status == LXP_OK &&
                        (maintenance.batch_number != header->batch_number ||
                         maintenance.global_sequence != header->last_sequence ||
                         lxp_ct_memcmp(maintenance.resulting_state_root,
                             header->resulting_state_root, 32U) != 0))
                        status = LXP_ERR_LOG_CORRUPT;
                    if (status == LXP_OK) {
                        (void)memmove(body, body + 13U, record.body_length - 13U);
                        receipts[index] = (lxp_byte_span){body, record.body_length - 13U};
                        body = NULL;
                        has_maintenance = true;
                    }
                }
            } else if (status == LXP_OK) {
                if (receipts[index].bytes != NULL)
                    status = LXP_ERR_LOG_CORRUPT;
                else {
                    receipts[index] =
                        (lxp_byte_span){body, record.body_length};
                    body = NULL;
                }
            }
            free(body);
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)record.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + record.body_length;
        }
    }
    if (has_maintenance) --count;
    if (status == LXP_OK && (count == 0U || count > LXP_DAEMON_MAX_BATCH_ACTIVITIES))
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK && has_maintenance) {
        status = lxp_programs_occupancy_receipt_decode(
            receipts[count].bytes, receipts[count].length, &maintenance);
        if (status == LXP_OK)
            status = lxp_merkle_leaf_hash(receipts[count].bytes,
                receipts[count].length, receipt_hashes[count]);
    }
    mark = lxp_arena_mark(&process->owner_scratch);
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_activity activity;
        uint8_t activity_id[32];
        if (activities[i].bytes == NULL || receipts[i].bytes == NULL)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK)
            status = lxp_activity_decode(activities[i].bytes,
                                         activities[i].length, &activity);
        if (status == LXP_OK &&
            activity.protocol_version != process->protocol_version)
            status = LXP_ERR_VERSION_UNSUPPORTED;
        if (status == LXP_OK)
            status = lxp_activity_check_envelope(
                &activity, process->network_id);
        if (status == LXP_OK)
            status = lxp_activity_verify_payload_hash(&activity);
        if (status == LXP_OK)
            status = lxp_activity_verify_signature(&activity);
        if (status == LXP_OK)
            status = lxp_activity_id(activities[i].bytes,
                                     activities[i].length, activity_id);
        if (status == LXP_OK)
            status = lxp_receipt_decode(receipts[i].bytes,
                                        receipts[i].length, true,
                                        &decoded[i]);
        if (status == LXP_OK)
            status = lxp_receipt_verify(
                &decoded[i], process->sequencer_authorization.public_key,
                &process->owner_scratch);
        if (status == LXP_OK &&
            (decoded[i].global_sequence != header->first_sequence + i ||
             decoded[i].protocol_version != header->protocol_version ||
             decoded[i].timestamp != header->timestamp_ms ||
             lxp_ct_memcmp(decoded[i].activity_id, activity_id, 32U) != 0 ||
             (i != 0U &&
              lxp_ct_memcmp(decoded[i - 1U].resulting_state_root,
                            decoded[i].previous_state_root, 32U) != 0)))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_merkle_leaf_hash(receipts[i].bytes,
                                          receipts[i].length,
                                          receipt_hashes[i]);
        if (status == LXP_OK)
            status = lxp_programs_project_receipt_events(
                &decoded[i], &process->owner_scratch, &events[i]);
    }
    if (status == LXP_OK &&
        (lxp_ct_memcmp(decoded[0].previous_state_root,
                       header->previous_state_root, 32U) != 0 ||
         lxp_ct_memcmp(decoded[count - 1U].resulting_state_root,
                       has_maintenance ? maintenance.previous_state_root :
                           header->resulting_state_root, 32U) != 0 ||
         (has_maintenance && maintenance.parameter_version != decoded[0].parameter_version)))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){activities, count, receipts, receipt_count,
                                     events, count, NULL, 0U, NULL, 0U},
            &process->owner_scratch, &roots);
    if (status == LXP_OK) {
        lxp_batch_body body;
        uint8_t legacy_root[32];
        status = lxp_merkle_leaf_hash(NULL, 0U, legacy_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(header->data_availability_root, legacy_root, 32U) == 0) {
            process->owner.availability_ready = false;
        } else if (status == LXP_OK) {
            status = lxp_da_log_read_body(&process->availability_log, header->batch_number,
                                          &process->owner_scratch, &body);
            if (status == LXP_OK)
                status = lxp_replay_section_encode(activities, count, &process->owner_scratch,
                                                   &body.activities);
            if (status == LXP_OK)
                status = lxp_da_receipt_section_encode(receipts, receipt_count,
                    events, count, &process->owner_scratch, &body.receipts);
            if (status == LXP_OK)
                status = lxp_batch_availability_root(&body, &process->owner_scratch,
                                                     roots.data_availability_root);
        }
    }
    if (status == LXP_OK &&
        (lxp_ct_memcmp(roots.activity_merkle_root,
                       header->activity_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.receipt_merkle_root,
                       header->receipt_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.event_merkle_root,
                       header->event_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.oracle_root,
                       header->oracle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.data_availability_root,
                       header->data_availability_root, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_batch_sign(
            header, process->sequencer_private_key,
            &process->sequencer_authorization, signature,
            &process->owner_scratch);
    (void)memset(&head_receipt_proof, 0, sizeof(head_receipt_proof));
    for (i = 0U; status == LXP_OK && i < count; ++i) {
        lxp_daemon_receipt_evidence existing;
        lxp_merkle_proof receipt_proof;
        uint8_t digest[32];
        uint8_t proof_root[32];
        bool exists = false;
        size_t lookup_mark = lxp_arena_mark(&process->owner_scratch);
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, receipt_count, i,
            &process->owner_scratch, &receipt_proof, proof_root);
        if (status == LXP_OK &&
            lxp_ct_memcmp(proof_root,
                          header->receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK && i + 1U == count)
            head_receipt_proof = receipt_proof;
        if (status == LXP_OK)
            status = lxp_receipt_digest(&decoded[i],
                                        &process->owner_scratch, digest);
        if (status == LXP_OK)
            status = lxp_daemon_receipt_authority_lookup(
                &process->receipt_authority, digest,
                &process->owner_scratch, &existing);
        if (status == LXP_OK &&
            (existing.global_sequence != decoded[i].global_sequence ||
             existing.canonical_header.length != header_length ||
             lxp_ct_memcmp(existing.canonical_header.bytes,
                           canonical_header, header_length) != 0 ||
             lxp_ct_memcmp(existing.header_signature, signature, 64U) != 0 ||
             existing.receipt_proof.leaf_index != i ||
             existing.receipt_proof.leaf_count != receipt_count ||
             existing.receipt_proof.depth != receipt_proof.depth ||
             lxp_ct_memcmp(existing.receipt_proof.siblings,
                           receipt_proof.siblings,
                           (size_t)receipt_proof.depth * 32U) != 0))
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK) exists = true;
        (void)lxp_arena_reset(&process->owner_scratch, lookup_mark);
        if (!exists) {
            if (status != LXP_ERR_UNKNOWN_ACTIVITY) break;
            status = LXP_OK;
            if (status == LXP_OK)
                status = lxp_daemon_receipt_authority_append(
                    &process->receipt_authority,
                    receipts[i].bytes, receipts[i].length,
                    canonical_header, header_length, signature,
                    &receipt_proof,
                    &process->owner_scratch);
            if (status == LXP_OK)
                status = lxp_daemon_authority_replica_publish(
                    process->authority_replica_address,
                    process->authority_replica_port,
                    process->authority_replica_token,
                    process->authority_replica_token_length,
                    process->authority_replica_id,
                    receipts[i].bytes, receipts[i].length,
                    canonical_header, header_length, signature,
                    &receipt_proof);
        }
        if (status == LXP_OK)
            status = lxp_verified_receipt_index_add(
                &process->verified_receipts, &decoded[i],
                process->sequencer_authorization.public_key,
                &process->owner_scratch);
    }
    if (status == LXP_OK && has_maintenance) {
        uint8_t proof_root[32];
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes, receipt_count, count,
            &process->owner_scratch, &head_receipt_proof, proof_root);
        if (status == LXP_OK && lxp_ct_memcmp(proof_root, header->receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_REPLAY_DIVERGENCE;
        if (status == LXP_OK)
            status = lxp_daemon_receipt_authority_append_maintenance(
                &process->receipt_authority, receipts[count].bytes, receipts[count].length,
                canonical_header, header_length, signature, &head_receipt_proof,
                &process->owner_scratch);
        if (status == LXP_OK)
            status = lxp_daemon_authority_replica_publish_maintenance(
                process->authority_replica_address, process->authority_replica_port,
                process->authority_replica_token, process->authority_replica_token_length,
                process->authority_replica_id, receipts[count].bytes, receipts[count].length,
                canonical_header, header_length, signature, &head_receipt_proof);
    }
    if (status == LXP_OK)
        status = lxp_daemon_activity_evidence_recover_batch(
            &process->evidence_store, &process->canonical_log,
            &process->receipt_authority,
            &process->sequencer_authorization,
            (lxp_byte_span){canonical_header, header_length}, signature,
            &process->owner_scratch);
    if (status == LXP_OK && process->protocol_version ==
                                LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        status = recover_batch_account_evidence(
            process, header,
            (lxp_byte_span){canonical_header, header_length}, signature,
            receipts[receipt_count - 1U], &head_receipt_proof, has_maintenance);
    (void)lxp_arena_reset(&process->owner_scratch, mark);
    free_batch_spans(activities, receipt_count < LXP_DAEMON_MAX_BATCH_ACTIVITIES ?
        receipt_count : LXP_DAEMON_MAX_BATCH_ACTIVITIES);
    free_batch_spans(receipts, receipt_count);
    free(decoded);
    return status;
}

static lxp_result recover_ranged_batch_authorities(
    lxp_daemon_process *process)
{
    uint64_t offset = 0U;
    uint64_t prior_batch = 0U;
    uint64_t prior_last_sequence = 0U;
    uint64_t prior_epoch = 0U;
    uint8_t prior_resulting_root[32] = {0};
    lxp_result status = LXP_OK;
    while (status == LXP_OK && offset < process->batch_log.write_offset) {
        lxp_log_record_header record;
        uint8_t body[LXP_BATCH_HEADER_ENCODED_SIZE];
        lxp_batch_header header;
        status = lxp_log_read(&process->batch_log, offset,
                              &record, body, sizeof(body));
        if (status == LXP_OK &&
            (record.record_kind != (uint8_t)LXP_LOG_BATCH_HEADER ||
             record.body_length != sizeof(body) ||
             record.global_sequence == 0U))
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK)
            status = lxp_batch_header_decode(body, sizeof(body), &header);
        if (status == LXP_OK &&
            (record.global_sequence != header.last_sequence ||
             header.network_id != process->network_id ||
             header.epoch == 0U || header.epoch > process->kernel.epoch ||
             (prior_batch != 0U && header.epoch < prior_epoch) ||
             header.batch_number <
                 process->sequencer_authorization.first_batch_number ||
             header.batch_number >
                 process->sequencer_authorization.last_batch_number ||
             (prior_batch == 0U &&
              header.batch_number !=
                  process->sequencer_authorization.first_batch_number) ||
             lxp_ct_memcmp(header.sequencer_id,
                           process->sequencer_authorization.sequencer_id,
                           32U) != 0 ||
             (prior_batch != 0U &&
              (prior_batch == UINT64_MAX ||
               prior_last_sequence == UINT64_MAX ||
               header.batch_number != prior_batch + 1U ||
               header.first_sequence != prior_last_sequence + 1U ||
               lxp_ct_memcmp(header.previous_state_root,
                             prior_resulting_root, 32U) != 0))))
            status = LXP_ERR_BATCH_GAP;
        if (status == LXP_OK)
            status = recover_ranged_batch_authority(
                process, &header, body, sizeof(body));
        if (status == LXP_OK) {
            prior_batch = header.batch_number;
            prior_last_sequence = header.last_sequence;
            prior_epoch = header.epoch;
            (void)memcpy(prior_resulting_root,
                         header.resulting_state_root, 32U);
        }
        if (status == LXP_OK) {
            if (offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)record.body_length)
                status = LXP_ERR_OVERFLOW;
            else
                offset += LXP_LOG_HEADER_BYTES + record.body_length;
        }
    }
    return status;
}

static lxp_result resume_batch_number(lxp_daemon_process *process)
{
    uint64_t next = process->sequencer_authorization.first_batch_number;
    uint64_t offset = 0U;
    uint64_t active_batch = 0U;
    uint64_t expected_sequence = 0U;
    uint64_t active_last_sequence = 0U;
    uint8_t active_header[LXP_BATCH_HEADER_ENCODED_SIZE];
    uint8_t active_signature[64];
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
        if (status == LXP_OK && present &&
            evidence.canonical_header.length != sizeof(active_header))
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK && present &&
            header.batch_number != active_batch) {
            if ((active_batch != 0U &&
                 expected_sequence != active_last_sequence + 1U) ||
                next == 0U || header.batch_number != next ||
                evidence.global_sequence != header.first_sequence)
                status = LXP_ERR_BATCH_GAP;
            else {
                active_batch = header.batch_number;
                expected_sequence = header.first_sequence;
                active_last_sequence = header.last_sequence;
                (void)memcpy(active_header,
                             evidence.canonical_header.bytes,
                             sizeof(active_header));
                (void)memcpy(active_signature,
                             evidence.header_signature, 64U);
            }
        }
        if (status == LXP_OK && present &&
            (evidence.global_sequence != expected_sequence ||
             header.batch_number != active_batch ||
             header.last_sequence != active_last_sequence ||
             lxp_ct_memcmp(evidence.canonical_header.bytes, active_header,
                           sizeof(active_header)) != 0 ||
             lxp_ct_memcmp(evidence.header_signature, active_signature,
                           64U) != 0))
            status = LXP_ERR_BATCH_GAP;
        if (status == LXP_OK && present) {
            if (expected_sequence == UINT64_MAX)
                status = LXP_ERR_OVERFLOW;
            else
                ++expected_sequence;
            if (status == LXP_OK &&
                expected_sequence == active_last_sequence + 1U)
                next = active_batch ==
                               process->sequencer_authorization.last_batch_number ?
                           0U : active_batch + 1U;
        }
        (void)lxp_arena_reset(&process->owner_scratch, mark);
    }
    if (status == LXP_OK && active_batch != 0U &&
        expected_sequence != active_last_sequence + 1U)
        status = LXP_ERR_BATCH_GAP;
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
            status = (evidence.format_version == 3U ?
                lxp_daemon_authority_replica_publish_maintenance :
                lxp_daemon_authority_replica_publish)(
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
    static const uint8_t key[32] = {
        'p','a','r','a','m','e','t','e','r','-','v','e','r','s','i','o','n'
    };
    const lxp_module_kv_entry *parameter = NULL;
    uint32_t parameter_version;
    size_t index;
    if (process == NULL) return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < process->kernel.module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &process->kernel.module_kv[index];
        if (entry->module_id == LXP_MODULE_GOVERNANCE &&
            entry->key_length == sizeof(key) &&
            memcmp(entry->key, key, sizeof(key)) == 0) {
            if (parameter != NULL) return LXP_ERR_SEQUENCE_REUSED;
            parameter = entry;
        }
    }
    if (parameter == NULL || parameter->value_length != 32U ||
        !lxp_ct_is_zero(parameter->value, 28U))
        return LXP_ERR_VERSION_UNSUPPORTED;
    parameter_version = ((uint32_t)parameter->value[28] << 24U) |
        ((uint32_t)parameter->value[29] << 16U) |
        ((uint32_t)parameter->value[30] << 8U) | parameter->value[31];
    if (parameter_version == 0U || parameter_version > UINT16_MAX)
        return LXP_ERR_VERSION_UNSUPPORTED;
    process->parameter_version = parameter_version;
    return lxp_fee_committed_schedule(&process->kernel, parameter_version, &process->fees);
}

static lxp_result path_empty_or_absent(const char *path)
{
    struct stat information;
    if (path == NULL) return LXP_ERR_NON_CANONICAL;
    if (lstat(path, &information) != 0)
        return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
    return S_ISREG(information.st_mode) && information.st_nlink == 1 &&
           information.st_size == 0 ?
        LXP_OK : LXP_ERR_ROOT_MISMATCH;
}

static bool bootstrap_checkpoint_entry(const char *name)
{
    static const char *const allowed[] = {
        ".layerxd-lni-admission.log",
        ".layerxd-lni-admission.tmp"
    };
    size_t index;
    if (name == NULL) return false;
    for (index = 0U; index < sizeof(allowed) / sizeof(allowed[0]); ++index)
        if (strcmp(name, allowed[index]) == 0) return true;
    return false;
}

static lxp_result bootstrap_checkpoint_directory_clean(const char *path)
{
    DIR *directory;
    struct dirent *entry;
    lxp_result status = LXP_OK;
    if (path == NULL) return LXP_ERR_NON_CANONICAL;
    directory = opendir(path);
    if (directory == NULL) return LXP_ERR_IO;
    errno = 0;
    while ((entry = readdir(directory)) != NULL) {
        struct stat metadata;
        if (strcmp(entry->d_name, ".") == 0 ||
            strcmp(entry->d_name, "..") == 0)
            continue;
        if (!bootstrap_checkpoint_entry(entry->d_name) ||
            fstatat(dirfd(directory), entry->d_name, &metadata,
                    AT_SYMLINK_NOFOLLOW) != 0 ||
            !S_ISREG(metadata.st_mode) || metadata.st_nlink != 1 ||
            metadata.st_uid != geteuid() ||
            (metadata.st_mode & 0777U) != 0600U) {
            status = LXP_ERR_ROOT_MISMATCH;
            break;
        }
    }
    if (status == LXP_OK && errno != 0) status = LXP_ERR_IO;
    if (closedir(directory) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result bootstrap_storage_empty(const char *checkpoint_directory)
{
    static const char *const names[] = {
        "LAYERX_NODE_PROGRAM_FEED_LOG", "LAYERX_NODE_CANONICAL_LOG",
        "LAYERX_NODE_RECEIPT_AUTHORITY_LOG", "LAYERX_NODE_BATCH_LOG",
        "LAYERX_NODE_EVIDENCE_LOG", "LAYERX_NODE_HISTORY_DATABASE"
    };
    size_t index;
    lxp_result status =
        bootstrap_checkpoint_directory_clean(checkpoint_directory);
    for (index = 0U; status == LXP_OK &&
         index < sizeof(names) / sizeof(names[0]); ++index)
        status = path_empty_or_absent(required_environment(names[index]));
    return status;
}

static lxp_result load_genesis_registration(
    lxp_genesis_bootstrap_registration *registration)
{
    const char *path = required_environment("LAYERX_NODE_GENESIS_REGISTRATION");
    uint8_t *encoded = NULL;
    size_t length = 0U;
    lxp_result status;
    if (registration == NULL || path == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_artifact_read(
        path, LXP_GENESIS_REGISTRATION_BYTES,
        LXP_GENESIS_REGISTRATION_BYTES, &encoded, &length);
    if (status == LXP_OK)
        status = lxp_genesis_registration_parse(encoded, length,
                                                 registration);
    if (encoded != NULL) {
        lxp_secure_zero(encoded, length);
        free(encoded);
    }
    return status;
}

static lxp_result initialized_genesis_marker_identity(
    lxp_daemon_process *process, bool create, bool *present,
    const uint8_t identity_digest[32])
{
    static const uint8_t domain[] = "LXP/initialized-genesis/v1";
    static const char *const inputs[] = {
        "LAYERX_NODE_GENESIS_MANIFEST", "LAYERX_NODE_GENESIS_REGISTRATION",
        "LAYERX_NODE_SNAPSHOT", "LAYERX_NODE_IDENTITIES"
    };
    static const char *const paths[] = {
        "LAYERX_NODE_CHECKPOINT_DIRECTORY", "LAYERX_NODE_PROGRAM_FEED_LOG",
        "LAYERX_NODE_CANONICAL_LOG", "LAYERX_NODE_RECEIPT_AUTHORITY_LOG",
        "LAYERX_NODE_BATCH_LOG", "LAYERX_NODE_EVIDENCE_LOG",
        "LAYERX_NODE_HISTORY_DATABASE"
    };
    uint8_t record[sizeof(domain) + 11U * 32U + 64U];
    uint8_t public_key[32];
    uint8_t private_key[32] = {0};
    uint8_t *stored = NULL;
    size_t stored_length = 0U;
    size_t offset = sizeof(domain);
    const size_t body_length = sizeof(record) - 64U;
    char final[4096];
    char temporary[4096];
    struct stat metadata;
    int descriptor = -1;
    int directory_descriptor = -1;
    int length;
    size_t index;
    lxp_result status = LXP_OK;
    *present = false;
    length = snprintf(final, sizeof(final), "%s/initialized-genesis.lxg",
                      process->checkpoint_directory);
    if (length < 0 || (size_t)length >= sizeof(final))
        return LXP_ERR_LENGTH_LIMIT;
    if (!create) {
        if (lstat(final, &metadata) != 0)
            return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
        if (!S_ISREG(metadata.st_mode) || metadata.st_nlink != 1 ||
            metadata.st_uid != geteuid() ||
            (metadata.st_mode & 0777U) != 0600U)
            return LXP_ERR_ROOT_MISMATCH;
        status = lxp_daemon_artifact_read(final, sizeof(record), sizeof(record),
                                          &stored, &stored_length);
    }
    (void)memcpy(record, domain, sizeof(domain));
    for (index = 0U; status == LXP_OK &&
         index < sizeof(inputs) / sizeof(inputs[0]); ++index) {
        uint8_t *bytes = NULL;
        size_t count = 0U;
        const char *path = required_environment(inputs[index]);
        if (index == 3U && identity_digest != NULL) {
            (void)memcpy(record + offset, identity_digest, 32U);
        } else {
            status = lxp_daemon_artifact_read(path, NODE_SNAPSHOT_ARENA_BYTES,
                                              0U, &bytes, &count);
            if (status == LXP_OK)
                status = lxp_hash_sha256(bytes, count, record + offset);
            if (status == LXP_OK && index == 3U)
                (void)memcpy(process->admitted_identity_digest, record + offset, 32U);
        }
        free(bytes);
        offset += 32U;
    }
    for (index = 0U; status == LXP_OK &&
         index < sizeof(paths) / sizeof(paths[0]); ++index) {
        const char *path = required_environment(paths[index]);
        if (path == NULL) status = LXP_ERR_NON_CANONICAL;
        else status = lxp_hash_sha256(path, strlen(path), record + offset);
        offset += 32U;
    }
    if (status == LXP_OK && offset != body_length)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = decode_hex(required_environment("LAYERX_NODE_SEQUENCER_PUBLIC_KEY"),
                            public_key, sizeof(public_key));
    if (status == LXP_OK && create) {
        EVP_PKEY *key = NULL;
        EVP_MD_CTX *context = NULL;
        size_t signature_length = 64U;
        status = decode_hex(required_environment("LAYERX_NODE_SEQUENCER_PRIVATE_KEY"),
                            private_key, sizeof(private_key));
        if (status == LXP_OK) {
            key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                               private_key, sizeof(private_key));
            context = EVP_MD_CTX_new();
            if (key == NULL || context == NULL ||
                EVP_DigestSignInit(context, NULL, NULL, NULL, key) != 1 ||
                EVP_DigestSign(context, record + body_length, &signature_length,
                               record, body_length) != 1 || signature_length != 64U)
                status = LXP_ERR_IO;
        }
        EVP_MD_CTX_free(context);
        EVP_PKEY_free(key);
        lxp_secure_zero(private_key, sizeof(private_key));
    }
    if (status == LXP_OK && !create) {
        if (stored_length != sizeof(record) ||
            lxp_ct_memcmp(stored, record, body_length) != 0)
            status = LXP_ERR_ROOT_MISMATCH;
        else (void)memcpy(record + body_length, stored + body_length, 64U);
    }
    if (status == LXP_OK)
        status = lxp_ed25519_verify_raw(public_key, record + body_length,
                                        record, body_length);
    free(stored);
    if (status == LXP_OK && create) {
        length = snprintf(temporary, sizeof(temporary), "%s.tmp", final);
        if (length < 0 || (size_t)length >= sizeof(temporary))
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK) {
            descriptor = open(temporary,
                O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW, 0600);
            if (descriptor < 0) status = LXP_ERR_IO;
        }
        if (status == LXP_OK)
            status = write_file_bytes(descriptor, record, sizeof(record));
        if (status == LXP_OK && fdatasync(descriptor) != 0) status = LXP_ERR_IO;
        if (descriptor >= 0 && close(descriptor) != 0 && status == LXP_OK)
            status = LXP_ERR_IO;
        if (status == LXP_OK && rename(temporary, final) != 0)
            status = LXP_ERR_IO;
        if (status == LXP_OK) {
            directory_descriptor = open(process->checkpoint_directory,
                O_RDONLY | O_DIRECTORY | O_CLOEXEC | O_NOFOLLOW);
            if (directory_descriptor < 0 || fsync(directory_descriptor) != 0)
                status = LXP_ERR_IO;
        }
        if (directory_descriptor >= 0) (void)close(directory_descriptor);
    }
    if (status == LXP_OK) *present = true;
    return status;
}

static lxp_result initialized_genesis_marker(
    lxp_daemon_process *process, bool create, bool *present)
{
    return initialized_genesis_marker_identity(process, create, present, NULL);
}

static lxp_result verify_bootstrap_genesis(
    lxp_daemon_process *process, const lxp_snapshot_manifest_record *snapshot,
    bool storage_empty, bool initialized)
{
    const char *path = required_environment("LAYERX_NODE_GENESIS_MANIFEST");
    lxp_genesis_manifest *genesis = NULL;
    lxp_genesis_bootstrap_registration registration = {0};
    uint8_t *bytes = NULL;
    bool activities_enabled = false;
    size_t length = 0U;
    size_t mark;
    lxp_result status;
    if (process == NULL || snapshot == NULL || path == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(&process->owner_scratch);
    status = lxp_daemon_artifact_read(
        path, LXP_GENESIS_MAX_ENCODED_BYTES, 0U, &bytes, &length);
    if (status == LXP_OK)
        genesis = (lxp_genesis_manifest *)malloc(sizeof(*genesis));
    if (status == LXP_OK && genesis == NULL) {
        free(bytes);
        free(genesis);
        return LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_genesis_parse(bytes, (size_t)length,
                                   LXP_GENESIS_INPUT_MANIFEST, genesis);
    if (status == LXP_OK) status = load_genesis_registration(&registration);
    if (status == LXP_OK && initialized)
        status = lxp_genesis_initialized_verify(
            genesis, &registration, process->network_id,
            snapshot, &process->kernel, &process->owner_scratch,
            &activities_enabled);
    if (status == LXP_OK && !initialized)
        status = lxp_genesis_bootstrap_verify(
            genesis, &registration, process->network_id, storage_empty,
            snapshot, &process->kernel, &process->owner_scratch,
            &activities_enabled);
    if (status == LXP_OK && !activities_enabled)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        process->bootstrap_sealed_timestamp =
            genesis->genesis_timestamp_ms;
    }
    if (bytes != NULL) lxp_secure_zero(bytes, length);
    if (genesis != NULL) lxp_secure_zero(genesis, sizeof(*genesis));
    lxp_secure_zero(&registration, sizeof(registration));
    free(bytes);
    free(genesis);
    (void)lxp_arena_reset(&process->owner_scratch, mark);
    return status;
}

static lxp_result verify_snapshot_migration(
    lxp_daemon_process *process,
    const lxp_snapshot_manifest_record *snapshot)
{
    const char *path = required_environment("LAYERX_NODE_GENESIS_MANIFEST");
    lxp_genesis_manifest *genesis = NULL;
    uint8_t *bytes = NULL;
    size_t length = 0U;
    size_t mark;
    lxp_result status;
    if (process == NULL || snapshot == NULL ||
        !snapshot->migration.present || path == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(&process->owner_scratch);
    status = lxp_daemon_artifact_read(
        path, LXP_GENESIS_MAX_ENCODED_BYTES, 0U, &bytes, &length);
    if (status == LXP_OK)
        genesis = (lxp_genesis_manifest *)malloc(sizeof(*genesis));
    if (status == LXP_OK && genesis == NULL) status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_genesis_parse(
            bytes, length, LXP_GENESIS_INPUT_MANIFEST, genesis);
    if (status == LXP_OK)
        status = lxp_genesis_verify_signature(
            genesis, &process->owner_scratch);
    if (status == LXP_OK)
        status = lxp_snapshot_migration_authorization_verify(
            snapshot, process->network_id, genesis->signer_public_key);
    if (bytes != NULL) lxp_secure_zero(bytes, length);
    if (genesis != NULL) lxp_secure_zero(genesis, sizeof(*genesis));
    free(bytes);
    free(genesis);
    (void)lxp_arena_reset(&process->owner_scratch, mark);
    return status;
}

static lxp_result load_genesis_settlement_anchor(
    lxp_daemon_process *process, uint8_t settlement_anchor[32],
    lxp_genesis_module_plan *module_plan)
{
    const char *path = required_environment("LAYERX_NODE_GENESIS_MANIFEST");
    lxp_genesis_manifest *genesis = NULL;
    lxp_genesis_bootstrap_registration registration = {0};
    uint8_t *bytes = NULL;
    size_t length = 0U;
    size_t mark;
    lxp_result status;
    if (process == NULL || settlement_anchor == NULL || module_plan == NULL ||
        path == NULL)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(&process->owner_scratch);
    status = lxp_daemon_artifact_read(
        path, LXP_GENESIS_MAX_ENCODED_BYTES, 0U, &bytes, &length);
    if (status == LXP_OK)
        genesis = (lxp_genesis_manifest *)malloc(sizeof(*genesis));
    if (status == LXP_OK && genesis == NULL) {
        free(bytes);
        free(genesis);
        return LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_genesis_parse(bytes, (size_t)length,
                                   LXP_GENESIS_INPUT_MANIFEST, genesis);
    if (status == LXP_OK)
        status = lxp_genesis_verify_signature(genesis,
                                              &process->owner_scratch);
    if (status == LXP_OK)
        status = load_genesis_registration(&registration);
    if (status == LXP_OK &&
        (genesis->network_id != process->network_id ||
         lxp_ct_is_zero(genesis->genesis_receipt_state_root, 32U) ||
         !registration.finalised || registration.registration_index != 0U ||
         registration.network_id != process->network_id ||
         lxp_ct_memcmp(registration.settlement_anchor,
                       genesis->genesis_receipt_state_root, 32U) != 0 ||
         lxp_ct_memcmp(registration.state_root,
                       genesis->genesis_receipt_state_root, 32U) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) {
        process->protocol_version = genesis->protocol_version;
        lxp_bridge_profile profile;
        status = lxp_bridge_genesis_profile(genesis, &profile,
                                             &process->custody_credit_enabled);
        if (status == LXP_OK)
            status = lxp_genesis_module_plan_resolve(genesis, module_plan);
        (void)memcpy(settlement_anchor,
                     genesis->genesis_receipt_state_root, 32U);
    }
    if (bytes != NULL) lxp_secure_zero(bytes, length);
    if (genesis != NULL) lxp_secure_zero(genesis, sizeof(*genesis));
    lxp_secure_zero(&registration, sizeof(registration));
    free(bytes);
    free(genesis);
    (void)lxp_arena_reset(&process->owner_scratch, mark);
    return status;
}

static void close_process(lxp_daemon_process *process)
{
    if (process->lni_started) {
        (void)lxp_daemon_lni_stop(&process->lni);
        process->lni_started = false;
    }
    if (process->daemon_started) {
        process->daemon.protocol_owner = NULL;
        (void)lxp_daemon_shutdown(&process->daemon);
        process->daemon_started = false;
    }
    if (process->owner.listener_started)
        (void)lxp_daemon_protocol_listener_stop(&process->owner);
    (void)postcommit_stop(process);
    if (process->owner.attached)
        (void)lxp_daemon_protocol_owner_detach(&process->owner);
    if (process->history_open) (void)lxp_history_close(&process->history);
    if (process->evidence_open) (void)lxp_log_close(&process->evidence_log);
    if (process->availability_log_open) (void)lxp_log_close(&process->availability_log);
    if (process->batch_open) (void)lxp_log_close(&process->batch_log);
    if (process->authority_open) (void)lxp_log_close(&process->authority_log);
    if (process->canonical_open) (void)lxp_log_close(&process->canonical_log);
    if (process->feed_open) (void)lxp_log_close(&process->feed_log);
    if (process->state_open) (void)lxp_state_store_destroy(&process->state);
    lx_account_registry_release(&process->accounts);
    lxp_secure_zero(process->sequencer_private_key, 32U);
    lxp_secure_zero(process->authority_replica_token,
                    sizeof(process->authority_replica_token));
    free(process->checkpoint_arena_bytes);
    free(process->execution_arena_bytes);
    free(process->availability_scratch_bytes);
    free(process->owner_scratch_bytes);
}

static lxp_result open_process(lxp_daemon_process *process,
                               const char *configuration_path,
                               lxp_daemon_configuration *configuration,
                               const char **listener_address,
                               uint16_t *listener_port,
                               lxp_daemon_lni_configuration *lni_configuration)
{
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    lxp_arena snapshot_arena;
    lxp_daemon_batch_wal_record *startup_wal = NULL;
    uint8_t *snapshot_bytes;
    uint8_t genesis_settlement_anchor[32];
    char snapshot_path[4096];
    bool checkpoint_selected = false;
    bool pending_wal_present = false;
    bool initial_storage_empty = false;
    bool initialized = false;
    uint64_t pending_wal_last_sequence = 0U;
    uint64_t value;
    const char *bearer;
    const char *replica_token;
    const char *stage = "configuration";
    lxp_result status;
    (void)memset(process, 0, sizeof(*process));
    process->owner_scratch_bytes =
        (uint8_t *)malloc(LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    process->availability_scratch_bytes =
        (uint8_t *)malloc(LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES);
    process->execution_arena_bytes =
        (uint8_t *)malloc(NODE_EXECUTION_ARENA_BYTES);
    process->checkpoint_arena_bytes =
        (uint8_t *)malloc(NODE_SNAPSHOT_ARENA_BYTES);
    snapshot_bytes = (uint8_t *)malloc(NODE_SNAPSHOT_ARENA_BYTES);
    if (process->owner_scratch_bytes == NULL ||
        process->availability_scratch_bytes == NULL ||
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
        status = lxp_arena_init(&process->availability_scratch,
            process->availability_scratch_bytes,
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
    if (status == LXP_OK) {
        lxp_genesis_module_plan genesis_module_plan;
        status = load_genesis_settlement_anchor(
            process, genesis_settlement_anchor, &genesis_module_plan);
        if (status == LXP_OK)
            status = lxp_genesis_module_plan_register(&genesis_module_plan,
                                                      &process->kernel);
    }
    if (status == LXP_OK)
        status = lxp_kernel_set_capabilities(
            &process->kernel, NULL, lxp_kernel_canonical_ledger_apply);
    process->checkpoint_directory = required_environment(
        "LAYERX_NODE_CHECKPOINT_DIRECTORY");
    if (status == LXP_OK)
        status = decode_hex(
            required_environment("LAYERX_NODE_SEQUENCER_ID"),
            process->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = decode_hex(
            required_environment("LAYERX_NODE_SEQUENCER_PUBLIC_KEY"),
            process->sequencer_authorization.public_key, 32U);
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_FIRST_BATCH"), &value);
    if (status == LXP_OK)
        process->sequencer_authorization.first_batch_number = value;
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_LAST_BATCH"), &value);
    if (status == LXP_OK)
        process->sequencer_authorization.last_batch_number = value;
    process->sequencer_authorization.authorized = 1U;
    if (status == LXP_OK)
        status = latest_snapshot_path(
            process->checkpoint_directory,
            required_environment("LAYERX_NODE_SNAPSHOT"), snapshot_path,
            0U, &checkpoint_selected);
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_load(
            process->checkpoint_directory,
            &process->sequencer_authorization, &startup_wal,
            &pending_wal_present);
    if (status == LXP_OK && pending_wal_present) {
        const lxp_daemon_batch_wal_input *startup_view =
            lxp_daemon_batch_wal_view(startup_wal);
        if (startup_view == NULL ||
            lxp_daemon_batch_wal_record_state(startup_wal) !=
                LXP_DAEMON_BATCH_WAL_PREPARED)
            status = LXP_ERR_LOG_CORRUPT;
        else
            pending_wal_last_sequence = startup_view->last_sequence;
    }
    lxp_daemon_batch_wal_destroy(startup_wal);
    startup_wal = NULL;
    if (status == LXP_OK)
        lxp_log_set_prepared_recovery(pending_wal_present);
    if (status == LXP_OK && checkpoint_selected && pending_wal_present) {
        const char *name = strrchr(snapshot_path, '/');
        uint64_t selected_sequence = 0U;
        name = name == NULL ? snapshot_path : name + 1U;
        if (!checkpoint_name(name, &selected_sequence))
            status = LXP_ERR_SNAPSHOT_MISMATCH;
        else if (selected_sequence == pending_wal_last_sequence) {
            status = remove_pending_checkpoint_pair(
                process->checkpoint_directory, pending_wal_last_sequence);
            if (status == LXP_OK)
                status = latest_snapshot_path(
                    process->checkpoint_directory,
                    required_environment("LAYERX_NODE_SNAPSHOT"),
                    snapshot_path, pending_wal_last_sequence,
                    &checkpoint_selected);
        }
    }
    if (status == LXP_OK) process->checkpoint_selected = checkpoint_selected;
    if (status == LXP_OK && !checkpoint_selected)
        status = initialized_genesis_marker(process, false, &initialized);
    if (status == LXP_OK && !checkpoint_selected && !initialized) {
        status = bootstrap_storage_empty(process->checkpoint_directory);
        initial_storage_empty = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(snapshot_path, &snapshot_arena,
                                         &manifest, &snapshot);
    if (status == LXP_OK && checkpoint_selected &&
        manifest.migration.present)
        status = verify_snapshot_migration(process, &manifest);
    if (status == LXP_OK)
        status = lxp_snapshot_load(snapshot.bytes, snapshot.length,
                                   &manifest, &process->kernel);
    if (status == LXP_OK && !checkpoint_selected)
        status = verify_bootstrap_genesis(process, &manifest,
                                          initial_storage_empty, initialized);
    free(snapshot_bytes);
    if (status == LXP_OK &&
        configuration->start_sequence > process->state.next_sequence)
        status = LXP_ERR_SEQUENCE_GAP;
    if (status == LXP_OK)
        configuration->start_sequence = process->state.next_sequence;
    if (status == LXP_OK) status = collect_assets(process);
    if (status == LXP_OK &&
        process->protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT) {
        process->asset_runtime = (lx_asset_runtime){
            &process->accounts, process->send_assets, process->asset_count,
            process->assets, process->asset_count, process->network_id,
            process->protocol_version};
        status = lxp_kernel_bind_module_runtime(
            &process->kernel, LXP_MODULE_ASSET, &process->asset_runtime);
    }
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
    if (status == LXP_OK && initial_storage_empty)
        status = initialized_genesis_marker(process, true, &initialized);
    if (status == LXP_OK) {
        char directory[LXP_DA_STORE_PATH_BYTES];
        const char *retain = getenv("LAYERX_NODE_DA_RETAIN_BATCHES");
        int length = snprintf(directory, sizeof(directory), "%s/da", process->checkpoint_directory);
        process->availability_retain_batches = 100000U;
        if (retain != NULL)
            status = parse_u64_text(retain, &process->availability_retain_batches);
        if (status == LXP_OK && process->availability_retain_batches < 1024U)
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK && (length < 0 || (size_t)length >= sizeof(directory)))
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK && mkdir(directory, 0700) != 0 && errno != EEXIST)
            status = LXP_ERR_IO;
        if (status == LXP_OK)
            status = lxp_da_store_init(&process->availability_store, directory);
    }
    if (status == LXP_OK) stage = "logs";
    if (status == LXP_OK) status = open_log(
        &process->feed_log, "LAYERX_NODE_PROGRAM_FEED_LOG",
        &process->feed_open);
    if (status == LXP_OK) status = open_log(
        &process->canonical_log, "LAYERX_NODE_CANONICAL_LOG",
        &process->canonical_open);
    if (status == LXP_OK) {
        if (lxp_protocol_version_uses_occupancy(process->protocol_version))
            status = lxp_log_recover_complete_records(&process->canonical_log, NULL, NULL);
        else
            status = lxp_log_recover(&process->canonical_log, NULL, NULL);
    }
    if (status == LXP_OK) status = open_log(
        &process->authority_log, "LAYERX_NODE_RECEIPT_AUTHORITY_LOG",
        &process->authority_open);
    if (status == LXP_OK) status = open_log(
        &process->batch_log, "LAYERX_NODE_BATCH_LOG", &process->batch_open);
    if (status == LXP_OK)
        status = lxp_log_recover_complete_records(
            &process->batch_log, NULL, NULL);
    if (status == LXP_OK) {
        char path[LXP_DA_STORE_PATH_BYTES];
        int length = snprintf(path, sizeof(path), "%s/da-bodies.log", process->checkpoint_directory);
        if (length < 0 || (size_t)length >= sizeof(path)) status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            status = lxp_log_open_or_create(&process->availability_log, path,
                                            process->batch_log.capacity);
        if (status == LXP_OK) {
            process->availability_log_open = true;
            status = lxp_log_recover_complete_records(&process->availability_log, NULL, NULL);
        }
    }
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
    if (status == LXP_OK) stage = "batch wal";
    if (status == LXP_OK)
        status = lxp_daemon_batch_wal_initialize(
            process->checkpoint_directory);
    if (status == LXP_OK)
        status = lxp_daemon_receipt_authority_open(
            &process->receipt_authority, &process->authority_log,
            &process->sequencer_authorization);
    if (status == LXP_OK) status = open_log(
        &process->evidence_log, "LAYERX_NODE_EVIDENCE_LOG",
        &process->evidence_open);
    if (status == LXP_OK) {
        lxp_log *logs[5] = {
            &process->feed_log, &process->canonical_log,
            &process->authority_log, &process->batch_log,
            &process->evidence_log};
        status = require_distinct_logs(logs, 5U);
    }
    if (status == LXP_OK) stage = "finality authority";
    if (status == LXP_OK)
        status = lxp_daemon_finality_authority_init(
            &process->finality_authority, &process->evidence_store);
    if (status == LXP_OK)
        status = lxp_daemon_evidence_open(
            &process->evidence_store, &process->evidence_log,
            process->network_id, &process->sequencer_authorization,
            genesis_settlement_anchor, true,
            lxp_daemon_finality_authority_verify,
            &process->finality_authority, &process->owner_scratch);
    if (status == LXP_OK)
        process->evidence_store.availability_log = &process->availability_log;
    if (status == LXP_OK &&
        process->evidence_store.verify_finality_authority == NULL)
        status = LXP_ERR_MODULE_DISABLED;
    if (status == LXP_OK) stage = "history";
    if (status == LXP_OK)
        status = lxp_history_open(
            &process->history, &process->canonical_log,
            required_environment("LAYERX_NODE_HISTORY_DATABASE"),
            required_environment("LAYERX_NODE_HISTORY_MIGRATIONS"));
    process->history_open = status == LXP_OK;
    if (status == LXP_OK)
        status = lxp_verified_receipt_index_init(
            &process->verified_receipts);
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
    if (status == LXP_OK) stage = "metering";
    if (status == LXP_OK) {
        lx_programs_metering_schedule metering_schedule;
        status = lxp_programs_metering_schedule_current(
            &process->kernel,
            process->next_batch != 0U ? process->next_batch :
                process->sequencer_authorization.last_batch_number,
            &metering_schedule);
    }
    if (status == LXP_OK) stage = "fee schedule";
    if (status == LXP_OK) {
        lx_programs_fee_schedule fee_schedule;
        uint8_t occupancy_asset_id[32];
        status = lxp_programs_fee_governance_resolve_runtime(
            &process->kernel, 0U, &fee_schedule, occupancy_asset_id);
    }
    if (status == LXP_OK) stage = "replica recovery";
    if (status == LXP_OK) status = replicate_authority_history(process);
    bearer = required_environment("LAYERX_NODE_PROGRAM_BEARER_TOKEN");
    if (status == LXP_OK) stage = "protocol owner";
    if (status == LXP_OK)
        status = lxp_daemon_protocol_owner_attach(
            &process->owner, &process->kernel, &process->identities,
            process->network_id, process->bootstrap_sealed_timestamp,
            &process->programs,
            &process->feed_log, &process->canonical_log, &process->history,
            &process->verified_receipts, &process->receipt_authority,
            &process->owner_scratch, replay_canonical_after_snapshot,
            process, (const uint8_t *)bearer,
            bearer == NULL ? 0U : strlen(bearer));
    if (status == LXP_OK) process->owner.protocol_version = process->protocol_version;
    if (status == LXP_OK) stage = "evidence binding";
    if (status == LXP_OK)
        status = lxp_daemon_protocol_owner_bind_evidence(
            &process->owner, &process->evidence_store);
    if (status == LXP_OK) availability_verify_retained(process);
    if (status == LXP_OK) stage = "protocol listener configuration";
    if (status == LXP_OK)
        status = parse_u64_text(
            required_environment("LAYERX_NODE_PROGRAM_PORT"), &value);
    if (status == LXP_OK && (value == 0U || value > UINT16_MAX))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) *listener_port = (uint16_t)value;
    *listener_address = required_environment("LAYERX_NODE_PROGRAM_ADDRESS");
    if (status == LXP_OK &&
        (*listener_address == NULL ||
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
    if (status == LXP_OK) stage = "LNI configuration";
    if (status == LXP_OK) {
        const char *lni_socket = required_environment("LAYERX_NODE_LNI_SOCKET");
        const char *admission_directory = required_environment(
            "LAYERX_NODE_CHECKPOINT_DIRECTORY");
        (void)memset(lni_configuration, 0, sizeof(*lni_configuration));
        lni_configuration->socket_path = lni_socket;
        lni_configuration->admission_directory = admission_directory;
        lni_configuration->socket_mode = 0660U;
        status = parse_u64_text(
            required_environment("LAYERX_NODE_LNI_ALLOWED_UID"), &value);
        if (status == LXP_OK && value > UINT32_MAX)
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK)
            lni_configuration->allowed_peer_uid = (uint32_t)value;
        if (status == LXP_OK)
            status = parse_u64_text(
                required_environment("LAYERX_NODE_LNI_ALLOWED_GID"), &value);
        if (status == LXP_OK && value > UINT32_MAX)
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK)
            lni_configuration->allowed_peer_gid = (uint32_t)value;
        if (status == LXP_OK)
            status = parse_u64_text(
                required_environment("LAYERX_NODE_LNI_FRAME_BYTES"), &value);
        if (status == LXP_OK && value != LXP_DAEMON_LNI_MAX_FRAME_BYTES)
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            lni_configuration->frame_bytes = (uint32_t)value;
        if (status == LXP_OK)
            status = parse_u64_text(
                required_environment("LAYERX_NODE_LNI_DEADLINE_MS"), &value);
        if (status == LXP_OK && (value == 0U || value > 60000U))
            status = LXP_ERR_LENGTH_LIMIT;
        if (status == LXP_OK)
            lni_configuration->deadline_milliseconds = (uint32_t)value;
        if (status == LXP_OK &&
            (lni_socket == NULL || admission_directory == NULL))
            status = LXP_ERR_NON_CANONICAL;
    }
    lxp_log_set_prepared_recovery(false);
    if (status != LXP_OK)
        (void)fprintf(stderr, "layerxd: bootstrap %s failed with result %d\n",
                      stage, (int)status);
    return status;
}

lxp_result lxp_daemon_serve(const char *configuration_path)
{
    lxp_daemon_process *process;
    lxp_daemon_configuration configuration;
    const char *listener_address = NULL;
    uint16_t listener_port = 0U;
    lxp_daemon_lni_configuration lni_configuration;
    lxp_result status;
    process = (lxp_daemon_process *)calloc(1U, sizeof(*process));
    if (process == NULL) return LXP_ERR_IO;
    status = open_process(process, configuration_path, &configuration,
                          &listener_address, &listener_port,
                          &lni_configuration);
    if (status == LXP_OK) {
        status = postcommit_start(process);
        if (status != LXP_OK)
            (void)fprintf(stderr,
                          "layerxd: post-commit start failed with result %d\n",
                          (int)status);
    }
    if (status == LXP_OK) {
        status = lxp_daemon_start_protocol_batch(
            &process->daemon, &configuration, apply_canonical_batch,
            process, &process->owner, listener_address, listener_port);
        if (status != LXP_OK)
            (void)fprintf(stderr, "layerxd: protocol start failed with result %d\n", (int)status);
    }
    process->daemon_started = status == LXP_OK;
    if (status == LXP_OK) {
        status = lxp_daemon_lni_serve(
            &process->lni, &process->daemon, &process->owner,
            &lni_configuration);
        if (status != LXP_OK)
            (void)fprintf(stderr, "layerxd: LNI start failed with result %d\n", (int)status);
    }
    process->lni_started = status == LXP_OK;
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
        struct timespec interval = {0, 100000000L};
        status = admit_provisioned_identities(process);
        if (status == LXP_OK) status = lxp_daemon_lni_status(&process->lni);
        if (status == LXP_OK && nanosleep(&interval, NULL) != 0 &&
            errno != EINTR)
            status = LXP_ERR_IO;
    }
    close_process(process);
    free(process);
    return status;
}
