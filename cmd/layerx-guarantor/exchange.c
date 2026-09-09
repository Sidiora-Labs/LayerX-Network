#define _POSIX_C_SOURCE 200809L
#include "exchange.h"
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <netdb.h>
#include <openssl/x509v3.h>
#include <poll.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <sys/socket.h>
#include <time.h>
#include <unistd.h>

#define GP_HEADER_MAX 4096u
#define GP_IO_TIMEOUT_MS 5000
static SSL_CTX *tls_context(const struct gp_exchange_tls *c, int server)
{
    SSL_CTX *ctx;
    if (!c || !c->certificate || !c->private_key || !c->ca)
        return NULL;
    ctx = SSL_CTX_new(server ? TLS_server_method() : TLS_client_method());
    if (!ctx)
        return NULL;
    if (SSL_CTX_set_min_proto_version(ctx, TLS1_2_VERSION) != 1 ||
        SSL_CTX_use_certificate_chain_file(ctx, c->certificate) != 1 ||
        SSL_CTX_use_PrivateKey_file(ctx, c->private_key, SSL_FILETYPE_PEM) != 1 ||
        SSL_CTX_check_private_key(ctx) != 1 ||
        SSL_CTX_load_verify_locations(ctx, c->ca, NULL) != 1) {
        SSL_CTX_free(ctx);
        return NULL;
    }
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER | (server ? SSL_VERIFY_FAIL_IF_NO_PEER_CERT : 0), NULL);
    SSL_CTX_set_verify_depth(ctx, 8);
    SSL_CTX_set_options(ctx, SSL_OP_NO_COMPRESSION);
    return ctx;
}
static int64_t monotonic_ms(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now))
        return -1;
    return (int64_t)now.tv_sec * 1000 + (int64_t)now.tv_nsec / 1000000;
}
static int socket_nonblocking(int fd)
{
    int flags = fcntl(fd, F_GETFL, 0);
    return flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) ? -1 : 0;
}
static int ssl_wait(SSL *ssl, int result, int64_t deadline)
{
    int error = SSL_get_error(ssl, result);
    struct pollfd fd = {SSL_get_fd(ssl), 0, 0};
    if (error == SSL_ERROR_WANT_READ)
        fd.events = POLLIN;
    else if (error == SSL_ERROR_WANT_WRITE)
        fd.events = POLLOUT;
    else
        return -1;
    for (;;) {
        int64_t now = monotonic_ms();
        if (now < 0 || now >= deadline)
            return -1;
        int ready = poll(&fd, 1, (int)(deadline - now));
        if (ready < 0 && errno == EINTR)
            continue;
        return ready > 0 && (fd.revents & fd.events) ? 0 : -1;
    }
}
static int handshake(SSL *ssl, int server)
{
    int64_t deadline = monotonic_ms();
    if (deadline < 0)
        return -1;
    deadline += GP_IO_TIMEOUT_MS;
    for (;;) {
        int result = server ? SSL_accept(ssl) : SSL_connect(ssl);
        if (result == 1)
            return 0;
        if (ssl_wait(ssl, result, deadline))
            return -1;
    }
}
static int read_some(SSL *ssl, void *bytes, int capacity, int64_t deadline)
{
    for (;;) {
        int result = SSL_read(ssl, bytes, capacity);
        if (result > 0)
            return result;
        if (ssl_wait(ssl, result, deadline))
            return -1;
    }
}
static int write_all(SSL *ssl, const void *data, size_t n)
{
    const unsigned char *p = data;
    int64_t deadline = monotonic_ms();
    if (deadline < 0)
        return -1;
    deadline += GP_IO_TIMEOUT_MS;
    while (n) {
        int k = SSL_write(ssl, p, (int)n);
        if (k <= 0) {
            if (ssl_wait(ssl, k, deadline))
                return -1;
            continue;
        }
        p += k;
        n -= (size_t)k;
    }
    return 0;
}
static int read_message(SSL *ssl, char *header, uint8_t *body, size_t capacity, size_t *length)
{
    size_t used = 0, content_length = 0;
    int found_length = 0;
    char *line, *end;
    int64_t deadline = monotonic_ms();
    if (deadline < 0)
        return -1;
    deadline += GP_IO_TIMEOUT_MS;
    while (used + 1 < GP_HEADER_MAX) {
        if (read_some(ssl, header + used, 1, deadline) != 1)
            return -1;
        used++;
        if (used >= 4 && memcmp(header + used - 4, "\r\n\r\n", 4) == 0)
            break;
    }
    if (used < 4 || memcmp(header + used - 4, "\r\n\r\n", 4))
        return -1;
    header[used] = '\0';
    line = strstr(header, "\r\n");
    if (!line)
        return -1;
    line += 2;
    while (*line != '\r') {
        end = strstr(line, "\r\n");
        if (!end || *line == ' ' || *line == '\t')
            return -1;
        if ((size_t)(end - line) >= 15 && !strncasecmp(line, "Content-Length:", 15)) {
            char *p = line + 15;
            if (found_length++)
                return -1;
            while (p < end && (*p == ' ' || *p == '\t'))
                p++;
            if (p == end)
                return -1;
            for (; p < end; p++) {
                if (*p < '0' || *p > '9' ||
                    content_length > (GP_EXCHANGE_MAX_BODY - (size_t)(*p - '0')) / 10)
                    return -1;
                content_length = content_length * 10 + (size_t)(*p - '0');
            }
        } else if ((size_t)(end - line) >= 18 && !strncasecmp(line, "Transfer-Encoding:", 18))
            return -1;
        else if (!memchr(line, ':', (size_t)(end - line)))
            return -1;
        line = end + 2;
    }
    if (!found_length || content_length > capacity)
        return -1;
    used = 0;
    while (used < content_length) {
        int k = read_some(ssl, body + used, (int)(content_length - used), deadline);
        if (k <= 0)
            return -1;
        used += (size_t)k;
    }
    *length = content_length;
    return 0;
}
static int parse_id(const char *p, uint8_t id[32])
{
    for (size_t i = 0; i < 32; i++) {
        unsigned v = 0;
        for (size_t j = 0; j < 2; j++) {
            unsigned char c = (unsigned char)p[i * 2 + j];
            unsigned d;
            if (c >= '0' && c <= '9')
                d = c - '0';
            else if (c >= 'a' && c <= 'f')
                d = (unsigned)(c - 'a') + 10U;
            else
                return -1;
            v = v * 16 + d;
        }
        id[i] = (uint8_t)v;
    }
    return 0;
}
int gp_exchange_open(struct gp_exchange *s, const struct gp_exchange_tls *tls, uint16_t port,
                     const struct gp_exchange_callbacks *callbacks)
{
    struct sockaddr_in address;
    int reuse = 1;
    if (!s || !callbacks || !callbacks->receive || !callbacks->get)
        return -1;
    memset(s, 0, sizeof(*s));
    s->socket_fd = -1;
    s->callbacks = *callbacks;
    s->tls = tls_context(tls, 1);
    if (!s->tls)
        return -1;
    s->socket_fd = socket(AF_INET, SOCK_STREAM, 0);
    memset(&address, 0, sizeof(address));
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons(port);
    if (s->socket_fd < 0 ||
        setsockopt(s->socket_fd, SOL_SOCKET, SO_REUSEADDR, &reuse, sizeof(reuse)) ||
        bind(s->socket_fd, (struct sockaddr *)&address, sizeof(address)) ||
        listen(s->socket_fd, 16)) {
        gp_exchange_close(s);
        return -1;
    }
    return 0;
}
void gp_exchange_close(struct gp_exchange *s)
{
    if (!s)
        return;
    if (s->socket_fd >= 0)
        close(s->socket_fd);
    SSL_CTX_free(s->tls);
    s->tls = NULL;
    s->socket_fd = -1;
}
int gp_exchange_poll(struct gp_exchange *s, int timeout_ms)
{
    struct pollfd pfd = {s->socket_fd, POLLIN, 0};
    char header[GP_HEADER_MAX], response[160];
    uint8_t body[GP_EXCHANGE_MAX_BODY], output[GP_EXCHANGE_MAX_BODY], id[32];
    size_t length = 0, output_length = 0;
    int fd, status = 400, result = -1, ready = poll(&pfd, 1, timeout_ms);
    SSL *ssl;
    if (ready == 0)
        return 0;
    if (ready < 0 || !(pfd.revents & POLLIN))
        return -1;
    fd = accept(s->socket_fd, NULL, NULL);
    if (fd < 0)
        return -1;
    ssl = SSL_new(s->tls);
    if (!ssl || socket_nonblocking(fd) || SSL_set_fd(ssl, fd) != 1 || handshake(ssl, 1) ||
        SSL_get_verify_result(ssl) != X509_V_OK)
        goto done;
    if (read_message(ssl, header, body, sizeof(body), &length) == 0) {
        if (!strncmp(header, "POST /v1/attestations HTTP/1.1\r\n", 32)) {
            status =
                length && s->callbacks.receive(s->callbacks.context, body, length) == 0 ? 200 : 422;
        } else if (strlen(header) >= 100 && !strncmp(header, "GET /v1/attestations/", 21) &&
                   !strncmp(header + 85, " HTTP/1.1\r\n", 11) && !length &&
                   parse_id(header + 21, id) == 0) {
            status = s->callbacks.get(s->callbacks.context, id, output, sizeof(output),
                                      &output_length) == 0
                         ? 200
                         : 404;
            if (output_length > sizeof(output)) {
                status = 500;
                output_length = 0;
            }
        }
    }
    if (status != 200)
        output_length = 0;
    int n = snprintf(response, sizeof(response),
                     "HTTP/1.1 %d Result\r\nContent-Length: %zu\r\nContent-Type: "
                     "application/octet-stream\r\nConnection: close\r\n\r\n",
                     status, output_length);
    if (n > 0 && (size_t)n < sizeof(response) && !write_all(ssl, response, (size_t)n) &&
        !write_all(ssl, output, output_length))
        result = 1;
done:
    SSL_free(ssl);
    close(fd);
    return result;
}
static int connect_peer(const char *host, uint16_t port)
{
    struct addrinfo hints, *addresses = NULL, *a;
    char service[8];
    int fd = -1;
    memset(&hints, 0, sizeof(hints));
    hints.ai_socktype = SOCK_STREAM;
    hints.ai_family = AF_UNSPEC;
    snprintf(service, sizeof(service), "%u", (unsigned)port);
    if (getaddrinfo(host, service, &hints, &addresses))
        return -1;
    for (a = addresses; a; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd < 0)
            continue;
        int flags = fcntl(fd, F_GETFL, 0);
        if (flags >= 0 && !fcntl(fd, F_SETFL, flags | O_NONBLOCK)) {
            int rc = connect(fd, a->ai_addr, a->ai_addrlen);
            if (rc < 0 && errno == EINPROGRESS) {
                struct pollfd p = {fd, POLLOUT, 0};
                int error = 0;
                socklen_t size = sizeof(error);
                if (poll(&p, 1, GP_IO_TIMEOUT_MS) > 0 &&
                    !getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &size) && !error)
                    rc = 0;
            }
            if (!rc && !socket_nonblocking(fd))
                break;
        }
        close(fd);
        fd = -1;
    }
    freeaddrinfo(addresses);
    return fd;
}
int gp_exchange_peer(const struct gp_exchange_tls *tls, const char *host, uint16_t port,
                     const uint8_t *id, const uint8_t *body, size_t length, uint8_t *output,
                     size_t capacity, size_t *output_length)
{
    SSL_CTX *ctx = NULL;
    SSL *ssl = NULL;
    int fd = -1, result = -1, n;
    char path[96] = "/v1/attestations", request[512], header[GP_HEADER_MAX];
    unsigned char ip[16];
    if (!host || !*host || !output_length || length > GP_EXCHANGE_MAX_BODY || (length && !body) ||
        (capacity && !output))
        return -1;
    *output_length = 0;
    for (const unsigned char *p = (const unsigned char *)host; *p; p++)
        if (*p <= 32 || *p >= 127 || *p == '/' || *p == '\\')
            return -1;
    if (id) {
        strcpy(path, "/v1/attestations/");
        for (size_t i = 0; i < 32; i++)
            snprintf(path + 17 + 2 * i, 3, "%02x", id[i]);
        if (length)
            return -1;
    }
    ctx = tls_context(tls, 0);
    if (!ctx)
        goto done;
    ssl = SSL_new(ctx);
    if (!ssl)
        goto done;
    if (inet_pton(AF_INET, host, ip) == 1 || inet_pton(AF_INET6, host, ip) == 1) {
        if (X509_VERIFY_PARAM_set1_ip_asc(SSL_get0_param(ssl), host) != 1)
            goto done;
    } else if (SSL_set1_host(ssl, host) != 1 || SSL_set_tlsext_host_name(ssl, host) != 1)
        goto done;
    fd = connect_peer(host, port);
    if (fd < 0 || SSL_set_fd(ssl, fd) != 1 || handshake(ssl, 0) ||
        SSL_get_verify_result(ssl) != X509_V_OK)
        goto done;
    n = snprintf(request, sizeof(request),
                 "%s %s HTTP/1.1\r\nHost: %s:%u\r\nContent-Length: %zu\r\nContent-Type: "
                 "application/octet-stream\r\nConnection: close\r\n\r\n",
                 id ? "GET" : "POST", path, host, (unsigned)port, length);
    if (n <= 0 || (size_t)n >= sizeof(request) || write_all(ssl, request, (size_t)n) ||
        write_all(ssl, body, length) ||
        read_message(ssl, header, output, capacity, output_length) ||
        strncmp(header, "HTTP/1.1 200 ", 13))
        goto done;
    result = 0;
done:
    SSL_free(ssl);
    SSL_CTX_free(ctx);
    if (fd >= 0)
        close(fd);
    return result;
}
