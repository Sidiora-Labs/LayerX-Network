#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include "layerx/lxp_activity.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_receipt.h"

#include "lxp_daemon_lni_internal.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/file.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

enum {
    LNI_VERSION_MAJOR = 1,
    LNI_VERSION_MINOR = 3,
    LNI_NODE_INFO_REQUEST = 1,
    LNI_NODE_INFO_RESPONSE = 2,
    LNI_SUBMIT_REQUEST = 3,
    LNI_SUBMIT_RESPONSE = 4,
    LNI_RECEIPT_LOOKUP_REQUEST = 5,
    LNI_RECEIPT_LOOKUP_RESPONSE = 6,
    LNI_ACCOUNT_READ_REQUEST = 7,
    LNI_ACCOUNT_READ_RESPONSE = 8,
    LNI_BATCH_HEADER_REQUEST = 12,
    LNI_BATCH_HEADER_RESPONSE = 13,
    LNI_CHECKPOINT_REQUEST = 14,
    LNI_CHECKPOINT_RESPONSE = 15,
    LNI_PROOF_BUNDLE_REQUEST = 16,
    LNI_PROOF_BUNDLE_RESPONSE = 17,
    LNI_ERROR_RESPONSE = 25,
    LNI_PREPARATION_STATE_REQUEST = 26,
    LNI_PREPARATION_STATE_RESPONSE = 27,
    LNI_FINALITY_EVIDENCE_REGISTER_REQUEST = 28,
    LNI_FINALITY_EVIDENCE_REGISTER_RESPONSE = 29,
    LNI_ENVELOPE_FIXED_BYTES = 22,
    LNI_NODE_INFO_FIXED_BYTES = 93,
    LNI_PREPARATION_STATE_MAX_BYTES = 4096,
    LNI_BACKLOG = 16,
    LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES = 32,
    LNI_ADMISSION_JOURNAL_RECORD_BYTES = 64,
    LNI_ADMISSION_JOURNAL_VERSION = 1
};

static const char LNI_LIFETIME_LOCK_NAME[] = ".layerxd-lni.lock";
static const char LNI_ADMISSION_JOURNAL_NAME[] =
    ".layerxd-lni-admission.log";
static const char LNI_ADMISSION_JOURNAL_TEMP_NAME[] =
    ".layerxd-lni-admission.tmp";
static const uint32_t LNI_ADMISSION_JOURNAL_MAGIC = UINT32_C(0x4c58414a);
static const uint32_t LNI_ADMISSION_RECORD_MAGIC = UINT32_C(0x4c584152);

typedef struct lni_envelope {
    uint16_t major;
    uint16_t minor;
    uint16_t tag;
    uint64_t correlation_id;
    const uint8_t *payload;
    size_t payload_length;
    const uint8_t *proof;
    size_t proof_length;
} lni_envelope;

static uint16_t load_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t load_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t load_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static void store_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void store_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void store_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static uint64_t admission_journal_max_bytes(void)
{
    return (uint64_t)LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES +
        (uint64_t)LXP_DAEMON_QUEUE_MAX_BYTES +
        (uint64_t)LXP_DAEMON_QUEUE_CAPACITY *
            (uint64_t)LNI_ADMISSION_JOURNAL_RECORD_BYTES;
}

static lxp_result file_read_exact(int descriptor, uint8_t *bytes,
                                  size_t length, uint64_t offset)
{
    size_t consumed = 0U;
    while (consumed < length) {
        ssize_t result = pread(descriptor, bytes + consumed,
                               length - consumed,
                               (off_t)(offset + consumed));
        if (result < 0 && errno == EINTR) continue;
        if (result <= 0) return LXP_ERR_LOG_TRUNCATED;
        consumed += (size_t)result;
    }
    return LXP_OK;
}

static lxp_result file_write_exact(int descriptor, const uint8_t *bytes,
                                   size_t length, uint64_t offset)
{
    size_t written = 0U;
    while (written < length) {
        ssize_t result = pwrite(descriptor, bytes + written,
                                length - written,
                                (off_t)(offset + written));
        if (result < 0 && errno == EINTR) continue;
        if (result <= 0) return LXP_ERR_IO;
        written += (size_t)result;
    }
    return LXP_OK;
}

static void admission_superblock_encode(uint32_t network_id,
    uint8_t bytes[LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES])
{
    (void)memset(bytes, 0, LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES);
    store_u32(bytes, LNI_ADMISSION_JOURNAL_MAGIC);
    store_u16(bytes + 4U, LNI_ADMISSION_JOURNAL_VERSION);
    store_u16(bytes + 6U, LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES);
    store_u32(bytes + 8U, network_id);
    store_u32(bytes + 28U, lxp_log_crc32c(bytes, 28U));
}

static bool admission_superblock_valid(const uint8_t *bytes,
                                       uint32_t network_id)
{
    size_t index;
    if (load_u32(bytes) != LNI_ADMISSION_JOURNAL_MAGIC ||
        load_u16(bytes + 4U) != LNI_ADMISSION_JOURNAL_VERSION ||
        load_u16(bytes + 6U) != LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES ||
        load_u32(bytes + 8U) != network_id ||
        load_u32(bytes + 28U) != lxp_log_crc32c(bytes, 28U))
        return false;
    for (index = 12U; index < 28U; ++index)
        if (bytes[index] != 0U) return false;
    return true;
}

static void admission_record_encode(
    uint64_t global_sequence, const uint8_t activity_id[32],
    const uint8_t *activity, size_t activity_length,
    uint8_t bytes[LNI_ADMISSION_JOURNAL_RECORD_BYTES])
{
    (void)memset(bytes, 0, LNI_ADMISSION_JOURNAL_RECORD_BYTES);
    store_u32(bytes, LNI_ADMISSION_RECORD_MAGIC);
    store_u16(bytes + 4U, LNI_ADMISSION_JOURNAL_VERSION);
    store_u16(bytes + 6U, LNI_ADMISSION_JOURNAL_RECORD_BYTES);
    store_u64(bytes + 8U, global_sequence);
    store_u32(bytes + 16U, (uint32_t)activity_length);
    store_u32(bytes + 20U, lxp_log_crc32c(activity, activity_length));
    (void)memcpy(bytes + 24U, activity_id, 32U);
    store_u32(bytes + 56U, lxp_log_crc32c(bytes, 56U));
}

static bool admission_record_header_valid(const uint8_t *bytes)
{
    return load_u32(bytes) == LNI_ADMISSION_RECORD_MAGIC &&
        load_u16(bytes + 4U) == LNI_ADMISSION_JOURNAL_VERSION &&
        load_u16(bytes + 6U) == LNI_ADMISSION_JOURNAL_RECORD_BYTES &&
        load_u32(bytes + 16U) != 0U &&
        load_u32(bytes + 16U) <= LXP_MAX_ACTIVITY_BYTES &&
        load_u32(bytes + 56U) == lxp_log_crc32c(bytes, 56U) &&
        load_u32(bytes + 60U) == 0U;
}

static bool admission_journal_named(
    const lxp_daemon_lni_server *server, int descriptor,
    uint64_t expected_device, uint64_t expected_inode)
{
    struct stat opened;
    struct stat named;
    struct stat parent;
    struct stat parent_named;
    return descriptor >= 0 && server->admission_parent_descriptor >= 0 &&
        fstat(server->admission_parent_descriptor, &parent) == 0 &&
        lstat(server->admission_directory, &parent_named) == 0 &&
        S_ISDIR(parent.st_mode) && S_ISDIR(parent_named.st_mode) &&
        parent.st_uid == geteuid() && parent_named.st_uid == geteuid() &&
        (parent.st_mode & 0022U) == 0U &&
        (parent_named.st_mode & 0022U) == 0U &&
        parent.st_dev == parent_named.st_dev &&
        parent.st_ino == parent_named.st_ino &&
        (uint64_t)parent.st_dev == server->admission_parent_device &&
        (uint64_t)parent.st_ino == server->admission_parent_inode &&
        fstat(descriptor, &opened) == 0 &&
        fstatat(server->admission_parent_descriptor,
                LNI_ADMISSION_JOURNAL_NAME,
                &named, AT_SYMLINK_NOFOLLOW) == 0 &&
        S_ISREG(opened.st_mode) && S_ISREG(named.st_mode) &&
        opened.st_nlink == 1 && named.st_nlink == 1 &&
        opened.st_uid == geteuid() && named.st_uid == geteuid() &&
        (opened.st_mode & 0777U) == 0600U &&
        (named.st_mode & 0777U) == 0600U &&
        opened.st_dev == named.st_dev && opened.st_ino == named.st_ino &&
        (uint64_t)opened.st_dev == expected_device &&
        (uint64_t)opened.st_ino == expected_inode;
}

static lxp_result admission_journal_create(
    lxp_daemon_lni_server *server, int *descriptor)
{
    uint8_t superblock[LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES];
    struct stat metadata;
    int opened = openat(server->admission_parent_descriptor,
                        LNI_ADMISSION_JOURNAL_NAME,
                        O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC | O_NOFOLLOW,
                        0600);
    bool created = opened >= 0;
    lxp_result status = opened < 0 ? LXP_ERR_IO : LXP_OK;
    admission_superblock_encode(server->daemon->config.network_id,
                                superblock);
    if (status == LXP_OK)
        status = file_write_exact(opened, superblock, sizeof(superblock), 0U);
    if (status == LXP_OK && fdatasync(opened) != 0) status = LXP_ERR_IO;
    if (status == LXP_OK && fsync(server->admission_parent_descriptor) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK &&
        (fstat(opened, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
         metadata.st_nlink != 1 || metadata.st_uid != geteuid() ||
         (metadata.st_mode & 0777U) != 0600U))
        status = LXP_ERR_AUTH_SCOPE;
    if (status != LXP_OK) {
        if (opened >= 0) (void)close(opened);
        if (created)
            (void)unlinkat(server->admission_parent_descriptor,
                           LNI_ADMISSION_JOURNAL_NAME, 0);
        return status;
    }
    *descriptor = opened;
    return LXP_OK;
}

static lxp_result admission_journal_open(lxp_daemon_lni_server *server)
{
    uint8_t superblock[LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES];
    struct stat metadata;
    int descriptor = openat(server->admission_parent_descriptor,
                            LNI_ADMISSION_JOURNAL_NAME,
                            O_RDWR | O_CLOEXEC | O_NOFOLLOW);
    lxp_result status;
    if (descriptor < 0 && errno == ENOENT)
        status = admission_journal_create(server, &descriptor);
    else
        status = descriptor < 0 ? LXP_ERR_IO : LXP_OK;
    if (status == LXP_OK &&
        (fstat(descriptor, &metadata) != 0 || metadata.st_size < 0 ||
         (uint64_t)metadata.st_size > admission_journal_max_bytes() ||
         !S_ISREG(metadata.st_mode) || metadata.st_nlink != 1 ||
         metadata.st_uid != geteuid() ||
         (metadata.st_mode & 0777U) != 0600U))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = file_read_exact(descriptor, superblock,
                                 sizeof(superblock), 0U);
    if (status == LXP_OK &&
        !admission_superblock_valid(
            superblock, server->daemon->config.network_id))
        status = LXP_ERR_LOG_CORRUPT;
    if (status != LXP_OK) {
        if (descriptor >= 0) (void)close(descriptor);
        return status;
    }
    server->journal_descriptor = descriptor;
    server->journal_device = (uint64_t)metadata.st_dev;
    server->journal_inode = (uint64_t)metadata.st_ino;
    server->journal_end = (uint64_t)metadata.st_size;
    return admission_journal_named(
        server, descriptor, server->journal_device, server->journal_inode) ?
        LXP_OK : LXP_ERR_AUTH_SCOPE;
}

static void recovered_admissions_release(lxp_daemon_activity *activities,
                                         size_t count)
{
    size_t index;
    if (activities == NULL) return;
    for (index = 0U; index < count; ++index) {
        if (activities[index].bytes != NULL) {
            lxp_secure_zero(activities[index].bytes,
                            activities[index].length);
            free(activities[index].bytes);
        }
    }
    free(activities);
}

static lxp_result completed_activity_matches(
    lxp_daemon_protocol_owner *owner, uint64_t global_sequence,
    const uint8_t activity_id[32])
{
    lxp_receipt_query query;
    lxp_byte_span canonical_receipt = {NULL, 0U};
    lxp_receipt receipt;
    size_t mark;
    lxp_result status;
    if (pthread_mutex_lock(&owner->mutex) != 0) return LXP_ERR_IO;
    (void)memset(&query, 0, sizeof(query));
    query.kind = LXP_RECEIPT_BY_GLOBAL_SEQUENCE;
    query.global_sequence = global_sequence;
    query.maximum_response_bytes = LXP_MAX_ACTIVITY_BYTES;
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_receipt_lookup(owner->history, &query, owner->scratch,
                                &canonical_receipt);
    if (status == LXP_OK)
        status = lxp_receipt_decode(canonical_receipt.bytes,
                                    canonical_receipt.length, true,
                                    &receipt);
    if (status == LXP_OK &&
        (receipt.global_sequence != global_sequence ||
         lxp_ct_memcmp(receipt.activity_id, activity_id, 32U) != 0))
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_ERR_UNKNOWN_ACTIVITY)
        status = LXP_ERR_LOG_CORRUPT;
    (void)lxp_arena_reset(owner->scratch, mark);
    if (pthread_mutex_unlock(&owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result admission_journal_recover(
    lxp_daemon_lni_server *server)
{
    lxp_daemon_activity *recovered = calloc(
        LXP_DAEMON_QUEUE_CAPACITY, sizeof(*recovered));
    lxp_daemon_lni_journal_entry *recovered_entries = calloc(
        LXP_DAEMON_QUEUE_CAPACITY, sizeof(*recovered_entries));
    uint64_t offset = LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES;
    uint64_t valid_end = LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES;
    uint64_t previous_sequence = 0U;
    uint64_t floor;
    size_t recovered_count = 0U;
    size_t recovered_bytes = 0U;
    bool have_previous = false;
    bool incomplete_tail = false;
    lxp_result status = recovered == NULL || recovered_entries == NULL ?
        LXP_ERR_ARENA_EXHAUSTED : LXP_OK;
    if (status != LXP_OK) {
        free(recovered);
        free(recovered_entries);
        return status;
    }
    if (pthread_mutex_lock(&server->owner->mutex) != 0) {
        free(recovered);
        free(recovered_entries);
        return LXP_ERR_IO;
    }
    if (!server->owner->feed_store.baseline_present ||
        server->owner->feed_store.baseline_next_sequence == 0U ||
        server->owner->feed_store.scanned_through_sequence == UINT64_MAX)
        status = LXP_ERR_PROJECTION_STALE;
    floor = server->owner->feed_store.scanned_through_sequence == 0U ?
        server->owner->feed_store.baseline_next_sequence :
        server->owner->feed_store.scanned_through_sequence + 1U;
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK && pthread_mutex_lock(&server->daemon->mutex) != 0) {
        free(recovered);
        free(recovered_entries);
        return LXP_ERR_IO;
    }
    if (status == LXP_OK) {
        if (server->daemon->queue_count != 0U ||
            server->daemon->next_sequence != floor)
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    while (status == LXP_OK && offset < server->journal_end) {
        uint8_t header[LNI_ADMISSION_JOURNAL_RECORD_BYTES];
        uint8_t computed_id[32];
        uint8_t *activity;
        uint64_t sequence;
        uint32_t length;
        lxp_activity decoded;
        if (server->journal_end - offset < sizeof(header)) {
            incomplete_tail = true;
            break;
        }
        status = file_read_exact(server->journal_descriptor, header,
                                 sizeof(header), offset);
        if (status != LXP_OK) break;
        if (!admission_record_header_valid(header)) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        sequence = load_u64(header + 8U);
        length = load_u32(header + 16U);
        if ((uint64_t)length > server->journal_end - offset -
                sizeof(header)) {
            incomplete_tail = true;
            break;
        }
        if (have_previous &&
            (previous_sequence == UINT64_MAX ||
             sequence != previous_sequence + 1U)) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        activity = (uint8_t *)malloc(length);
        if (activity == NULL) {
            status = LXP_ERR_ARENA_EXHAUSTED;
            break;
        }
        status = file_read_exact(server->journal_descriptor, activity,
                                 length, offset + sizeof(header));
        if (status == LXP_OK &&
            lxp_log_crc32c(activity, length) != load_u32(header + 20U))
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_activity_id(activity, length, computed_id) != LXP_OK)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_ct_memcmp(computed_id, header + 24U, 32U) != 0)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_activity_decode(activity, length, &decoded) != LXP_OK)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_activity_check_envelope(
                &decoded, server->daemon->config.network_id) != LXP_OK)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK && decoded.protocol_version != server->owner->protocol_version)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_activity_verify_payload_hash(&decoded) != LXP_OK)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            lxp_activity_verify_signature(&decoded) != LXP_OK)
            status = LXP_ERR_LOG_CORRUPT;
        if (status == LXP_OK &&
            decoded.protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT &&
            decoded.activity_type == LX_ASSET_SEND) {
            lxp_send send;
            if (lxp_send_decode(decoded.payload.bytes, decoded.payload.length,
                                 &send) != LXP_OK)
                status = LXP_ERR_LOG_CORRUPT;
        }
        if (status != LXP_OK) {
            lxp_secure_zero(activity, length);
            free(activity);
            break;
        }
        if (sequence >= floor) {
            size_t prior;
            if (recovered_count == LXP_DAEMON_QUEUE_CAPACITY ||
                length > LXP_DAEMON_QUEUE_MAX_BYTES - recovered_bytes ||
                sequence != floor + recovered_count) {
                lxp_secure_zero(activity, length);
                free(activity);
                status = LXP_ERR_LOG_CORRUPT;
                break;
            }
            for (prior = 0U; prior < recovered_count; ++prior)
                if (lxp_ct_memcmp(recovered[prior].activity_id,
                                  computed_id, 32U) == 0)
                    status = LXP_ERR_LOG_CORRUPT;
            if (status != LXP_OK) {
                lxp_secure_zero(activity, length);
                free(activity);
                break;
            }
            recovered[recovered_count].bytes = activity;
            recovered[recovered_count].length = length;
            (void)memcpy(recovered[recovered_count].activity_id,
                         computed_id, 32U);
            recovered[recovered_count].global_sequence = sequence;
            recovered[recovered_count].durable_admission = true;
            recovered_entries[recovered_count].global_sequence = sequence;
            recovered_entries[recovered_count].file_offset = offset;
            recovered_entries[recovered_count].activity_length = length;
            (void)memcpy(recovered_entries[recovered_count].activity_id,
                         computed_id, 32U);
            ++recovered_count;
            recovered_bytes += length;
        } else {
            status = completed_activity_matches(server->owner, sequence,
                                                computed_id);
            lxp_secure_zero(activity, length);
            free(activity);
            if (status != LXP_OK) break;
        }
        have_previous = true;
        previous_sequence = sequence;
        offset += sizeof(header) + length;
        valid_end = offset;
    }
    if (status == LXP_OK && incomplete_tail) {
        if (ftruncate(server->journal_descriptor, (off_t)valid_end) != 0 ||
            fdatasync(server->journal_descriptor) != 0)
            status = LXP_ERR_IO;
        else
            server->journal_end = valid_end;
    }
    if (status == LXP_OK && pthread_mutex_lock(&server->daemon->mutex) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        size_t index;
        if (server->daemon->queue_count != 0U ||
            server->daemon->next_sequence != floor)
            status = LXP_ERR_CONTEXT_MISMATCH;
        for (index = 0U; status == LXP_OK && index < recovered_count;
             ++index) {
            server->daemon->queue[index] = recovered[index];
            recovered[index].bytes = NULL;
            server->journal_entries[index] = recovered_entries[index];
        }
        if (status == LXP_OK) {
            server->daemon->queue_head = 0U;
            server->daemon->queue_count = recovered_count;
            server->daemon->queue_bytes = recovered_bytes;
            server->journal_entry_count = recovered_count;
            if (recovered_count != 0U)
                (void)pthread_cond_signal(&server->daemon->queue_changed);
        }
        if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    recovered_admissions_release(recovered, recovered_count);
    free(recovered_entries);
    return status;
}

static lxp_result admission_temp_remove(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    if (fstatat(server->admission_parent_descriptor,
                LNI_ADMISSION_JOURNAL_TEMP_NAME, &metadata,
                AT_SYMLINK_NOFOLLOW) != 0)
        return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
    if (!S_ISREG(metadata.st_mode) || metadata.st_nlink != 1 ||
        metadata.st_uid != geteuid() ||
        (metadata.st_mode & 0777U) != 0600U)
        return LXP_ERR_AUTH_SCOPE;
    return unlinkat(server->admission_parent_descriptor,
                    LNI_ADMISSION_JOURNAL_TEMP_NAME, 0) == 0 ?
        LXP_OK : LXP_ERR_IO;
}

static lxp_result admission_journal_compact_locked(
    lxp_daemon_lni_server *server)
{
    uint8_t superblock[LNI_ADMISSION_JOURNAL_SUPERBLOCK_BYTES];
    lxp_daemon_lni_journal_entry rebuilt[LXP_DAEMON_QUEUE_CAPACITY];
    struct stat metadata;
    uint64_t offset = sizeof(superblock);
    size_t index;
    int descriptor = -1;
    bool renamed = false;
    lxp_result status = admission_temp_remove(server);
    if (status == LXP_OK && !admission_journal_named(
            server, server->journal_descriptor,
            server->journal_device, server->journal_inode))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK) {
        descriptor = openat(server->admission_parent_descriptor,
                            LNI_ADMISSION_JOURNAL_TEMP_NAME,
                            O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC |
                                O_NOFOLLOW,
                            0600);
        if (descriptor < 0) status = LXP_ERR_IO;
    }
    admission_superblock_encode(server->daemon->config.network_id,
                                superblock);
    if (status == LXP_OK)
        status = file_write_exact(descriptor, superblock,
                                  sizeof(superblock), 0U);
    (void)memset(rebuilt, 0, sizeof(rebuilt));
    for (index = 0U; status == LXP_OK &&
         index < server->daemon->queue_count; ++index) {
        size_t at = (server->daemon->queue_head + index) %
            LXP_DAEMON_QUEUE_CAPACITY;
        lxp_daemon_activity *activity = &server->daemon->queue[at];
        uint8_t header[LNI_ADMISSION_JOURNAL_RECORD_BYTES];
        uint8_t activity_id[32];
        uint64_t expected = server->daemon->next_sequence + index;
        if (activity->global_sequence != expected ||
            activity->length == 0U ||
            activity->length > LXP_MAX_ACTIVITY_BYTES)
            status = LXP_FATAL_INVARIANT;
        if (status == LXP_OK)
            status = lxp_activity_id(activity->bytes, activity->length,
                                     activity_id);
        if (status == LXP_OK && activity->durable_admission &&
            lxp_ct_memcmp(activity->activity_id, activity_id, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
        if (status != LXP_OK) break;
        admission_record_encode(expected, activity_id, activity->bytes,
                                activity->length, header);
        status = file_write_exact(descriptor, header, sizeof(header), offset);
        if (status == LXP_OK)
            status = file_write_exact(descriptor, activity->bytes,
                                      activity->length,
                                      offset + sizeof(header));
        if (status == LXP_OK) {
            rebuilt[index].global_sequence = expected;
            rebuilt[index].file_offset = offset;
            rebuilt[index].activity_length = (uint32_t)activity->length;
            (void)memcpy(rebuilt[index].activity_id, activity_id, 32U);
            offset += sizeof(header) + activity->length;
        }
    }
    if (status == LXP_OK && fdatasync(descriptor) != 0) status = LXP_ERR_IO;
    if (status == LXP_OK &&
        (fstat(descriptor, &metadata) != 0 || !S_ISREG(metadata.st_mode) ||
         metadata.st_nlink != 1 || metadata.st_uid != geteuid() ||
         (metadata.st_mode & 0777U) != 0600U))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK && renameat(
            server->admission_parent_descriptor,
            LNI_ADMISSION_JOURNAL_TEMP_NAME,
            server->admission_parent_descriptor,
            LNI_ADMISSION_JOURNAL_NAME) != 0)
        status = LXP_ERR_IO;
    else if (status == LXP_OK)
        renamed = true;
    if (status == LXP_OK && fsync(server->admission_parent_descriptor) != 0)
        status = LXP_FATAL_INVARIANT;
    if (renamed) {
        int old = server->journal_descriptor;
        server->journal_descriptor = descriptor;
        server->journal_device = (uint64_t)metadata.st_dev;
        server->journal_inode = (uint64_t)metadata.st_ino;
        server->journal_end = offset;
        server->journal_entry_count = server->daemon->queue_count;
        (void)memcpy(server->journal_entries, rebuilt,
                     server->journal_entry_count * sizeof(rebuilt[0]));
        descriptor = -1;
        if (close(old) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    }
    if (descriptor >= 0) (void)close(descriptor);
    if (!renamed) (void)admission_temp_remove(server);
    return status;
}

static lxp_result admission_journal_persist(
    void *context, uint64_t global_sequence,
    const uint8_t activity_id[32],
    const uint8_t *activity, size_t activity_length)
{
    lxp_daemon_lni_server *server = (lxp_daemon_lni_server *)context;
    uint8_t header[LNI_ADMISSION_JOURNAL_RECORD_BYTES];
    uint64_t prior_end;
    size_t index;
    bool append_started = false;
    bool expected = false;
    lxp_result status = LXP_OK;
    if (server == NULL || activity_id == NULL || activity == NULL ||
        activity_length == 0U || activity_length > LXP_MAX_ACTIVITY_BYTES)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (pthread_mutex_lock(&server->mutex) != 0) return LXP_ERR_IO;
    expected = server->admission_sequence_expected &&
        global_sequence == server->expected_admission_sequence &&
        lxp_ct_memcmp(server->expected_admission_activity_id,
                      activity_id, 32U) == 0 &&
        pthread_equal(server->expected_admission_submitter,
                      pthread_self()) != 0;
    if (pthread_mutex_unlock(&server->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    if (!expected) return LXP_ERR_CONTEXT_MISMATCH;
    for (index = 0U; index < server->journal_entry_count; ++index)
        if (lxp_ct_memcmp(server->journal_entries[index].activity_id,
                          activity_id, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
    if (server->journal_entry_count == LXP_DAEMON_QUEUE_CAPACITY ||
        activity_length + LNI_ADMISSION_JOURNAL_RECORD_BYTES >
            admission_journal_max_bytes() - server->journal_end)
        status = admission_journal_compact_locked(server);
    if (status == LXP_OK &&
        server->journal_entry_count == LXP_DAEMON_QUEUE_CAPACITY)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK && !admission_journal_named(
            server, server->journal_descriptor,
            server->journal_device, server->journal_inode))
        status = LXP_ERR_AUTH_SCOPE;
    prior_end = server->journal_end;
    admission_record_encode(global_sequence, activity_id, activity,
                            activity_length, header);
    if (status == LXP_OK) {
        append_started = true;
        status = file_write_exact(server->journal_descriptor,
                                  header, sizeof(header), prior_end);
    }
    if (status == LXP_OK)
        status = file_write_exact(server->journal_descriptor,
                                  activity, activity_length,
                                  prior_end + sizeof(header));
    if (status == LXP_OK && fdatasync(server->journal_descriptor) != 0)
        status = LXP_ERR_IO;
    if (status != LXP_OK) {
        if (append_started &&
            (ftruncate(server->journal_descriptor, (off_t)prior_end) != 0 ||
             fdatasync(server->journal_descriptor) != 0))
            return LXP_FATAL_INVARIANT;
        return status;
    }
    server->journal_entries[server->journal_entry_count].global_sequence =
        global_sequence;
    server->journal_entries[server->journal_entry_count].file_offset =
        prior_end;
    server->journal_entries[server->journal_entry_count].activity_length =
        (uint32_t)activity_length;
    (void)memcpy(
        server->journal_entries[server->journal_entry_count].activity_id,
        activity_id, 32U);
    ++server->journal_entry_count;
    server->journal_end = prior_end + sizeof(header) + activity_length;
    return LXP_OK;
}

static lxp_result secure_parent_open(lxp_daemon_lni_server *server,
                                     const char *socket_path)
{
    char parent[PATH_MAX];
    char resolved[PATH_MAX];
    const char *separator;
    struct stat metadata;
    size_t length;
    int descriptor;
    if (socket_path == NULL || socket_path[0] != '/')
        return LXP_ERR_NON_CANONICAL;
    separator = strrchr(socket_path, '/');
    if (separator == NULL || separator[1] == '\0')
        return LXP_ERR_NON_CANONICAL;
    length = separator == socket_path ? 1U : (size_t)(separator - socket_path);
    if (length >= sizeof(parent)) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(parent, socket_path, length);
    parent[length] = '\0';
    if (realpath(parent, resolved) == NULL || strcmp(parent, resolved) != 0)
        return LXP_ERR_AUTH_SCOPE;
    descriptor = open(parent, O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (descriptor < 0) return LXP_ERR_IO;
    if (fstat(descriptor, &metadata) != 0 || !S_ISDIR(metadata.st_mode) ||
        metadata.st_uid != geteuid() ||
        metadata.st_gid != (gid_t)server->allowed_peer_gid ||
        (metadata.st_mode & 0777U) != 0750U) {
        (void)close(descriptor);
        return LXP_ERR_AUTH_SCOPE;
    }
    server->parent_descriptor = descriptor;
    server->parent_device = (uint64_t)metadata.st_dev;
    server->parent_inode = (uint64_t)metadata.st_ino;
    (void)memcpy(server->parent_path, parent, length + 1U);
    return LXP_OK;
}

static lxp_result secure_admission_parent_open(
    lxp_daemon_lni_server *server, const char *directory)
{
    char resolved[LXP_DAEMON_LNI_ADMISSION_PATH_BYTES];
    struct stat metadata;
    size_t length;
    int descriptor;
    if (directory == NULL || directory[0] != '/')
        return LXP_ERR_NON_CANONICAL;
    length = strlen(directory);
    if (length == 0U || length >= sizeof(server->admission_directory) ||
        realpath(directory, resolved) == NULL ||
        strcmp(directory, resolved) != 0 ||
        strcmp(resolved, server->parent_path) == 0)
        return LXP_ERR_AUTH_SCOPE;
    descriptor = open(resolved,
                      O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    if (descriptor < 0) return LXP_ERR_IO;
    if (fstat(descriptor, &metadata) != 0 ||
        !S_ISDIR(metadata.st_mode) || metadata.st_uid != geteuid() ||
        (metadata.st_mode & 0022U) != 0U) {
        (void)close(descriptor);
        return LXP_ERR_AUTH_SCOPE;
    }
    server->admission_parent_descriptor = descriptor;
    server->admission_parent_device = (uint64_t)metadata.st_dev;
    server->admission_parent_inode = (uint64_t)metadata.st_ino;
    (void)memcpy(server->admission_directory, resolved, length + 1U);
    return LXP_OK;
}

static bool pinned_parent(const lxp_daemon_lni_server *server)
{
    struct stat metadata;
    struct stat named;
    return server->parent_descriptor >= 0 &&
        fstat(server->parent_descriptor, &metadata) == 0 &&
        lstat(server->parent_path, &named) == 0 && S_ISDIR(named.st_mode) &&
        (uint64_t)metadata.st_dev == server->parent_device &&
        (uint64_t)metadata.st_ino == server->parent_inode &&
        named.st_dev == metadata.st_dev && named.st_ino == metadata.st_ino &&
        metadata.st_uid == geteuid() &&
        metadata.st_gid == (gid_t)server->allowed_peer_gid &&
        (metadata.st_mode & 0777U) == 0750U;
}

static bool pinned_lifetime_lock(const lxp_daemon_lni_server *server)
{
    struct stat metadata;
    struct stat named;
    return pinned_parent(server) && server->lifetime_lock_descriptor >= 0 &&
        fstat(server->lifetime_lock_descriptor, &metadata) == 0 &&
        fstatat(server->parent_descriptor, LNI_LIFETIME_LOCK_NAME, &named,
                AT_SYMLINK_NOFOLLOW) == 0 &&
        S_ISREG(metadata.st_mode) && S_ISREG(named.st_mode) &&
        metadata.st_nlink == 1 && named.st_nlink == 1 &&
        metadata.st_uid == geteuid() && named.st_uid == geteuid() &&
        (metadata.st_mode & 0777U) == 0600U &&
        (named.st_mode & 0777U) == 0600U &&
        metadata.st_dev == named.st_dev && metadata.st_ino == named.st_ino &&
        (uint64_t)metadata.st_dev == server->lifetime_lock_device &&
        (uint64_t)metadata.st_ino == server->lifetime_lock_inode;
}

static lxp_result acquire_lifetime_lock(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    struct stat named;
    int descriptor;
    if (!pinned_parent(server)) return LXP_ERR_AUTH_SCOPE;
    descriptor = openat(server->parent_descriptor, LNI_LIFETIME_LOCK_NAME,
                        O_RDWR | O_CREAT | O_CLOEXEC | O_NOFOLLOW, 0600);
    if (descriptor < 0) return LXP_ERR_AUTH_SCOPE;
    if (fstat(descriptor, &metadata) != 0 ||
        fstatat(server->parent_descriptor, LNI_LIFETIME_LOCK_NAME, &named,
                AT_SYMLINK_NOFOLLOW) != 0 ||
        !S_ISREG(metadata.st_mode) || !S_ISREG(named.st_mode) ||
        metadata.st_nlink != 1 || named.st_nlink != 1 ||
        metadata.st_uid != geteuid() || named.st_uid != geteuid() ||
        (metadata.st_mode & 0777U) != 0600U ||
        (named.st_mode & 0777U) != 0600U ||
        metadata.st_dev != named.st_dev || metadata.st_ino != named.st_ino) {
        (void)close(descriptor);
        return LXP_ERR_AUTH_SCOPE;
    }
    if (flock(descriptor, LOCK_EX | LOCK_NB) != 0) {
        (void)close(descriptor);
        return LXP_ERR_AUTH_SCOPE;
    }
    server->lifetime_lock_descriptor = descriptor;
    server->lifetime_lock_device = (uint64_t)metadata.st_dev;
    server->lifetime_lock_inode = (uint64_t)metadata.st_ino;
    if (!pinned_lifetime_lock(server)) {
        (void)flock(descriptor, LOCK_UN);
        (void)close(descriptor);
        server->lifetime_lock_descriptor = -1;
        return LXP_ERR_AUTH_SCOPE;
    }
    return LXP_OK;
}

static lxp_result pin_bound_socket(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    if (!pinned_lifetime_lock(server) ||
        lstat(server->socket_path, &metadata) != 0 ||
        !S_ISSOCK(metadata.st_mode) || metadata.st_uid != geteuid())
        return LXP_ERR_AUTH_SCOPE;
    server->socket_device = (uint64_t)metadata.st_dev;
    server->socket_inode = (uint64_t)metadata.st_ino;
    return LXP_OK;
}

static lxp_result validate_pinned_socket(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    if (!pinned_lifetime_lock(server) ||
        lstat(server->socket_path, &metadata) != 0 ||
        !S_ISSOCK(metadata.st_mode) || metadata.st_uid != geteuid() ||
        metadata.st_gid != (gid_t)server->allowed_peer_gid ||
        (metadata.st_mode & 0777U) != 0660U ||
        (uint64_t)metadata.st_dev != server->socket_device ||
        (uint64_t)metadata.st_ino != server->socket_inode)
        return LXP_ERR_AUTH_SCOPE;
    return LXP_OK;
}

static lxp_result unlink_pinned_socket(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    if (!pinned_lifetime_lock(server) ||
        lstat(server->socket_path, &metadata) != 0 ||
        !S_ISSOCK(metadata.st_mode) ||
        (uint64_t)metadata.st_dev != server->socket_device ||
        (uint64_t)metadata.st_ino != server->socket_inode)
        return LXP_ERR_AUTH_SCOPE;
    return unlink(server->socket_path) == 0 ? LXP_OK : LXP_ERR_IO;
}

static lxp_result socket_listener_live(const char *path, bool *live)
{
    struct sockaddr_un address;
    int descriptor;
    int result;
    int saved_errno;
    if (path == NULL || live == NULL || strlen(path) >= sizeof(address.sun_path))
        return LXP_ERR_NON_CANONICAL;
    descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    if (descriptor < 0) return LXP_ERR_IO;
    (void)memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, path, strlen(path) + 1U);
    result = connect(descriptor, (struct sockaddr *)&address, sizeof(address));
    saved_errno = errno;
    (void)close(descriptor);
    if (result == 0 || saved_errno == EINPROGRESS || saved_errno == EAGAIN ||
        saved_errno == EALREADY || saved_errno == EISCONN) {
        *live = true;
        return LXP_OK;
    }
    if (saved_errno == ECONNREFUSED) {
        *live = false;
        return LXP_OK;
    }
    return LXP_ERR_AUTH_SCOPE;
}

static lxp_result recover_stale_socket(lxp_daemon_lni_server *server)
{
    struct stat metadata;
    bool live;
    lxp_result status;
    if (lstat(server->socket_path, &metadata) != 0)
        return errno == ENOENT ? LXP_OK : LXP_ERR_IO;
    if (!pinned_lifetime_lock(server) || !S_ISSOCK(metadata.st_mode) ||
        metadata.st_uid != geteuid())
        return LXP_ERR_AUTH_SCOPE;
    status = socket_listener_live(server->socket_path, &live);
    if (status != LXP_OK || live) return LXP_ERR_AUTH_SCOPE;
    server->socket_device = (uint64_t)metadata.st_dev;
    server->socket_inode = (uint64_t)metadata.st_ino;
    return unlink_pinned_socket(server);
}

static lxp_result monotonic_milliseconds(int64_t *milliseconds)
{
    struct timespec now;
    if (milliseconds == NULL || clock_gettime(CLOCK_MONOTONIC, &now) != 0 ||
        now.tv_sec > (time_t)(INT64_MAX / 1000))
        return LXP_ERR_IO;
    *milliseconds = (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
    return LXP_OK;
}

static lxp_result request_deadline(const lxp_daemon_lni_server *server,
                                   int64_t *deadline)
{
    int64_t now;
    lxp_result status = monotonic_milliseconds(&now);
    if (status != LXP_OK ||
        now > INT64_MAX - (int64_t)server->deadline_milliseconds)
        return LXP_ERR_IO;
    *deadline = now + (int64_t)server->deadline_milliseconds;
    return LXP_OK;
}

static lxp_result wait_ready(int descriptor, short events, int64_t deadline)
{
    struct pollfd poll_descriptor;
    for (;;) {
        int64_t now;
        int64_t remaining;
        int timeout;
        int result;
        lxp_result status = monotonic_milliseconds(&now);
        if (status != LXP_OK) return status;
        remaining = deadline - now;
        if (remaining <= 0) return LXP_ERR_EXPIRED;
        timeout = remaining > INT_MAX ? INT_MAX : (int)remaining;
        poll_descriptor.fd = descriptor;
        poll_descriptor.events = events;
        poll_descriptor.revents = 0;
        result = poll(&poll_descriptor, 1U, timeout);
        if (result > 0) {
            if ((poll_descriptor.revents & events) != 0) return LXP_OK;
            if ((poll_descriptor.revents & (POLLHUP | POLLERR | POLLNVAL)) != 0)
                return LXP_ERR_TRUNCATED;
        } else if (result == 0) return LXP_ERR_EXPIRED;
        else if (errno != EINTR) return LXP_ERR_IO;
    }
}

static lxp_result exact_read(int descriptor, uint8_t *bytes, size_t length,
                             int64_t deadline)
{
    size_t offset = 0U;
    while (offset < length) {
        lxp_result status = wait_ready(descriptor, POLLIN, deadline);
        if (status != LXP_OK) return status;
        ssize_t received = recv(descriptor, bytes + offset, length - offset, 0);
        if (received > 0) offset += (size_t)received;
        else if (received == 0) return LXP_ERR_TRUNCATED;
        else if (errno == EINTR) continue;
        else if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
        else return LXP_ERR_IO;
    }
    return LXP_OK;
}

static lxp_result exact_write(int descriptor, const uint8_t *bytes,
                              size_t length, int64_t deadline)
{
    size_t offset = 0U;
    while (offset < length) {
        lxp_result status = wait_ready(descriptor, POLLOUT, deadline);
        if (status != LXP_OK) return status;
        ssize_t written = send(descriptor, bytes + offset, length - offset,
                               MSG_NOSIGNAL);
        if (written > 0) offset += (size_t)written;
        else if (written == 0) return LXP_ERR_TRUNCATED;
        else if (errno == EINTR) continue;
        else if (errno == EAGAIN || errno == EWOULDBLOCK) continue;
        else return LXP_ERR_IO;
    }
    return LXP_OK;
}

static lxp_result decode_envelope(const uint8_t *bytes, size_t length,
                                  lni_envelope *envelope)
{
    size_t cursor = 0U;
    uint32_t payload_length;
    uint32_t proof_length;
    if (bytes == NULL || envelope == NULL || length < LNI_ENVELOPE_FIXED_BYTES)
        return LXP_ERR_MALFORMED_ENVELOPE;
    envelope->major = load_u16(bytes + cursor); cursor += 2U;
    envelope->minor = load_u16(bytes + cursor); cursor += 2U;
    envelope->tag = load_u16(bytes + cursor); cursor += 2U;
    envelope->correlation_id = load_u64(bytes + cursor); cursor += 8U;
    payload_length = load_u32(bytes + cursor); cursor += 4U;
    if ((size_t)payload_length > length - cursor - 4U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    envelope->payload = bytes + cursor;
    envelope->payload_length = payload_length;
    cursor += payload_length;
    proof_length = load_u32(bytes + cursor); cursor += 4U;
    if ((size_t)proof_length != length - cursor)
        return LXP_ERR_MALFORMED_ENVELOPE;
    envelope->proof = bytes + cursor;
    envelope->proof_length = proof_length;
    if (envelope->major != LNI_VERSION_MAJOR ||
        envelope->minor > LNI_VERSION_MINOR)
        return LXP_ERR_VERSION_UNSUPPORTED;
    return LXP_OK;
}

static lxp_result send_envelope(int descriptor, uint32_t maximum,
                                uint16_t tag, uint64_t correlation_id,
                                const uint8_t *payload, size_t payload_length,
                                const uint8_t *proof, size_t proof_length,
                                int64_t deadline)
{
    uint8_t prefix[4];
    uint8_t *body;
    size_t length;
    size_t cursor = 0U;
    lxp_result status;
    if ((payload == NULL && payload_length != 0U) ||
        (proof == NULL && proof_length != 0U) ||
        payload_length > UINT32_MAX || proof_length > UINT32_MAX ||
        payload_length > SIZE_MAX - LNI_ENVELOPE_FIXED_BYTES - proof_length)
        return LXP_ERR_LENGTH_LIMIT;
    length = LNI_ENVELOPE_FIXED_BYTES + payload_length + proof_length;
    if (length == 0U || length > maximum || length > UINT32_MAX)
        return LXP_ERR_LENGTH_LIMIT;
    body = (uint8_t *)malloc(length);
    if (body == NULL) return LXP_ERR_IO;
    store_u16(body + cursor, LNI_VERSION_MAJOR); cursor += 2U;
    store_u16(body + cursor, LNI_VERSION_MINOR); cursor += 2U;
    store_u16(body + cursor, tag); cursor += 2U;
    store_u64(body + cursor, correlation_id); cursor += 8U;
    store_u32(body + cursor, (uint32_t)payload_length); cursor += 4U;
    if (payload_length != 0U) {
        (void)memcpy(body + cursor, payload, payload_length);
        cursor += payload_length;
    }
    store_u32(body + cursor, (uint32_t)proof_length); cursor += 4U;
    if (proof_length != 0U) {
        (void)memcpy(body + cursor, proof, proof_length);
        cursor += proof_length;
    }
    store_u32(prefix, (uint32_t)length);
    status = exact_write(descriptor, prefix, sizeof(prefix), deadline);
    if (status == LXP_OK)
        status = exact_write(descriptor, body, cursor, deadline);
    lxp_secure_zero(body, length);
    free(body);
    return status;
}

static lxp_result send_refusal(int descriptor, uint32_t maximum,
                               uint64_t correlation_id, uint8_t refusal,
                               lxp_result result, int64_t deadline)
{
    uint8_t payload[5];
    payload[0] = refusal;
    store_u32(payload + 1U, (uint32_t)result);
    return send_envelope(descriptor, maximum, LNI_ERROR_RESPONSE,
                         correlation_id, payload, sizeof(payload), NULL, 0U,
                         deadline);
}

static uint8_t role_tag(lxp_daemon_role_kind role)
{
    switch (role) {
    case LXP_DAEMON_SEQUENCER: return 1U;
    case LXP_DAEMON_REPLICA: return 2U;
    case LXP_DAEMON_GUARANTOR: return 4U;
    default: return 0U;
    }
}

static lxp_result receipt_refusal(int descriptor, uint32_t maximum,
                                  uint64_t correlation_id, lxp_result status,
                                  int64_t deadline);

static lxp_result send_node_info(lxp_daemon_lni_server *server,
                                 int descriptor, uint64_t correlation_id,
                                 int64_t deadline)
{
    static const char *sequencer_capabilities[] = {
        "authenticated_durable_submit", "batch_header", "node_info",
        "preparation_state",
        "receipt_lookup", "submit"
    };
    static const char *evidence_capabilities[] = {
        "account_read", "authenticated_durable_submit", "batch_header",
        "checkpoint", "historical_proofs", "node_info",
        "preparation_state", "proof_bundle", "receipt_lookup", "submit"
    };
    static const char *finalizer_capabilities[] = {
        "account_read", "authenticated_durable_submit", "batch_header",
        "checkpoint", "finality_evidence_register", "historical_proofs",
        "node_info", "preparation_state", "proof_bundle", "receipt_lookup",
        "submit"
    };
    static const char *reader_capabilities[] = {
        "batch_header", "node_info", "receipt_lookup"
    };
    static const char *evidence_reader_capabilities[] = {
        "account_read", "batch_header", "checkpoint", "historical_proofs",
        "node_info", "proof_bundle", "receipt_lookup"
    };
    bool evidence_available = server->owner->evidence_store != NULL;
    bool finalizer = server->daemon->config.role == LXP_DAEMON_SEQUENCER &&
        evidence_available &&
        server->owner->evidence_store->verify_finality_authority != NULL;
    const char *const *capabilities = finalizer ? finalizer_capabilities :
        server->daemon->config.role == LXP_DAEMON_SEQUENCER ?
            (evidence_available ? evidence_capabilities :
                                  sequencer_capabilities) :
            (evidence_available ? evidence_reader_capabilities :
                                  reader_capabilities);
    uint8_t payload[512];
    uint64_t head;
    uint64_t batch;
    size_t cursor = 0U;
    size_t index;
    size_t capability_count = finalizer ?
            sizeof(finalizer_capabilities) /
                sizeof(finalizer_capabilities[0]) :
        server->daemon->config.role == LXP_DAEMON_SEQUENCER ?
            (evidence_available ?
                sizeof(evidence_capabilities) /
                    sizeof(evidence_capabilities[0]) :
                sizeof(sequencer_capabilities) /
                    sizeof(sequencer_capabilities[0])) :
            (evidence_available ?
                sizeof(evidence_reader_capabilities) /
                    sizeof(evidence_reader_capabilities[0]) :
                sizeof(reader_capabilities) /
                    sizeof(reader_capabilities[0]));
    lxp_result status = LXP_OK;
    for (index = 0U; index < capability_count; ++index) {
        size_t length = strlen(capabilities[index]);
        if (length > UINT16_MAX || length + 2U > sizeof(payload) - cursor)
            return LXP_ERR_LENGTH_LIMIT;
        cursor += length + 2U;
    }
    if (LNI_NODE_INFO_FIXED_BYTES > sizeof(payload) - cursor)
        return LXP_ERR_LENGTH_LIMIT;
    cursor = 0U;
    if (pthread_mutex_lock(&server->daemon->mutex) != 0) return LXP_ERR_IO;
    head = server->daemon->next_sequence == 0U ? 0U :
        server->daemon->next_sequence - 1U;
    if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    batch = server->owner->receipt_authority->last_batch_number;
    store_u16(payload + cursor, LNI_VERSION_MAJOR); cursor += 2U;
    store_u16(payload + cursor, LNI_VERSION_MINOR); cursor += 2U;
    store_u16(payload + cursor, server->owner->protocol_version); cursor += 2U;
    store_u32(payload + cursor, server->daemon->config.network_id); cursor += 4U;
    payload[cursor++] = role_tag(server->daemon->config.role);
    store_u64(payload + cursor, head); cursor += 8U;
    store_u64(payload + cursor, batch); cursor += 8U;
    if (server->owner->evidence_store != NULL)
        (void)memcpy(payload + cursor,
                     server->owner->evidence_store->latest_checkpoint_id,
                     32U);
    else
        (void)memset(payload + cursor, 0, 32U);
    cursor += 32U;
    (void)memcpy(payload + cursor,
                 server->owner->receipt_authority->authorization.public_key,
                 32U); cursor += 32U;
    store_u16(payload + cursor, (uint16_t)capability_count); cursor += 2U;
    for (index = 0U; index < capability_count; ++index) {
        size_t length = strlen(capabilities[index]);
        store_u16(payload + cursor, (uint16_t)length); cursor += 2U;
        (void)memcpy(payload + cursor, capabilities[index], length);
        cursor += length;
    }
    if (pthread_mutex_unlock(&server->owner->mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = send_envelope(descriptor, server->frame_bytes,
                               LNI_NODE_INFO_RESPONSE, correlation_id,
                               payload, cursor, NULL, 0U, deadline);
    return status;
}

static lxp_result send_batch_header(lxp_daemon_lni_server *server,
                                    int descriptor,
                                    const lni_envelope *request,
                                    int64_t deadline)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header header;
    uint8_t proof[146];
    uint64_t record_offset = 0U;
    uint64_t selected;
    bool present = false;
    bool found = false;
    size_t mark;
    lxp_result status = LXP_OK;
    if (request->proof_length != 0U || request->payload_length != 10U ||
        load_u16(request->payload) != 1U ||
        load_u64(request->payload + 2U) == 0U)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    selected = load_u64(request->payload + 2U);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    while (status == LXP_OK && !found) {
        status = lxp_daemon_receipt_authority_scan(
            server->owner->receipt_authority, &record_offset,
            server->owner->scratch, &evidence, &present);
        if (status != LXP_OK || !present) break;
        status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                                         evidence.canonical_header.length,
                                         &header);
        if (status == LXP_OK && header.batch_number == selected) found = true;
        if (!found) {
            (void)lxp_arena_reset(server->owner->scratch, mark);
            mark = lxp_arena_mark(server->owner->scratch);
        }
    }
    if (status == LXP_OK && !found) {
        status = send_envelope(descriptor, server->frame_bytes,
                               LNI_BATCH_HEADER_RESPONSE,
                               request->correlation_id, NULL, 0U, NULL, 0U,
                               deadline);
    } else if (status == LXP_OK) {
        store_u16(proof, 1U);
        (void)memcpy(proof + 2U,
                     server->owner->receipt_authority->authorization.sequencer_id,
                     32U);
        (void)memcpy(proof + 34U,
                     server->owner->receipt_authority->authorization.public_key,
                     32U);
        store_u64(proof + 66U,
                  server->owner->receipt_authority->authorization.first_batch_number);
        store_u64(proof + 74U,
                  server->owner->receipt_authority->authorization.last_batch_number);
        (void)memcpy(proof + 82U, evidence.header_signature, 64U);
        status = send_envelope(descriptor, server->frame_bytes,
                               LNI_BATCH_HEADER_RESPONSE,
                               request->correlation_id,
                               evidence.canonical_header.bytes,
                               evidence.canonical_header.length,
                               proof, sizeof(proof), deadline);
    } else {
        status = receipt_refusal(descriptor, server->frame_bytes,
                                 request->correlation_id, status, deadline);
    }
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result wall_clock_milliseconds(uint64_t *milliseconds)
{
    struct timespec now;
    if (milliseconds == NULL || clock_gettime(CLOCK_REALTIME, &now) != 0 ||
        now.tv_sec < 0 || now.tv_nsec < 0 ||
        (uint64_t)now.tv_sec > UINT64_MAX / UINT64_C(1000))
        return LXP_ERR_IO;
    *milliseconds = (uint64_t)now.tv_sec * UINT64_C(1000) +
        (uint64_t)now.tv_nsec / UINT64_C(1000000);
    return LXP_OK;
}

static lxp_result admission_journal_contains(
    lxp_daemon_lni_server *server, const uint8_t activity_id[32],
    bool *present)
{
    size_t index;
    lxp_result status = LXP_OK;
    *present = false;
    if (pthread_mutex_lock(&server->daemon->mutex) != 0) return LXP_ERR_IO;
    for (index = 0U; index < server->journal_entry_count; ++index)
        if (lxp_ct_memcmp(server->journal_entries[index].activity_id,
                          activity_id, 32U) == 0) {
            *present = true;
            break;
        }
    if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result committed_activity_present(
    lxp_daemon_protocol_owner *owner, const uint8_t activity_id[32],
    bool *present)
{
    lxp_receipt_query query;
    lxp_byte_span canonical_receipt = {NULL, 0U};
    lxp_receipt receipt;
    size_t mark;
    lxp_result status;
    *present = false;
    if (owner->history == NULL || owner->scratch == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&query, 0, sizeof(query));
    query.kind = LXP_RECEIPT_BY_TRANSACTION_ID;
    (void)memcpy(query.identifier, activity_id, 32U);
    query.maximum_response_bytes = LXP_MAX_ACTIVITY_BYTES;
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_receipt_lookup(owner->history, &query, owner->scratch,
                                &canonical_receipt);
    if (status == LXP_ERR_UNKNOWN_ACTIVITY)
        status = LXP_OK;
    else if (status == LXP_OK)
        status = lxp_receipt_decode(canonical_receipt.bytes,
                                    canonical_receipt.length, true,
                                    &receipt);
    if (status == LXP_OK && canonical_receipt.bytes != NULL &&
        lxp_ct_memcmp(receipt.activity_id, activity_id, 32U) != 0)
        status = LXP_ERR_LOG_CORRUPT;
    if (status == LXP_OK && canonical_receipt.bytes != NULL)
        *present = receipt.global_sequence <
                owner->feed_store.baseline_next_sequence ||
            (owner->feed_store.scanned_through_sequence != 0U &&
             receipt.global_sequence <=
                owner->feed_store.scanned_through_sequence);
    (void)lxp_arena_reset(owner->scratch, mark);
    return status;
}

static void authentication_refusal_record(
    lxp_daemon_lni_server *server, const struct ucred *credential)
{
    size_t index;
    if (pthread_mutex_lock(&server->mutex) != 0) return;
    for (index = 0U; index < server->observed_peer_count; ++index) {
        lxp_daemon_lni_peer_observation *peer =
            &server->observed_peers[index];
        if (peer->pid != (uint32_t)credential->pid ||
            peer->uid != (uint32_t)credential->uid ||
            peer->gid != (uint32_t)credential->gid)
            continue;
        if (peer->authentication_refusals != UINT64_MAX)
            ++peer->authentication_refusals;
        (void)pthread_mutex_unlock(&server->mutex);
        return;
    }
    if (server->evicted_authentication_refusals != UINT64_MAX)
        ++server->evicted_authentication_refusals;
    (void)pthread_mutex_unlock(&server->mutex);
}

static lxp_result authentication_refusal(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, const struct ucred *credential,
    lxp_result result, int64_t deadline)
{
    authentication_refusal_record(server, credential);
    return send_refusal(descriptor, server->frame_bytes,
                        request->correlation_id, 6U, result, deadline);
}

static lxp_result fail_stop_submit_daemon(lxp_daemon *daemon,
                                          lxp_result failure)
{
    if (pthread_mutex_lock(&daemon->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    daemon->failure = failure;
    daemon->accepting = false;
    daemon->stop_requested = true;
    if (pthread_cond_broadcast(&daemon->queue_changed) != 0)
        failure = LXP_FATAL_INVARIANT;
    if (pthread_mutex_unlock(&daemon->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    return failure;
}

static lxp_result send_submit(lxp_daemon_lni_server *server, int descriptor,
                              const lni_envelope *request,
                              const struct ucred *credential,
                              int64_t deadline)
{
    lxp_activity activity;
    lxp_identity *identity = NULL;
    uint8_t activity_id[32];
    uint64_t timestamp;
    uint64_t expected_sequence;
    bool known = false;
    bool submitted = false;
    lxp_result status;
    if (request->proof_length != 0U || request->payload_length == 0U ||
        request->payload_length > LXP_MAX_ACTIVITY_BYTES)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    if (server->daemon->config.role != LXP_DAEMON_SEQUENCER)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_MODULE_DISABLED, deadline);
    status = lxp_activity_decode(request->payload, request->payload_length,
                                 &activity);
    if (status == LXP_OK && activity.protocol_version != server->owner->protocol_version)
        status = LXP_ERR_VERSION_UNSUPPORTED;
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(
            &activity, server->daemon->config.network_id);
    if (status == LXP_OK)
        status = lxp_activity_verify_payload_hash(&activity);
    if (status == LXP_OK)
        status = lxp_activity_id(request->payload, request->payload_length,
                                 activity_id);
    if (status != LXP_OK)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 4U, status, deadline);
    status = lxp_activity_verify_signature(&activity);
    if (status != LXP_OK)
        return authentication_refusal(
            server, descriptor, request, credential,
            status, deadline);
    if (activity.protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT &&
        activity.activity_type == LX_ASSET_SEND) {
        lxp_send send;
        status = lxp_send_decode(activity.payload.bytes, activity.payload.length, &send);
        if (status != LXP_OK)
            return send_refusal(descriptor, server->frame_bytes,
                                request->correlation_id, 4U, status, deadline);
    }
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    status = admission_journal_contains(server, activity_id, &known);
    if (status == LXP_OK && !known)
        status = committed_activity_present(server->owner, activity_id,
                                            &known);
    if (status == LXP_OK)
        status = wall_clock_milliseconds(&timestamp);
    if (status == LXP_OK &&
        pthread_mutex_lock(&server->daemon->mutex) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        if (server->daemon->queue_count >=
            UINT64_MAX - server->daemon->next_sequence)
            status = LXP_ERR_SEQUENCE_GAP;
        else
            expected_sequence = server->daemon->next_sequence +
                server->daemon->queue_count;
        if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    if (status == LXP_OK)
        status = lxp_identity_resolve(server->owner->identities,
                                      activity.actor_did.bytes,
                                      activity.actor_did.length, &identity);
    if (status == LXP_OK &&
        (activity.authority.length != 32U ||
         !lxp_identity_key_valid(identity, activity.authority.bytes,
                                 timestamp, expected_sequence)))
        status = LXP_ERR_BAD_SIGNATURE;
    if (status == LXP_ERR_BAD_SIGNATURE || status == LXP_ERR_UNKNOWN_DID ||
        status == LXP_ERR_IDENTITY_FROZEN) {
        lxp_result unlock_status = pthread_mutex_unlock(
            &server->owner->mutex) == 0 ? LXP_OK : LXP_FATAL_INVARIANT;
        if (unlock_status != LXP_OK) return unlock_status;
        return authentication_refusal(
            server, descriptor, request, credential,
            status, deadline);
    }
    if (status != LXP_OK)
        goto unlock_owner;
    if (known) {
        if (pthread_mutex_unlock(&server->owner->mutex) != 0)
            return LXP_FATAL_INVARIANT;
        return send_envelope(descriptor, server->frame_bytes,
                             LNI_SUBMIT_RESPONSE, request->correlation_id,
                             request->payload, request->payload_length,
                             activity_id, sizeof(activity_id), deadline);
    }
    if (pthread_mutex_lock(&server->mutex) != 0) {
        status = LXP_ERR_IO;
        goto unlock_owner;
    }
    if (server->admission_sequence_expected) {
        status = LXP_ERR_CONTEXT_MISMATCH;
    } else {
        server->expected_admission_sequence = expected_sequence;
        (void)memcpy(server->expected_admission_activity_id,
                     activity_id, 32U);
        server->expected_admission_submitter = pthread_self();
        server->admission_sequence_expected = true;
    }
    if (pthread_mutex_unlock(&server->mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = lxp_daemon_submit(server->daemon, request->payload,
                                   request->payload_length);
    submitted = status == LXP_OK;
    if (pthread_mutex_lock(&server->mutex) != 0) {
        status = LXP_FATAL_INVARIANT;
    } else {
        server->admission_sequence_expected = false;
        server->expected_admission_sequence = 0U;
        (void)memset(server->expected_admission_activity_id, 0,
                     sizeof(server->expected_admission_activity_id));
        if (pthread_mutex_unlock(&server->mutex) != 0)
            status = LXP_FATAL_INVARIANT;
    }
unlock_owner:
    if (pthread_mutex_unlock(&server->owner->mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_FATAL_INVARIANT) {
        if (submitted)
            status = fail_stop_submit_daemon(server->daemon, status);
        return status;
    }
    if (status != LXP_OK)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 4U,
                            status == LXP_ERR_LENGTH_LIMIT ? status :
                                LXP_ERR_MODULE_DISABLED,
                            deadline);
    return send_envelope(descriptor, server->frame_bytes,
                         LNI_SUBMIT_RESPONSE, request->correlation_id,
                         request->payload, request->payload_length,
                         activity_id, sizeof(activity_id), deadline);
}

static lxp_result receipt_refusal(int descriptor, uint32_t maximum,
                                  uint64_t correlation_id, lxp_result status,
                                  int64_t deadline)
{
    lxp_result public_result;
    if (status == LXP_ERR_LENGTH_LIMIT || status == LXP_ERR_ARENA_EXHAUSTED)
        public_result = LXP_ERR_LENGTH_LIMIT;
    else if (status == LXP_ERR_IO)
        public_result = LXP_ERR_MODULE_DISABLED;
    else
        public_result = LXP_ERR_MALFORMED_RECEIVE;
    return send_refusal(descriptor, maximum, correlation_id, 5U,
                        public_result, deadline);
}

static lxp_result send_receipt(lxp_daemon_lni_server *server, int descriptor,
                               const lni_envelope *request, int64_t deadline)
{
    lxp_receipt_query query;
    lxp_byte_span receipt;
    size_t mark;
    lxp_result status;
    (void)memset(&query, 0, sizeof(query));
    if (request->proof_length != 0U || request->payload_length < 1U)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    if (request->payload[0] == 1U && request->payload_length == 33U) {
        query.kind = LXP_RECEIPT_BY_TRANSACTION_ID;
        (void)memcpy(query.identifier, request->payload + 1U, 32U);
    } else if (request->payload[0] == 2U && request->payload_length == 33U) {
        query.kind = LXP_RECEIPT_BY_IDEMPOTENCY_KEY;
        (void)memcpy(query.identifier, request->payload + 1U, 32U);
    } else if (request->payload[0] == 3U && request->payload_length == 9U) {
        query.kind = LXP_RECEIPT_BY_GLOBAL_SEQUENCE;
        query.global_sequence = load_u64(request->payload + 1U);
    } else {
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    }
    query.maximum_response_bytes = server->frame_bytes -
        LNI_ENVELOPE_FIXED_BYTES;
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    status = lxp_receipt_lookup(server->owner->history, &query,
                                server->owner->scratch, &receipt);
    if (status == LXP_ERR_UNKNOWN_ACTIVITY)
        status = send_envelope(descriptor, server->frame_bytes,
                               LNI_RECEIPT_LOOKUP_RESPONSE,
                               request->correlation_id, NULL, 0U, NULL, 0U,
                               deadline);
    else if (status == LXP_OK)
        status = send_envelope(descriptor, server->frame_bytes,
                               LNI_RECEIPT_LOOKUP_RESPONSE,
                               request->correlation_id,
                               receipt.bytes, receipt.length, NULL, 0U,
                               deadline);
    else
        status = receipt_refusal(descriptor, server->frame_bytes,
                                 request->correlation_id, status, deadline);
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static bool registration_active(
    const lxp_module_registration *registration, uint64_t epoch)
{
    return registration->enabled && epoch >= registration->enabled_epoch &&
        epoch < registration->disabled_epoch;
}

static lxp_result encode_active_registrations(
    const lxp_kernel *kernel, uint8_t *payload, size_t capacity,
    size_t *cursor)
{
    const lxp_module_registration *active[LXP_MODULE_RESERVED_COUNT] = {0};
    size_t index;
    uint16_t module_id;
    uint16_t count = 0U;
    if (kernel == NULL || payload == NULL || cursor == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < kernel->module_count; ++index) {
        const lxp_module_registration *registration = &kernel->modules[index];
        size_t activity_index;
        if (!registration_active(registration, kernel->epoch)) continue;
        if (registration->module_id == 0U ||
            registration->module_id > LXP_MODULE_RESERVED_COUNT ||
            registration->activity_type_count == 0U ||
            registration->activity_type_count > LXP_MODULE_MAX_ACTIVITY_TYPES ||
            active[registration->module_id - 1U] != NULL)
            return LXP_ERR_UNKNOWN_MODULE;
        for (activity_index = 0U;
             activity_index < registration->activity_type_count;
             ++activity_index) {
            uint32_t activity_type =
                registration->activity_types[activity_index];
            if ((activity_type >> 16U) != registration->module_id ||
                (activity_type & UINT32_C(0xffff)) == 0U ||
                (activity_index != 0U &&
                 registration->activity_types[activity_index - 1U] >=
                     activity_type))
                return LXP_ERR_UNKNOWN_ACTIVITY;
        }
        active[registration->module_id - 1U] = registration;
        ++count;
    }
    if (count == 0U || *cursor > capacity - 2U)
        return count == 0U ? LXP_ERR_MODULE_DISABLED : LXP_ERR_LENGTH_LIMIT;
    store_u16(payload + *cursor, count);
    *cursor += 2U;
    for (module_id = 1U; module_id <= LXP_MODULE_RESERVED_COUNT;
         ++module_id) {
        const lxp_module_registration *registration = active[module_id - 1U];
        size_t activity_index;
        size_t required;
        if (registration == NULL) continue;
        required = 4U + registration->activity_type_count * 4U;
        if (*cursor > capacity || required > capacity - *cursor)
            return LXP_ERR_LENGTH_LIMIT;
        store_u16(payload + *cursor, module_id);
        *cursor += 2U;
        store_u16(payload + *cursor,
                  (uint16_t)registration->activity_type_count);
        *cursor += 2U;
        for (activity_index = 0U;
             activity_index < registration->activity_type_count;
             ++activity_index) {
            store_u32(payload + *cursor,
                      registration->activity_types[activity_index]);
            *cursor += 4U;
        }
    }
    return LXP_OK;
}

static lxp_result preparation_snapshot_valid(
    const lxp_daemon_protocol_owner *owner)
{
    const lxp_kernel *kernel;
    if (owner == NULL || !owner->attached || owner->kernel == NULL ||
        owner->kernel->state == NULL || owner->identities == NULL ||
        owner->receipt_authority == NULL ||
        owner->network_id == 0U ||
        owner->latest_sealed_timestamp == 0U)
        return LXP_ERR_MODULE_DISABLED;
    kernel = owner->kernel;
    if (kernel->publication_poisoned || kernel->batch_publication_pending ||
        kernel->state->next_sequence == 0U || kernel->epoch == 0U ||
        kernel->module_count == 0U ||
        kernel->module_count > LXP_MODULE_RESERVED_COUNT ||
        lxp_ct_is_zero(kernel->current_state_root, 32U))
        return LXP_ERR_MODULE_DISABLED;
    if (owner->receipt_authority->record_count == 0U) {
        if (kernel->state->next_sequence != 1U)
            return LXP_ERR_PROJECTION_STALE;
    } else if (owner->receipt_authority->last_global_sequence == UINT64_MAX ||
               owner->receipt_authority->last_global_sequence + 1U !=
                   kernel->state->next_sequence ||
               owner->receipt_authority->last_sealed_timestamp !=
                   owner->latest_sealed_timestamp) {
        return LXP_ERR_PROJECTION_STALE;
    }
    if ((owner->feed_store.scanned_through_sequence == 0U &&
         (owner->feed_store.baseline_next_sequence !=
              kernel->state->next_sequence ||
          lxp_ct_memcmp(owner->feed_store.baseline_state_root,
                        kernel->current_state_root, 32U) != 0)) ||
        (owner->feed_store.scanned_through_sequence != 0U &&
         (owner->feed_store.scanned_through_sequence == UINT64_MAX ||
          owner->feed_store.scanned_through_sequence + 1U !=
              kernel->state->next_sequence ||
          owner->feed_store.head_timestamp !=
              owner->latest_sealed_timestamp ||
          lxp_ct_memcmp(owner->feed_store.head_state_root,
                        kernel->current_state_root, 32U) != 0)))
        return LXP_ERR_PROJECTION_STALE;
    return LXP_OK;
}

lxp_result lxp_daemon_lni_preparation_state(
    lxp_daemon_protocol_owner *owner, const uint8_t *request,
    size_t request_length,
    uint8_t *response, size_t response_capacity,
    size_t *response_length)
{
    const uint8_t *actor;
    size_t bounded_capacity;
    size_t actor_length;
    size_t cursor = 0U;
    lxp_identity *identity = NULL;
    lxp_result status;
    if (response_length == NULL) return LXP_ERR_MALFORMED_ENVELOPE;
    *response_length = 0U;
    if (owner == NULL || request == NULL || response == NULL ||
        request_length < 4U || load_u16(request) != 1U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    bounded_capacity = response_capacity < LNI_PREPARATION_STATE_MAX_BYTES ?
        response_capacity : LNI_PREPARATION_STATE_MAX_BYTES;
    actor_length = load_u16(request + 2U);
    if (actor_length == 0U || actor_length > LXP_MAX_DID_LENGTH ||
        request_length != 4U + actor_length)
        return LXP_ERR_MALFORMED_ENVELOPE;
    if (bounded_capacity < 4U ||
        actor_length > bounded_capacity - 4U ||
        bounded_capacity - 4U - actor_length < 78U)
        return LXP_ERR_LENGTH_LIMIT;
    actor = request + 4U;
    if (pthread_mutex_lock(&owner->mutex) != 0) return LXP_ERR_IO;
    status = preparation_snapshot_valid(owner);
    if (status == LXP_OK)
        status = lxp_identity_resolve(owner->identities, actor,
                                      actor_length, &identity);
    if (status == LXP_OK && identity->status != LXP_IDENTITY_ACTIVE)
        status = LXP_ERR_UNKNOWN_DID;
    if (status == LXP_OK) {
        const lxp_kernel *kernel = owner->kernel;
        store_u16(response + cursor, 1U); cursor += 2U;
        store_u16(response + cursor, (uint16_t)actor_length); cursor += 2U;
        (void)memcpy(response + cursor, actor, actor_length);
        cursor += actor_length;
        store_u32(response + cursor, owner->network_id);
        cursor += 4U;
        store_u64(response + cursor, identity->next_sequence); cursor += 8U;
        store_u64(response + cursor,
                  owner->latest_sealed_timestamp); cursor += 8U;
        store_u64(response + cursor, kernel->state->next_sequence - 1U);
        cursor += 8U;
        (void)memcpy(response + cursor, kernel->current_state_root, 32U);
        cursor += 32U;
        store_u64(response + cursor, kernel->epoch); cursor += 8U;
        status = encode_active_registrations(
            kernel, response, bounded_capacity, &cursor);
    }
    if (pthread_mutex_unlock(&owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) *response_length = cursor;
    return status;
}

static lxp_result send_preparation_state(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    uint8_t payload[LNI_PREPARATION_STATE_MAX_BYTES];
    size_t payload_length = 0U;
    lxp_result status;
    if (request->minor < 1U)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_VERSION_UNSUPPORTED, deadline);
    if (request->correlation_id == 0U || request->proof_length != 0U ||
        server->daemon->config.role != LXP_DAEMON_SEQUENCER)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id,
                            request->correlation_id == 0U ||
                            request->proof_length != 0U ? 1U : 3U,
                            request->correlation_id == 0U ||
                            request->proof_length != 0U ?
                                LXP_ERR_MALFORMED_ENVELOPE :
                                LXP_ERR_MODULE_DISABLED,
                            deadline);
    status = lxp_daemon_lni_preparation_state(
        server->owner, request->payload, request->payload_length,
        payload, sizeof(payload), &payload_length);
    if (status != LXP_OK) {
        lxp_result public_status =
            status == LXP_ERR_IO || status == LXP_FATAL_INVARIANT ?
                LXP_ERR_MODULE_DISABLED : status;
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id,
                            status == LXP_ERR_MALFORMED_ENVELOPE ? 1U : 4U,
                            public_status, deadline);
    }
    return send_envelope(descriptor, server->frame_bytes,
                         LNI_PREPARATION_STATE_RESPONSE,
                         request->correlation_id, payload, payload_length,
                         NULL, 0U, deadline);
}

static lxp_result evidence_refusal(
    lxp_daemon_lni_server *server, int descriptor,
    uint64_t correlation_id, lxp_result status, int64_t deadline)
{
    lxp_result public_status =
        status == LXP_ERR_IO || status == LXP_FATAL_INVARIANT ?
            LXP_ERR_MODULE_DISABLED : status;
    return send_refusal(descriptor, server->frame_bytes, correlation_id,
                        4U, public_status, deadline);
}

static lxp_result latest_receipt_evidence(
    lxp_daemon_protocol_owner *owner, lxp_arena *arena,
    lxp_daemon_receipt_evidence *evidence)
{
    uint64_t offset = 0U;
    uint64_t target;
    size_t mark;
    bool present = true;
    lxp_result status = LXP_OK;
    if (owner == NULL || owner->receipt_authority == NULL || arena == NULL ||
        evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    target = owner->receipt_authority->last_global_sequence;
    if (target == 0U) return LXP_ERR_MODULE_DISABLED;
    mark = lxp_arena_mark(arena);
    while (status == LXP_OK && present) {
        status = lxp_daemon_receipt_authority_scan(
            owner->receipt_authority, &offset, arena, evidence, &present);
        if (status != LXP_OK || !present) break;
        if (evidence->global_sequence == target) return LXP_OK;
        if (evidence->global_sequence > target)
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_arena_reset(arena, mark);
    }
    return status == LXP_OK ? LXP_ERR_PROJECTION_STALE : status;
}

static lxp_result latest_account_evidence(
    lxp_daemon_protocol_owner *owner, const uint8_t account_id[32],
    const uint8_t *asset_id, const uint8_t *target_activity_id,
    lxp_arena *arena, lxp_daemon_account_evidence *evidence)
{
    lxp_daemon_receipt_evidence head;
    lxp_receipt receipt;
    const lx_account_registry *accounts;
    uint8_t receipt_digest[32];
    size_t index;
    bool found = false;
    lxp_result status;
    if (owner == NULL || owner->kernel == NULL || owner->kernel->state == NULL ||
        owner->kernel->state->accounts == NULL || account_id == NULL ||
        arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    accounts = owner->kernel->state->accounts;
    for (index = 0U; index < accounts->count; ++index) {
        const lx_account *account = &accounts->accounts[index];
        if (lxp_ct_memcmp(account->id, account_id, 32U) != 0) continue;
        if (found) return LXP_FATAL_INVARIANT;
        found = true;
        if (asset_id != NULL &&
            (!account->has_asset ||
             lxp_ct_memcmp(account->asset_id, asset_id, 32U) != 0))
            return LXP_ERR_ASSET_MISMATCH;
    }
    if (!found) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    status = latest_receipt_evidence(owner, arena, &head);
    if (status == LXP_OK)
        status = lxp_receipt_decode(head.canonical_receipt.bytes,
                                    head.canonical_receipt.length,
                                    true, &receipt);
    if (status == LXP_OK && target_activity_id != NULL &&
        lxp_ct_memcmp(receipt.activity_id, target_activity_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, receipt_digest);
    if (status == LXP_OK)
        status = lxp_daemon_account_evidence_build(
            owner->kernel, owner->network_id, account_id, receipt_digest,
            receipt.timestamp, head.canonical_receipt, &head.receipt_proof,
            &owner->receipt_authority->authorization,
            head.canonical_header, head.header_signature, arena, evidence);
    return status;
}

static lxp_result parse_account_read_request(
    const lni_envelope *request, uint8_t *kind,
    const uint8_t **account_id, const uint8_t **asset_id,
    uint8_t *selector_kind, uint64_t *selector_batch,
    const uint8_t **selector_checkpoint, uint8_t *requested_rank)
{
    size_t cursor = 0U;
    if (request == NULL || kind == NULL || account_id == NULL ||
        asset_id == NULL || selector_kind == NULL || selector_batch == NULL ||
        selector_checkpoint == NULL || requested_rank == NULL ||
        request->proof_length != 0U || request->payload_length < 37U ||
        load_u16(request->payload) != 1U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    cursor = 2U;
    *kind = request->payload[cursor++];
    if (*kind != 1U && *kind != 2U) return LXP_ERR_MALFORMED_ENVELOPE;
    if (cursor > request->payload_length - 32U)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *account_id = request->payload + cursor;
    cursor += 32U;
    *asset_id = NULL;
    if (*kind == 1U) {
        if (cursor > request->payload_length - 32U)
            return LXP_ERR_MALFORMED_ENVELOPE;
        *asset_id = request->payload + cursor;
        cursor += 32U;
    }
    if (cursor >= request->payload_length)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *selector_kind = request->payload[cursor++];
    *selector_batch = 0U;
    *selector_checkpoint = NULL;
    if (*selector_kind == 2U) {
        if (cursor > request->payload_length - 8U)
            return LXP_ERR_MALFORMED_ENVELOPE;
        *selector_batch = load_u64(request->payload + cursor);
        cursor += 8U;
        if (*selector_batch == 0U) return LXP_ERR_MALFORMED_ENVELOPE;
    } else if (*selector_kind == 3U) {
        if (cursor > request->payload_length - 32U)
            return LXP_ERR_MALFORMED_ENVELOPE;
        *selector_checkpoint = request->payload + cursor;
        cursor += 32U;
        if (lxp_ct_is_zero(*selector_checkpoint, 32U))
            return LXP_ERR_MALFORMED_ENVELOPE;
    } else if (*selector_kind != 1U) {
        return LXP_ERR_MALFORMED_ENVELOPE;
    }
    if (cursor + 1U != request->payload_length)
        return LXP_ERR_MALFORMED_ENVELOPE;
    *requested_rank = request->payload[cursor];
    return *requested_rank <= 5U ? LXP_OK : LXP_ERR_MALFORMED_ENVELOPE;
}

static lxp_result account_value_asset_matches(
    lxp_byte_span canonical_value, const uint8_t asset_id[32])
{
    size_t name_length;
    size_t asset_offset;
    if (canonical_value.bytes == NULL || asset_id == NULL ||
        canonical_value.length < 2U)
        return LXP_ERR_NON_CANONICAL;
    name_length = load_u16(canonical_value.bytes);
    if (name_length == 0U || name_length > LX_ACCOUNT_NAME_MAX ||
        name_length > canonical_value.length - 2U)
        return LXP_ERR_NON_CANONICAL;
    asset_offset = 2U + name_length + 1U + 16U;
    if (asset_offset > canonical_value.length ||
        canonical_value.length - asset_offset < 33U)
        return LXP_ERR_NON_CANONICAL;
    return canonical_value.bytes[asset_offset + 32U] == 1U &&
        lxp_ct_memcmp(canonical_value.bytes + asset_offset,
                      asset_id, 32U) == 0 ?
            LXP_OK : LXP_ERR_ASSET_MISMATCH;
}

static lxp_result send_account_read(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    lxp_daemon_account_evidence evidence;
    lxp_byte_span canonical_value;
    lxp_byte_span proof_material;
    const uint8_t *account_id;
    const uint8_t *asset_id;
    const uint8_t *selector_checkpoint;
    uint64_t selector_batch;
    uint8_t kind;
    uint8_t selector_kind;
    uint8_t requested_rank;
    size_t mark;
    lxp_result status;
    if (request->minor < 2U || request->correlation_id == 0U)
        return send_refusal(
            descriptor, server->frame_bytes, request->correlation_id, 1U,
            request->minor < 2U ? LXP_ERR_VERSION_UNSUPPORTED :
                                  LXP_ERR_MALFORMED_ENVELOPE,
            deadline);
    status = parse_account_read_request(
        request, &kind, &account_id, &asset_id, &selector_kind,
        &selector_batch, &selector_checkpoint, &requested_rank);
    if (status != LXP_OK)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U, status, deadline);
    if (server->owner->evidence_store == NULL || requested_rank > 4U ||
        ((selector_kind == 1U || selector_kind == 2U) &&
         requested_rank > 3U))
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_MODULE_DISABLED, deadline);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    if (selector_kind == 1U)
        status = latest_account_evidence(
            server->owner, account_id, kind == 1U ? asset_id : NULL, NULL,
            server->owner->scratch, &evidence);
    else
        status = LXP_OK;
    if (status == LXP_OK)
        status = lxp_daemon_account_evidence_wire_encode(
            server->owner->evidence_store,
            selector_kind == 1U ? &evidence : NULL,
            selector_kind == 1U ? server->owner->kernel : NULL,
            server->owner->network_id, account_id, selector_kind,
            selector_batch, selector_checkpoint,
            server->owner->scratch, &canonical_value, &proof_material);
    if (status == LXP_OK && kind == 1U)
        status = account_value_asset_matches(canonical_value, asset_id);
    if (status == LXP_OK)
        status = send_envelope(
            descriptor, server->frame_bytes, LNI_ACCOUNT_READ_RESPONSE,
            request->correlation_id, canonical_value.bytes,
            canonical_value.length, proof_material.bytes,
            proof_material.length, deadline);
    else
        status = evidence_refusal(server, descriptor,
                                  request->correlation_id, status, deadline);
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result send_checkpoint(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    lxp_daemon_finality_evidence evidence;
    uint8_t checkpoint_id[32] = {0};
    uint64_t batch_number = 0U;
    size_t mark;
    lxp_result status;
    if (request->minor < 2U || request->correlation_id == 0U ||
        request->proof_length != 0U || request->payload_length < 11U ||
        load_u16(request->payload) != 1U)
        return send_refusal(
            descriptor, server->frame_bytes, request->correlation_id, 1U,
            request->minor < 2U ? LXP_ERR_VERSION_UNSUPPORTED :
                                  LXP_ERR_MALFORMED_ENVELOPE,
            deadline);
    if (request->payload[2] == 1U && request->payload_length == 35U) {
        (void)memcpy(checkpoint_id, request->payload + 3U, 32U);
        if (lxp_ct_is_zero(checkpoint_id, 32U))
            return send_refusal(descriptor, server->frame_bytes,
                                request->correlation_id, 1U,
                                LXP_ERR_MALFORMED_ENVELOPE, deadline);
    } else if (request->payload[2] == 2U && request->payload_length == 11U) {
        batch_number = load_u64(request->payload + 3U);
        if (batch_number == 0U)
            return send_refusal(descriptor, server->frame_bytes,
                                request->correlation_id, 1U,
                                LXP_ERR_MALFORMED_ENVELOPE, deadline);
    } else {
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    }
    if (server->owner->evidence_store == NULL)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_MODULE_DISABLED, deadline);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    status = lxp_daemon_finality_evidence_lookup(
        server->owner->evidence_store, checkpoint_id, batch_number,
        server->owner->scratch, &evidence);
    if (status == LXP_OK)
        status = send_envelope(
            descriptor, server->frame_bytes, LNI_CHECKPOINT_RESPONSE,
            request->correlation_id, evidence.checkpoint_payload.bytes,
            evidence.checkpoint_payload.length, evidence.finality_proof.bytes,
            evidence.finality_proof.length, deadline);
    else
        status = evidence_refusal(server, descriptor,
                                  request->correlation_id, status, deadline);
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result send_proof_bundle(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    lxp_daemon_activity_evidence activity;
    lxp_daemon_account_evidence account;
    lxp_byte_span canonical_value;
    lxp_byte_span proof_material;
    const uint8_t *target_activity_id;
    uint8_t kind;
    size_t mark;
    lxp_result status;
    if (request->minor < 2U || request->correlation_id == 0U ||
        request->proof_length != 0U ||
        (request->payload_length != 35U &&
         request->payload_length != 67U) ||
        load_u16(request->payload) != 1U)
        return send_refusal(
            descriptor, server->frame_bytes, request->correlation_id, 1U,
            request->minor < 2U ? LXP_ERR_VERSION_UNSUPPORTED :
                                  LXP_ERR_MALFORMED_ENVELOPE,
            deadline);
    kind = request->payload[2U];
    target_activity_id = request->payload + 3U;
    if (((kind == 1U || kind == 3U) && request->payload_length != 35U) ||
        (kind == 2U && request->payload_length != 67U) ||
        (kind != 1U && kind != 2U && kind != 3U) ||
        lxp_ct_is_zero(target_activity_id, 32U) ||
        (kind == 2U && lxp_ct_is_zero(request->payload + 35U, 32U)))
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 1U,
                            LXP_ERR_MALFORMED_ENVELOPE, deadline);
    if (server->owner->evidence_store == NULL)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_MODULE_DISABLED, deadline);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    if (kind == 2U) {
        status = latest_account_evidence(
            server->owner, request->payload + 35U, NULL,
            target_activity_id, server->owner->scratch, &account);
        if (status == LXP_OK)
            status = lxp_daemon_account_evidence_wire_encode(
                server->owner->evidence_store, &account,
                server->owner->kernel, server->owner->network_id,
                request->payload + 35U, 1U, 0U, NULL,
                server->owner->scratch, &canonical_value, &proof_material);
    } else {
        status = lxp_daemon_activity_evidence_lookup(
            server->owner->evidence_store, target_activity_id,
            server->owner->scratch, &activity);
        if (status == LXP_OK)
            status = lxp_daemon_activity_evidence_wire_encode(
                &activity, server->owner->network_id, kind,
                server->owner->scratch, &canonical_value, &proof_material);
    }
    if (status == LXP_OK)
        status = send_envelope(
            descriptor, server->frame_bytes, LNI_PROOF_BUNDLE_RESPONSE,
            request->correlation_id, canonical_value.bytes,
            canonical_value.length, proof_material.bytes,
            proof_material.length, deadline);
    else
        status = evidence_refusal(server, descriptor,
                                  request->correlation_id, status, deadline);
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result send_finality_evidence_register(
    lxp_daemon_lni_server *server, int descriptor,
    const lni_envelope *request, int64_t deadline)
{
    lxp_daemon_finality_evidence evidence;
    uint8_t response[74];
    size_t mark;
    lxp_result status;
    if (request->minor < 2U || request->correlation_id == 0U ||
        request->payload_length == 0U || request->proof_length == 0U ||
        request->payload_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
        request->proof_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES ||
        request->payload_length > LXP_DAEMON_FINALITY_REGISTER_MAX_BYTES -
                                      request->proof_length)
        return send_refusal(
            descriptor, server->frame_bytes, request->correlation_id, 1U,
            request->minor < 2U ? LXP_ERR_VERSION_UNSUPPORTED :
                                  LXP_ERR_MALFORMED_ENVELOPE,
            deadline);
    if (server->daemon->config.role != LXP_DAEMON_SEQUENCER ||
        server->owner->evidence_store == NULL ||
        server->owner->evidence_store->verify_finality_authority == NULL)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 3U,
                            LXP_ERR_MODULE_DISABLED, deadline);
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(server->owner->scratch);
    status = lxp_daemon_finality_evidence_register(
        server->owner->evidence_store,
        (lxp_byte_span){request->payload, request->payload_length},
        (lxp_byte_span){request->proof, request->proof_length},
        server->owner->scratch, &evidence);
    if (status == LXP_OK) {
        store_u16(response, 1U);
        (void)memcpy(response + 2U, evidence.checkpoint_id, 32U);
        store_u64(response + 34U, evidence.batch_number);
        (void)memcpy(response + 42U, evidence.record_digest, 32U);
        status = send_envelope(
            descriptor, server->frame_bytes,
            LNI_FINALITY_EVIDENCE_REGISTER_RESPONSE,
            request->correlation_id, response, sizeof(response),
            NULL, 0U, deadline);
    } else {
        lxp_result public_status =
            status == LXP_ERR_IO || status == LXP_FATAL_INVARIANT ?
                LXP_ERR_MODULE_DISABLED : status;
        status = send_refusal(
            descriptor, server->frame_bytes, request->correlation_id, 4U,
            public_status, deadline);
    }
    (void)lxp_arena_reset(server->owner->scratch, mark);
    if (pthread_mutex_unlock(&server->owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result peer_credentials(const lxp_daemon_lni_server *server,
                                   int descriptor,
                                   struct ucred *credential)
{
    socklen_t length = sizeof(*credential);
    uint8_t expected[8];
    uint8_t observed[8];
    if (getsockopt(descriptor, SOL_SOCKET, SO_PEERCRED,
                   credential, &length) != 0 ||
        length != sizeof(*credential) || credential->pid <= 0)
        return LXP_ERR_AUTH_SCOPE;
    store_u32(expected, server->allowed_peer_uid);
    store_u32(expected + 4U, server->allowed_peer_gid);
    store_u32(observed, (uint32_t)credential->uid);
    store_u32(observed + 4U, (uint32_t)credential->gid);
    return lxp_ct_memcmp(expected, observed, sizeof(expected)) == 0 ?
        LXP_OK : LXP_ERR_AUTH_SCOPE;
}

static lxp_result peer_observation_begin(
    lxp_daemon_lni_server *server, const struct ucred *credential,
    uint64_t *generation)
{
    lxp_daemon_lni_peer_observation *peer;
    size_t index;
    if (pthread_mutex_lock(&server->mutex) != 0) return LXP_ERR_IO;
    if (server->connection_generation != UINT64_MAX)
        ++server->connection_generation;
    *generation = server->connection_generation;
    for (index = 0U; index < server->observed_peer_count; ++index) {
        peer = &server->observed_peers[index];
        if (peer->pid != (uint32_t)credential->pid ||
            peer->uid != (uint32_t)credential->uid ||
            peer->gid != (uint32_t)credential->gid)
            continue;
        if (peer->active_connections != UINT32_MAX)
            ++peer->active_connections;
        peer->active = true;
        peer->latest_connection_generation = *generation;
        if (pthread_mutex_unlock(&server->mutex) != 0)
            return LXP_FATAL_INVARIANT;
        return LXP_OK;
    }
    if (server->observed_peer_count < LXP_DAEMON_LNI_MAX_OBSERVED_PEERS) {
        index = server->observed_peer_count++;
    } else {
        size_t searched;
        index = LXP_DAEMON_LNI_MAX_OBSERVED_PEERS;
        for (searched = 0U;
             searched < LXP_DAEMON_LNI_MAX_OBSERVED_PEERS; ++searched) {
            size_t candidate = (server->observed_peer_next + searched) %
                LXP_DAEMON_LNI_MAX_OBSERVED_PEERS;
            if (!server->observed_peers[candidate].active) {
                index = candidate;
                server->observed_peer_next = (candidate + 1U) %
                    LXP_DAEMON_LNI_MAX_OBSERVED_PEERS;
                break;
            }
        }
        if (index == LXP_DAEMON_LNI_MAX_OBSERVED_PEERS) {
            if (server->evicted_peers != UINT64_MAX)
                ++server->evicted_peers;
            (void)pthread_mutex_unlock(&server->mutex);
            return LXP_OK;
        }
        peer = &server->observed_peers[index];
        if (server->evicted_peers != UINT64_MAX) ++server->evicted_peers;
        if (UINT64_MAX - server->evicted_authentication_refusals <
            peer->authentication_refusals)
            server->evicted_authentication_refusals = UINT64_MAX;
        else
            server->evicted_authentication_refusals +=
                peer->authentication_refusals;
    }
    peer = &server->observed_peers[index];
    (void)memset(peer, 0, sizeof(*peer));
    peer->pid = (uint32_t)credential->pid;
    peer->uid = (uint32_t)credential->uid;
    peer->gid = (uint32_t)credential->gid;
    peer->latest_connection_generation = *generation;
    peer->active_connections = 1U;
    peer->active = true;
    if (pthread_mutex_unlock(&server->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static void peer_observation_end(lxp_daemon_lni_server *server,
                                 const struct ucred *credential)
{
    size_t index;
    if (pthread_mutex_lock(&server->mutex) != 0) return;
    for (index = 0U; index < server->observed_peer_count; ++index)
        if (server->observed_peers[index].pid ==
                (uint32_t)credential->pid &&
            server->observed_peers[index].uid ==
                (uint32_t)credential->uid &&
            server->observed_peers[index].gid ==
                (uint32_t)credential->gid) {
            if (server->observed_peers[index].active_connections != 0U)
                --server->observed_peers[index].active_connections;
            server->observed_peers[index].active =
                server->observed_peers[index].active_connections != 0U;
            break;
        }
    (void)pthread_mutex_unlock(&server->mutex);
}

static lxp_result configure_connection(lxp_daemon_lni_server *server,
                                       int descriptor,
                                       struct ucred *credential,
                                       uint64_t *connection_generation)
{
    int flags;
    lxp_result status = peer_credentials(server, descriptor, credential);
    if (status != LXP_OK) return status;
    flags = fcntl(descriptor, F_GETFL, 0);
    if (flags < 0 || fcntl(descriptor, F_SETFL, flags | O_NONBLOCK) != 0)
        return LXP_ERR_IO;
    return peer_observation_begin(server, credential,
                                  connection_generation);
}

static lxp_result serve_connection(lxp_daemon_lni_server *server,
                                   int descriptor,
                                   const struct ucred *credential)
{
    bool handshaken = false;
    for (;;) {
        uint8_t prefix[4];
        uint8_t *frame;
        uint32_t length;
        lni_envelope request;
        int64_t deadline;
        lxp_result status = request_deadline(server, &deadline);
        if (status == LXP_OK)
            status = exact_read(descriptor, prefix, sizeof(prefix), deadline);
        if (status == LXP_ERR_TRUNCATED) return LXP_OK;
        if (status != LXP_OK) return status;
        length = load_u32(prefix);
        if (length < LNI_ENVELOPE_FIXED_BYTES || length > server->frame_bytes)
            return LXP_ERR_LENGTH_LIMIT;
        frame = (uint8_t *)malloc(length);
        if (frame == NULL) return LXP_ERR_IO;
        status = exact_read(descriptor, frame, length, deadline);
        if (status == LXP_OK) status = decode_envelope(frame, length, &request);
        if (status != LXP_OK) {
            lxp_secure_zero(frame, length);
            free(frame);
            return status;
        }
        if (!handshaken) {
            if (request.tag != LNI_NODE_INFO_REQUEST ||
                request.correlation_id != 0U ||
                request.payload_length != 0U || request.proof_length != 0U)
                status = send_refusal(descriptor, server->frame_bytes,
                                      request.correlation_id, 2U,
                                      LXP_ERR_AUTH_SCOPE, deadline);
            else {
                status = send_node_info(server, descriptor,
                                        request.correlation_id, deadline);
                handshaken = status == LXP_OK;
            }
        } else if (request.tag == LNI_NODE_INFO_REQUEST) {
            status = send_refusal(descriptor, server->frame_bytes,
                                  request.correlation_id, 1U,
                                  LXP_ERR_NON_CANONICAL, deadline);
        } else if (request.tag == LNI_SUBMIT_REQUEST) {
            status = send_submit(server, descriptor, &request,
                                 credential, deadline);
        } else if (request.tag == LNI_RECEIPT_LOOKUP_REQUEST) {
            status = send_receipt(server, descriptor, &request, deadline);
        } else if (request.tag == LNI_ACCOUNT_READ_REQUEST) {
            status = send_account_read(server, descriptor, &request, deadline);
        } else if (request.tag == LNI_BATCH_HEADER_REQUEST) {
            status = send_batch_header(server, descriptor, &request, deadline);
        } else if (request.tag == LNI_CHECKPOINT_REQUEST) {
            status = send_checkpoint(server, descriptor, &request, deadline);
        } else if (request.tag == LNI_PROOF_BUNDLE_REQUEST) {
            status = send_proof_bundle(
                server, descriptor, &request, deadline);
        } else if (request.tag == LNI_PREPARATION_STATE_REQUEST) {
            status = send_preparation_state(
                server, descriptor, &request, deadline);
        } else if (request.tag == LNI_FINALITY_EVIDENCE_REGISTER_REQUEST) {
            status = send_finality_evidence_register(
                server, descriptor, &request, deadline);
        } else {
            status = send_refusal(descriptor, server->frame_bytes,
                                  request.correlation_id, 3U,
                                  LXP_ERR_MODULE_DISABLED, deadline);
        }
        lxp_secure_zero(frame, length);
        free(frame);
        if (status != LXP_OK) return status;
    }
}

lxp_result lxp_daemon_lni_serve_connected(
    lxp_daemon_lni_server *server, int descriptor)
{
    struct ucred credential;
    uint64_t connection_generation = 0U;
    bool observed = false;
    lxp_result status;
    if (server == NULL || descriptor < 0 || !server->mutex_initialized)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&server->daemon->mutex) != 0)
        return LXP_ERR_IO;
    status = server->journal_bound && server->journal_descriptor >= 0 &&
            server->daemon->persist_admission == admission_journal_persist &&
            server->daemon->persist_admission_context == server &&
            admission_journal_named(
                server, server->journal_descriptor,
                server->journal_device, server->journal_inode) ?
        LXP_OK : LXP_ERR_CONTEXT_MISMATCH;
    if (pthread_mutex_unlock(&server->daemon->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        status = configure_connection(server, descriptor, &credential,
                                  &connection_generation);
    if (status == LXP_OK) {
        observed = true;
        status = serve_connection(server, descriptor, &credential);
    }
    if (observed) peer_observation_end(server, &credential);
    return status;
}

static bool server_stopping(lxp_daemon_lni_server *server)
{
    bool stopping;
    (void)pthread_mutex_lock(&server->mutex);
    stopping = server->stopping;
    (void)pthread_mutex_unlock(&server->mutex);
    return stopping;
}

static void *server_run(void *context)
{
    lxp_daemon_lni_server *server = (lxp_daemon_lni_server *)context;
    while (!server_stopping(server)) {
        int descriptor = accept(server->listener_descriptor, NULL, NULL);
        lxp_result status;
        if (descriptor < 0) {
            if (errno == EINTR) continue;
            if (server_stopping(server)) break;
            status = LXP_ERR_IO;
        } else {
            (void)pthread_mutex_lock(&server->mutex);
            server->connection_descriptor = descriptor;
            (void)pthread_mutex_unlock(&server->mutex);
            status = lxp_daemon_lni_serve_connected(server, descriptor);
            (void)shutdown(descriptor, SHUT_RDWR);
            (void)close(descriptor);
            (void)pthread_mutex_lock(&server->mutex);
            server->connection_descriptor = -1;
            (void)pthread_mutex_unlock(&server->mutex);
            if (status == LXP_ERR_TRUNCATED || status == LXP_ERR_EXPIRED ||
                status == LXP_ERR_IO ||
                status == LXP_ERR_AUTH_SCOPE || status == LXP_ERR_LENGTH_LIMIT ||
                status == LXP_ERR_MALFORMED_ENVELOPE ||
                status == LXP_ERR_VERSION_UNSUPPORTED)
                status = LXP_OK;
        }
        if (status != LXP_OK) {
            (void)pthread_mutex_lock(&server->mutex);
            server->failure = status;
            server->stopping = true;
            (void)pthread_mutex_unlock(&server->mutex);
        }
    }
    return NULL;
}

lxp_result lxp_daemon_lni_serve(
    lxp_daemon_lni_server *server, lxp_daemon *daemon,
    lxp_daemon_protocol_owner *owner,
    const lxp_daemon_lni_configuration *configuration)
{
    struct sockaddr_un address;
    int descriptor = -1;
    lxp_result status = LXP_OK;
    if (server == NULL || daemon == NULL || owner == NULL ||
        configuration == NULL || configuration->socket_path == NULL ||
        configuration->admission_directory == NULL ||
        !daemon->primitives_initialized || !owner->attached ||
        daemon->config.network_id == 0U ||
        daemon->config.network_id != owner->network_id ||
        configuration->frame_bytes != LXP_DAEMON_LNI_MAX_FRAME_BYTES ||
        configuration->deadline_milliseconds == 0U ||
        configuration->socket_mode != 0660U ||
        configuration->allowed_peer_uid == (uint32_t)geteuid() ||
        strlen(configuration->socket_path) == 0U ||
        strlen(configuration->socket_path) >= sizeof(address.sun_path))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(server, 0, sizeof(*server));
    server->daemon = daemon;
    server->owner = owner;
    server->listener_descriptor = -1;
    server->connection_descriptor = -1;
    server->parent_descriptor = -1;
    server->admission_parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
    server->journal_descriptor = -1;
    if (pthread_mutex_init(&server->mutex, NULL) != 0) return LXP_ERR_IO;
    server->mutex_initialized = true;
    server->allowed_peer_uid = configuration->allowed_peer_uid;
    server->allowed_peer_gid = configuration->allowed_peer_gid;
    (void)memcpy(server->socket_path, configuration->socket_path,
                 strlen(configuration->socket_path) + 1U);
    status = secure_parent_open(server, configuration->socket_path);
    if (status != LXP_OK) goto fail;
    status = secure_admission_parent_open(
        server, configuration->admission_directory);
    if (status != LXP_OK) goto fail;
    status = acquire_lifetime_lock(server);
    if (status != LXP_OK) goto fail;
    status = recover_stale_socket(server);
    if (status != LXP_OK) goto fail;
    status = admission_journal_open(server);
    if (status != LXP_OK) goto fail;
    status = admission_journal_recover(server);
    if (status != LXP_OK) goto fail;
    descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (descriptor < 0) {
        status = LXP_ERR_IO;
        goto fail;
    }
    (void)memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, configuration->socket_path,
                 strlen(configuration->socket_path) + 1U);
    if (bind(descriptor, (struct sockaddr *)&address, sizeof(address)) != 0) {
        status = LXP_ERR_IO;
        goto fail_path;
    }
    status = pin_bound_socket(server);
    if (status != LXP_OK) goto fail_path;
    if (chown(configuration->socket_path, geteuid(),
              (gid_t)configuration->allowed_peer_gid) != 0 ||
        chmod(configuration->socket_path,
              (mode_t)configuration->socket_mode) != 0) {
        status = LXP_ERR_IO;
        goto fail_created;
    }
    status = validate_pinned_socket(server);
    if (status != LXP_OK) goto fail_created;
    if (listen(descriptor, LNI_BACKLOG) != 0) {
        status = LXP_ERR_IO;
        goto fail_created;
    }
    server->frame_bytes = configuration->frame_bytes;
    server->deadline_milliseconds = configuration->deadline_milliseconds;
    server->listener_descriptor = descriptor;
    if (pthread_mutex_lock(&daemon->mutex) != 0) {
        status = LXP_ERR_IO;
        goto fail_created;
    }
    if (daemon->persist_admission != NULL) {
        (void)pthread_mutex_unlock(&daemon->mutex);
        status = LXP_ERR_CONTEXT_MISMATCH;
        goto fail_created;
    }
    daemon->persist_admission = admission_journal_persist;
    daemon->persist_admission_context = server;
    server->journal_bound = true;
    if (pthread_mutex_unlock(&daemon->mutex) != 0) {
        status = LXP_FATAL_INVARIANT;
        goto fail_created;
    }
    if (pthread_create(&server->thread, NULL, server_run, server) != 0) {
        status = LXP_ERR_IO;
        goto fail_created;
    }
    server->started = true;
    return LXP_OK;
fail_created:
    if (server->journal_bound && pthread_mutex_lock(&daemon->mutex) == 0) {
        if (daemon->persist_admission_context == server) {
            daemon->persist_admission = NULL;
            daemon->persist_admission_context = NULL;
        }
        server->journal_bound = false;
        (void)pthread_mutex_unlock(&daemon->mutex);
    }
    (void)unlink_pinned_socket(server);
fail_path:
fail:
    if (descriptor >= 0) (void)close(descriptor);
    if (server->journal_descriptor >= 0)
        (void)close(server->journal_descriptor);
    if (server->lifetime_lock_descriptor >= 0) {
        (void)flock(server->lifetime_lock_descriptor, LOCK_UN);
        (void)close(server->lifetime_lock_descriptor);
    }
    if (server->parent_descriptor >= 0)
        (void)close(server->parent_descriptor);
    if (server->admission_parent_descriptor >= 0)
        (void)close(server->admission_parent_descriptor);
    (void)pthread_mutex_destroy(&server->mutex);
    (void)memset(server, 0, sizeof(*server));
    server->listener_descriptor = -1;
    server->connection_descriptor = -1;
    server->parent_descriptor = -1;
    server->admission_parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
    server->journal_descriptor = -1;
    return status;
}

lxp_result lxp_daemon_lni_stop(lxp_daemon_lni_server *server)
{
    lxp_result status;
    if (server == NULL || !server->started || !server->mutex_initialized)
        return LXP_ERR_NON_CANONICAL;
    (void)pthread_mutex_lock(&server->mutex);
    server->stopping = true;
    if (server->connection_descriptor >= 0)
        (void)shutdown(server->connection_descriptor, SHUT_RDWR);
    if (server->listener_descriptor >= 0)
        (void)shutdown(server->listener_descriptor, SHUT_RDWR);
    (void)pthread_mutex_unlock(&server->mutex);
    status = close(server->listener_descriptor) == 0 || errno == EBADF ?
        LXP_OK : LXP_ERR_IO;
    if (pthread_join(server->thread, NULL) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (pthread_mutex_lock(&server->daemon->mutex) != 0) {
        if (status == LXP_OK) status = LXP_ERR_IO;
    } else {
        if (server->daemon->persist_admission_context == server) {
            server->daemon->persist_admission = NULL;
            server->daemon->persist_admission_context = NULL;
        } else if (status == LXP_OK) {
            status = LXP_ERR_CONTEXT_MISMATCH;
        }
        server->journal_bound = false;
        if (pthread_mutex_unlock(&server->daemon->mutex) != 0 &&
            status == LXP_OK)
            status = LXP_FATAL_INVARIANT;
    }
    if (server->failure != LXP_OK && status == LXP_OK)
        status = server->failure;
    {
        lxp_result unlink_status = unlink_pinned_socket(server);
        if (status == LXP_OK) status = unlink_status;
    }
    if (!pinned_lifetime_lock(server) && status == LXP_OK)
        status = LXP_ERR_AUTH_SCOPE;
    if (!admission_journal_named(
            server, server->journal_descriptor,
            server->journal_device, server->journal_inode) &&
        status == LXP_OK)
        status = LXP_ERR_AUTH_SCOPE;
    if (close(server->journal_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (flock(server->lifetime_lock_descriptor, LOCK_UN) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    if (close(server->lifetime_lock_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (close(server->parent_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (close(server->admission_parent_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    server->started = false;
    server->listener_descriptor = -1;
    server->parent_descriptor = -1;
    server->admission_parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
    server->journal_descriptor = -1;
    if (pthread_mutex_destroy(&server->mutex) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    server->mutex_initialized = false;
    return status;
}

lxp_result lxp_daemon_lni_status(lxp_daemon_lni_server *server)
{
    lxp_result status;
    if (server == NULL || !server->started || !server->mutex_initialized)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&server->mutex) != 0) return LXP_ERR_IO;
    status = server->failure;
    if (pthread_mutex_unlock(&server->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

lxp_result lxp_daemon_lni_observability_snapshot(
    lxp_daemon_lni_server *server,
    lxp_daemon_lni_observability *observability)
{
    lxp_result status = LXP_OK;
    if (server == NULL || observability == NULL ||
        !server->mutex_initialized)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&server->mutex) != 0) return LXP_ERR_IO;
    (void)memset(observability, 0, sizeof(*observability));
    observability->peer_count = server->observed_peer_count;
    observability->evicted_peers = server->evicted_peers;
    observability->evicted_authentication_refusals =
        server->evicted_authentication_refusals;
    (void)memcpy(observability->peers, server->observed_peers,
                 server->observed_peer_count *
                    sizeof(server->observed_peers[0]));
    if (pthread_mutex_unlock(&server->mutex) != 0)
        status = LXP_FATAL_INVARIANT;
    return status;
}
