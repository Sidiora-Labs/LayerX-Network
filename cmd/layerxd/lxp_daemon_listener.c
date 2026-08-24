#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <limits.h>
#include <netinet/in.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <time.h>
#include <unistd.h>

enum {
    LISTENER_REQUEST_BYTES = 8192,
    LISTENER_PATH_BYTES = 4096,
    LISTENER_RESPONSE_ARENA_BYTES = 64 * 1024 * 1024,
    LISTENER_ACCEPT_POLL_MILLISECONDS = 250,
    LISTENER_IO_DEADLINE_MILLISECONDS = 5000
};

typedef struct listener_connection {
    lxp_daemon_protocol_owner *owner;
    int descriptor;
    size_t slot;
} listener_connection;

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
    timeout.tv_sec = LISTENER_IO_DEADLINE_MILLISECONDS / 1000;
    timeout.tv_usec =
        (LISTENER_IO_DEADLINE_MILLISECONDS % 1000) * 1000;
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

static lxp_result request_parse(
    char *request, size_t length, char method[8], char path[LISTENER_PATH_BYTES],
    const uint8_t **token, size_t *token_length)
{
    static const char authorization[] = "Authorization: Bearer ";
    char *line_end;
    char *first_space;
    char *second_space;
    char *line;
    if (request == NULL || length == 0U || method == NULL || path == NULL ||
        token == NULL || token_length == NULL ||
        strstr(request, "\r\n\r\n") == NULL)
        return LXP_ERR_NON_CANONICAL;
    line_end = strstr(request, "\r\n");
    first_space = strchr(request, ' ');
    second_space = first_space == NULL ? NULL : strchr(first_space + 1, ' ');
    if (line_end == NULL || first_space == NULL || second_space == NULL ||
        first_space >= line_end || second_space >= line_end ||
        (size_t)(first_space - request) >= 8U ||
        (size_t)(second_space - first_space - 1) >= LISTENER_PATH_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(method, request, (size_t)(first_space - request));
    method[first_space - request] = '\0';
    (void)memcpy(path, first_space + 1,
                 (size_t)(second_space - first_space - 1));
    path[second_space - first_space - 1] = '\0';
    if (strncmp(second_space, " HTTP/1.1\r\n", 11U) != 0)
        return LXP_ERR_NON_CANONICAL;
    *token = NULL;
    *token_length = 0U;
    line = line_end + 2;
    while (line < request + length && strncmp(line, "\r\n", 2U) != 0) {
        char *end = strstr(line, "\r\n");
        if (end == NULL) return LXP_ERR_NON_CANONICAL;
        if (strncmp(line, authorization, sizeof(authorization) - 1U) == 0) {
            if (*token != NULL) return LXP_ERR_NON_CANONICAL;
            *token = (const uint8_t *)line + sizeof(authorization) - 1U;
            *token_length = (size_t)(end - line) -
                            (sizeof(authorization) - 1U);
        }
        line = end + 2;
    }
    return LXP_OK;
}

static void serve_connection(lxp_daemon_protocol_owner *owner, int client)
{
    char request[LISTENER_REQUEST_BYTES + 1U];
    char method[8];
    char path[LISTENER_PATH_BYTES];
    const uint8_t *token = NULL;
    size_t token_length = 0U;
    size_t used = 0U;
    uint8_t *arena_bytes = NULL;
    lxp_arena arena;
    lxp_daemon_protocol_response response;
    lxp_result status = LXP_OK;
    int64_t request_deadline =
        deadline_after(LISTENER_IO_DEADLINE_MILLISECONDS);
    while (used < LISTENER_REQUEST_BYTES) {
        ssize_t count = read_some_until(
            client, (uint8_t *)request + used,
            LISTENER_REQUEST_BYTES - used, request_deadline);
        if (count > 0) {
            used += (size_t)count;
            request[used] = '\0';
            if (strstr(request, "\r\n\r\n") != NULL) break;
        } else {
            status = LXP_ERR_IO;
            break;
        }
    }
    if (status == LXP_OK &&
        (used == LISTENER_REQUEST_BYTES ||
         request_parse(request, used, method, path,
                       &token, &token_length) != LXP_OK))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        arena_bytes = (uint8_t *)malloc(LISTENER_RESPONSE_ARENA_BYTES);
    if (status == LXP_OK && arena_bytes == NULL) status = LXP_ERR_IO;
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_bytes,
                                LISTENER_RESPONSE_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_daemon_protocol_route(
            owner, token, token_length, method, path, &arena, &response);
    if (status == LXP_OK) {
        char header[256];
        int64_t response_deadline =
            deadline_after(LISTENER_IO_DEADLINE_MILLISECONDS);
        int header_length = snprintf(
            header, sizeof(header),
            "HTTP/1.1 %u %s\r\nContent-Type: application/json\r\n"
            "Cache-Control: no-store\r\nContent-Length: %zu\r\n"
            "Connection: close\r\n\r\n",
            response.status, response.status == 200U ? "OK" : "Refused",
            response.body.length);
        if (header_length > 0 && (size_t)header_length < sizeof(header)) {
            if (write_exact_until(
                    client, (const uint8_t *)header,
                    (size_t)header_length, response_deadline) == 0)
                (void)write_exact_until(
                    client, response.body.bytes,
                    response.body.length, response_deadline);
        }
    } else {
        static const char refused_body[] = "{\"error\":\"refused\"}";
        char header[256];
        int64_t response_deadline =
            deadline_after(LISTENER_IO_DEADLINE_MILLISECONDS);
        int header_length = snprintf(
            header, sizeof(header),
            "HTTP/1.1 400 Refused\r\nContent-Type: application/json\r\n"
            "Cache-Control: no-store\r\nContent-Length: %zu\r\n"
            "Connection: close\r\n\r\n",
            sizeof(refused_body) - 1U);
        if (header_length > 0 && (size_t)header_length < sizeof(header)) {
            if (write_exact_until(
                    client, (const uint8_t *)header,
                    (size_t)header_length, response_deadline) == 0)
                (void)write_exact_until(
                    client, (const uint8_t *)refused_body,
                    sizeof(refused_body) - 1U, response_deadline);
        }
    }
    free(arena_bytes);
}

static void connection_release(listener_connection *connection)
{
    lxp_daemon_protocol_owner *owner = connection->owner;
    (void)pthread_mutex_lock(&owner->mutex);
    if (owner->listener_connections[connection->slot] ==
        connection->descriptor) {
        owner->listener_connections[connection->slot] = -1;
        if (owner->listener_active_connections != 0U)
            --owner->listener_active_connections;
    }
    (void)pthread_cond_broadcast(&owner->listener_changed);
    (void)pthread_mutex_unlock(&owner->mutex);
    (void)shutdown(connection->descriptor, SHUT_RDWR);
    (void)close(connection->descriptor);
    free(connection);
}

static void *connection_run(void *context)
{
    listener_connection *connection = (listener_connection *)context;
    serve_connection(connection->owner, connection->descriptor);
    connection_release(connection);
    return NULL;
}

static bool listener_is_stopping(lxp_daemon_protocol_owner *owner)
{
    bool stopping;
    (void)pthread_mutex_lock(&owner->mutex);
    stopping = owner->listener_stopping;
    (void)pthread_mutex_unlock(&owner->mutex);
    return stopping;
}

static void listener_fail(lxp_daemon_protocol_owner *owner,
                          lxp_result failure)
{
    size_t index;
    (void)pthread_mutex_lock(&owner->mutex);
    if (!owner->listener_stopping && owner->listener_failure == LXP_OK)
        owner->listener_failure = failure;
    owner->listener_stopping = true;
    for (index = 0U; index < LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS; ++index)
        if (owner->listener_connections[index] >= 0)
            (void)shutdown(owner->listener_connections[index], SHUT_RDWR);
    (void)pthread_cond_broadcast(&owner->listener_changed);
    (void)pthread_mutex_unlock(&owner->mutex);
    (void)shutdown(owner->listener_descriptor, SHUT_RDWR);
}

static void dispatch_connection(lxp_daemon_protocol_owner *owner,
                                int client)
{
    listener_connection *connection;
    pthread_attr_t attributes;
    pthread_t thread;
    size_t slot;
    int create_status;
    bool attributes_initialized = false;
    if (configure_connection(client) != 0) {
        (void)close(client);
        return;
    }
    connection = (listener_connection *)malloc(sizeof(*connection));
    if (connection == NULL) {
        (void)close(client);
        return;
    }
    (void)pthread_mutex_lock(&owner->mutex);
    for (slot = 0U; slot < LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS; ++slot)
        if (owner->listener_connections[slot] < 0) break;
    if (owner->listener_stopping ||
        slot == LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS) {
        (void)pthread_mutex_unlock(&owner->mutex);
        free(connection);
        (void)close(client);
        return;
    }
    owner->listener_connections[slot] = client;
    ++owner->listener_active_connections;
    (void)pthread_mutex_unlock(&owner->mutex);
    connection->owner = owner;
    connection->descriptor = client;
    connection->slot = slot;
    create_status = pthread_attr_init(&attributes);
    if (create_status == 0) {
        attributes_initialized = true;
        create_status = pthread_attr_setdetachstate(
            &attributes, PTHREAD_CREATE_DETACHED);
    }
    if (create_status == 0)
        create_status = pthread_create(
            &thread, &attributes, connection_run, connection);
    if (attributes_initialized) (void)pthread_attr_destroy(&attributes);
    if (create_status == 0) return;
    connection_release(connection);
}

static void *listener_run(void *context)
{
    lxp_daemon_protocol_owner *owner =
        (lxp_daemon_protocol_owner *)context;
    for (;;) {
        struct pollfd watched;
        int ready;
        int client;
        if (listener_is_stopping(owner)) break;
        watched.fd = owner->listener_descriptor;
        watched.events = POLLIN;
        watched.revents = 0;
        ready = poll(&watched, 1U, LISTENER_ACCEPT_POLL_MILLISECONDS);
        if (ready == 0) continue;
        if (ready < 0) {
            if (errno == EINTR) continue;
            listener_fail(owner, LXP_ERR_IO);
            break;
        }
        if ((watched.revents & POLLNVAL) != 0) {
            listener_fail(owner, LXP_ERR_IO);
            break;
        }
        if ((watched.revents & (POLLERR | POLLHUP)) != 0 &&
            (watched.revents & POLLIN) == 0) {
            if (!listener_is_stopping(owner))
                listener_fail(owner, LXP_ERR_IO);
            break;
        }
        client = accept(owner->listener_descriptor, NULL, NULL);
        if (client < 0) {
            if (errno == EINTR || errno == EAGAIN ||
                errno == EWOULDBLOCK)
                continue;
            if (listener_is_stopping(owner)) break;
            listener_fail(owner, LXP_ERR_IO);
            break;
        }
        dispatch_connection(owner, client);
    }
    return NULL;
}

lxp_result lxp_daemon_protocol_listener_start(
    lxp_daemon_protocol_owner *owner, const char *loopback_address,
    uint16_t port)
{
    struct sockaddr_in address;
    size_t index;
    int descriptor;
    int reuse = 1;
    if (owner == NULL || !owner->attached || owner->listener_started ||
        loopback_address == NULL ||
        strcmp(loopback_address, "127.0.0.1") != 0 || port == 0U)
        return LXP_ERR_NON_CANONICAL;
    descriptor = socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK,
                        0);
    if (descriptor < 0 ||
        setsockopt(descriptor, SOL_SOCKET, SO_REUSEADDR,
                   &reuse, sizeof(reuse)) != 0) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    (void)memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    if (inet_pton(AF_INET, loopback_address, &address.sin_addr) != 1 ||
        bind(descriptor, (const struct sockaddr *)&address,
             sizeof(address)) != 0 || listen(descriptor, 64) != 0) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    owner->listener_descriptor = descriptor;
    owner->listener_port = port;
    owner->listener_failure = LXP_OK;
    owner->listener_active_connections = 0U;
    for (index = 0U; index < LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS; ++index)
        owner->listener_connections[index] = -1;
    owner->listener_stopping = false;
    if (pthread_cond_init(&owner->listener_changed, NULL) != 0) {
        (void)close(descriptor);
        owner->listener_descriptor = -1;
        owner->listener_port = 0U;
        return LXP_ERR_IO;
    }
    if (pthread_create(&owner->listener_thread, NULL,
                       listener_run, owner) != 0) {
        (void)close(descriptor);
        (void)pthread_cond_destroy(&owner->listener_changed);
        owner->listener_descriptor = -1;
        owner->listener_port = 0U;
        return LXP_ERR_IO;
    }
    owner->listener_started = true;
    return LXP_OK;
}

lxp_result lxp_daemon_protocol_listener_stop(
    lxp_daemon_protocol_owner *owner)
{
    lxp_result status;
    size_t index;
    if (owner == NULL || !owner->listener_started)
        return LXP_ERR_NON_CANONICAL;
    (void)pthread_mutex_lock(&owner->mutex);
    owner->listener_stopping = true;
    for (index = 0U; index < LXP_DAEMON_PROTOCOL_MAX_CONNECTIONS; ++index)
        if (owner->listener_connections[index] >= 0)
            (void)shutdown(owner->listener_connections[index], SHUT_RDWR);
    (void)pthread_mutex_unlock(&owner->mutex);
    (void)shutdown(owner->listener_descriptor, SHUT_RDWR);
    (void)pthread_join(owner->listener_thread, NULL);
    (void)pthread_mutex_lock(&owner->mutex);
    while (owner->listener_active_connections != 0U)
        (void)pthread_cond_wait(&owner->listener_changed, &owner->mutex);
    status = owner->listener_failure;
    (void)pthread_mutex_unlock(&owner->mutex);
    (void)close(owner->listener_descriptor);
    (void)pthread_cond_destroy(&owner->listener_changed);
    owner->listener_descriptor = -1;
    owner->listener_port = 0U;
    owner->listener_started = false;
    owner->listener_stopping = false;
    owner->listener_failure = LXP_OK;
    return status;
}
