#ifndef LAYERX_LX_STREAM_H
#define LAYERX_LX_STREAM_H

#include "layerx/lx_asset.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_STREAM_MAX_METER_AUTHORITIES = 8,
    LX_STREAM_PAYLOAD_VERSION = 1,
    LX_STREAM_KEY_BYTES = 39,
    LX_STREAM_RECORD_BYTES = 541,
    LX_STREAM_RESULT_BYTES = 67,
    LX_STREAM_OPEN_PAYLOAD_FIXED = 204,
    LX_STREAM_OPEN_PAYLOAD_MAX =
        LX_STREAM_OPEN_PAYLOAD_FIXED + LX_STREAM_MAX_METER_AUTHORITIES * 32,
    LX_STREAM_TOP_UP_PAYLOAD_BYTES = 50,
    LX_STREAM_METER_PAYLOAD_BYTES = 138,
    LX_STREAM_KEYED_PAYLOAD_BYTES = 66,
    LX_STREAM_ID_PAYLOAD_BYTES = 34,
    LX_STREAM_OPEN = 0x00040001,
    LX_STREAM_TOP_UP = 0x00040002,
    LX_STREAM_METER = 0x00040003,
    LX_STREAM_SETTLE = 0x00040004,
    LX_STREAM_PAUSE = 0x00040005,
    LX_STREAM_RESUME = 0x00040006,
    LX_STREAM_CLOSE = 0x00040007
};

typedef enum lx_stream_mode {
    LX_STREAM_MODE_TIME = 1,
    LX_STREAM_MODE_METERED = 2
} lx_stream_mode;

typedef struct lx_stream_record {
    uint8_t stream_id[32];
    uint8_t payer[32];
    uint8_t stream_account[32];
    uint8_t recipient[32];
    uint8_t asset_id[32];
    lx_stream_mode mode;
    lxp_u128 rate;
    uint64_t rate_unit;
    uint64_t start_timestamp;
    uint64_t last_accrual_timestamp;
    uint64_t end_timestamp;
    lxp_u128 total_cap;
    lxp_u128 accrued_total;
    lxp_u128 settled_total;
    lxp_u128 remainder_carry;
    uint64_t cumulative_meter;
    uint8_t meter_authorities[LX_STREAM_MAX_METER_AUTHORITIES][32];
    size_t meter_authority_count;
    bool underfunded;
    bool paused;
    bool closed;
} lx_stream_record;

/* Durable economic result of one settlement or closure. The ledger writes
 * only the transfer set root into a receipt, so replaying an idempotency key
 * reproduces the original receipt byte for byte from this record. */
typedef struct lx_stream_economic_result {
    uint8_t transfer_set_root[32];
    lxp_u128 paid;
    lxp_u128 refunded;
    uint16_t ordinal;
    uint8_t leg_count;
} lx_stream_economic_result;

/* Asset states the stream module is permitted to move. Bound by the host at
 * LXP_MODULE_STREAM through lxp_kernel_bind_module_runtime; a stream may only
 * fund, draw or refund an asset the host has published here. */
typedef struct lx_stream_runtime {
    const lxp_transfer_asset_state *assets;
    size_t asset_count;
} lx_stream_runtime;

typedef struct lx_stream_fund_request {
    lx_account *payer;
    lx_account *stream_account;
    uint8_t asset_id[32];
    lxp_u128 amount;
    lxp_transfer_context context;
    lx_stream_record record;
} lx_stream_fund_request;

typedef struct lx_stream_meter_attestation {
    uint8_t stream_id[32];
    uint64_t cumulative_reading;
    uint8_t authority_key[32];
    uint8_t signature[64];
} lx_stream_meter_attestation;

typedef struct lx_stream_settle_request {
    const uint8_t *stream_id;
    lx_account *stream_account;
    lx_account *recipient;
    uint8_t asset_id[32];
    uint8_t idempotency_key[32];
    lxp_transfer_context context;
} lx_stream_settle_request;

typedef struct lx_stream_lifecycle_request {
    const uint8_t *stream_id;
    lx_account *stream_account;
    lx_account *payer;
    lx_account *recipient;
    uint8_t asset_id[32];
    const lxp_authority_resolved *authority;
    uint8_t idempotency_key[32];
    lxp_transfer_context context;
} lx_stream_lifecycle_request;

typedef struct lx_stream_open_payload {
    lx_stream_record record;
    lxp_u128 initial_funding;
} lx_stream_open_payload;

typedef struct lx_stream_amount_payload {
    uint8_t stream_id[32];
    lxp_u128 amount;
} lx_stream_amount_payload;

typedef struct lx_stream_keyed_payload {
    uint8_t stream_id[32];
    uint8_t idempotency_key[32];
} lx_stream_keyed_payload;

typedef struct lx_stream_id_payload {
    uint8_t stream_id[32];
} lx_stream_id_payload;

typedef lxp_result (*lx_stream_visit_fn)(const lx_stream_record *record,
                                         void *user);

const lxp_module_iface *lx_stream_module_iface(void);

lxp_result lx_stream_record_validate(const lx_stream_record *record);
lxp_result lx_stream_record_encode(const lx_stream_record *record,
                                   uint8_t bytes[LX_STREAM_RECORD_BYTES]);
lxp_result lx_stream_record_decode(const uint8_t *bytes, size_t length,
                                   lx_stream_record *record);
lxp_result lx_stream_state_key(const uint8_t stream_id[32],
                               uint8_t key[LX_STREAM_KEY_BYTES]);
lxp_result lx_stream_result_key(const uint8_t idempotency_key[32],
                                uint8_t key[LX_STREAM_KEY_BYTES]);
lxp_result lx_stream_load(lxp_module_ctx *ctx, const uint8_t stream_id[32],
                          lx_stream_record *record);
lxp_result lx_stream_save(lxp_module_ctx *ctx, const lx_stream_record *record);
lxp_result lx_stream_iter(lxp_module_ctx *ctx, lx_stream_visit_fn visit,
                          void *user);
lxp_result lx_stream_result_load(lxp_module_ctx *ctx,
                                 const uint8_t idempotency_key[32],
                                 lx_stream_economic_result *result,
                                 bool *found);
lxp_result lx_stream_result_save(lxp_module_ctx *ctx,
                                 const uint8_t idempotency_key[32],
                                 const lx_stream_economic_result *result);
lxp_result lx_stream_result_receipt(const lx_stream_economic_result *result,
                                    lxp_receipt *receipt);
lxp_result lx_stream_transfer_source(lxp_transfer_source_authority *source,
                                     const lx_account *account,
                                     lxp_authorization_kind kind);
/* Builds the module-authorized debit context for a stream-account leg. The
 * stream account is its own debit source under LXP_AUTH_PROTOCOL_MODULE, the
 * only authorization lx_stream_authority_check accepts for that custody
 * kind. */
lxp_result lx_stream_draw_context(lxp_module_ctx *ctx,
                                  lx_account *stream_account,
                                  const lxp_transfer_context *caller,
                                  lxp_transfer_source_authority *source,
                                  lxp_transfer_context *context);

lxp_result lx_stream_open_encode(const lx_stream_open_payload *payload,
                                 uint8_t *bytes, size_t capacity,
                                 size_t *length);
lxp_result lx_stream_open_decode(const uint8_t *bytes, size_t length,
                                 lx_stream_open_payload *payload);
lxp_result lx_stream_amount_encode(const lx_stream_amount_payload *payload,
                                   uint8_t *bytes, size_t capacity,
                                   size_t *length);
lxp_result lx_stream_amount_decode(const uint8_t *bytes, size_t length,
                                   lx_stream_amount_payload *payload);
lxp_result lx_stream_meter_encode(const lx_stream_meter_attestation *payload,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length);
lxp_result lx_stream_meter_decode(const uint8_t *bytes, size_t length,
                                  lx_stream_meter_attestation *payload);
lxp_result lx_stream_keyed_encode(const lx_stream_keyed_payload *payload,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length);
lxp_result lx_stream_keyed_decode(const uint8_t *bytes, size_t length,
                                  lx_stream_keyed_payload *payload);
lxp_result lx_stream_id_encode(const lx_stream_id_payload *payload,
                               uint8_t *bytes, size_t capacity,
                               size_t *length);
lxp_result lx_stream_id_decode(const uint8_t *bytes, size_t length,
                               lx_stream_id_payload *payload);

lxp_result lx_stream_open_execute(lxp_module_ctx *ctx,
                                  const lx_stream_fund_request *request,
                                  lxp_receipt *receipt);
lxp_result lx_stream_top_up_execute(lxp_module_ctx *ctx,
                                    const lx_stream_fund_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_stream_elapsed_ms(const lx_stream_record *record,
                                uint64_t batch_timestamp,
                                uint64_t *elapsed_ms);
lxp_result lx_stream_carry_apply(lx_stream_record *record,
                                 lxp_u128 remainder);
lxp_result lx_stream_accrue(lx_stream_record *record,
                            uint64_t batch_timestamp,
                            lxp_u128 *newly_accrued);
lxp_result lx_stream_meter_attestation_bytes(
    const lx_stream_meter_attestation *attestation,
    uint8_t *bytes, size_t capacity, size_t *length);
lxp_result lx_stream_meter_authority_check(
    const lx_stream_record *record,
    const lx_stream_meter_attestation *attestation);
lxp_result lx_stream_metered_accrue(lx_stream_record *record,
                                    uint64_t cumulative_reading,
                                    lxp_u128 *newly_accrued);
lxp_result lx_stream_meter_execute(
    lx_stream_record *record,
    const lx_stream_meter_attestation *attestation,
    lxp_u128 *newly_accrued);
lxp_result lx_stream_settle_amount(const lx_stream_record *record,
                                   lxp_u128 *amount);
lxp_result lx_stream_mark_underfunded(lx_stream_record *record,
                                      uint64_t batch_timestamp,
                                      lxp_u128 settled_amount);
lxp_result lx_stream_settle_execute(lxp_module_ctx *ctx,
                                    const lx_stream_settle_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_stream_pause_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request);
lxp_result lx_stream_resume_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request);
lxp_result lx_stream_close_execute(
    lxp_module_ctx *ctx, const lx_stream_lifecycle_request *request,
    lxp_receipt *receipt);
lxp_result lx_stream_authority_check(const lx_account *account,
                                     lxp_authorization_kind authority_kind,
                                     uint16_t origin_module_id,
                                     uint16_t reason);

#endif
