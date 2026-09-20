#ifndef LAYERX_LXP_CRYPTO_H
#define LAYERX_LXP_CRYPTO_H

#include "layerx/lxp_hash.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

int lxp_ct_memcmp(const void *left, const void *right, size_t length);
bool lxp_ct_is_zero(const void *bytes, size_t length);
void lxp_secure_zero(void *bytes, size_t length);

typedef struct lxp_ed25519_verify_item {
    const uint8_t *public_key;
    const uint8_t *signature;
    lxp_domain_tag_id domain;
    const void *message;
    size_t message_length;
} lxp_ed25519_verify_item;

bool lxp_ed25519_pubkey_is_canonical(const uint8_t public_key[32]);
lxp_result lxp_ed25519_verify_raw(const uint8_t public_key[32],
                                  const uint8_t signature[64],
                                  const void *message, size_t message_length);
lxp_result lxp_ed25519_verify(const uint8_t public_key[32],
                              const uint8_t signature[64],
                              lxp_domain_tag_id domain, const void *message,
                              size_t message_length);
lxp_result lxp_ed25519_verify_batch(const lxp_ed25519_verify_item *items,
                                    size_t count, bool *valid);
bool lxp_secp256k1_sig_is_low_s(const uint8_t signature[64]);
lxp_result lxp_secp256k1_sign(const uint8_t private_key[32],
                              lxp_domain_tag_id domain,
                              const void *message, size_t message_length,
                              uint8_t signature[64]);
lxp_result lxp_secp256k1_verify(const uint8_t *public_key,
                                size_t public_key_length,
                                const uint8_t signature[64],
                                lxp_domain_tag_id domain,
                                const void *message, size_t message_length);
lxp_result lxp_secp256k1_recover_address(const uint8_t signature[64],
                                         uint8_t recovery_id,
                                         const uint8_t digest[32],
                                         uint8_t address[20]);
lxp_result lxp_secp256k1_address(const uint8_t *public_key,
                                 size_t public_key_length,
                                 uint8_t address[20]);
lxp_result lxp_keccak256(const uint8_t *data, size_t length,
                         uint8_t out[32]);

#endif
