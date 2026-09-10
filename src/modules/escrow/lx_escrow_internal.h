#ifndef LAYERX_LX_ESCROW_INTERNAL_H
#define LAYERX_LX_ESCROW_INTERNAL_H

#include "layerx/lx_escrow.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_ESCROW_HOLD_PREFIX_BYTES = 5,
    LX_ESCROW_RESULT_PREFIX_BYTES = 7,
    LX_ESCROW_HOLD_KEY_BYTES = LX_ESCROW_HOLD_PREFIX_BYTES + 32,
    LX_ESCROW_RESULT_KEY_BYTES = LX_ESCROW_RESULT_PREFIX_BYTES + 32
};

/* One settlement of an escrow hold.  The second leg exists only for a dispute
 * resolution split; every leg debits the same escrow account so the ledger
 * sees exactly one source authority. */
typedef struct lx_escrow_settlement {
    lx_account *from;
    lx_account *to;
    lx_account *secondary_to;
    const lx_asset_record *asset;
    lxp_u128 amount;
    lxp_u128 secondary_amount;
    uint16_t reason;
} lx_escrow_settlement;

void lx_escrow_write_u64(uint8_t out[8], uint64_t value);
uint64_t lx_escrow_read_u64(const uint8_t in[8]);
lxp_result lx_escrow_hold_key(const uint8_t escrow_id[32],
                              uint8_t key[LX_ESCROW_HOLD_KEY_BYTES]);
lxp_result lx_escrow_result_key(const uint8_t idempotency_key[32],
                                uint8_t key[LX_ESCROW_RESULT_KEY_BYTES]);
lxp_result lx_escrow_hold_prefix(uint8_t prefix[LX_ESCROW_HOLD_PREFIX_BYTES]);
bool lx_escrow_active_state(lx_escrow_status state);
bool lx_escrow_terminal_state(lx_escrow_status state);
void lx_escrow_receipt_from_result(const lx_escrow_economic_result *result,
                                   lxp_receipt *receipt);
lxp_result lx_escrow_settle(lxp_module_ctx *ctx,
                            const lxp_transfer_context *base,
                            const lx_escrow_settlement *settlement,
                            lxp_authorization_kind authority_kind,
                            lxp_receipt *receipt);
lxp_result lx_escrow_state_write(lxp_module_ctx *ctx,
                                 const lx_escrow_record *record);
lxp_result lx_escrow_commit_result(lxp_module_ctx *ctx,
                                   const lx_escrow_record *record,
                                   const uint8_t idempotency_key[32],
                                   const lx_escrow_settlement *settlement,
                                   uint16_t ordinal, lxp_receipt *receipt);
lxp_result lx_escrow_event_body(const lx_escrow_record *record,
                                uint16_t ordinal,
                                uint8_t body[LX_ESCROW_EVENT_BYTES]);
lx_escrow_runtime *lx_escrow_require_runtime(lxp_module_ctx *ctx);
lxp_result lx_escrow_resolve_account(lx_escrow_runtime *runtime,
                                     const uint8_t id[32],
                                     lx_account **account);

#endif
