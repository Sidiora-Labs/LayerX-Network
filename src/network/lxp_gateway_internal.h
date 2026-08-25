#ifndef LAYERX_LXP_GATEWAY_INTERNAL_H
#define LAYERX_LXP_GATEWAY_INTERNAL_H

#include "layerx/lxp_gateway.h"

#include <pthread.h>
#include <stdatomic.h>

struct lxp_gateway_invoice_registry {
    lxp_gateway_invoice_record records[LXP_GATEWAY_INVOICE_CAPACITY];
    size_t count;
    pthread_mutex_t coordination_mutex;
    lx_account_registry *owner_accounts;
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

#endif
