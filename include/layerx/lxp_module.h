#ifndef LAYERX_LXP_MODULE_H
#define LAYERX_LXP_MODULE_H

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_authority.h"
#include "layerx/lxp_result.h"

#include <stddef.h>
#include <stdint.h>

enum {
    LXP_MODULE_ASSET = 1,
    LXP_MODULE_ESCROW = 2,
    LXP_MODULE_BUDGET = 3,
    LXP_MODULE_STREAM = 4,
    LXP_MODULE_SERVICE = 5,
    LXP_MODULE_PERPS = 6,
    LXP_MODULE_GOVERNANCE = 7,
    LXP_MODULE_BRIDGE = 8,
    LXP_MODULE_PROGRAMS = 9,
    LXP_MODULE_RESERVED_COUNT = 9,
    LXP_MODULE_MAX_NAME = 31,
    LXP_MODULE_MAX_ACTIVITY_TYPES = 64
};

typedef struct lxp_module_ctx lxp_module_ctx;
typedef struct lxp_effect_buffer lxp_effect_buffer;
typedef struct lxp_transfer_set lxp_transfer_set;
typedef struct lxp_receipt lxp_receipt;
typedef struct lxp_verified_receipt_facts lxp_verified_receipt_facts;
#define lxp_module_ctx lxp_module_ctx
#define lxp_effect_buffer lxp_effect_buffer
#define lxp_transfer_set lxp_transfer_set
#define lxp_receipt lxp_receipt


typedef lxp_result (*lxp_kv_visit_fn)(const uint8_t *key, size_t key_length,
                                     const uint8_t *value,
                                     size_t value_length, void *user);

typedef lxp_result (*lxp_module_genesis_fn)(lxp_module_ctx *ctx,
                                            const uint8_t *manifest,
                                            size_t manifest_length);
typedef lxp_result (*lxp_module_decode_fn)(lxp_module_ctx *ctx,
                                           uint16_t type_ordinal,
                                           const uint8_t *payload,
                                           size_t payload_length,
                                           void **decoded);
typedef lxp_result (*lxp_module_validate_fn)(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded);
typedef lxp_result (*lxp_module_execute_fn)(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects);
typedef lxp_result (*lxp_module_epoch_fn)(lxp_module_ctx *ctx,
                                          uint64_t epoch,
                                          uint64_t timestamp_ms);
typedef lxp_result (*lxp_module_state_root_fn)(lxp_module_ctx *ctx,
                                               uint8_t root[32]);
typedef void (*lxp_module_release_fn)(lxp_module_ctx *ctx, void *decoded);

typedef struct lxp_module_iface {
    uint16_t module_id;
    uint32_t abi_version;
    const char *name;
    const uint32_t *activity_types;
    size_t activity_type_count;
    lxp_module_genesis_fn genesis;
    lxp_module_decode_fn decode;
    lxp_module_validate_fn validate;
    lxp_module_execute_fn execute;
    lxp_module_epoch_fn epoch_begin;
    lxp_module_epoch_fn epoch_end;
    lxp_module_state_root_fn state_root;
    lxp_module_release_fn release;
} lxp_module_iface;
#define lxp_module_iface lxp_module_iface

lxp_result lxp_ctx_kv_get(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t **value,
                          size_t *value_length);
lxp_result lxp_ctx_kv_put(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t *value,
                          size_t value_length);
lxp_result lxp_ctx_kv_del(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length);
lxp_result lxp_ctx_kv_iter(lxp_module_ctx *ctx, const uint8_t *prefix,
                           size_t prefix_length, lxp_kv_visit_fn visit,
                           void *user);
lxp_result lxp_ctx_emit_transfer_set(lxp_module_ctx *ctx,
                                     const lxp_transfer_set *set,
                                     lxp_receipt *receipt);
lxp_result lxp_ctx_emit_event(lxp_module_ctx *ctx, uint16_t event_type,
                              const uint8_t *body, size_t body_length);
uint64_t lxp_ctx_batch_timestamp_ms(const lxp_module_ctx *ctx);
uint64_t lxp_ctx_batch_number(const lxp_module_ctx *ctx);
uint64_t lxp_ctx_epoch(const lxp_module_ctx *ctx);
uint64_t lxp_ctx_global_sequence(const lxp_module_ctx *ctx);
lxp_result lxp_ctx_read_param(const lxp_module_ctx *ctx, uint32_t parameter_id,
                              uint64_t *value);
lxp_result lxp_ctx_charge_gas(lxp_module_ctx *ctx, uint64_t units);
lxp_result lxp_ctx_arena_alloc(lxp_module_ctx *ctx, size_t size,
                               size_t alignment, void **allocation);
void *lxp_ctx_module_runtime(const lxp_module_ctx *ctx);
lxp_result lxp_ctx_verified_receipt_facts(
    const lxp_module_ctx *ctx, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts);

#endif
