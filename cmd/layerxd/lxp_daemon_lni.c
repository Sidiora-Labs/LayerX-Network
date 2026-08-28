#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_receipt.h"

#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <poll.h>
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
    LNI_VERSION_MINOR = 0,
    LNI_NODE_INFO_REQUEST = 1,
    LNI_NODE_INFO_RESPONSE = 2,
    LNI_SUBMIT_REQUEST = 3,
    LNI_SUBMIT_RESPONSE = 4,
    LNI_RECEIPT_LOOKUP_REQUEST = 5,
    LNI_RECEIPT_LOOKUP_RESPONSE = 6,
    LNI_ERROR_RESPONSE = 25,
    LNI_ENVELOPE_FIXED_BYTES = 22,
    LNI_NODE_INFO_FIXED_BYTES = 93,
    LNI_BACKLOG = 16
};

static const char LNI_LIFETIME_LOCK_NAME[] = ".layerxd-lni.lock";

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

static lxp_result send_node_info(lxp_daemon_lni_server *server,
                                 int descriptor, uint64_t correlation_id,
                                 int64_t deadline)
{
    static const char *capabilities[] = {
        "node_info", "receipt_lookup", "submit"
    };
    uint8_t payload[256];
    uint8_t checkpoint[32] = {0};
    uint64_t head;
    uint64_t batch;
    size_t cursor = 0U;
    size_t index;
    size_t capability_count = server->daemon->config.role ==
        LXP_DAEMON_SEQUENCER ? 3U : 2U;
    lxp_result status = LXP_OK;
    if (pthread_mutex_lock(&server->daemon->mutex) != 0) return LXP_ERR_IO;
    head = server->daemon->next_sequence == 0U ? 0U :
        server->daemon->next_sequence - 1U;
    if (pthread_mutex_unlock(&server->daemon->mutex) != 0)
        return LXP_FATAL_INVARIANT;
    if (pthread_mutex_lock(&server->owner->mutex) != 0) return LXP_ERR_IO;
    batch = server->owner->receipt_authority->last_batch_number;
    store_u16(payload + cursor, LNI_VERSION_MAJOR); cursor += 2U;
    store_u16(payload + cursor, LNI_VERSION_MINOR); cursor += 2U;
    store_u16(payload + cursor, LXP_PROTOCOL_VERSION); cursor += 2U;
    store_u32(payload + cursor, server->daemon->config.network_id); cursor += 4U;
    payload[cursor++] = role_tag(server->daemon->config.role);
    store_u64(payload + cursor, head); cursor += 8U;
    store_u64(payload + cursor, batch); cursor += 8U;
    (void)memcpy(payload + cursor, checkpoint, 32U); cursor += 32U;
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

static lxp_result send_submit(lxp_daemon_lni_server *server, int descriptor,
                              const lni_envelope *request, int64_t deadline)
{
    lxp_activity activity;
    uint8_t activity_id[32];
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
    if (pthread_mutex_lock(&server->daemon->mutex) != 0) return LXP_ERR_IO;
    if (!server->daemon->accepting)
        status = server->daemon->failure == LXP_OK ?
            LXP_ERR_MODULE_DISABLED : server->daemon->failure;
    else if (server->daemon->queue_count == LXP_DAEMON_QUEUE_CAPACITY ||
             request->payload_length > LXP_DAEMON_QUEUE_MAX_BYTES -
                 server->daemon->queue_bytes)
        status = LXP_ERR_LENGTH_LIMIT;
    if (pthread_mutex_unlock(&server->daemon->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK)
        return send_refusal(descriptor, server->frame_bytes,
                            request->correlation_id, 4U,
                            status == LXP_ERR_LENGTH_LIMIT ? status :
                                LXP_ERR_MODULE_DISABLED,
                            deadline);
    status = lxp_daemon_submit(server->daemon, request->payload,
                               request->payload_length);
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

static bool peer_authorized(const lxp_daemon_lni_server *server,
                            int descriptor)
{
    struct ucred credential;
    socklen_t length = sizeof(credential);
    uint8_t expected[8];
    uint8_t observed[8];
    if (getsockopt(descriptor, SOL_SOCKET, SO_PEERCRED,
                   &credential, &length) != 0 || length != sizeof(credential))
        return false;
    store_u32(expected, server->allowed_peer_uid);
    store_u32(expected + 4U, server->allowed_peer_gid);
    store_u32(observed, (uint32_t)credential.uid);
    store_u32(observed + 4U, (uint32_t)credential.gid);
    return lxp_ct_memcmp(expected, observed, sizeof(expected)) == 0;
}

static lxp_result configure_connection(lxp_daemon_lni_server *server,
                                       int descriptor)
{
    int flags;
    if (!peer_authorized(server, descriptor)) return LXP_ERR_AUTH_SCOPE;
    flags = fcntl(descriptor, F_GETFL, 0);
    if (flags < 0 || fcntl(descriptor, F_SETFL, flags | O_NONBLOCK) != 0)
        return LXP_ERR_IO;
    return LXP_OK;
}

static lxp_result serve_connection(lxp_daemon_lni_server *server,
                                   int descriptor)
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
            status = send_submit(server, descriptor, &request, deadline);
        } else if (request.tag == LNI_RECEIPT_LOOKUP_REQUEST) {
            status = send_receipt(server, descriptor, &request, deadline);
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
            status = configure_connection(server, descriptor);
            if (status == LXP_OK)
                status = serve_connection(server, descriptor);
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
    if (server == NULL || daemon == NULL || owner == NULL ||
        configuration == NULL || configuration->socket_path == NULL ||
        !daemon->primitives_initialized || !owner->attached ||
        configuration->frame_bytes != LXP_DAEMON_LNI_MAX_FRAME_BYTES ||
        configuration->deadline_milliseconds == 0U ||
        configuration->socket_mode != 0660U ||
        configuration->allowed_peer_uid == (uint32_t)geteuid() ||
        strlen(configuration->socket_path) == 0U ||
        strlen(configuration->socket_path) >= sizeof(address.sun_path))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(server, 0, sizeof(*server));
    server->listener_descriptor = -1;
    server->connection_descriptor = -1;
    server->parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
    if (pthread_mutex_init(&server->mutex, NULL) != 0) return LXP_ERR_IO;
    server->mutex_initialized = true;
    server->allowed_peer_uid = configuration->allowed_peer_uid;
    server->allowed_peer_gid = configuration->allowed_peer_gid;
    (void)memcpy(server->socket_path, configuration->socket_path,
                 strlen(configuration->socket_path) + 1U);
    if (secure_parent_open(server, configuration->socket_path) != LXP_OK)
        goto fail;
    if (acquire_lifetime_lock(server) != LXP_OK) goto fail;
    if (recover_stale_socket(server) != LXP_OK) goto fail;
    descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (descriptor < 0) goto fail;
    (void)memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, configuration->socket_path,
                 strlen(configuration->socket_path) + 1U);
    if (bind(descriptor, (struct sockaddr *)&address, sizeof(address)) != 0)
        goto fail_path;
    if (pin_bound_socket(server) != LXP_OK) goto fail_path;
    if (chown(configuration->socket_path, geteuid(),
              (gid_t)configuration->allowed_peer_gid) != 0 ||
        chmod(configuration->socket_path,
              (mode_t)configuration->socket_mode) != 0 ||
        validate_pinned_socket(server) != LXP_OK ||
        listen(descriptor, LNI_BACKLOG) != 0)
        goto fail_created;
    server->daemon = daemon;
    server->owner = owner;
    server->frame_bytes = configuration->frame_bytes;
    server->deadline_milliseconds = configuration->deadline_milliseconds;
    server->listener_descriptor = descriptor;
    if (pthread_create(&server->thread, NULL, server_run, server) != 0)
        goto fail_created;
    server->started = true;
    return LXP_OK;
fail_created:
    (void)unlink_pinned_socket(server);
fail_path:
fail:
    if (descriptor >= 0) (void)close(descriptor);
    if (server->lifetime_lock_descriptor >= 0) {
        (void)flock(server->lifetime_lock_descriptor, LOCK_UN);
        (void)close(server->lifetime_lock_descriptor);
    }
    if (server->parent_descriptor >= 0)
        (void)close(server->parent_descriptor);
    (void)pthread_mutex_destroy(&server->mutex);
    (void)memset(server, 0, sizeof(*server));
    server->listener_descriptor = -1;
    server->connection_descriptor = -1;
    server->parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
    return LXP_ERR_IO;
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
    if (server->failure != LXP_OK && status == LXP_OK)
        status = server->failure;
    {
        lxp_result unlink_status = unlink_pinned_socket(server);
        if (status == LXP_OK) status = unlink_status;
    }
    if (!pinned_lifetime_lock(server) && status == LXP_OK)
        status = LXP_ERR_AUTH_SCOPE;
    if (flock(server->lifetime_lock_descriptor, LOCK_UN) != 0 &&
        status == LXP_OK)
        status = LXP_ERR_IO;
    if (close(server->lifetime_lock_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (close(server->parent_descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    server->started = false;
    server->listener_descriptor = -1;
    server->parent_descriptor = -1;
    server->lifetime_lock_descriptor = -1;
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
