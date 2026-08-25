#ifndef LAYERX_LXP_GATEWAY_H
#define LAYERX_LXP_GATEWAY_H

#include "layerx/lxp_receipt.h"
#include "layerx/lxp_transfer.h"
#include "layerx/lx_asset.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_GATEWAY_HTTP_PAYMENT_REQUIRED = 402,
    LXP_PAYMENT_REQUIREMENT_PREIMAGE_SIZE = 160,
    LXP_PAYMENT_REQUIREMENT_ENCODED_SIZE = 224
};

enum {
    LXP_GATEWAY_INVOICE_CAPACITY = 256
};

typedef struct lxp_payment_requirement {
    uint32_t network_id;
    uint8_t recipient[32];
    uint8_t asset[32];
    lxp_u128 amount;
    uint8_t invoice_id[32];
    uint8_t purpose_hash[32];
    uint64_t expiry;
    uint32_t acceptable_conditions;
    uint8_t service_signature[64];
} lxp_payment_requirement;
#define lxp_payment_requirement lxp_payment_requirement

typedef struct lxp_gateway_invoice_record {
    uint8_t invoice_id[32];
    uint8_t idempotency_key[32];
    lxp_receipt receipt;
} lxp_gateway_invoice_record;

typedef struct lxp_gateway_invoice_registry lxp_gateway_invoice_registry;

typedef struct lxp_gateway_settlement_context {
    lx_asset_registry *assets;
    lxp_send_environment *send_environment;
    lxp_gateway_invoice_registry *invoices;
    const uint8_t *service_public_key;
    const uint8_t *sequencer_private_key;
    uint64_t global_sequence;
    uint8_t batch_id[32];
    lxp_arena *arena;
} lxp_gateway_settlement_context;

typedef struct lxp_gateway_receive_context {
    lx_asset_registry *assets;
    lxp_receive_environment *receive_environment;
    lxp_gateway_invoice_registry *invoices;
    const uint8_t *service_public_key;
    const uint8_t *sequencer_private_key;
    uint64_t global_sequence;
    uint8_t batch_id[32];
    lxp_arena *arena;
} lxp_gateway_receive_context;

lxp_gateway_invoice_registry *lxp_gateway_invoice_registry_create(
    lx_account_registry *owner_accounts,
    lxp_result *status);
lxp_result lxp_gateway_invoice_registry_destroy(
    lxp_gateway_invoice_registry **registry);

lxp_result lxp_payment_requirement_encode(
    const lxp_payment_requirement *requirement,
    bool include_signature,
    uint8_t *bytes,
    size_t capacity,
    size_t *length);
lxp_result lxp_payment_requirement_verify(
    const lxp_payment_requirement *requirement,
    uint32_t executing_network_id,
    const uint8_t service_public_key[32]);
lxp_result lxp_gateway_translate(
    const uint8_t *json,
    size_t json_length,
    lxp_payment_requirement *requirement,
    uint8_t canonical[LXP_PAYMENT_REQUIREMENT_ENCODED_SIZE],
    size_t *canonical_length);
lxp_result lxp_gateway_invoice_state(
    lxp_gateway_invoice_registry *registry,
    const uint8_t invoice_id[32],
    const uint8_t idempotency_key[32],
    lxp_receipt *receipt,
    bool *settled);
lxp_result lxp_gateway_send_settle(
    const lxp_payment_requirement *requirement,
    const lxp_send *send,
    lxp_gateway_settlement_context *context,
    lxp_receipt *receipt);
lxp_result lxp_gateway_receipt_return(
    const lxp_receipt *receipt,
    lxp_arena *arena,
    lxp_byte_span *canonical_receipt);
lxp_result lxp_gateway_grant_bounds_check(
    const lxp_payment_requirement *requirement,
    const lxp_receive *receive,
    const lxp_grant_state *grant_state,
    const lxp_receive_environment *environment);
lxp_result lxp_gateway_receive_claim(
    const lxp_payment_requirement *requirement,
    const lxp_receive *receive,
    lxp_gateway_receive_context *context,
    lxp_receipt *receipt);

#endif
