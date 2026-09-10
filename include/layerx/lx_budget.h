#ifndef LAYERX_LX_BUDGET_H
#define LAYERX_LX_BUDGET_H

#include "layerx/lx_asset.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_BUDGET_STORE_CAPACITY = 128,
    LX_BUDGET_MAX_DELEGATES = 16,
    LX_BUDGET_CREATE = 0x00030001,
    LX_BUDGET_FUND = 0x00030002,
    LX_BUDGET_AMEND = 0x00030003,
    LX_BUDGET_DELEGATE_ADD = 0x00030004,
    LX_BUDGET_DELEGATE_REMOVE = 0x00030005,
    LX_BUDGET_SPEND = 0x00030006,
    LX_BUDGET_CLOSE = 0x00030007
};

enum {
    LX_BUDGET_RECORD_FIXED_BYTES = 278,
    LX_BUDGET_RECORD_MAX_BYTES =
        LX_BUDGET_RECORD_FIXED_BYTES + LX_BUDGET_MAX_DELEGATES * 32,
    LX_BUDGET_STATE_KEY_BYTES = 39,
    LX_BUDGET_CREATE_PAYLOAD_BYTES = 211,
    LX_BUDGET_FUND_PAYLOAD_BYTES = 50,
    LX_BUDGET_AMEND_PAYLOAD_BYTES = 75,
    LX_BUDGET_DELEGATE_PAYLOAD_BYTES = 66,
    LX_BUDGET_SPEND_PAYLOAD_BYTES = 82,
    LX_BUDGET_CLOSE_PAYLOAD_BYTES = 42
};

typedef enum lx_budget_rollover_policy {
    LX_BUDGET_ROLLOVER_NONE = 1,
    LX_BUDGET_ROLLOVER_CAPPED = 2
} lx_budget_rollover_policy;

typedef struct lx_budget_record {
    uint8_t budget_id[32];
    uint8_t owner[32];
    uint8_t budget_account[32];
    uint8_t asset_id[32];
    lxp_u128 per_period_limit;
    lxp_u128 configured_period_limit;
    uint64_t period_length;
    uint64_t period_start;
    lx_budget_rollover_policy rollover_policy;
    lxp_u128 carry_cap;
    uint8_t delegates[LX_BUDGET_MAX_DELEGATES][32];
    size_t delegate_count;
    uint8_t purpose_hash[32];
    uint64_t expiry;
    uint64_t revocation_sequence;
    lxp_u128 spent_this_period;
    lxp_u128 carried;
    bool closed;
    bool revoked;
} lx_budget_record;

typedef struct lx_budget_store {
    lx_budget_record records[LX_BUDGET_STORE_CAPACITY];
    size_t count;
} lx_budget_store;

typedef struct lx_budget_runtime {
    lx_budget_store *store;
} lx_budget_runtime;

typedef struct lx_budget_fund_request {
    lx_budget_store *store;
    lx_account *owner;
    lx_account *budget_account;
    const lx_asset_record *asset;
    lxp_u128 amount;
    lxp_transfer_context context;
    lx_budget_record record;
} lx_budget_fund_request;

typedef struct lx_budget_spend_request {
    lx_budget_store *store;
    const uint8_t *budget_id;
    lx_account *budget_account;
    lx_account *recipient;
    const lx_asset_record *asset;
    lxp_u128 amount;
    lxp_transfer_context context;
} lx_budget_spend_request;

typedef struct lx_budget_delegate_capability {
    uint8_t holder[32];
    uint8_t asset_id[32];
    uint8_t recipient[32];
    lxp_u128 maximum_per_spend;
    lxp_u128 maximum_total;
    lxp_u128 consumed;
    uint64_t expiry;
    uint64_t revocation_sequence;
    uint8_t purpose_hash[32];
    bool revoked;
} lx_budget_delegate_capability;

typedef struct lx_budget_delegate_spend_request {
    lx_budget_spend_request spend;
    const uint8_t *submitter;
    lx_budget_delegate_capability *capability;
} lx_budget_delegate_spend_request;

typedef struct lx_budget_pull_request {
    lx_budget_spend_request spend;
    const lxp_payer_grant *grant;
    const lx_account *grantor;
} lx_budget_pull_request;

typedef struct lx_budget_close_request {
    lx_budget_store *store;
    const uint8_t *budget_id;
    lx_account *budget_account;
    lx_account *owner;
    const lx_asset_record *asset;
    lxp_u128 amount;
    uint64_t revocation_sequence;
    lxp_transfer_context context;
} lx_budget_close_request;

typedef struct lx_budget_create_payload {
    uint8_t budget_id[32];
    uint8_t budget_account[32];
    uint8_t asset_id[32];
    uint8_t purpose_hash[32];
    lxp_u128 per_period_limit;
    lxp_u128 carry_cap;
    lxp_u128 amount;
    uint64_t period_length;
    uint64_t period_start;
    uint64_t expiry;
    uint64_t revocation_sequence;
    uint8_t rollover_policy;
} lx_budget_create_payload;

typedef struct lx_budget_amount_payload {
    uint8_t budget_id[32];
    lxp_u128 amount;
} lx_budget_amount_payload;

typedef struct lx_budget_amend_payload {
    uint8_t budget_id[32];
    lxp_u128 per_period_limit;
    lxp_u128 carry_cap;
    uint64_t expiry;
    uint8_t rollover_policy;
} lx_budget_amend_payload;

typedef struct lx_budget_delegate_payload {
    uint8_t budget_id[32];
    uint8_t delegate[32];
} lx_budget_delegate_payload;

typedef struct lx_budget_spend_payload {
    uint8_t budget_id[32];
    uint8_t recipient[32];
    lxp_u128 amount;
} lx_budget_spend_payload;

typedef struct lx_budget_close_payload {
    uint8_t budget_id[32];
    uint64_t revocation_sequence;
} lx_budget_close_payload;

const lxp_module_iface *lx_budget_module_iface(void);
lxp_result lx_budget_lookup(lx_budget_store *store,
                            const uint8_t budget_id[32],
                            lx_budget_record **record);
lxp_result lx_budget_record_validate(const lx_budget_record *record);
lxp_result lx_budget_state_key(const uint8_t budget_id[32],
                               uint8_t key[LX_BUDGET_STATE_KEY_BYTES]);
lxp_result lx_budget_record_encode(const lx_budget_record *record,
                                   uint8_t *bytes, size_t capacity,
                                   size_t *length);
lxp_result lx_budget_record_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_record *record);
lxp_result lx_budget_create_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_create_payload *payload);
lxp_result lx_budget_amount_decode(const uint8_t *bytes, size_t length,
                                   lx_budget_amount_payload *payload);
lxp_result lx_budget_amend_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_amend_payload *payload);
lxp_result lx_budget_delegate_decode(const uint8_t *bytes, size_t length,
                                     lx_budget_delegate_payload *payload);
lxp_result lx_budget_spend_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_spend_payload *payload);
lxp_result lx_budget_close_decode(const uint8_t *bytes, size_t length,
                                  lx_budget_close_payload *payload);
void lx_budget_bind_source_authority(lxp_transfer_set *set,
                                     lxp_transfer_source_authority *source);
lxp_result lx_budget_state_put(lx_budget_store *store,
                               const lx_budget_record *record);
lxp_result lx_budget_create_execute(lxp_module_ctx *ctx,
                                    const lx_budget_fund_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_budget_fund_execute(lxp_module_ctx *ctx,
                                  const lx_budget_fund_request *request,
                                  lxp_receipt *receipt);
lxp_result lx_budget_periods_elapsed(const lx_budget_record *record,
                                     uint64_t batch_timestamp,
                                     uint64_t *periods);
lxp_result lx_budget_rollover(lx_budget_record *record,
                              uint64_t batch_timestamp);
lxp_result lx_budget_epoch_begin(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp);
lxp_result lx_budget_allowance_debit(lx_budget_record *record,
                                     lxp_u128 amount);
lxp_result lx_budget_remaining(lx_budget_record *record,
                               const lx_account *budget_account,
                               lxp_u128 *remaining);
lxp_result lx_budget_spend_prepare(lx_budget_record *record,
                                   uint64_t batch_timestamp,
                                   lxp_u128 balance, lxp_u128 amount);
lxp_result lx_budget_spend_execute(lxp_module_ctx *ctx,
                                   const lx_budget_spend_request *request,
                                   lxp_receipt *receipt);
lxp_result lx_budget_delegate_add_execute(lx_budget_record *record,
                                          const uint8_t delegate[32]);
lxp_result lx_budget_delegate_remove_execute(lx_budget_record *record,
                                             const uint8_t delegate[32]);
bool lx_budget_delegate_present(const lx_budget_record *record,
                                const uint8_t delegate[32]);
lxp_result lx_budget_authorize_delegate(
    const lx_budget_record *record, const uint8_t submitter[32],
    lx_budget_delegate_capability *capability,
    const uint8_t recipient[32], lxp_u128 amount,
    uint64_t batch_timestamp);
lxp_result lx_budget_delegate_spend_execute(
    lxp_module_ctx *ctx, const lx_budget_delegate_spend_request *request,
    lxp_receipt *receipt);
lxp_result lx_budget_pull_execute(lxp_module_ctx *ctx,
                                  const lx_budget_pull_request *request,
                                  lxp_receipt *receipt);
lxp_result lx_budget_defund_execute(lxp_module_ctx *ctx,
                                    const lx_budget_close_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_budget_revoke_execute(lxp_module_ctx *ctx,
                                    const lx_budget_close_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_budget_close_execute(lxp_module_ctx *ctx,
                                   const lx_budget_close_request *request,
                                   lxp_receipt *receipt);
lxp_result lx_budget_authority_check(const lx_account *account,
                                     lxp_authorization_kind authority_kind,
                                     uint16_t origin_module_id,
                                     uint16_t reason);

#endif
