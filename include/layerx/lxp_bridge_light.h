#ifndef LAYERX_LXP_BRIDGE_LIGHT_H
#define LAYERX_LXP_BRIDGE_LIGHT_H

#include "layerx/lxp_result.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_BRIDGE_LIGHT_MAX_VALIDATORS = 1000,
    LXP_BRIDGE_LIGHT_MAX_CHAIN_ID = 32,
    LXP_BRIDGE_LIGHT_MAX_RECORD = 4096,
    LXP_BRIDGE_LIGHT_MAX_IAVL_STEPS = 64,
    LXP_BRIDGE_LIGHT_MAX_STORE_STEPS = 32,
    LXP_BRIDGE_LIGHT_TRUST_BYTES = 89
};

typedef struct lxp_bridge_light_trust {
    uint64_t height;
    uint8_t header_hash[32];
    uint8_t next_validators_hash[32];
    int64_t time_seconds;
    uint32_t time_nanos;
} lxp_bridge_light_trust;

typedef struct lxp_bridge_light_result {
    uint64_t height;
    uint8_t header_hash[32];
    uint8_t app_hash[32];
    uint8_t validators_hash[32];
    const uint8_t *key;
    size_t key_length;
    const uint8_t *value;
    size_t value_length;
    lxp_bridge_light_trust advanced;
} lxp_bridge_light_result;

typedef struct lxp_bridge_light_deposit {
    uint8_t deposit_id[32];
    uint8_t depositor[20];
    uint8_t beneficiary[32];
    uint8_t asset_id[32];
    uint8_t amount[16];
    uint64_t nonce;
    uint64_t height;
} lxp_bridge_light_deposit;

lxp_result lxp_bridge_light_verify(const uint8_t *chain_id, size_t chain_id_length,
                                   const uint8_t *store, size_t store_length,
                                   const lxp_bridge_light_trust *trusted,
                                   const uint8_t *bundle, size_t bundle_length,
                                   lxp_bridge_light_result *result);
lxp_result lxp_bridge_light_deposit_decode(const uint8_t *value, size_t length,
                                           lxp_bridge_light_deposit *deposit);
lxp_result lxp_bridge_light_trust_encode(const lxp_bridge_light_trust *trust,
                                         uint8_t bytes[LXP_BRIDGE_LIGHT_TRUST_BYTES]);
lxp_result lxp_bridge_light_trust_decode(const uint8_t *bytes, size_t length,
                                         lxp_bridge_light_trust *trust);

#endif
