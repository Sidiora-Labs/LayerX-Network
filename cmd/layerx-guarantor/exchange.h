#ifndef LAYERX_GUARANTOR_EXCHANGE_H
#define LAYERX_GUARANTOR_EXCHANGE_H
#include <openssl/ssl.h>
#include <stddef.h>
#include <stdint.h>
#define GP_EXCHANGE_MAX_BODY 65536u
struct gp_exchange_tls {
    const char *certificate;
    const char *private_key;
    const char *ca;
};
struct gp_exchange_callbacks {
    int (*receive)(void *, const uint8_t *, size_t);
    int (*get)(void *, const uint8_t[32], uint8_t *, size_t, size_t *);
    void *context;
};
struct gp_exchange {
    SSL_CTX *tls;
    int socket_fd;
    struct gp_exchange_callbacks callbacks;
};
int gp_exchange_open(struct gp_exchange *, const struct gp_exchange_tls *, uint16_t,
                     const struct gp_exchange_callbacks *);
int gp_exchange_poll(struct gp_exchange *, int);
void gp_exchange_close(struct gp_exchange *);
int gp_exchange_peer(const struct gp_exchange_tls *, const char *, uint16_t, const uint8_t *,
                     const uint8_t *, size_t, uint8_t *, size_t, size_t *);
#endif
