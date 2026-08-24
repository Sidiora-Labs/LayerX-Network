#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include "layerx/lxp_crypto.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/in.h>
#include <poll.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

enum {
    REPLICA_REQUEST_MAX = LXP_MAX_ACTIVITY_BYTES * 3U,
    REPLICA_RESPONSE_MAX = LXP_MAX_ACTIVITY_BYTES * 4U,
    REPLICA_SCRATCH_BYTES = LXP_MAX_ACTIVITY_BYTES * 4U,
    REPLICA_HEADER_MAX = 16384,
    REPLICA_MAX_CONNECTIONS = 16,
    REPLICA_ACCEPT_POLL_MILLISECONDS = 250,
    REPLICA_IO_DEADLINE_MILLISECONDS = 5000
};

static const uint8_t replica_magic[5] = {'L', 'X', 'A', 'R', '1'};
static volatile sig_atomic_t replica_stop;

typedef struct authority_replica {
    lxp_log log;
    lxp_daemon_receipt_authority_store store;
    lxp_sequencer_authorization authorization;
    uint8_t replica_id[32];
    uint8_t bearer_token[LXP_DAEMON_BEARER_MAX_BYTES];
    size_t bearer_token_length;
    uint8_t *scratch_bytes;
    lxp_arena scratch;
    pthread_mutex_t mutex;
    pthread_cond_t connections_changed;
    int connections[REPLICA_MAX_CONNECTIONS];
    size_t active_connections;
    bool log_open;
    bool mutex_initialized;
    bool condition_initialized;
    bool stopping;
} authority_replica;

typedef struct authority_connection {
    authority_replica *replica;
    int descriptor;
    size_t slot;
} authority_connection;

static void stop_replica(int signal_number)
{
    (void)signal_number;
    replica_stop = 1;
}

static uint16_t read_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static void write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static int64_t deadline_after(int milliseconds)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000 +
           milliseconds;
}

static int deadline_remaining(int64_t deadline)
{
    struct timespec now;
    int64_t remaining;
    if (deadline < 0 || clock_gettime(CLOCK_MONOTONIC, &now) != 0)
        return -1;
    remaining = deadline -
        ((int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000);
    if (remaining <= 0) return 0;
    return remaining > INT_MAX ? INT_MAX : (int)remaining;
}

static int wait_ready(int descriptor, short events, int64_t deadline)
{
    struct pollfd watched;
    for (;;) {
        int remaining = deadline_remaining(deadline);
        int ready;
        if (remaining <= 0) return -1;
        watched.fd = descriptor;
        watched.events = events;
        watched.revents = 0;
        ready = poll(&watched, 1U, remaining);
        if (ready > 0) {
            if ((watched.revents & POLLNVAL) != 0) return -1;
            if ((watched.revents &
                 (events | POLLERR | POLLHUP)) != 0)
                return 0;
            continue;
        }
        if (ready < 0 && errno == EINTR) continue;
        return -1;
    }
}

static int configure_connection(int descriptor)
{
    struct timeval timeout;
    int flags = fcntl(descriptor, F_GETFL, 0);
    int descriptor_flags = fcntl(descriptor, F_GETFD, 0);
    timeout.tv_sec = REPLICA_IO_DEADLINE_MILLISECONDS / 1000;
    timeout.tv_usec =
        (REPLICA_IO_DEADLINE_MILLISECONDS % 1000) * 1000;
    if (flags < 0 || descriptor_flags < 0 ||
        fcntl(descriptor, F_SETFL, flags | O_NONBLOCK) != 0 ||
        fcntl(descriptor, F_SETFD, descriptor_flags | FD_CLOEXEC) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_RCVTIMEO,
                   &timeout, sizeof(timeout)) != 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_SNDTIMEO,
                   &timeout, sizeof(timeout)) != 0)
        return -1;
    return 0;
}

static ssize_t read_some_until(int descriptor, uint8_t *bytes,
                               size_t length, int64_t deadline)
{
    for (;;) {
        ssize_t count;
        if (wait_ready(descriptor, POLLIN, deadline) != 0) return -1;
        count = recv(descriptor, bytes, length, 0);
        if (count >= 0) return count;
        if (errno == EINTR || errno == EAGAIN || errno == EWOULDBLOCK)
            continue;
        return -1;
    }
}

static int write_exact_until(int descriptor, const uint8_t *bytes,
                             size_t length, int64_t deadline)
{
    size_t written = 0U;
    while (written < length) {
        ssize_t count;
        if (wait_ready(descriptor, POLLOUT, deadline) != 0) return -1;
        count = send(descriptor, bytes + written, length - written,
                     MSG_NOSIGNAL);
        if (count > 0) written += (size_t)count;
        else if (count < 0 &&
                 (errno == EINTR || errno == EAGAIN ||
                  errno == EWOULDBLOCK))
            continue;
        else return -1;
    }
    return 0;
}

static int read_exact_until(int descriptor, uint8_t *bytes, size_t length,
                            int64_t deadline)
{
    size_t read_count = 0U;
    while (read_count < length) {
        ssize_t count = read_some_until(
            descriptor, bytes + read_count, length - read_count, deadline);
        if (count > 0) read_count += (size_t)count;
        else return -1;
    }
    return 0;
}

static int connect_until(int descriptor, const struct sockaddr *address,
                         socklen_t address_length, int64_t deadline)
{
    int error = 0;
    socklen_t error_length = sizeof(error);
    if (connect(descriptor, address, address_length) == 0) return 0;
    if (errno != EINTR && errno != EINPROGRESS &&
        errno != EAGAIN && errno != EWOULDBLOCK)
        return -1;
    if (wait_ready(descriptor, POLLOUT, deadline) != 0 ||
        getsockopt(descriptor, SOL_SOCKET, SO_ERROR,
                   &error, &error_length) != 0 || error != 0)
        return -1;
    return 0;
}

static int hex_nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

static lxp_result decode_hex32(const char *text, uint8_t output[32])
{
    size_t index;
    if (text == NULL || strlen(text) != 64U) return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < 32U; ++index) {
        int high = hex_nibble(text[index * 2U]);
        int low = hex_nibble(text[index * 2U + 1U]);
        if (high < 0 || low < 0) return LXP_ERR_NON_CANONICAL;
        output[index] = (uint8_t)(((unsigned int)high << 4U) |
                                 (unsigned int)low);
    }
    return LXP_OK;
}

static void hex_encode(const uint8_t *bytes, size_t length, char *output)
{
    static const char alphabet[] = "0123456789abcdef";
    size_t index;
    for (index = 0U; index < length; ++index) {
        output[index * 2U] = alphabet[bytes[index] >> 4U];
        output[index * 2U + 1U] = alphabet[bytes[index] & 15U];
    }
    output[length * 2U] = '\0';
}

static lxp_result append_wire(authority_replica *replica,
                              const uint8_t *body, size_t length)
{
    uint16_t header_length;
    uint32_t receipt_length;
    uint32_t leaf_index;
    uint32_t leaf_count;
    uint8_t depth;
    size_t offset = 0U;
    size_t proof_bytes;
    lxp_merkle_proof proof;
    lxp_result status;
    if (replica == NULL || body == NULL || length < 5U + 2U + 64U + 9U + 4U ||
        memcmp(body, replica_magic, sizeof(replica_magic)) != 0)
        return LXP_ERR_NON_CANONICAL;
    offset += sizeof(replica_magic);
    header_length = read_u16(body + offset); offset += 2U;
    if (header_length != LXP_BATCH_HEADER_ENCODED_SIZE ||
        header_length > length - offset)
        return LXP_ERR_NON_CANONICAL;
    {
        const uint8_t *header = body + offset;
        const uint8_t *signature;
        const uint8_t *receipt;
        offset += header_length;
        if (64U > length - offset) return LXP_ERR_TRUNCATED;
        signature = body + offset; offset += 64U;
        if (9U > length - offset) return LXP_ERR_TRUNCATED;
        depth = body[offset++];
        leaf_index = read_u32(body + offset); offset += 4U;
        leaf_count = read_u32(body + offset); offset += 4U;
        if (depth > LXP_MERKLE_MAX_DEPTH) return LXP_ERR_LENGTH_LIMIT;
        proof_bytes = (size_t)depth * 32U;
        if (proof_bytes + 4U > length - offset) return LXP_ERR_TRUNCATED;
        (void)memset(&proof, 0, sizeof(proof));
        proof.depth = depth;
        proof.leaf_index = leaf_index;
        proof.leaf_count = leaf_count;
        (void)memcpy(proof.siblings, body + offset, proof_bytes);
        offset += proof_bytes;
        receipt_length = read_u32(body + offset); offset += 4U;
        if (receipt_length == 0U || receipt_length != length - offset)
            return LXP_ERR_NON_CANONICAL;
        receipt = body + offset;
        if (pthread_mutex_lock(&replica->mutex) != 0) return LXP_ERR_IO;
        {
            size_t mark = lxp_arena_mark(&replica->scratch);
            status = lxp_daemon_receipt_authority_append(
                &replica->store, receipt, receipt_length, header,
                header_length, signature, &proof, &replica->scratch);
            (void)lxp_arena_reset(&replica->scratch, mark);
        }
        if (pthread_mutex_unlock(&replica->mutex) != 0 && status == LXP_OK)
            status = LXP_FATAL_INVARIANT;
    }
    return status;
}

static lxp_result evidence_json(authority_replica *replica,
                                const uint8_t batch_id[32],
                                const uint8_t receipt_digest[32],
                                char **body, size_t *body_length)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_codec_writer proof_writer;
    char *response;
    char replica_hex[65];
    char key_hex[65];
    char header_hex[LXP_BATCH_HEADER_ENCODED_SIZE * 2U + 1U];
    char signature_hex[129];
    char *proof_hex;
    size_t capacity;
    size_t mark;
    int length;
    lxp_result status;
    if (pthread_mutex_lock(&replica->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(&replica->scratch);
    status = lxp_daemon_receipt_authority_lookup(
        &replica->store, receipt_digest, &replica->scratch, &evidence);
    if (status == LXP_OK &&
        lxp_ct_memcmp(evidence.batch_id, batch_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_codec_writer_init(
            &proof_writer, &replica->scratch,
            16U + LXP_MERKLE_MAX_DEPTH * 32U);
    if (status == LXP_OK)
        status = lxp_merkle_proof_encode(&proof_writer,
                                         &evidence.receipt_proof);
    if (status == LXP_OK) {
        proof_hex = (char *)malloc(proof_writer.length * 2U + 1U);
        capacity = 1024U + proof_writer.length * 2U;
        response = (char *)malloc(capacity);
        if (proof_hex == NULL || response == NULL) {
            free(proof_hex); free(response); status = LXP_ERR_IO;
        } else {
            hex_encode(replica->replica_id, 32U, replica_hex);
            hex_encode(replica->authorization.public_key, 32U, key_hex);
            hex_encode(evidence.canonical_header.bytes,
                       evidence.canonical_header.length, header_hex);
            hex_encode(evidence.header_signature, 64U, signature_hex);
            hex_encode(proof_writer.bytes, proof_writer.length, proof_hex);
            length = snprintf(response, capacity,
                "{\"authority_replica_id\":\"%s\","
                "\"sequencer_public_key\":\"%s\","
                "\"batch_evidence\":{\"header_hex\":\"%s\","
                "\"header_signature\":\"%s\","
                "\"receipt_proof_hex\":\"%s\"}}",
                replica_hex, key_hex, header_hex, signature_hex, proof_hex);
            free(proof_hex);
            if (length < 0 || (size_t)length >= capacity) {
                free(response); status = LXP_ERR_LENGTH_LIMIT;
            } else {
                *body = response;
                *body_length = (size_t)length;
            }
        }
    }
    (void)lxp_arena_reset(&replica->scratch, mark);
    if (pthread_mutex_unlock(&replica->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static bool authorized(const authority_replica *replica,
                       const char *request, const char *header_end)
{
    static const char prefix[] = "Authorization: Bearer ";
    const char *line = strstr(request, prefix);
    const char *end;
    if (line == NULL || line >= header_end) return false;
    line += sizeof(prefix) - 1U;
    end = strstr(line, "\r\n");
    return end != NULL && end <= header_end &&
           (size_t)(end - line) == replica->bearer_token_length &&
           lxp_ct_memcmp(line, replica->bearer_token,
                         replica->bearer_token_length) == 0;
}

static lxp_result serve_connection(authority_replica *replica, int descriptor)
{
    char headers[REPLICA_HEADER_MAX + 1U];
    size_t used = 0U;
    char *header_end = NULL;
    char method[8];
    char path[4096];
    size_t content_length = 0U;
    char *response_body = NULL;
    size_t response_length = 0U;
    uint16_t http_status = 503U;
    lxp_result status = LXP_OK;
    int64_t request_deadline =
        deadline_after(REPLICA_IO_DEADLINE_MILLISECONDS);
    while (header_end == NULL && used < REPLICA_HEADER_MAX) {
        ssize_t count = read_some_until(
            descriptor, (uint8_t *)headers + used,
            REPLICA_HEADER_MAX - used, request_deadline);
        if (count > 0) used += (size_t)count;
        else return LXP_ERR_IO;
        headers[used] = '\0';
        header_end = strstr(headers, "\r\n\r\n");
    }
    if (header_end == NULL || sscanf(headers, "%7s %4095s HTTP/1.1", method,
                                     path) != 2)
        return LXP_ERR_NON_CANONICAL;
    if (!authorized(replica, headers, header_end)) {
        status = LXP_ERR_BAD_SIGNATURE;
        http_status = 401U;
    } else if (strcmp(method, "POST") == 0 &&
               strcmp(path, "/v1/receipt-authority/ingest") == 0) {
        const char *length_header = strstr(headers, "Content-Length: ");
        uint8_t *body;
        size_t header_bytes = (size_t)(header_end + 4U - headers);
        size_t buffered = used - header_bytes;
        if (length_header == NULL ||
            sscanf(length_header, "Content-Length: %zu", &content_length) != 1 ||
            content_length == 0U || content_length > REPLICA_REQUEST_MAX ||
            buffered > content_length) {
            status = LXP_ERR_LENGTH_LIMIT;
        } else {
            body = (uint8_t *)malloc(content_length);
            if (body == NULL) status = LXP_ERR_IO;
            else {
                (void)memcpy(body, headers + header_bytes, buffered);
                if (read_exact_until(
                        descriptor, body + buffered,
                        content_length - buffered, request_deadline) != 0)
                    status = LXP_ERR_IO;
                if (status == LXP_OK)
                    status = append_wire(replica, body, content_length);
                free(body);
            }
        }
        if (status == LXP_OK) {
            char replica_hex[65];
            hex_encode(replica->replica_id, 32U, replica_hex);
            response_body = (char *)malloc(96U);
            if (response_body == NULL) status = LXP_ERR_IO;
            else {
                int length = snprintf(response_body, 96U,
                    "{\"authority_replica_id\":\"%s\"}", replica_hex);
                if (length < 0 || length >= 96) status = LXP_ERR_LENGTH_LIMIT;
                else response_length = (size_t)length;
            }
        }
        http_status = status == LXP_OK ? 201U : 503U;
    } else if (strcmp(method, "GET") == 0 &&
               strncmp(path, "/v1/batches/", 12U) == 0) {
        const char *tail = strstr(path + 12U,
            "/receipt-authority?receipt_digest=");
        char batch_text[65];
        uint8_t batch_id[32];
        uint8_t receipt_digest[32];
        if (tail == NULL || (size_t)(tail - (path + 12U)) != 64U ||
            strlen(tail + 34U) != 64U) {
            status = LXP_ERR_NON_CANONICAL;
        } else {
            (void)memcpy(batch_text, path + 12U, 64U);
            batch_text[64] = '\0';
            status = decode_hex32(batch_text, batch_id);
            if (status == LXP_OK)
                status = decode_hex32(tail + 34U, receipt_digest);
            if (status == LXP_OK)
                status = evidence_json(replica, batch_id, receipt_digest,
                                       &response_body, &response_length);
        }
        http_status = status == LXP_OK ? 200U : 404U;
    } else {
        status = LXP_ERR_UNKNOWN_ACTIVITY;
        http_status = 404U;
    }
    if (response_body == NULL) {
        response_body = (char *)malloc(64U);
        if (response_body == NULL) return LXP_ERR_IO;
        {
            int length = snprintf(response_body, 64U,
                                  "{\"error\":%d}", status);
            if (length < 0 || length >= 64) { free(response_body); return LXP_ERR_IO; }
            response_length = (size_t)length;
        }
    }
    {
        char response_headers[256];
        int64_t response_deadline =
            deadline_after(REPLICA_IO_DEADLINE_MILLISECONDS);
        int length = snprintf(response_headers, sizeof(response_headers),
            "HTTP/1.1 %u %s\r\nContent-Type: application/json\r\n"
            "Cache-Control: no-store\r\nContent-Length: %zu\r\n"
            "Connection: close\r\n\r\n", http_status,
            http_status < 300U ? "OK" : "Refused", response_length);
        if (length < 0 || (size_t)length >= sizeof(response_headers) ||
            write_exact_until(
                descriptor, (const uint8_t *)response_headers,
                (size_t)length, response_deadline) != 0 ||
            write_exact_until(
                descriptor, (const uint8_t *)response_body,
                response_length, response_deadline) != 0)
            status = LXP_ERR_IO;
    }
    free(response_body);
    return status;
}

static void authority_connection_release(authority_connection *connection)
{
    authority_replica *replica = connection->replica;
    (void)pthread_mutex_lock(&replica->mutex);
    if (replica->connections[connection->slot] == connection->descriptor) {
        replica->connections[connection->slot] = -1;
        if (replica->active_connections != 0U)
            --replica->active_connections;
    }
    (void)pthread_cond_broadcast(&replica->connections_changed);
    (void)pthread_mutex_unlock(&replica->mutex);
    (void)shutdown(connection->descriptor, SHUT_RDWR);
    (void)close(connection->descriptor);
    free(connection);
}

static void *authority_connection_run(void *context)
{
    authority_connection *connection = (authority_connection *)context;
    (void)serve_connection(connection->replica, connection->descriptor);
    authority_connection_release(connection);
    return NULL;
}

static void dispatch_authority_connection(authority_replica *replica,
                                          int descriptor)
{
    authority_connection *connection;
    pthread_attr_t attributes;
    pthread_t thread;
    size_t slot;
    int create_status;
    bool attributes_initialized = false;
    if (configure_connection(descriptor) != 0) {
        (void)close(descriptor);
        return;
    }
    connection = (authority_connection *)malloc(sizeof(*connection));
    if (connection == NULL) {
        (void)close(descriptor);
        return;
    }
    (void)pthread_mutex_lock(&replica->mutex);
    for (slot = 0U; slot < REPLICA_MAX_CONNECTIONS; ++slot)
        if (replica->connections[slot] < 0) break;
    if (replica->stopping || slot == REPLICA_MAX_CONNECTIONS) {
        (void)pthread_mutex_unlock(&replica->mutex);
        free(connection);
        (void)close(descriptor);
        return;
    }
    replica->connections[slot] = descriptor;
    ++replica->active_connections;
    (void)pthread_mutex_unlock(&replica->mutex);
    connection->replica = replica;
    connection->descriptor = descriptor;
    connection->slot = slot;
    create_status = pthread_attr_init(&attributes);
    if (create_status == 0) {
        attributes_initialized = true;
        create_status = pthread_attr_setdetachstate(
            &attributes, PTHREAD_CREATE_DETACHED);
    }
    if (create_status == 0)
        create_status = pthread_create(
            &thread, &attributes, authority_connection_run, connection);
    if (attributes_initialized) (void)pthread_attr_destroy(&attributes);
    if (create_status == 0) return;
    authority_connection_release(connection);
}

static void authority_connections_stop(authority_replica *replica)
{
    size_t index;
    if (!replica->mutex_initialized || !replica->condition_initialized)
        return;
    (void)pthread_mutex_lock(&replica->mutex);
    replica->stopping = true;
    for (index = 0U; index < REPLICA_MAX_CONNECTIONS; ++index)
        if (replica->connections[index] >= 0)
            (void)shutdown(replica->connections[index], SHUT_RDWR);
    while (replica->active_connections != 0U)
        (void)pthread_cond_wait(
            &replica->connections_changed, &replica->mutex);
    (void)pthread_mutex_unlock(&replica->mutex);
}

static lxp_result parse_u64(const char *text, uint64_t *value)
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

static const char *required(const char *name)
{
    const char *value = getenv(name);
    return value != NULL && value[0] != '\0' ? value : NULL;
}

lxp_result lxp_daemon_authority_replica_serve(
    const char *configuration_path)
{
    authority_replica replica;
    lxp_daemon_configuration configuration;
    struct sockaddr_in address;
    struct sigaction action;
    const char *token;
    const char *bind_address;
    uint64_t value;
    size_t index;
    int listener = -1;
    int reuse = 1;
    lxp_result status;
    (void)memset(&replica, 0, sizeof(replica));
    replica_stop = 0;
    for (index = 0U; index < REPLICA_MAX_CONNECTIONS; ++index)
        replica.connections[index] = -1;
    value = 0U;
    status = lxp_daemon_config_load(configuration_path, &configuration);
    if (status == LXP_OK && configuration.role != LXP_DAEMON_REPLICA)
        status = LXP_ERR_CONTEXT_MISMATCH;
    replica.scratch_bytes = (uint8_t *)malloc(REPLICA_SCRATCH_BYTES);
    if (status == LXP_OK && replica.scratch_bytes == NULL) status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_arena_init(&replica.scratch, replica.scratch_bytes,
                                REPLICA_SCRATCH_BYTES);
    if (status == LXP_OK)
        status = lxp_log_open(&replica.log,
                              required("LAYERX_AUTHORITY_REPLICA_LOG"));
    replica.log_open = status == LXP_OK;
    if (status == LXP_OK)
        status = decode_hex32(required("LAYERX_AUTHORITY_REPLICA_ID"),
                              replica.replica_id);
    if (status == LXP_OK)
        status = decode_hex32(required("LAYERX_AUTHORITY_SEQUENCER_ID"),
                              replica.authorization.sequencer_id);
    if (status == LXP_OK)
        status = decode_hex32(required("LAYERX_AUTHORITY_SEQUENCER_PUBLIC_KEY"),
                              replica.authorization.public_key);
    if (status == LXP_OK)
        status = parse_u64(required("LAYERX_AUTHORITY_FIRST_BATCH"), &value);
    if (status == LXP_OK) replica.authorization.first_batch_number = value;
    if (status == LXP_OK)
        status = parse_u64(required("LAYERX_AUTHORITY_LAST_BATCH"), &value);
    if (status == LXP_OK) replica.authorization.last_batch_number = value;
    replica.authorization.authorized = 1U;
    token = required("LAYERX_AUTHORITY_BEARER_TOKEN");
    if (status == LXP_OK &&
        (token == NULL || strlen(token) < 32U ||
         strlen(token) > sizeof(replica.bearer_token)))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        replica.bearer_token_length = strlen(token);
        (void)memcpy(replica.bearer_token, token,
                     replica.bearer_token_length);
        status = lxp_daemon_receipt_authority_open(
            &replica.store, &replica.log, &replica.authorization);
    }
    if (status == LXP_OK) {
        if (pthread_mutex_init(&replica.mutex, NULL) != 0)
            status = LXP_ERR_IO;
        else
            replica.mutex_initialized = true;
    }
    if (status == LXP_OK) {
        if (pthread_cond_init(&replica.connections_changed, NULL) != 0)
            status = LXP_ERR_IO;
        else
            replica.condition_initialized = true;
    }
    bind_address = required("LAYERX_AUTHORITY_ADDRESS");
    if (status == LXP_OK)
        status = parse_u64(required("LAYERX_AUTHORITY_PORT"), &value);
    if (status == LXP_OK && (value == 0U || value > UINT16_MAX ||
        bind_address == NULL || strcmp(bind_address, "127.0.0.1") != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) {
        listener = socket(
            AF_INET, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
        if (listener < 0 || setsockopt(listener, SOL_SOCKET, SO_REUSEADDR,
                                      &reuse, sizeof(reuse)) != 0)
            status = LXP_ERR_IO;
    }
    (void)memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons((uint16_t)value);
    if (status == LXP_OK &&
        inet_pton(AF_INET, bind_address, &address.sin_addr) != 1)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK &&
        (bind(listener, (const struct sockaddr *)&address, sizeof(address)) != 0 ||
         listen(listener, 128) != 0))
        status = LXP_ERR_IO;
    (void)memset(&action, 0, sizeof(action));
    action.sa_handler = stop_replica;
    (void)sigemptyset(&action.sa_mask);
    if (status == LXP_OK &&
        (sigaction(SIGINT, &action, NULL) != 0 ||
         sigaction(SIGTERM, &action, NULL) != 0))
        status = LXP_ERR_IO;
    while (status == LXP_OK && !replica_stop) {
        struct pollfd watched;
        int ready;
        int connection;
        watched.fd = listener;
        watched.events = POLLIN;
        watched.revents = 0;
        ready = poll(&watched, 1U, REPLICA_ACCEPT_POLL_MILLISECONDS);
        if (ready == 0) continue;
        if (ready < 0) {
            if (errno == EINTR) continue;
            status = LXP_ERR_IO;
            break;
        }
        if ((watched.revents & POLLNVAL) != 0 ||
            ((watched.revents & (POLLERR | POLLHUP)) != 0 &&
             (watched.revents & POLLIN) == 0)) {
            if (!replica_stop) status = LXP_ERR_IO;
            break;
        }
        connection = accept(listener, NULL, NULL);
        if (connection < 0) {
            if (errno == EINTR || errno == EAGAIN ||
                errno == EWOULDBLOCK)
                continue;
            status = LXP_ERR_IO;
            break;
        }
        dispatch_authority_connection(&replica, connection);
    }
    if (listener >= 0) (void)close(listener);
    authority_connections_stop(&replica);
    if (replica.log_open) (void)lxp_log_close(&replica.log);
    if (replica.condition_initialized)
        (void)pthread_cond_destroy(&replica.connections_changed);
    if (replica.mutex_initialized)
        (void)pthread_mutex_destroy(&replica.mutex);
    lxp_secure_zero(replica.bearer_token, sizeof(replica.bearer_token));
    free(replica.scratch_bytes);
    return status;
}

lxp_result lxp_daemon_authority_replica_publish(
    const char *loopback_address, uint16_t port,
    const uint8_t *bearer_token, size_t bearer_token_length,
    const uint8_t expected_replica_id[32],
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof)
{
    struct sockaddr_in address;
    uint8_t *body;
    char *request;
    char response[512];
    char expected_hex[65];
    size_t proof_bytes;
    size_t body_length;
    size_t offset = 0U;
    int descriptor;
    int request_length;
    size_t response_length = 0U;
    int64_t deadline;
    lxp_result status = LXP_OK;
    if (loopback_address == NULL || port == 0U || bearer_token == NULL ||
        bearer_token_length < 32U ||
        bearer_token_length > LXP_DAEMON_BEARER_MAX_BYTES ||
        expected_replica_id == NULL ||
        lxp_ct_is_zero(expected_replica_id, 32U) ||
        canonical_receipt == NULL || receipt_length == 0U ||
        canonical_header == NULL ||
        header_length != LXP_BATCH_HEADER_ENCODED_SIZE ||
        header_signature == NULL || receipt_proof == NULL ||
        receipt_proof->depth > LXP_MERKLE_MAX_DEPTH ||
        strcmp(loopback_address, "127.0.0.1") != 0)
        return LXP_ERR_NON_CANONICAL;
    proof_bytes = (size_t)receipt_proof->depth * 32U;
    body_length = sizeof(replica_magic) + 2U + header_length + 64U + 9U +
                  proof_bytes + 4U + receipt_length;
    if (body_length > REPLICA_REQUEST_MAX) return LXP_ERR_LENGTH_LIMIT;
    body = (uint8_t *)malloc(body_length);
    request = (char *)malloc(REPLICA_HEADER_MAX);
    if (body == NULL || request == NULL) {
        free(body); free(request); return LXP_ERR_IO;
    }
    (void)memcpy(body + offset, replica_magic, sizeof(replica_magic));
    offset += sizeof(replica_magic);
    write_u16(body + offset, (uint16_t)header_length); offset += 2U;
    (void)memcpy(body + offset, canonical_header, header_length);
    offset += header_length;
    (void)memcpy(body + offset, header_signature, 64U); offset += 64U;
    body[offset++] = receipt_proof->depth;
    write_u32(body + offset, receipt_proof->leaf_index); offset += 4U;
    write_u32(body + offset, receipt_proof->leaf_count); offset += 4U;
    (void)memcpy(body + offset, receipt_proof->siblings, proof_bytes);
    offset += proof_bytes;
    write_u32(body + offset, (uint32_t)receipt_length); offset += 4U;
    (void)memcpy(body + offset, canonical_receipt, receipt_length);
    request_length = snprintf(request, REPLICA_HEADER_MAX,
        "POST /v1/receipt-authority/ingest HTTP/1.1\r\n"
        "Host: %s:%u\r\nAuthorization: Bearer %.*s\r\n"
        "Content-Type: application/octet-stream\r\nContent-Length: %zu\r\n"
        "Connection: close\r\n\r\n", loopback_address, port,
        (int)bearer_token_length, (const char *)bearer_token, body_length);
    if (request_length < 0 || request_length >= REPLICA_HEADER_MAX)
        status = LXP_ERR_LENGTH_LIMIT;
    descriptor = status == LXP_OK ?
        socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0) : -1;
    (void)memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    if (status == LXP_OK &&
        (descriptor < 0 || configure_connection(descriptor) != 0))
        status = LXP_ERR_IO;
    deadline = deadline_after(REPLICA_IO_DEADLINE_MILLISECONDS);
    if (status == LXP_OK &&
        (inet_pton(AF_INET, loopback_address, &address.sin_addr) != 1 ||
         connect_until(descriptor, (const struct sockaddr *)&address,
                       sizeof(address), deadline) != 0 ||
         write_exact_until(
             descriptor, (const uint8_t *)request,
             (size_t)request_length, deadline) != 0 ||
         write_exact_until(
             descriptor, body, body_length, deadline) != 0))
        status = LXP_ERR_IO;
    deadline = deadline_after(REPLICA_IO_DEADLINE_MILLISECONDS);
    while (status == LXP_OK && response_length < sizeof(response) - 1U) {
        ssize_t count = read_some_until(
            descriptor, (uint8_t *)response + response_length,
            sizeof(response) - 1U - response_length, deadline);
        if (count > 0) response_length += (size_t)count;
        else if (count == 0) break;
        else status = LXP_ERR_IO;
    }
    if (status == LXP_OK && response_length == 0U) status = LXP_ERR_IO;
    if (status == LXP_OK) {
        char expected_body[96];
        char *body_start;
        int expected_length;
        response[response_length] = '\0';
        hex_encode(expected_replica_id, 32U, expected_hex);
        expected_length = snprintf(expected_body, sizeof(expected_body),
            "{\"authority_replica_id\":\"%s\"}", expected_hex);
        body_start = strstr(response, "\r\n\r\n");
        if (strncmp(response, "HTTP/1.1 201", 12U) != 0 ||
            expected_length < 0 ||
            (size_t)expected_length >= sizeof(expected_body) ||
            body_start == NULL ||
            strcmp(body_start + 4U, expected_body) != 0)
            status = LXP_ERR_BAD_SIGNATURE;
    }
    if (descriptor >= 0) (void)close(descriptor);
    free(request);
    free(body);
    return status;
}
