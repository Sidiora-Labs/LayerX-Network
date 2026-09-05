#define _GNU_SOURCE

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_storage.h"

#include "lxp_daemon_lni_internal.h"

#include <openssl/evp.h>

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
#include <pthread.h>
#include <signal.h>
#include <spawn.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <sys/uio.h>
#include <sys/wait.h>
#include <time.h>
#include <unistd.h>

#define REQUIRE(condition) \
    do { \
        if (!(condition)) { \
            (void)fprintf(stderr, "test_daemon_lni_admission:%d: %s\n", \
                          __LINE__, #condition); \
            return 1; \
        } \
    } while (0)

enum {
    NETWORK_ID = 77,
    LNI_MAJOR = 1,
    LNI_MINOR = 4,
    NODE_INFO_REQUEST = 1,
    NODE_INFO_RESPONSE = 2,
    SUBMIT_REQUEST = 3,
    SUBMIT_RESPONSE = 4,
    ERROR_RESPONSE = 25,
    ENVELOPE_FIXED_BYTES = 22,
    JOURNAL_SUPERBLOCK_BYTES = 32,
    JOURNAL_RECORD_BYTES = 64,
    ACTIVITY_CAPACITY = 4096,
    OWNER_SCRATCH_BYTES = 2 * 1024 * 1024,
    INVALID_FLOOD = LXP_DAEMON_QUEUE_CAPACITY + 16,
    WAIT_POLLS = 12000,
    IO_DEADLINE_MILLISECONDS = 10000
};

static const uint8_t REGISTERED_DID[] = "did:layerx:durable-admission";
static const char *test_program_path;
extern char **environ;

typedef struct signer {
    uint8_t private_key[32];
    uint8_t public_key[32];
} signer;

typedef enum apply_mode {
    APPLY_BLOCK = 1,
    APPLY_NOTIFY = 2
} apply_mode;

typedef struct apply_observation {
    uint64_t global_sequence;
    uint8_t activity_id[32];
} apply_observation;

typedef struct blocking_executor {
    pthread_mutex_t mutex;
    pthread_cond_t changed;
    apply_mode mode;
    bool release;
    size_t applied;
    uint64_t next_sequence;
    int notify_descriptor;
    lxp_result failure;
} blocking_executor;

typedef struct admission_fixture {
    lxp_identity_store identities;
    lxp_daemon_protocol_owner owner;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_history history;
    lxp_log canonical_log;
    lxp_arena scratch;
    uint8_t *scratch_bytes;
    lxp_daemon daemon;
    lxp_daemon_lni_server server;
    blocking_executor executor;
    char socket_path[LXP_DAEMON_LNI_SOCKET_PATH_BYTES];
    bool daemon_started;
    bool lni_started;
} admission_fixture;

typedef struct wire_envelope {
    uint8_t *owned;
    size_t owned_length;
    uint16_t major;
    uint16_t minor;
    uint16_t tag;
    uint64_t correlation_id;
    const uint8_t *payload;
    size_t payload_length;
    const uint8_t *proof;
    size_t proof_length;
} wire_envelope;

typedef struct connection_thread {
    lxp_daemon_lni_server *server;
    int descriptor;
    lxp_result status;
} connection_thread;

typedef struct durable_state_snapshot {
    size_t queue_count;
    size_t queue_bytes;
    uint64_t journal_end;
    off_t journal_size;
    size_t journal_entry_count;
} durable_state_snapshot;

typedef struct syscall_observer {
    pthread_mutex_t mutex;
    pthread_cond_t changed;
    int send_descriptor;
    int journal_descriptor;
    uint64_t expected_record_offset;
    const uint8_t *expected_activity;
    size_t expected_activity_length;
    size_t header_written;
    size_t activity_written;
    size_t successful_fdatasyncs;
    uint8_t header[JOURNAL_RECORD_BYTES];
    bool write_ranges_valid;
    bool record_synced;
    bool armed;
    bool send_waiting;
    bool release_send;
} syscall_observer;

typedef struct journal_write_evidence {
    uint8_t header[JOURNAL_RECORD_BYTES];
    size_t header_written;
    size_t activity_written;
    size_t successful_fdatasyncs;
    bool write_ranges_valid;
    bool record_synced;
} journal_write_evidence;

static syscall_observer observer = {
    .mutex = PTHREAD_MUTEX_INITIALIZER,
    .changed = PTHREAD_COND_INITIALIZER,
    .send_descriptor = -1,
    .journal_descriptor = -1
};

ssize_t __real_send(int descriptor, const void *bytes, size_t length,
                    int flags);
int __real_fdatasync(int descriptor);
ssize_t __real_pwrite(int descriptor, const void *bytes, size_t length,
                      off_t offset);

ssize_t __wrap_send(int descriptor, const void *bytes, size_t length,
                    int flags)
{
    bool gate = false;
    if (pthread_mutex_lock(&observer.mutex) == 0) {
        gate = observer.armed && descriptor == observer.send_descriptor;
        if (gate) {
            observer.send_waiting = true;
            (void)pthread_cond_broadcast(&observer.changed);
            while (!observer.release_send)
                if (pthread_cond_wait(&observer.changed, &observer.mutex) != 0)
                    break;
            observer.armed = false;
        }
        (void)pthread_mutex_unlock(&observer.mutex);
    }
    return __real_send(descriptor, bytes, length, flags);
}

int __wrap_fdatasync(int descriptor)
{
    int result = __real_fdatasync(descriptor);
    if (result == 0 && pthread_mutex_lock(&observer.mutex) == 0) {
        if (observer.armed && descriptor == observer.journal_descriptor) {
            ++observer.successful_fdatasyncs;
            if (observer.write_ranges_valid &&
                observer.header_written == JOURNAL_RECORD_BYTES &&
                observer.activity_written == observer.expected_activity_length)
                observer.record_synced = true;
        }
        (void)pthread_mutex_unlock(&observer.mutex);
    }
    return result;
}

ssize_t __wrap_pwrite(int descriptor, const void *bytes, size_t length,
                      off_t offset)
{
    ssize_t result = __real_pwrite(descriptor, bytes, length, offset);
    if (result > 0 && pthread_mutex_lock(&observer.mutex) == 0) {
        if (observer.armed && descriptor == observer.journal_descriptor) {
            size_t written = (size_t)result;
            uint64_t expected_offset;
            if (observer.header_written < JOURNAL_RECORD_BYTES) {
                expected_offset = observer.expected_record_offset +
                    observer.header_written;
                if (offset < 0 || (uint64_t)offset != expected_offset ||
                    written > JOURNAL_RECORD_BYTES - observer.header_written) {
                    observer.write_ranges_valid = false;
                } else {
                    (void)memcpy(observer.header + observer.header_written,
                                 bytes, written);
                    observer.header_written += written;
                }
            } else {
                expected_offset = observer.expected_record_offset +
                    JOURNAL_RECORD_BYTES + observer.activity_written;
                if (offset < 0 || (uint64_t)offset != expected_offset ||
                    written > observer.expected_activity_length -
                        observer.activity_written ||
                    memcmp(bytes, observer.expected_activity +
                           observer.activity_written, written) != 0) {
                    observer.write_ranges_valid = false;
                } else {
                    observer.activity_written += written;
                }
            }
        }
        (void)pthread_mutex_unlock(&observer.mutex);
    }
    return result;
}

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

static int descriptor_write_all(int descriptor, const uint8_t *bytes,
                                size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written > 0) offset += (size_t)written;
        else if (written < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int64_t monotonic_milliseconds(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static int descriptor_read_all_deadline(int descriptor, uint8_t *bytes,
                                        size_t length, int timeout_milliseconds)
{
    size_t offset = 0U;
    int64_t start = monotonic_milliseconds();
    int64_t deadline;
    if (start < 0 || timeout_milliseconds <= 0 ||
        start > INT64_MAX - timeout_milliseconds)
        return 1;
    deadline = start + timeout_milliseconds;
    while (offset < length) {
        struct pollfd pending;
        int64_t now = monotonic_milliseconds();
        int remaining;
        int ready;
        if (now < 0 || now >= deadline) return 1;
        remaining = deadline - now > INT_MAX ?
            INT_MAX : (int)(deadline - now);
        pending.fd = descriptor;
        pending.events = POLLIN;
        pending.revents = 0;
        ready = poll(&pending, 1U, remaining);
        if (ready < 0 && errno == EINTR) continue;
        if (ready <= 0 ||
            (pending.revents & (POLLERR | POLLNVAL)) != 0)
            return 1;
        ssize_t received = read(descriptor, bytes + offset, length - offset);
        if (received > 0) offset += (size_t)received;
        else if (received < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

static int descriptor_read_all(int descriptor, uint8_t *bytes, size_t length)
{
    return descriptor_read_all_deadline(
        descriptor, bytes, length, IO_DEADLINE_MILLISECONDS);
}

static int send_request(int descriptor, uint16_t minor, uint16_t tag,
                        uint64_t correlation_id, const uint8_t *payload,
                        size_t payload_length)
{
    uint8_t prefix[4];
    uint8_t *frame;
    size_t length;
    size_t cursor = 0U;
    int result;
    if ((payload == NULL && payload_length != 0U) ||
        payload_length > UINT32_MAX ||
        payload_length > SIZE_MAX - ENVELOPE_FIXED_BYTES)
        return 1;
    length = ENVELOPE_FIXED_BYTES + payload_length;
    frame = (uint8_t *)malloc(length);
    if (frame == NULL) return 1;
    store_u16(frame + cursor, LNI_MAJOR); cursor += 2U;
    store_u16(frame + cursor, minor); cursor += 2U;
    store_u16(frame + cursor, tag); cursor += 2U;
    store_u64(frame + cursor, correlation_id); cursor += 8U;
    store_u32(frame + cursor, (uint32_t)payload_length); cursor += 4U;
    if (payload_length != 0U) {
        (void)memcpy(frame + cursor, payload, payload_length);
        cursor += payload_length;
    }
    store_u32(frame + cursor, 0U); cursor += 4U;
    store_u32(prefix, (uint32_t)cursor);
    result = descriptor_write_all(descriptor, prefix, sizeof(prefix));
    if (result == 0) result = descriptor_write_all(descriptor, frame, cursor);
    free(frame);
    return result;
}

static int receive_envelope(int descriptor, wire_envelope *envelope)
{
    uint8_t prefix[4];
    uint32_t length;
    uint32_t payload_length;
    uint32_t proof_length;
    size_t cursor = 0U;
    (void)memset(envelope, 0, sizeof(*envelope));
    if (descriptor_read_all(descriptor, prefix, sizeof(prefix)) != 0)
        return 1;
    length = load_u32(prefix);
    if (length < ENVELOPE_FIXED_BYTES ||
        length > LXP_DAEMON_LNI_MAX_FRAME_BYTES)
        return 1;
    envelope->owned = (uint8_t *)malloc(length);
    if (envelope->owned == NULL ||
        descriptor_read_all(descriptor, envelope->owned, length) != 0) {
        free(envelope->owned);
        envelope->owned = NULL;
        return 1;
    }
    envelope->owned_length = length;
    envelope->major = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->minor = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->tag = load_u16(envelope->owned + cursor); cursor += 2U;
    envelope->correlation_id = load_u64(envelope->owned + cursor); cursor += 8U;
    payload_length = load_u32(envelope->owned + cursor); cursor += 4U;
    if ((size_t)payload_length > length - cursor - 4U) return 1;
    envelope->payload = envelope->owned + cursor;
    envelope->payload_length = payload_length;
    cursor += payload_length;
    proof_length = load_u32(envelope->owned + cursor); cursor += 4U;
    if ((size_t)proof_length != length - cursor) return 1;
    envelope->proof = envelope->owned + cursor;
    envelope->proof_length = proof_length;
    return 0;
}

static void release_envelope(wire_envelope *envelope)
{
    free(envelope->owned);
    (void)memset(envelope, 0, sizeof(*envelope));
}

static int signer_init(signer *key, uint8_t seed)
{
    EVP_PKEY *pkey;
    size_t length = 32U;
    int ok;
    (void)memset(key->private_key, seed, sizeof(key->private_key));
    key->private_key[31] = (uint8_t)(seed ^ 0x5aU);
    pkey = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                        key->private_key, 32U);
    ok = pkey != NULL && EVP_PKEY_get_raw_public_key(
        pkey, key->public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(pkey);
    return ok ? 0 : 1;
}

static int sign_raw(const signer *key, const uint8_t *message,
                    size_t message_length, uint8_t signature[64])
{
    EVP_PKEY *pkey = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                   key->private_key, 32U);
    EVP_MD_CTX *context = pkey == NULL ? NULL : EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, pkey) == 1 &&
        EVP_DigestSign(context, signature, &signature_length, message,
                       message_length) == 1 && signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(pkey);
    return ok ? 0 : 1;
}

static int build_activity(const signer *key, uint64_t account_sequence,
                          bool corrupt_signature, uint8_t *output,
                          size_t capacity, size_t *length)
{
    static const uint8_t payload[] = {9U, 8U, 7U};
    uint8_t *arena_storage;
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t preimage[32];
    uint8_t signature[64];
    size_t index;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = NETWORK_ID;
    activity.activity_type = UINT32_C(0x00010001);
    activity.actor_did = (lxp_byte_span){
        REGISTERED_DID, sizeof(REGISTERED_DID) - 1U};
    activity.authority = (lxp_byte_span){key->public_key, 32U};
    activity.account_sequence = account_sequence;
    activity.timestamp_bound.not_before = 1U;
    activity.timestamp_bound.not_after = UINT64_C(4102444800000);
    for (index = 0U; index < 8U; ++index)
        activity.idempotency_key[index] =
            (uint8_t)(account_sequence >> ((7U - index) * 8U));
    activity.idempotency_key[31] = 0xa5U;
    activity.fee_limit = (lxp_u128){0U, 10U};
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) !=
            LXP_OK ||
        lxp_activity_signing_preimage(&activity, preimage) != LXP_OK ||
        sign_raw(key, preimage, sizeof(preimage), signature) != 0)
        return 1;
    if (corrupt_signature) signature[0] ^= 0x01U;
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    arena_storage = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
    if (arena_storage == NULL) return 1;
    if (lxp_arena_init(&arena, arena_storage, LXP_MAX_ACTIVITY_BYTES) !=
            LXP_OK ||
        lxp_activity_encode(&activity, &arena, &encoded) != LXP_OK ||
        encoded.length > capacity) {
        free(arena_storage);
        return 1;
    }
    (void)memcpy(output, encoded.bytes, encoded.length);
    *length = encoded.length;
    free(arena_storage);
    return 0;
}

static lxp_result apply_activity(void *context, uint64_t global_sequence,
                                 const uint8_t *bytes, size_t length)
{
    blocking_executor *executor = (blocking_executor *)context;
    apply_observation observation;
    lxp_activity activity;
    lxp_result status = lxp_activity_decode(bytes, length, &activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK && global_sequence != executor->next_sequence)
        status = LXP_ERR_SEQUENCE_GAP;
    if (status == LXP_OK)
        status = lxp_activity_id(bytes, length, observation.activity_id);
    observation.global_sequence = global_sequence;
    if (pthread_mutex_lock(&executor->mutex) != 0) return LXP_ERR_IO;
    while (status == LXP_OK && executor->mode == APPLY_BLOCK &&
           !executor->release)
        if (pthread_cond_wait(&executor->changed, &executor->mutex) != 0)
            status = LXP_ERR_IO;
    if (status == LXP_OK && executor->notify_descriptor >= 0 &&
        descriptor_write_all(executor->notify_descriptor,
                             (const uint8_t *)&observation,
                             sizeof(observation)) != 0)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        ++executor->applied;
        ++executor->next_sequence;
    } else if (executor->failure == LXP_OK) {
        executor->failure = status;
    }
    if (pthread_mutex_unlock(&executor->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static uint32_t distinct_uid(void)
{
    uint32_t current = (uint32_t)geteuid();
    return current == UINT32_MAX ? current - 1U : current + 1U;
}

static int fixture_start(admission_fixture *fixture,
                         const char *socket_directory,
                         const char *admission_directory,
                         const signer *registered_key, apply_mode mode,
                         int notify_descriptor, lxp_result *lni_status)
{
    lxp_daemon_configuration daemon_configuration;
    lxp_daemon_lni_configuration lni_configuration;
    lxp_identity *identity = NULL;
    int written;
    (void)memset(fixture, 0, sizeof(*fixture));
    fixture->canonical_log.descriptor = -1;
    fixture->executor.notify_descriptor = notify_descriptor;
    fixture->executor.mode = mode;
    fixture->executor.next_sequence = 1U;
    fixture->scratch_bytes = (uint8_t *)malloc(OWNER_SCRATCH_BYTES);
    if (fixture->scratch_bytes == NULL ||
        lxp_arena_init(&fixture->scratch, fixture->scratch_bytes,
                       OWNER_SCRATCH_BYTES) != LXP_OK ||
        pthread_mutex_init(&fixture->owner.mutex, NULL) != 0 ||
        pthread_mutex_init(&fixture->executor.mutex, NULL) != 0 ||
        pthread_cond_init(&fixture->executor.changed, NULL) != 0)
        return 1;
    fixture->history.log = &fixture->canonical_log;
    fixture->owner.identities = &fixture->identities;
    fixture->owner.network_id = NETWORK_ID;
    fixture->owner.protocol_version = LXP_PROTOCOL_VERSION;
    fixture->owner.history = &fixture->history;
    fixture->owner.receipt_authority = &fixture->receipt_authority;
    fixture->owner.scratch = &fixture->scratch;
    fixture->owner.feed_store.baseline_present = true;
    fixture->owner.feed_store.baseline_next_sequence = 1U;
    fixture->owner.feed_store.scanned_through_sequence = 0U;
    fixture->owner.attached = true;
    (void)memcpy(
        fixture->receipt_authority.authorization.public_key,
        registered_key->public_key, 32U);
    if (lxp_identity_register(&fixture->identities, REGISTERED_DID,
                              sizeof(REGISTERED_DID) - 1U,
                              registered_key->public_key,
                              &identity) != LXP_OK || identity == NULL)
        return 1;
    (void)memset(&daemon_configuration, 0, sizeof(daemon_configuration));
    daemon_configuration.role = LXP_DAEMON_SEQUENCER;
    daemon_configuration.network_id = NETWORK_ID;
    daemon_configuration.start_sequence = 1U;
    daemon_configuration.serial_execution = true;
    if (lxp_daemon_start(&fixture->daemon, &daemon_configuration,
                         apply_activity, &fixture->executor) != LXP_OK)
        return 1;
    fixture->daemon_started = true;
    written = snprintf(fixture->socket_path, sizeof(fixture->socket_path),
                       "%s/lni.sock", socket_directory);
    if (written < 0 || (size_t)written >= sizeof(fixture->socket_path))
        return 1;
    (void)memset(&lni_configuration, 0, sizeof(lni_configuration));
    lni_configuration.socket_path = fixture->socket_path;
    lni_configuration.admission_directory = admission_directory;
    lni_configuration.allowed_peer_uid = distinct_uid();
    lni_configuration.allowed_peer_gid = (uint32_t)getegid();
    lni_configuration.frame_bytes = LXP_DAEMON_LNI_MAX_FRAME_BYTES;
    lni_configuration.deadline_milliseconds = 10000U;
    lni_configuration.socket_mode = 0660U;
    {
        lxp_result status = lxp_daemon_lni_serve(
            &fixture->server, &fixture->daemon, &fixture->owner,
            &lni_configuration);
        if (lni_status != NULL) *lni_status = status;
        if (status != LXP_OK) return 1;
    }
    fixture->lni_started = true;

    /* Public startup still enforces a distinct service UID. This explicit
     * private-fixture override bypasses only peer policy so an unprivileged
     * same-UID socketpair can exercise the already-open, recovered, bound
     * production journal, authentication, framing, and queue path. */
    if (pthread_mutex_lock(&fixture->server.mutex) != 0) return 1;
    fixture->server.allowed_peer_uid = (uint32_t)geteuid();
    fixture->server.allowed_peer_gid = (uint32_t)getegid();
    if (pthread_mutex_unlock(&fixture->server.mutex) != 0) return 1;
    return 0;
}

static int queue_state(lxp_daemon *daemon, size_t *count, size_t *bytes)
{
    if (pthread_mutex_lock(&daemon->mutex) != 0) return 1;
    *count = daemon->queue_count;
    *bytes = daemon->queue_bytes;
    return pthread_mutex_unlock(&daemon->mutex) == 0 ? 0 : 1;
}

static int durable_state(admission_fixture *fixture,
                         durable_state_snapshot *snapshot)
{
    struct stat metadata;
    int stat_status;
    int unlock_status;
    if (pthread_mutex_lock(&fixture->daemon.mutex) != 0) return 1;
    snapshot->queue_count = fixture->daemon.queue_count;
    snapshot->queue_bytes = fixture->daemon.queue_bytes;
    snapshot->journal_end = fixture->server.journal_end;
    snapshot->journal_entry_count = fixture->server.journal_entry_count;
    stat_status = fstat(fixture->server.journal_descriptor, &metadata);
    unlock_status = pthread_mutex_unlock(&fixture->daemon.mutex);
    if (stat_status != 0 || unlock_status != 0) return 1;
    snapshot->journal_size = metadata.st_size;
    return 0;
}

static bool durable_state_equal(const durable_state_snapshot *left,
                                const durable_state_snapshot *right)
{
    return left->queue_count == right->queue_count &&
        left->queue_bytes == right->queue_bytes &&
        left->journal_end == right->journal_end &&
        left->journal_size == right->journal_size &&
        left->journal_entry_count == right->journal_entry_count;
}

static int observer_arm(int send_descriptor, int journal_descriptor,
                        uint64_t record_offset,
                        const uint8_t *activity, size_t activity_length)
{
    if (pthread_mutex_lock(&observer.mutex) != 0) return 1;
    observer.send_descriptor = send_descriptor;
    observer.journal_descriptor = journal_descriptor;
    observer.expected_record_offset = record_offset;
    observer.expected_activity = activity;
    observer.expected_activity_length = activity_length;
    observer.header_written = 0U;
    observer.activity_written = 0U;
    observer.successful_fdatasyncs = 0U;
    (void)memset(observer.header, 0, sizeof(observer.header));
    observer.write_ranges_valid = true;
    observer.record_synced = false;
    observer.armed = true;
    observer.send_waiting = false;
    observer.release_send = false;
    return pthread_mutex_unlock(&observer.mutex) == 0 ? 0 : 1;
}

static int observer_wait_for_send(journal_write_evidence *evidence)
{
    struct timespec interval = {0, 1000000L};
    size_t index;
    for (index = 0U; index < WAIT_POLLS; ++index) {
        bool waiting;
        if (pthread_mutex_lock(&observer.mutex) != 0) return 1;
        waiting = observer.send_waiting;
        (void)memcpy(evidence->header, observer.header,
                     sizeof(evidence->header));
        evidence->header_written = observer.header_written;
        evidence->activity_written = observer.activity_written;
        evidence->successful_fdatasyncs = observer.successful_fdatasyncs;
        evidence->write_ranges_valid = observer.write_ranges_valid;
        evidence->record_synced = observer.record_synced;
        if (pthread_mutex_unlock(&observer.mutex) != 0) return 1;
        if (waiting) return 0;
        (void)nanosleep(&interval, NULL);
    }
    return 1;
}

static int observer_release_send(void)
{
    if (pthread_mutex_lock(&observer.mutex) != 0) return 1;
    observer.release_send = true;
    (void)pthread_cond_broadcast(&observer.changed);
    return pthread_mutex_unlock(&observer.mutex) == 0 ? 0 : 1;
}

static int release_and_wait(admission_fixture *fixture, size_t expected)
{
    struct timespec interval = {0, 5000000L};
    size_t poll_index;
    if (pthread_mutex_lock(&fixture->executor.mutex) != 0) return 1;
    fixture->executor.release = true;
    (void)pthread_cond_broadcast(&fixture->executor.changed);
    if (pthread_mutex_unlock(&fixture->executor.mutex) != 0) return 1;
    for (poll_index = 0U; poll_index < WAIT_POLLS; ++poll_index) {
        size_t applied;
        size_t count;
        size_t bytes;
        lxp_result failure;
        if (pthread_mutex_lock(&fixture->executor.mutex) != 0) return 1;
        applied = fixture->executor.applied;
        failure = fixture->executor.failure;
        if (pthread_mutex_unlock(&fixture->executor.mutex) != 0) return 1;
        if (queue_state(&fixture->daemon, &count, &bytes) != 0) return 1;
        if (failure != LXP_OK) return 1;
        if (applied == expected && count == 0U && bytes == 0U) return 0;
        (void)nanosleep(&interval, NULL);
    }
    return 1;
}

static int fixture_stop(admission_fixture *fixture, size_t expected_applied)
{
    int result = 0;
    if (fixture->lni_started &&
        lxp_daemon_lni_stop(&fixture->server) != LXP_OK)
        result = 1;
    fixture->lni_started = false;
    if (fixture->daemon_started &&
        release_and_wait(fixture, expected_applied) != 0)
        result = 1;
    if (fixture->daemon_started &&
        lxp_daemon_shutdown(&fixture->daemon) != LXP_OK)
        result = 1;
    fixture->daemon_started = false;
    if (pthread_cond_destroy(&fixture->executor.changed) != 0 ||
        pthread_mutex_destroy(&fixture->executor.mutex) != 0 ||
        pthread_mutex_destroy(&fixture->owner.mutex) != 0)
        result = 1;
    free(fixture->scratch_bytes);
    fixture->scratch_bytes = NULL;
    return result;
}

static void *connection_run(void *context)
{
    connection_thread *connection = (connection_thread *)context;
    connection->status = lxp_daemon_lni_serve_connected(
        connection->server, connection->descriptor);
    (void)close(connection->descriptor);
    return NULL;
}

static int handshake(int descriptor)
{
    wire_envelope response;
    size_t cursor = 93U;
    uint16_t capability_count;
    size_t index;
    bool durable = false;
    bool complete;
    if (send_request(descriptor, LNI_MINOR, NODE_INFO_REQUEST, 0U,
                     NULL, 0U) != 0 ||
        receive_envelope(descriptor, &response) != 0)
        return 1;
    if (response.major != LNI_MAJOR || response.minor != LNI_MINOR ||
        response.tag != NODE_INFO_RESPONSE || response.correlation_id != 0U ||
        response.proof_length != 0U || response.payload_length < cursor)
        return 1;
    capability_count = load_u16(response.payload + 91U);
    for (index = 0U; index < capability_count; ++index) {
        uint16_t length;
        if (cursor > response.payload_length - 2U) return 1;
        length = load_u16(response.payload + cursor); cursor += 2U;
        if ((size_t)length > response.payload_length - cursor) return 1;
        if (length == sizeof("authenticated_durable_submit") - 1U &&
            memcmp(response.payload + cursor,
                   "authenticated_durable_submit", length) == 0)
            durable = true;
        cursor += length;
    }
    complete = cursor == response.payload_length;
    release_envelope(&response);
    return durable && complete ? 0 : 1;
}

static int expect_error(int descriptor, uint64_t correlation_id,
                        uint8_t refusal_class, lxp_result result)
{
    wire_envelope response;
    if (receive_envelope(descriptor, &response) != 0) return 1;
    if (response.tag != ERROR_RESPONSE ||
        response.correlation_id != correlation_id ||
        response.payload_length != 5U || response.proof_length != 0U ||
        response.payload[0] != refusal_class ||
        (lxp_result)load_u32(response.payload + 1U) != result) {
        release_envelope(&response);
        return 1;
    }
    release_envelope(&response);
    return 0;
}

static int expect_ack(int descriptor, uint64_t correlation_id,
                      const uint8_t *activity, size_t activity_length,
                      const uint8_t activity_id[32])
{
    wire_envelope response;
    if (receive_envelope(descriptor, &response) != 0) return 1;
    if (response.tag != SUBMIT_RESPONSE ||
        response.correlation_id != correlation_id ||
        response.payload_length != activity_length ||
        memcmp(response.payload, activity, activity_length) != 0 ||
        response.proof_length != 32U ||
        memcmp(response.proof, activity_id, 32U) != 0) {
        release_envelope(&response);
        return 1;
    }
    release_envelope(&response);
    return 0;
}

static void cleanup_directories(const char *socket_directory,
                                const char *admission_directory)
{
    char path[LXP_DAEMON_LNI_ADMISSION_PATH_BYTES];
    int written = snprintf(path, sizeof(path), "%s/lni.sock",
                           socket_directory);
    if (written >= 0 && (size_t)written < sizeof(path)) (void)unlink(path);
    written = snprintf(path, sizeof(path), "%s/.layerxd-lni.lock",
                       socket_directory);
    if (written >= 0 && (size_t)written < sizeof(path)) (void)unlink(path);
    written = snprintf(path, sizeof(path),
                       "%s/.layerxd-lni-admission.log",
                       admission_directory);
    if (written >= 0 && (size_t)written < sizeof(path)) (void)unlink(path);
    written = snprintf(path, sizeof(path),
                       "%s/.layerxd-lni-admission.tmp",
                       admission_directory);
    if (written >= 0 && (size_t)written < sizeof(path)) (void)unlink(path);
    (void)rmdir(socket_directory);
    (void)rmdir(admission_directory);
}

static int send_descriptor(int control_descriptor, int descriptor)
{
    uint8_t marker = 1U;
    struct iovec vector = {&marker, sizeof(marker)};
    uint8_t control[CMSG_SPACE(sizeof(descriptor))];
    struct msghdr message;
    struct cmsghdr *header;
    (void)memset(control, 0, sizeof(control));
    (void)memset(&message, 0, sizeof(message));
    message.msg_iov = &vector;
    message.msg_iovlen = 1U;
    message.msg_control = control;
    message.msg_controllen = sizeof(control);
    header = CMSG_FIRSTHDR(&message);
    if (header == NULL) return 1;
    header->cmsg_level = SOL_SOCKET;
    header->cmsg_type = SCM_RIGHTS;
    header->cmsg_len = CMSG_LEN(sizeof(descriptor));
    (void)memcpy(CMSG_DATA(header), &descriptor, sizeof(descriptor));
    return sendmsg(control_descriptor, &message, 0) == (ssize_t)sizeof(marker) ?
        0 : 1;
}

static int receive_descriptor(int control_descriptor, int *descriptor)
{
    struct pollfd pending = {control_descriptor, POLLIN, 0};
    uint8_t marker;
    struct iovec vector = {&marker, sizeof(marker)};
    uint8_t control[CMSG_SPACE(sizeof(*descriptor))];
    struct msghdr message;
    struct cmsghdr *header;
    int ready;
    (void)memset(control, 0, sizeof(control));
    (void)memset(&message, 0, sizeof(message));
    message.msg_iov = &vector;
    message.msg_iovlen = 1U;
    message.msg_control = control;
    message.msg_controllen = sizeof(control);
    do {
        ready = poll(&pending, 1U, IO_DEADLINE_MILLISECONDS);
    } while (ready < 0 && errno == EINTR);
    if (ready != 1 || (pending.revents & POLLIN) == 0 ||
        recvmsg(control_descriptor, &message, 0) != (ssize_t)sizeof(marker) ||
        (message.msg_flags & (MSG_CTRUNC | MSG_TRUNC)) != 0)
        return 1;
    header = CMSG_FIRSTHDR(&message);
    if (header == NULL || header->cmsg_level != SOL_SOCKET ||
        header->cmsg_type != SCM_RIGHTS ||
        header->cmsg_len != CMSG_LEN(sizeof(*descriptor)))
        return 1;
    (void)memcpy(descriptor, CMSG_DATA(header), sizeof(*descriptor));
    if (fcntl(*descriptor, F_SETFD, FD_CLOEXEC) != 0) {
        (void)close(*descriptor);
        *descriptor = -1;
        return 1;
    }
    return 0;
}

static int waitpid_deadline(pid_t child, int *status)
{
    struct timespec interval = {0, 5000000L};
    int64_t start = monotonic_milliseconds();
    int64_t deadline;
    if (start < 0 || start > INT64_MAX - IO_DEADLINE_MILLISECONDS)
        return 1;
    deadline = start + IO_DEADLINE_MILLISECONDS;
    for (;;) {
        pid_t observed = waitpid(child, status, WNOHANG);
        if (observed == child) return 0;
        if (observed < 0 && errno != EINTR) return 1;
        if (monotonic_milliseconds() >= deadline) return 1;
        (void)nanosleep(&interval, NULL);
    }
}

static int wait_child_exit(pid_t child, int expected_exit)
{
    int status = 0;
    bool timed_out = waitpid_deadline(child, &status) != 0;
    if (timed_out) {
        if (kill(child, SIGKILL) != 0 && errno != ESRCH) return 1;
        if (waitpid_deadline(child, &status) != 0) return 1;
    }
    return !timed_out && WIFEXITED(status) &&
        WEXITSTATUS(status) == expected_exit ? 0 : 1;
}

static int try_join_until(pthread_t thread, int64_t deadline, bool *joined)
{
    struct timespec interval = {0, 5000000L};
    *joined = false;
    for (;;) {
        int status = pthread_tryjoin_np(thread, NULL);
        int64_t now;
        if (status == 0) {
            *joined = true;
            return 0;
        }
        if (status != EBUSY) return 1;
        now = monotonic_milliseconds();
        if (now < 0 || now >= deadline) return 0;
        (void)nanosleep(&interval, NULL);
    }
}

static int connection_join_deadline(pthread_t thread, int guard_descriptor)
{
    int64_t now = monotonic_milliseconds();
    int64_t deadline;
    bool joined = false;
    int result = 0;
    if (now < 0 || now > INT64_MAX - IO_DEADLINE_MILLISECONDS)
        result = 1;
    else {
        deadline = now + IO_DEADLINE_MILLISECONDS;
        if (try_join_until(thread, deadline, &joined) != 0) result = 1;
    }
    if (!joined) {
        result = 1;
        if (shutdown(guard_descriptor, SHUT_RDWR) != 0 &&
            errno != ENOTCONN && errno != EINVAL)
            result = 1;
        now = monotonic_milliseconds();
        if (now >= 0 && now <= INT64_MAX - IO_DEADLINE_MILLISECONDS) {
            deadline = now + IO_DEADLINE_MILLISECONDS;
            if (try_join_until(thread, deadline, &joined) != 0) result = 1;
        }
    }
    if (!joined) {
        (void)pthread_cancel(thread);
        now = monotonic_milliseconds();
        if (now >= 0 && now <= INT64_MAX - IO_DEADLINE_MILLISECONDS) {
            deadline = now + IO_DEADLINE_MILLISECONDS;
            (void)try_join_until(thread, deadline, &joined);
        }
    }
    (void)close(guard_descriptor);
    if (!joined) {
        (void)fprintf(stderr,
                      "test_daemon_lni_admission: connection thread did not stop\n");
        _exit(1);
    }
    return result;
}

static int peer_worker(int control_descriptor, size_t attempts)
{
    signer registered_key;
    uint8_t invalid[ACTIVITY_CAPACITY];
    size_t invalid_length;
    size_t index;
    if (signer_init(&registered_key, 0x11U) != 0 ||
        build_activity(&registered_key, 1U, true, invalid,
                       sizeof(invalid), &invalid_length) != 0)
        return 1;
    for (index = 0U; index < attempts; ++index) {
        uint8_t complete = 1U;
        int sockets[2] = {-1, -1};
        if (socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) != 0 ||
            send_descriptor(control_descriptor, sockets[0]) != 0) {
            if (sockets[0] >= 0) (void)close(sockets[0]);
            if (sockets[1] >= 0) (void)close(sockets[1]);
            return 1;
        }
        (void)close(sockets[0]);
        if (handshake(sockets[1]) != 0 ||
            send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST,
                         (uint64_t)index + 1U, invalid, invalid_length) != 0 ||
            expect_error(sockets[1], (uint64_t)index + 1U, 6U,
                         LXP_ERR_BAD_SIGNATURE) != 0 ||
            descriptor_write_all(control_descriptor, &complete,
                                 sizeof(complete)) != 0 ||
            shutdown(sockets[1], SHUT_RDWR) != 0 ||
            close(sockets[1]) != 0)
            return 1;
    }
    return close(control_descriptor) == 0 ? 0 : 1;
}

static int exercise_peer_process(admission_fixture *fixture, size_t attempts,
                                 pid_t *peer_pid)
{
    char descriptor_text[32];
    char attempts_text[32];
    char *worker_arguments[5];
    int control[2];
    pid_t child;
    posix_spawn_file_actions_t actions;
    int spawn_status;
    size_t index;
    int result = 0;
    if (test_program_path == NULL || attempts == 0U ||
        socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, control) != 0)
        return 1;
    if (snprintf(descriptor_text, sizeof(descriptor_text), "%d", 3) < 0 ||
        snprintf(attempts_text, sizeof(attempts_text), "%zu", attempts) < 0 ||
        posix_spawn_file_actions_init(&actions) != 0) {
        (void)close(control[0]);
        (void)close(control[1]);
        return 1;
    }
    spawn_status = posix_spawn_file_actions_adddup2(&actions, control[1], 3);
    if (spawn_status == 0 && control[0] != 3)
        spawn_status = posix_spawn_file_actions_addclose(&actions, control[0]);
    if (spawn_status == 0 && control[1] != 3)
        spawn_status = posix_spawn_file_actions_addclose(&actions, control[1]);
    if (spawn_status == 0)
        spawn_status = posix_spawn_file_actions_addclosefrom_np(&actions, 4);
    worker_arguments[0] = (char *)test_program_path;
    worker_arguments[1] = (char *)"--peer-worker";
    worker_arguments[2] = descriptor_text;
    worker_arguments[3] = attempts_text;
    worker_arguments[4] = NULL;
    if (spawn_status == 0)
        spawn_status = posix_spawn(&child, test_program_path, &actions, NULL,
                                   worker_arguments, environ);
    if (posix_spawn_file_actions_destroy(&actions) != 0 && spawn_status == 0)
        spawn_status = EINVAL;
    if (spawn_status != 0) {
        (void)close(control[0]);
        (void)close(control[1]);
        return 1;
    }
    (void)close(control[1]);
    if (peer_pid != NULL) *peer_pid = child;
    for (index = 0U; index < attempts && result == 0; ++index) {
        durable_state_snapshot before;
        durable_state_snapshot after;
        connection_thread connection;
        pthread_t thread;
        uint8_t complete;
        int descriptor = -1;
        int guard_descriptor = -1;
        bool thread_started = false;
        if (durable_state(fixture, &before) != 0 ||
            receive_descriptor(control[0], &descriptor) != 0)
            result = 1;
        if (result == 0) {
            connection.server = &fixture->server;
            connection.descriptor = descriptor;
            connection.status = LXP_FATAL_INVARIANT;
            guard_descriptor = fcntl(descriptor, F_DUPFD_CLOEXEC, 0);
            if (guard_descriptor < 0 ||
                pthread_create(&thread, NULL, connection_run, &connection) != 0)
                result = 1;
            else
                thread_started = true;
        }
        if (result == 0 && descriptor_read_all_deadline(
                control[0], &complete, sizeof(complete),
                IO_DEADLINE_MILLISECONDS) != 0)
            result = 1;
        if (result == 0 && complete != 1U) result = 1;
        if (result != 0) (void)kill(child, SIGKILL);
        if (thread_started &&
            connection_join_deadline(thread, guard_descriptor) != 0)
            result = 1;
        if (thread_started && connection.status != LXP_OK) result = 1;
        if (!thread_started) {
            if (guard_descriptor >= 0) (void)close(guard_descriptor);
            if (descriptor >= 0) (void)close(descriptor);
        }
        if (result == 0 && (durable_state(fixture, &after) != 0 ||
                            !durable_state_equal(&before, &after)))
            result = 1;
    }
    if (result != 0) (void)kill(child, SIGKILL);
    if (wait_child_exit(child, result == 0 ? 0 : 255) != 0 && result == 0)
        result = 1;
    (void)close(control[0]);
    return result;
}

static const lxp_daemon_lni_peer_observation *observed_peer(
    const lxp_daemon_lni_observability *snapshot, pid_t pid)
{
    size_t index;
    for (index = 0U; index < snapshot->peer_count; ++index)
        if (snapshot->peers[index].pid == (uint32_t)pid &&
            snapshot->peers[index].uid == (uint32_t)geteuid() &&
            snapshot->peers[index].gid == (uint32_t)getegid())
            return &snapshot->peers[index];
    return NULL;
}

static int test_socket_admission(const signer *registered_key,
                                 const signer *foreign_key)
{
    admission_fixture fixture;
    connection_thread connection;
    pthread_t thread;
    lxp_daemon_lni_observability observability;
    struct pollfd readable;
    struct stat journal_metadata;
    durable_state_snapshot before;
    durable_state_snapshot after;
    durable_state_snapshot admitted;
    char socket_directory[] = "/tmp/lxp-lni-socket-XXXXXX";
    char admission_directory[] = "/tmp/lxp-lni-durable-XXXXXX";
    uint8_t invalid[ACTIVITY_CAPACITY];
    uint8_t valid[ACTIVITY_CAPACITY];
    uint8_t wrong_authority[ACTIVITY_CAPACITY];
    uint8_t activity_id[32];
    size_t invalid_length;
    size_t valid_length;
    size_t wrong_length;
    size_t queue_count;
    size_t queue_bytes;
    size_t index;
    journal_write_evidence write_evidence;
    bool queued_slot_valid = false;
    uint64_t correlation = 10U;
    uint64_t retained_refusals;
    uint64_t evicted_peers_before_extra;
    uint64_t expected_refusals;
    pid_t reconnect_peer_pid;
    pid_t distinct_peer_pid;
    pid_t extra_peer_pid;
    int sockets[2];
    int connection_guard;
    REQUIRE(mkdtemp(socket_directory) != NULL);
    REQUIRE(mkdtemp(admission_directory) != NULL);
    REQUIRE(chmod(socket_directory, 0750) == 0);
    REQUIRE(chmod(admission_directory, 0700) == 0);
    REQUIRE(fixture_start(&fixture, socket_directory, admission_directory,
                          registered_key, APPLY_BLOCK, -1, NULL) == 0);
    REQUIRE(socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) == 0);
    connection.server = &fixture.server;
    connection.descriptor = sockets[0];
    connection.status = LXP_FATAL_INVARIANT;
    connection_guard = fcntl(sockets[0], F_DUPFD_CLOEXEC, 0);
    REQUIRE(connection_guard >= 0);
    REQUIRE(pthread_create(&thread, NULL, connection_run, &connection) == 0);
    REQUIRE(handshake(sockets[1]) == 0);

    REQUIRE(build_activity(registered_key, 1U, true, invalid,
                           sizeof(invalid), &invalid_length) == 0);
    REQUIRE(build_activity(registered_key, 1U, false, valid,
                           sizeof(valid), &valid_length) == 0);
    REQUIRE(lxp_activity_id(valid, valid_length, activity_id) == LXP_OK);
    REQUIRE(build_activity(foreign_key, 1U, false, wrong_authority,
                           sizeof(wrong_authority), &wrong_length) == 0);

    /* Signature verification does not wait for queue capacity. A valid frame
     * cannot be acknowledged while the insertion lock is held. The syscall
     * observer calls through to the real fdatasync and gates only the first
     * subsequent response send, exposing the exact post-persistence queue. */
    REQUIRE(durable_state(&fixture, &before) == 0);
    REQUIRE(pthread_mutex_lock(&fixture.daemon.mutex) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         invalid, invalid_length) == 0);
    REQUIRE(expect_error(sockets[1], correlation++, 6U,
                         LXP_ERR_BAD_SIGNATURE) == 0);
    REQUIRE(pthread_mutex_unlock(&fixture.daemon.mutex) == 0);
    REQUIRE(durable_state(&fixture, &after) == 0);
    REQUIRE(durable_state_equal(&before, &after));

    REQUIRE(observer_arm(sockets[0], fixture.server.journal_descriptor,
                         before.journal_end, valid, valid_length) == 0);
    REQUIRE(pthread_mutex_lock(&fixture.daemon.mutex) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         valid, valid_length) == 0);
    readable.fd = sockets[1];
    readable.events = POLLIN;
    readable.revents = 0;
    REQUIRE(poll(&readable, 1U, 150) == 0);
    REQUIRE(pthread_mutex_unlock(&fixture.daemon.mutex) == 0);
    {
        int observation_status = observer_wait_for_send(&write_evidence);
        if (observation_status == 0 &&
            pthread_mutex_lock(&fixture.daemon.mutex) == 0) {
            const lxp_daemon_activity *queued =
                &fixture.daemon.queue[fixture.daemon.queue_head];
            queued_slot_valid = fixture.daemon.queue_count == 1U &&
                fixture.daemon.queue_bytes == valid_length &&
                queued->length == valid_length &&
                memcmp(queued->bytes, valid, valid_length) == 0 &&
                memcmp(queued->activity_id, activity_id, 32U) == 0 &&
                queued->global_sequence == 1U &&
                queued->durable_admission;
            if (pthread_mutex_unlock(&fixture.daemon.mutex) != 0)
                observation_status = 1;
        } else {
            observation_status = 1;
        }
        if (observation_status == 0 &&
            durable_state(&fixture, &admitted) != 0)
            observation_status = 1;
        if (observer_release_send() != 0) observation_status = 1;
        REQUIRE(observation_status == 0);
    }
    REQUIRE(write_evidence.write_ranges_valid);
    REQUIRE(write_evidence.record_synced);
    REQUIRE(write_evidence.header_written == JOURNAL_RECORD_BYTES);
    REQUIRE(write_evidence.activity_written == valid_length);
    REQUIRE(write_evidence.successful_fdatasyncs == 1U);
    REQUIRE(load_u32(write_evidence.header) == UINT32_C(0x4c584152));
    REQUIRE(load_u16(write_evidence.header + 4U) == 1U);
    REQUIRE(load_u16(write_evidence.header + 6U) == JOURNAL_RECORD_BYTES);
    REQUIRE(load_u64(write_evidence.header + 8U) == 1U);
    REQUIRE(load_u32(write_evidence.header + 16U) == valid_length);
    REQUIRE(load_u32(write_evidence.header + 20U) ==
            lxp_log_crc32c(valid, valid_length));
    REQUIRE(memcmp(write_evidence.header + 24U, activity_id, 32U) == 0);
    REQUIRE(load_u32(write_evidence.header + 56U) ==
            lxp_log_crc32c(write_evidence.header, 56U));
    REQUIRE(load_u32(write_evidence.header + 60U) == 0U);
    REQUIRE(queued_slot_valid);
    REQUIRE(admitted.queue_count == 1U);
    REQUIRE(admitted.queue_bytes == valid_length);
    REQUIRE(admitted.journal_entry_count == 1U);
    REQUIRE(admitted.journal_size == (off_t)admitted.journal_end);
    REQUIRE(expect_ack(sockets[1], correlation++, valid, valid_length,
                       activity_id) == 0);
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == 1U && queue_bytes == valid_length);
    REQUIRE(fstat(fixture.server.journal_descriptor, &journal_metadata) == 0);
    REQUIRE(journal_metadata.st_size > 32);
    REQUIRE((uint64_t)journal_metadata.st_size == fixture.server.journal_end);
    REQUIRE(fixture.server.journal_entry_count == 1U);

    /* An ack-loss retry is re-authenticated and receives the same tag-4
     * evidence without a second queue or journal entry. */
    REQUIRE(durable_state(&fixture, &before) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         valid, valid_length) == 0);
    REQUIRE(expect_ack(sockets[1], correlation++, valid, valid_length,
                       activity_id) == 0);
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == 1U && fixture.server.journal_entry_count == 1U);
    REQUIRE(durable_state(&fixture, &after) == 0);
    REQUIRE(durable_state_equal(&before, &after));

    REQUIRE(durable_state(&fixture, &before) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         wrong_authority, wrong_length) == 0);
    REQUIRE(expect_error(sockets[1], correlation++, 6U,
                         LXP_ERR_BAD_SIGNATURE) == 0);
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == 1U);
    REQUIRE(durable_state(&fixture, &after) == 0);
    REQUIRE(durable_state_equal(&before, &after));

    for (index = 0U; index < INVALID_FLOOD; ++index) {
        REQUIRE(durable_state(&fixture, &before) == 0);
        REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST,
                             correlation, invalid, invalid_length) == 0);
        REQUIRE(expect_error(sockets[1], correlation++, 6U,
                             LXP_ERR_BAD_SIGNATURE) == 0);
        REQUIRE(durable_state(&fixture, &after) == 0);
        REQUIRE(durable_state_equal(&before, &after));
    }
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == 1U);

    for (index = 1U; index < LXP_DAEMON_QUEUE_CAPACITY; ++index) {
        REQUIRE(build_activity(registered_key, (uint64_t)index + 1U,
                               false, valid, sizeof(valid),
                               &valid_length) == 0);
        REQUIRE(lxp_activity_id(valid, valid_length, activity_id) == LXP_OK);
        REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST,
                             correlation, valid, valid_length) == 0);
        REQUIRE(expect_ack(sockets[1], correlation++, valid, valid_length,
                           activity_id) == 0);
    }
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == LXP_DAEMON_QUEUE_CAPACITY);
    REQUIRE(fixture.server.journal_entry_count ==
            LXP_DAEMON_QUEUE_CAPACITY);

    REQUIRE(build_activity(registered_key,
                           (uint64_t)LXP_DAEMON_QUEUE_CAPACITY + 1U,
                           false, valid, sizeof(valid), &valid_length) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         valid, valid_length) == 0);
    REQUIRE(expect_error(sockets[1], correlation++, 4U,
                         LXP_ERR_LENGTH_LIMIT) == 0);
    REQUIRE(durable_state(&fixture, &before) == 0);
    REQUIRE(send_request(sockets[1], LNI_MINOR, SUBMIT_REQUEST, correlation,
                         invalid, invalid_length) == 0);
    REQUIRE(expect_error(sockets[1], correlation++, 6U,
                         LXP_ERR_BAD_SIGNATURE) == 0);
    REQUIRE(queue_state(&fixture.daemon, &queue_count, &queue_bytes) == 0);
    REQUIRE(queue_count == LXP_DAEMON_QUEUE_CAPACITY);
    REQUIRE(durable_state(&fixture, &after) == 0);
    REQUIRE(durable_state_equal(&before, &after));

    REQUIRE(shutdown(sockets[1], SHUT_RDWR) == 0);
    REQUIRE(close(sockets[1]) == 0);
    REQUIRE(connection_join_deadline(thread, connection_guard) == 0);
    REQUIRE(connection.status == LXP_OK);
    REQUIRE(lxp_daemon_lni_observability_snapshot(
                &fixture.server, &observability) == LXP_OK);
    REQUIRE(observability.peer_count == 1U);
    {
        const lxp_daemon_lni_peer_observation *peer =
            observed_peer(&observability, getpid());
        REQUIRE(peer != NULL);
        REQUIRE(peer->latest_connection_generation != 0U);
        REQUIRE(peer->authentication_refusals ==
                (uint64_t)INVALID_FLOOD + 3U);
        REQUIRE(!peer->active);
        REQUIRE(peer->active_connections == 0U);
    }

    /* The peer identity is the stable kernel pid/uid/gid triple: two
     * child-created socketpair connections aggregate into one row, while a
     * second child obtains a distinct row. */
    REQUIRE(exercise_peer_process(&fixture, 2U, &reconnect_peer_pid) == 0);
    REQUIRE(lxp_daemon_lni_observability_snapshot(
                &fixture.server, &observability) == LXP_OK);
    REQUIRE(observability.peer_count == 2U);
    {
        const lxp_daemon_lni_peer_observation *peer =
            observed_peer(&observability, reconnect_peer_pid);
        REQUIRE(peer != NULL);
        REQUIRE(peer->authentication_refusals == 2U);
        REQUIRE(peer->latest_connection_generation != 0U);
        REQUIRE(!peer->active);
    }
    REQUIRE(exercise_peer_process(&fixture, 1U, &distinct_peer_pid) == 0);
    REQUIRE(distinct_peer_pid != reconnect_peer_pid);
    REQUIRE(lxp_daemon_lni_observability_snapshot(
                &fixture.server, &observability) == LXP_OK);
    REQUIRE(observability.peer_count == 3U);
    {
        const lxp_daemon_lni_peer_observation *peer =
            observed_peer(&observability, distinct_peer_pid);
        REQUIRE(peer != NULL);
        REQUIRE(peer->authentication_refusals == 1U);
        REQUIRE(!peer->active);
    }

    for (index = 0U; index < LXP_DAEMON_LNI_MAX_OBSERVED_PEERS; ++index)
        REQUIRE(exercise_peer_process(&fixture, 1U, NULL) == 0);
    REQUIRE(lxp_daemon_lni_observability_snapshot(
                &fixture.server, &observability) == LXP_OK);
    REQUIRE(observability.peer_count ==
            LXP_DAEMON_LNI_MAX_OBSERVED_PEERS);
    REQUIRE(observability.evicted_peers != 0U);
    REQUIRE(observability.evicted_authentication_refusals != 0U);
    retained_refusals = 0U;
    for (index = 0U; index < observability.peer_count; ++index) {
        REQUIRE(UINT64_MAX - retained_refusals >=
                observability.peers[index].authentication_refusals);
        retained_refusals +=
            observability.peers[index].authentication_refusals;
    }
    expected_refusals = (uint64_t)INVALID_FLOOD +
        (uint64_t)LXP_DAEMON_LNI_MAX_OBSERVED_PEERS + 6U;
    REQUIRE(UINT64_MAX - retained_refusals >=
            observability.evicted_authentication_refusals);
    REQUIRE(retained_refusals +
            observability.evicted_authentication_refusals ==
            expected_refusals);
    evicted_peers_before_extra = observability.evicted_peers;

    REQUIRE(exercise_peer_process(&fixture, 1U, &extra_peer_pid) == 0);
    REQUIRE(lxp_daemon_lni_observability_snapshot(
                &fixture.server, &observability) == LXP_OK);
    REQUIRE(observability.peer_count ==
            LXP_DAEMON_LNI_MAX_OBSERVED_PEERS);
    REQUIRE(observability.evicted_peers ==
            evicted_peers_before_extra + 1U);
    {
        const lxp_daemon_lni_peer_observation *peer =
            observed_peer(&observability, extra_peer_pid);
        REQUIRE(peer != NULL);
        REQUIRE(peer->authentication_refusals == 1U);
        REQUIRE(!peer->active);
    }
    retained_refusals = 0U;
    for (index = 0U; index < observability.peer_count; ++index) {
        REQUIRE(UINT64_MAX - retained_refusals >=
                observability.peers[index].authentication_refusals);
        retained_refusals +=
            observability.peers[index].authentication_refusals;
    }
    REQUIRE(UINT64_MAX - retained_refusals >=
            observability.evicted_authentication_refusals);
    REQUIRE(retained_refusals +
            observability.evicted_authentication_refusals ==
            expected_refusals + 1U);
    REQUIRE(fixture_stop(&fixture, LXP_DAEMON_QUEUE_CAPACITY) == 0);
    cleanup_directories(socket_directory, admission_directory);
    return 0;
}

static int child_fixture_run(const char *socket_directory,
                             const char *admission_directory,
                             const signer *registered_key,
                             int connection_descriptor,
                             int ready_descriptor,
                             int notify_descriptor, apply_mode mode)
{
    admission_fixture fixture;
    uint8_t ready = 1U;
    if (fixture_start(&fixture, socket_directory, admission_directory,
                      registered_key, mode, notify_descriptor, NULL) != 0)
        return 1;
    if (descriptor_write_all(ready_descriptor, &ready, sizeof(ready)) != 0)
        return 1;
    (void)close(ready_descriptor);
    if (connection_descriptor >= 0) {
        lxp_result status = lxp_daemon_lni_serve_connected(
            &fixture.server, connection_descriptor);
        return status == LXP_OK ? 0 : 1;
    }
    for (;;) pause();
}

static int wait_killed(pid_t child)
{
    int status = 0;
    if (kill(child, SIGKILL) != 0 && errno != ESRCH) return 1;
    if (waitpid_deadline(child, &status) != 0) return 1;
    return WIFSIGNALED(status) && WTERMSIG(status) == SIGKILL ? 0 : 1;
}

static int no_notification_deadline(int descriptor,
                                    int timeout_milliseconds)
{
    struct pollfd pending = {descriptor, POLLIN, 0};
    int result;
    do {
        result = poll(&pending, 1U, timeout_milliseconds);
    } while (result < 0 && errno == EINTR);
    if (result == 0) return 0;
    return 1;
}

static int test_crash_recovery(const signer *registered_key)
{
    char socket_directory[] = "/tmp/lxp-lni-crash-socket-XXXXXX";
    char admission_directory[] = "/tmp/lxp-lni-crash-durable-XXXXXX";
    char journal_path[LXP_DAEMON_LNI_ADMISSION_PATH_BYTES];
    uint8_t activity[ACTIVITY_CAPACITY];
    uint8_t activity_id[32];
    uint8_t ready;
    size_t activity_length;
    apply_observation observation;
    int sockets[2];
    int ready_pipe[2];
    int notify_pipe[2];
    int journal;
    int phase_status;
    int written;
    off_t durable_journal_size = 0;
    pid_t child;
    REQUIRE(mkdtemp(socket_directory) != NULL);
    REQUIRE(mkdtemp(admission_directory) != NULL);
    REQUIRE(chmod(socket_directory, 0750) == 0);
    REQUIRE(chmod(admission_directory, 0700) == 0);
    REQUIRE(build_activity(registered_key, 1U, false, activity,
                           sizeof(activity), &activity_length) == 0);
    REQUIRE(lxp_activity_id(activity, activity_length, activity_id) == LXP_OK);
    REQUIRE(socketpair(AF_UNIX, SOCK_STREAM, 0, sockets) == 0);
    REQUIRE(pipe(ready_pipe) == 0);
    child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        (void)close(sockets[0]);
        (void)close(ready_pipe[0]);
        _exit(child_fixture_run(socket_directory, admission_directory,
                                registered_key, sockets[1], ready_pipe[1],
                                -1, APPLY_BLOCK));
    }
    (void)close(sockets[1]);
    (void)close(ready_pipe[1]);
    phase_status = 0;
    if (descriptor_read_all_deadline(ready_pipe[0], &ready, sizeof(ready),
                                     IO_DEADLINE_MILLISECONDS) != 0)
        phase_status = 1;
    if (close(ready_pipe[0]) != 0) phase_status = 1;
    if (phase_status == 0 && handshake(sockets[0]) != 0) phase_status = 1;
    if (phase_status == 0 && send_request(
            sockets[0], LNI_MINOR, SUBMIT_REQUEST, 1U,
            activity, activity_length) != 0)
        phase_status = 1;
    if (phase_status == 0 && expect_ack(
            sockets[0], 1U, activity, activity_length, activity_id) != 0)
        phase_status = 1;
    if (phase_status == 0) {
        struct stat metadata;
        written = snprintf(journal_path, sizeof(journal_path),
                           "%s/.layerxd-lni-admission.log",
                           admission_directory);
        if (written < 0 || (size_t)written >= sizeof(journal_path) ||
            stat(journal_path, &metadata) != 0 || metadata.st_size <= 32) {
            phase_status = 1;
        } else {
            durable_journal_size = metadata.st_size;
        }
    }
    if (wait_killed(child) != 0) phase_status = 1;
    if (close(sockets[0]) != 0) phase_status = 1;
    REQUIRE(phase_status == 0);

    REQUIRE(pipe(ready_pipe) == 0);
    REQUIRE(pipe(notify_pipe) == 0);
    child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        (void)close(ready_pipe[0]);
        (void)close(notify_pipe[0]);
        _exit(child_fixture_run(socket_directory, admission_directory,
                                registered_key, -1, ready_pipe[1],
                                notify_pipe[1], APPLY_NOTIFY));
    }
    (void)close(ready_pipe[1]);
    (void)close(notify_pipe[1]);
    phase_status = 0;
    if (descriptor_read_all_deadline(ready_pipe[0], &ready, sizeof(ready),
                                     IO_DEADLINE_MILLISECONDS) != 0)
        phase_status = 1;
    if (close(ready_pipe[0]) != 0) phase_status = 1;
    if (phase_status == 0 && descriptor_read_all_deadline(
            notify_pipe[0], (uint8_t *)&observation, sizeof(observation),
            IO_DEADLINE_MILLISECONDS) != 0)
        phase_status = 1;
    if (phase_status == 0 &&
        (observation.global_sequence != 1U ||
         memcmp(observation.activity_id, activity_id, 32U) != 0))
        phase_status = 1;
    if (phase_status == 0 &&
        no_notification_deadline(notify_pipe[0], 250) != 0)
        phase_status = 1;
    if (wait_killed(child) != 0) phase_status = 1;
    if (close(notify_pipe[0]) != 0) phase_status = 1;
    REQUIRE(phase_status == 0);

    journal = open(journal_path, O_WRONLY | O_APPEND | O_CLOEXEC);
    REQUIRE(journal >= 0);
    ready = 0xa5U;
    REQUIRE(descriptor_write_all(journal, &ready, sizeof(ready)) == 0);
    REQUIRE(fdatasync(journal) == 0);
    REQUIRE(close(journal) == 0);

    /* Recovery retains the fully validated acknowledged prefix and removes
     * only the incomplete terminal append before replaying the durable item. */
    REQUIRE(pipe(ready_pipe) == 0);
    REQUIRE(pipe(notify_pipe) == 0);
    child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        (void)close(ready_pipe[0]);
        (void)close(notify_pipe[0]);
        _exit(child_fixture_run(socket_directory, admission_directory,
                                registered_key, -1, ready_pipe[1],
                                notify_pipe[1], APPLY_NOTIFY));
    }
    (void)close(ready_pipe[1]);
    (void)close(notify_pipe[1]);
    phase_status = 0;
    if (descriptor_read_all_deadline(ready_pipe[0], &ready, sizeof(ready),
                                     IO_DEADLINE_MILLISECONDS) != 0)
        phase_status = 1;
    if (close(ready_pipe[0]) != 0) phase_status = 1;
    if (phase_status == 0 && descriptor_read_all_deadline(
            notify_pipe[0], (uint8_t *)&observation, sizeof(observation),
            IO_DEADLINE_MILLISECONDS) != 0)
        phase_status = 1;
    if (phase_status == 0 &&
        (observation.global_sequence != 1U ||
         memcmp(observation.activity_id, activity_id, 32U) != 0))
        phase_status = 1;
    if (wait_killed(child) != 0) phase_status = 1;
    if (close(notify_pipe[0]) != 0) phase_status = 1;
    if (phase_status == 0) {
        struct stat metadata;
        if (stat(journal_path, &metadata) != 0 ||
            metadata.st_size != durable_journal_size)
            phase_status = 1;
    }
    REQUIRE(phase_status == 0);

    /* A complete record with a corrupted header checksum is never treated as
     * a torn tail; startup returns the typed corruption result and fails
     * closed. */
    journal = open(journal_path, O_RDWR | O_CLOEXEC);
    REQUIRE(journal >= 0);
    REQUIRE(pread(journal, &ready, sizeof(ready), 32 + 56) ==
            (ssize_t)sizeof(ready));
    ready ^= 0x01U;
    REQUIRE(pwrite(journal, &ready, sizeof(ready), 32 + 56) ==
            (ssize_t)sizeof(ready));
    REQUIRE(fdatasync(journal) == 0);
    REQUIRE(close(journal) == 0);
    REQUIRE(pipe(ready_pipe) == 0);
    child = fork();
    REQUIRE(child >= 0);
    if (child == 0) {
        admission_fixture fixture;
        lxp_result lni_status = LXP_FATAL_INVARIANT;
        int started;
        (void)close(ready_pipe[0]);
        started = fixture_start(&fixture, socket_directory,
                                admission_directory, registered_key,
                                APPLY_BLOCK, -1, &lni_status);
        if (descriptor_write_all(ready_pipe[1],
                (const uint8_t *)&lni_status, sizeof(lni_status)) != 0)
            _exit(3);
        (void)close(ready_pipe[1]);
        _exit(started == 0 ? 2 : 0);
    }
    (void)close(ready_pipe[1]);
    {
        lxp_result lni_status = LXP_OK;
        phase_status = descriptor_read_all_deadline(
            ready_pipe[0], (uint8_t *)&lni_status, sizeof(lni_status),
            IO_DEADLINE_MILLISECONDS);
        if (close(ready_pipe[0]) != 0) phase_status = 1;
        if (wait_child_exit(child, 0) != 0) phase_status = 1;
        if (lni_status != LXP_ERR_LOG_CORRUPT) phase_status = 1;
    }
    REQUIRE(phase_status == 0);
    cleanup_directories(socket_directory, admission_directory);
    return 0;
}

int main(int argument_count, char **arguments)
{
    signer registered_key;
    signer foreign_key;
    test_program_path = argument_count > 0 ? arguments[0] : NULL;
    if (argument_count == 4 &&
        strcmp(arguments[1], "--peer-worker") == 0) {
        char *descriptor_end = NULL;
        char *attempts_end = NULL;
        unsigned long descriptor;
        unsigned long long attempts;
        errno = 0;
        descriptor = strtoul(arguments[2], &descriptor_end, 10);
        attempts = strtoull(arguments[3], &attempts_end, 10);
        if (errno != 0 || descriptor_end == arguments[2] ||
            *descriptor_end != '\0' || attempts_end == arguments[3] ||
            *attempts_end != '\0' || descriptor > INT_MAX || attempts == 0U ||
            attempts > SIZE_MAX)
            return 1;
        return peer_worker((int)descriptor, (size_t)attempts);
    }
    REQUIRE(signer_init(&registered_key, 0x11U) == 0);
    REQUIRE(signer_init(&foreign_key, 0x22U) == 0);
    REQUIRE(memcmp(registered_key.public_key, foreign_key.public_key, 32U) !=
            0);
    REQUIRE(test_crash_recovery(&registered_key) == 0);
    REQUIRE(test_socket_admission(&registered_key, &foreign_key) == 0);
    return 0;
}
