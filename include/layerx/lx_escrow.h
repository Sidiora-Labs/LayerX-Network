#ifndef LAYERX_LX_ESCROW_H
#define LAYERX_LX_ESCROW_H

#include "layerx/lx_asset.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_ESCROW_OPEN = 0x00020001,
    LX_ESCROW_CAPTURE = 0x00020002,
    LX_ESCROW_PARTIAL_CAPTURE = 0x00020003,
    LX_ESCROW_RELEASE = 0x00020004,
    LX_ESCROW_TIMEOUT = 0x00020005,
    LX_ESCROW_DISPUTE_OPEN = 0x00020006,
    LX_ESCROW_DISPUTE_RESOLVE = 0x00020007
};

enum {
    LX_ESCROW_RECORD_BYTES = 305,
    LX_ESCROW_RESULT_BYTES = 243,
    LX_ESCROW_OPEN_PAYLOAD_BYTES = 288,
    LX_ESCROW_CAPTURE_PAYLOAD_BYTES = 80,
    LX_ESCROW_RELEASE_PAYLOAD_BYTES = 64,
    LX_ESCROW_DISPUTE_OPEN_PAYLOAD_BYTES = 32,
    LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES = 68,
    LX_ESCROW_EVENT_BYTES = 67,
    LX_ESCROW_SWEEP_CAPACITY = 24
};

typedef enum lx_escrow_status {
    LX_ESCROW_STATE_OPEN = 1,
    LX_ESCROW_STATE_PARTIALLY_CAPTURED,
    LX_ESCROW_STATE_CAPTURED,
    LX_ESCROW_STATE_RELEASED,
    LX_ESCROW_STATE_DISPUTED,
    LX_ESCROW_STATE_RESOLVED,
    LX_ESCROW_STATE_TIMED_OUT
} lx_escrow_status;

typedef struct lx_escrow_record {
    uint8_t escrow_id[32];
    uint8_t owner[32];
    uint8_t escrow_account[32];
    uint8_t beneficiary[32];
    uint8_t arbiter[32];
    uint8_t asset_id[32];
    lxp_u128 locked_amount;
    lxp_u128 captured_amount;
    lx_escrow_status state;
    uint64_t expiry;
    uint64_t dispute_window;
    uint8_t terms_hash[32];
    uint8_t agreement_reference[32];
} lx_escrow_record;

/* Canonical projection of one settled escrow activity.  The full receipt is
 * rebuilt from these bytes on an idempotent replay, so the module never has to
 * persist a receipt structure in module state. */
typedef struct lx_escrow_economic_result {
    uint8_t escrow_id[32];
    uint16_t ordinal;
    lx_escrow_status state_after;
    lxp_u128 captured_after;
    lxp_u128 locked_after;
    uint8_t asset_id[32];
    uint8_t from[32];
    uint8_t to[32];
    lxp_u128 amount;
    lxp_u128 secondary_amount;
    uint8_t transfer_set_root[32];
    uint64_t global_sequence;
    uint64_t timestamp;
} lx_escrow_economic_result;

typedef struct lx_escrow_open_request {
    lx_account *owner;
    lx_account *escrow_account;
    const lx_asset_record *asset;
    lxp_u128 amount;
    lxp_transfer_context context;
    lx_escrow_record record;
} lx_escrow_open_request;

typedef struct lx_escrow_capture_request {
    const uint8_t *escrow_id;
    lx_account *escrow_account;
    lx_account *beneficiary_account;
    lx_account *owner_account;
    const lx_asset_record *asset;
    lxp_u128 amount;
    const lxp_authority_resolved *authority;
    uint8_t idempotency_key[32];
    lxp_transfer_context context;
} lx_escrow_capture_request;

typedef struct lx_escrow_release_request {
    const uint8_t *escrow_id;
    lx_account *escrow_account;
    lx_account *owner_account;
    const lx_asset_record *asset;
    const lxp_authority_resolved *authority;
    uint8_t idempotency_key[32];
    lxp_transfer_context context;
} lx_escrow_release_request;

typedef struct lx_escrow_runtime {
    lx_account_registry *accounts;
    lx_asset_registry *assets;
} lx_escrow_runtime;

typedef struct lx_escrow_dispute_request {
    const uint8_t *escrow_id;
    lx_account *escrow_account;
    lx_account *beneficiary_account;
    lx_account *owner_account;
    const lx_asset_record *asset;
    const lxp_authority_resolved *authority;
    uint32_t beneficiary_basis_points;
    uint8_t idempotency_key[32];
    lxp_transfer_context context;
} lx_escrow_dispute_request;

typedef lxp_result (*lx_escrow_visit_fn)(const lx_escrow_record *record,
                                         void *user);

const lxp_module_iface *lx_escrow_module_iface(void);
lxp_result lx_escrow_record_encode(const lx_escrow_record *record,
                                   uint8_t bytes[LX_ESCROW_RECORD_BYTES]);
lxp_result lx_escrow_record_decode(const uint8_t *bytes, size_t length,
                                   lx_escrow_record *record);
lxp_result lx_escrow_result_encode(const lx_escrow_economic_result *result,
                                   uint8_t bytes[LX_ESCROW_RESULT_BYTES]);
lxp_result lx_escrow_result_decode(const uint8_t *bytes, size_t length,
                                   lx_escrow_economic_result *result);
lxp_result lx_escrow_state_put(lxp_module_ctx *ctx,
                               const lx_escrow_record *record);
lxp_result lx_escrow_state_update(lxp_module_ctx *ctx,
                                  const lx_escrow_record *record);
lxp_result lx_escrow_lookup(lxp_module_ctx *ctx,
                            const uint8_t escrow_id[32],
                            lx_escrow_record *record);
lxp_result lx_escrow_state_iter(lxp_module_ctx *ctx, lx_escrow_visit_fn visit,
                                void *user);
lxp_result lx_escrow_open_execute(lxp_module_ctx *ctx,
                                  const lx_escrow_open_request *request,
                                  lxp_receipt *receipt);
lxp_result lx_escrow_remaining(const lx_escrow_record *record,
                               const lx_account *escrow_account,
                               lxp_u128 *remaining);
lxp_result lx_escrow_capture_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_capture_request *request,
                                     lxp_receipt *receipt);
lxp_result lx_escrow_partial_capture_execute(
    lxp_module_ctx *ctx, const lx_escrow_capture_request *request,
    lxp_receipt *receipt);
lxp_result lx_escrow_release_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_release_request *request,
                                     lxp_receipt *receipt);
lxp_result lx_escrow_timeout_execute(lxp_module_ctx *ctx,
                                     const lx_escrow_release_request *request,
                                     lxp_receipt *receipt);
lxp_result lx_escrow_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp);
lxp_result lx_escrow_receipt_replay(lxp_module_ctx *ctx,
                                    const uint8_t key[32],
                                    lxp_receipt *receipt, bool *found);
lxp_result lx_escrow_receipt_record(lxp_module_ctx *ctx,
                                    const uint8_t key[32],
                                    const lx_escrow_economic_result *result);
/* Canonical idempotency key of the expiry sweep transition for one hold. */
lxp_result lx_escrow_timeout_key(const lx_escrow_record *record,
                                 uint8_t key[32]);
lxp_result lx_escrow_dispute_open_execute(
    lxp_module_ctx *ctx, const lx_escrow_dispute_request *request);
lxp_result lx_escrow_split_bps(lxp_u128 balance,
                               uint32_t beneficiary_basis_points,
                               lxp_u128 *beneficiary, lxp_u128 *owner);
lxp_result lx_escrow_dispute_resolve_execute(
    lxp_module_ctx *ctx, const lx_escrow_dispute_request *request,
    lxp_receipt *receipt);
lxp_result lx_escrow_authority_check(const lx_account *account,
                                     lxp_authorization_kind authority_kind,
                                     uint16_t origin_module_id,
                                     uint16_t reason);
lxp_result lx_escrow_invariant_check(const lx_escrow_record *record,
                                     const lx_account *escrow_account);

#endif
