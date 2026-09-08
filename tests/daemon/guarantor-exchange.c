#define _POSIX_C_SOURCE 200809L
#include "../../cmd/layerx-guarantor/exchange.h"
#include <arpa/inet.h>
#include <assert.h>
#include <signal.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/wait.h>
#include <unistd.h>
struct record {
    uint8_t bytes[256];
    size_t length;
};
static int receive_record(void *context, const uint8_t *data, size_t length)
{
    struct record *r = context;
    if (length > sizeof(r->bytes))
        return -1;
    memcpy(r->bytes, data, length);
    r->length = length;
    return 0;
}
static int get_record(void *context, const uint8_t id[32], uint8_t *out, size_t capacity,
                      size_t *length)
{
    struct record *r = context;
    uint8_t expected[32] = {1};
    if (memcmp(id, expected, 32) || capacity < r->length)
        return -1;
    memcpy(out, r->bytes, r->length);
    *length = r->length;
    return 0;
}
static void no_client_certificate(const char *ca, uint16_t port)
{
    SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
    struct sockaddr_in address = {0};
    SSL *ssl;
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    char response[64];
    assert(ctx && fd >= 0);
    assert(SSL_CTX_load_verify_locations(ctx, ca, NULL) == 1);
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons(port);
    assert(connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0);
    ssl = SSL_new(ctx);
    assert(ssl && SSL_set_fd(ssl, fd) == 1 && SSL_set1_host(ssl, "localhost") == 1);
    int connected = SSL_connect(ssl);
    if (connected == 1) {
        const char request[] = "POST /v1/attestations HTTP/1.1\r\nContent-Length: 1\r\n\r\nx";
        (void)SSL_write(ssl, request, (int)strlen(request));
        assert(SSL_read(ssl, response, sizeof(response)) <= 0);
    }
    SSL_free(ssl);
    SSL_CTX_free(ctx);
    close(fd);
}
static void malformed_request(const struct gp_exchange_tls *tls, uint16_t port, const char *request)
{
    SSL_CTX *ctx = SSL_CTX_new(TLS_client_method());
    struct sockaddr_in address = {0};
    int fd = socket(AF_INET, SOCK_STREAM, 0);
    char response[13] = {0};
    size_t received = 0;
    assert(ctx && fd >= 0);
    assert(SSL_CTX_load_verify_locations(ctx, tls->ca, NULL) == 1);
    assert(SSL_CTX_use_certificate_chain_file(ctx, tls->certificate) == 1);
    assert(SSL_CTX_use_PrivateKey_file(ctx, tls->private_key, SSL_FILETYPE_PEM) == 1);
    SSL_CTX_set_verify(ctx, SSL_VERIFY_PEER, NULL);
    address.sin_family = AF_INET;
    address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    address.sin_port = htons(port);
    assert(connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0);
    SSL *ssl = SSL_new(ctx);
    assert(ssl && SSL_set_fd(ssl, fd) == 1 && SSL_set1_host(ssl, "localhost") == 1);
    assert(SSL_connect(ssl) == 1);
    assert(SSL_write(ssl, request, (int)strlen(request)) == (int)strlen(request));
    while (received < 12) {
        int count = SSL_read(ssl, response + received, (int)(12 - received));
        assert(count > 0);
        received += (size_t)count;
    }
    assert(!memcmp(response, "HTTP/1.1 400", 12));
    SSL_free(ssl);
    SSL_CTX_free(ctx);
    close(fd);
}
int main(int argc, char **argv)
{
    struct gp_exchange server;
    struct record record = {0};
    struct gp_exchange_callbacks callbacks = {receive_record, get_record, &record};
    struct sockaddr_in address;
    socklen_t address_length = sizeof(address);
    uint8_t id[32] = {1}, output[512], payload[] = {0, 1, 2, 255, 0, 3};
    size_t output_length;
    int status;
    assert(argc == 4);
    signal(SIGPIPE, SIG_IGN);
    struct gp_exchange_tls tls = {argv[1], argv[2], argv[3]};
    assert(gp_exchange_open(&server, &tls, 0, &callbacks) == 0);
    assert(getsockname(server.socket_fd, (struct sockaddr *)&address, &address_length) == 0);
    uint16_t port = ntohs(address.sin_port);
    pid_t child = fork();
    assert(child >= 0);
    if (!child) {
        int expected[] = {1, 1, 1, -1, -1, 1, 1, 1};
        for (size_t i = 0; i < sizeof(expected) / sizeof(expected[0]); i++)
            assert(gp_exchange_poll(&server, 5000) == expected[i]);
        gp_exchange_close(&server);
        _exit(0);
    }
    gp_exchange_close(&server);
    assert(gp_exchange_peer(&tls, "localhost", port, NULL, payload, sizeof(payload), output,
                            sizeof(output), &output_length) == 0);
    assert(output_length == 0);
    assert(gp_exchange_peer(&tls, "localhost", port, id, NULL, 0, output, sizeof(output),
                            &output_length) == 0);
    assert(output_length == sizeof(payload) && !memcmp(output, payload, sizeof(payload)));
    id[0] = 2;
    assert(gp_exchange_peer(&tls, "localhost", port, id, NULL, 0, output, sizeof(output),
                            &output_length) != 0);
    assert(gp_exchange_peer(&tls, "127.0.0.1", port, NULL, payload, sizeof(payload), output,
                            sizeof(output), &output_length) != 0);
    no_client_certificate(argv[3], port);
    malformed_request(
        &tls, port,
        "POST /v1/attestations HTTP/1.1\r\nContent-Length: 0\r\nContent-Length: 0\r\n\r\n");
    malformed_request(&tls, port,
                      "POST /v1/attestations HTTP/1.1\r\nContent-Length: 0\r\nTransfer-Encoding: "
                      "chunked\r\n\r\n");
    malformed_request(&tls, port,
                      "POST /v1/attestations HTTP/1.1\r\nContent-Length: 65537\r\n\r\n");
    assert(waitpid(child, &status, 0) == child && WIFEXITED(status) && WEXITSTATUS(status) == 0);
    puts("guarantor mTLS exchange: POST, GET, missing record, hostname refusal, missing client "
         "certificate, duplicate length, chunked encoding, oversized body refusals passed");
    return 0;
}
