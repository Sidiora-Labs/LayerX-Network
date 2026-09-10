#ifndef LAYERX_LXP_GATEWAY_INTERNAL_H
#define LAYERX_LXP_GATEWAY_INTERNAL_H

#include "layerx/lxp_gateway.h"

#include <pthread.h>
#include <stdatomic.h>

enum {
    LXP_GATEWAY_KV_INVOICE_KEY_BYTES = 70,
    LXP_GATEWAY_KV_IDEMPOTENCY_KEY_BYTES = 40,
    LXP_GATEWAY_KV_PROJECTION_BYTES = 97,
    LXP_GATEWAY_KV_IDEMPOTENCY_VALUE_BYTES = 129,
    LXP_GATEWAY_KV_INVOICE_VALUE_PREFIX_BYTES = 64,
    LXP_GATEWAY_KV_RECEIPT_FIXED_BYTES = 591,
    LXP_GATEWAY_KV_EFFECT_FIXED_BYTES = 42,
    LXP_GATEWAY_IDEMPOTENCY_DOMAIN_SEND = 1,
    LXP_GATEWAY_IDEMPOTENCY_DOMAIN_RECEIVE = 2
};

typedef struct lxp_gateway_kv_entry {
    uint8_t *key;
    size_t key_length;
    uint8_t *value;
    size_t value_length;
} lxp_gateway_kv_entry;

typedef struct lxp_gateway_kv_undo {
    size_t index;
    uint8_t *value;
    size_t value_length;
    bool created;
} lxp_gateway_kv_undo;

typedef struct lxp_gateway_kv {
    lxp_gateway_kv_entry *entries;
    size_t count;
    size_t capacity;
    lxp_gateway_kv_undo *undo;
    size_t undo_count;
    size_t undo_capacity;
    uint64_t stored_bytes;
} lxp_gateway_kv;

struct lxp_gateway_invoice_registry {
    lxp_gateway_kv kv;
    size_t count;
    lxp_receipt *scratch;
    pthread_mutex_t coordination_mutex;
    atomic_size_t active_users;
    atomic_uint lifecycle;
};

typedef enum lxp_gateway_transaction_boundary {
    LXP_GATEWAY_AFTER_GRANT_WRITE = 1,
    LXP_GATEWAY_AFTER_BALANCE_WRITE = 2,
    LXP_GATEWAY_AFTER_STATE_ROOT = 3,
    LXP_GATEWAY_AFTER_RECEIPT_SIGN = 4,
    LXP_GATEWAY_AFTER_IDEMPOTENCY_WRITE = 5,
    LXP_GATEWAY_AFTER_INVOICE_WRITE = 6
} lxp_gateway_transaction_boundary;

#ifdef LXP_TESTING
void lxp_gateway_send_test_fail_after(
    lxp_gateway_transaction_boundary boundary);
void lxp_gateway_receive_test_fail_after(
    lxp_gateway_transaction_boundary boundary);
void lxp_gateway_registry_test_pause_before_activation(void);
bool lxp_gateway_registry_test_activation_paused(void);
void lxp_gateway_registry_test_release_activation(void);
#endif
lxp_result lxp_gateway_invoice_state_locked(
    const lxp_gateway_invoice_registry *registry,
    const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32],
    lxp_receipt *receipt,
    bool *settled);
lxp_result lxp_gateway_registry_enter(
    lxp_gateway_invoice_registry *registry,
    lx_account_registry *accounts);
lxp_result lxp_gateway_registry_leave(
    lxp_gateway_invoice_registry *registry);
#ifdef LXP_TESTING
lxp_result lxp_gateway_grant_present_test_locked(
    const lxp_payer_grant *grant,
    lx_account_registry *accounts,
    lxp_grant_store *store);
#endif

lxp_result lxp_gateway_kv_init(lxp_gateway_kv *kv);
void lxp_gateway_kv_release(lxp_gateway_kv *kv);
lxp_result lxp_gateway_kv_get(
    const lxp_gateway_kv *kv, const uint8_t *key, size_t key_length,
    const uint8_t **value, size_t *value_length);
lxp_result lxp_gateway_kv_put(
    lxp_gateway_kv *kv, const uint8_t *key, size_t key_length,
    const uint8_t *value, size_t value_length, lxp_meter_ctx *meter);
size_t lxp_gateway_kv_mark(const lxp_gateway_kv *kv);
lxp_result lxp_gateway_kv_rollback(lxp_gateway_kv *kv, size_t mark);
void lxp_gateway_kv_commit(lxp_gateway_kv *kv, size_t mark);
lxp_result lxp_gateway_kv_root(const lxp_gateway_kv *kv, uint8_t root[32]);

void lxp_gateway_invoice_key(
    uint8_t key[LXP_GATEWAY_KV_INVOICE_KEY_BYTES],
    const uint8_t invoice_id[32], const uint8_t idempotency_key[32]);
void lxp_gateway_idempotency_key_bytes(
    uint8_t key[LXP_GATEWAY_KV_IDEMPOTENCY_KEY_BYTES], uint8_t domain,
    const uint8_t idempotency_key[32]);
lxp_result lxp_gateway_invoice_record_put(
    lxp_gateway_invoice_registry *registry, const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32], const lxp_receipt *receipt,
    lxp_meter_ctx *meter);
lxp_result lxp_gateway_invoice_record_get(
    const lxp_gateway_kv *kv, const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32], lxp_receipt *receipt, bool *settled);
lxp_result lxp_gateway_idempotency_record_put(
    lxp_gateway_kv *kv, uint8_t domain, const lxp_send_store_record *record,
    lxp_meter_ctx *meter);
lxp_result lxp_gateway_idempotency_precheck(
    const lxp_gateway_kv *kv, uint8_t domain,
    const uint8_t idempotency_key[32], const uint8_t activity_hash[32],
    lxp_send_receipt_projection *projection);
lxp_result lxp_gateway_window_reserve(
    lxp_gateway_kv *kv, lxp_send_store *store, uint8_t domain,
    lxp_meter_ctx *meter, lxp_send_store **backup);

#endif
