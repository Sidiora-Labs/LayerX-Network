#ifndef LAYERX_LXP_GATEWAY_INTERNAL_H
#define LAYERX_LXP_GATEWAY_INTERNAL_H

#include "layerx/lxp_gateway.h"

typedef enum lxp_gateway_transaction_boundary {
    LXP_GATEWAY_AFTER_GRANT_WRITE = 1,
    LXP_GATEWAY_AFTER_BALANCE_WRITE = 2,
    LXP_GATEWAY_AFTER_STATE_ROOT = 3,
    LXP_GATEWAY_AFTER_RECEIPT_SIGN = 4,
    LXP_GATEWAY_AFTER_IDEMPOTENCY_WRITE = 5,
    LXP_GATEWAY_AFTER_INVOICE_WRITE = 6
} lxp_gateway_transaction_boundary;

void lxp_gateway_send_test_fail_after(
    lxp_gateway_transaction_boundary boundary);
void lxp_gateway_receive_test_fail_after(
    lxp_gateway_transaction_boundary boundary);
lxp_result lxp_gateway_invoice_state_locked(
    const lxp_gateway_invoice_registry *registry,
    const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32],
    lxp_receipt *receipt,
    bool *settled);

#endif
