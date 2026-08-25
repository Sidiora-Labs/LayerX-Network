#ifndef LAYERX_LXP_LEDGER_H
#define LAYERX_LXP_LEDGER_H

#include "layerx/lxp_result.h"
#include "layerx/lxp_storage.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdatomic.h>
#include <stdint.h>

enum {
    LX_ACCOUNT_ID_BYTES = 32,
    LX_ACCOUNT_NAME_MAX = 512,
    LX_ACCOUNT_REGISTRY_CAPACITY = 512
};

typedef enum lx_account_kind {
    LX_ACCOUNT_AGENT_MAIN = 1,
    LX_ACCOUNT_AGENT_BUDGET = 2,
    LX_ACCOUNT_AGENT_ESCROW = 3,
    LX_ACCOUNT_AGENT_STREAM = 4,
    LX_ACCOUNT_AGENT_MARGIN = 5,
    LX_ACCOUNT_SYSTEM_LIQUIDITY = 6,
    LX_ACCOUNT_SYSTEM_FUNDING_LONG = 7,
    LX_ACCOUNT_SYSTEM_FUNDING_SHORT = 8,
    LX_ACCOUNT_SYSTEM_INSURANCE = 9,
    LX_ACCOUNT_SYSTEM_FEES = 10,
    LX_ACCOUNT_SYSTEM_PAXEER_RESERVE = 11,
    LX_ACCOUNT_SYSTEM_PAXEER_WITHDRAWALS = 12,
    LX_ACCOUNT_MODULE_VALUE = 13
} lx_account_kind;

typedef enum lx_account_open_authority {
    LX_ACCOUNT_OPEN_CREDIT = 1,
    LX_ACCOUNT_OPEN_GENESIS,
    LX_ACCOUNT_OPEN_GOVERNANCE
} lx_account_open_authority;

typedef struct lx_account_name {
    const uint8_t *bytes;
    size_t length;
    lx_account_kind kind;
} lx_account_name;

typedef struct lx_account {
    uint8_t id[LX_ACCOUNT_ID_BYTES];
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint16_t name_length;
    lx_account_kind kind;
    lxp_u128 balance;
    uint8_t asset_id[32];
    bool has_asset;
    uint64_t next_sequence;
    uint64_t created_at_sequence;
    bool frozen;
    bool has_open_reference;
    uint8_t authority_key[32];
    bool has_authority_key;
} lx_account;

typedef struct lx_account_registry lx_account_registry;

typedef struct lx_account_registration {
    lx_account account;
    size_t expected_count;
} lx_account_registration;

enum {
    LXP_SEND_MAX_CONDITIONS = 8,
    LXP_SEND_STORE_CAPACITY = 64,
    LXP_GRANT_STORE_CAPACITY = 64,
    LXP_REASON_PAYMENT = 1,
    LXP_REASON_DEPOSIT = 2,
    LXP_REASON_WITHDRAWAL = 3,
    LXP_REASON_ESCROW_LOCK = 4,
    LXP_REASON_ESCROW_CAPTURE = 5,
    LXP_REASON_ESCROW_RELEASE = 6,
    LXP_REASON_ESCROW_RESOLVE = 7,
    LXP_REASON_BUDGET_FUND = 8,
    LXP_REASON_BUDGET_SPEND = 9,
    LXP_REASON_BUDGET_DEFUND = 10,
    LXP_REASON_STREAM_FUND = 11,
    LXP_REASON_STREAM_DRAW = 12,
    LXP_REASON_STREAM_REFUND = 13,
    LXP_REASON_MARGIN_POST = 14,
    LXP_REASON_MARGIN_RELEASE = 15,
    LXP_REASON_TRADING_LOSS = 16,
    LXP_REASON_TRADING_PROFIT = 17,
    LXP_REASON_FUNDING = 18,
    LXP_REASON_LIQUIDATION_FEE = 19,
    LXP_REASON_INSURANCE = 20,
    LXP_REASON_ADL = 21,
    LXP_REASON_PROTOCOL_FEE = 22,
    LXP_REASON_STORAGE_OCCUPANCY = 23
};

typedef enum lxp_authorization_kind {
    LXP_AUTH_OWNER = 1,
    LXP_AUTH_SESSION_KEY,
    LXP_AUTH_DELEGATED_CAPABILITY,
    LXP_AUTH_BUDGET_ALLOWANCE,
    LXP_AUTH_ESCROW,
    LXP_AUTH_PROTOCOL_MODULE,
    LXP_AUTH_OCCUPANCY_RESPONSIBILITY,
    LXP_AUTH_PROGRAM_SPEND
} lxp_authorization_kind;

typedef enum lxp_condition_kind {
    LXP_CONDITION_NOT_BEFORE = 1,
    LXP_CONDITION_NOT_AFTER = 2
} lxp_condition_kind;

typedef struct lxp_send_condition {
    uint8_t kind;
    uint64_t timestamp;
} lxp_send_condition;

typedef struct lxp_send_authorization {
    uint8_t kind;
    uint8_t controller[32];
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t signed_context_hash[32];
    uint32_t network_id;
    uint16_t protocol_version;
} lxp_send_authorization;

typedef struct lxp_send {
    uint8_t from[32];
    uint8_t to[32];
    uint8_t asset[32];
    lxp_u128 amount;
    uint64_t sequence;
    uint8_t idempotency_key[32];
    uint64_t expires_at;
    uint8_t context_hash[32];
    lxp_send_condition conditions[LXP_SEND_MAX_CONDITIONS];
    size_t condition_count;
    lxp_send_authorization authorization;
} lxp_send;

struct lxp_transfer_asset_state;
struct lxp_transfer_leg;
typedef struct lxp_send_receipt_projection {
    lxp_u128 from_before;
    lxp_u128 from_after;
    lxp_u128 to_before;
    lxp_u128 to_after;
    uint8_t transfer_set_root[32];
    bool replayed;
} lxp_send_receipt_projection;

typedef struct lxp_send_store_record {
    uint8_t activity_hash[32];
    uint8_t idempotency_key[32];
    lxp_send_receipt_projection receipt;
} lxp_send_store_record;

typedef struct lxp_send_store {
    lxp_send_store_record records[LXP_SEND_STORE_CAPACITY];
    size_t count;
} lxp_send_store;

typedef struct lxp_send_environment {
    lx_account_registry *accounts;
    const struct lxp_transfer_asset_state *assets;
    size_t asset_count;
    lxp_send_store *store;
    uint64_t batch_timestamp;
    uint32_t network_id;
    uint16_t protocol_version;
} lxp_send_environment;

typedef struct lxp_payer_grant {
    uint8_t grant_id[32];
    uint8_t from[32];
    uint8_t recipient[32];
    uint8_t asset[32];
    lxp_u128 per_draw_maximum;
    lxp_u128 allowance;
    bool recurring;
    uint64_t window_length;
    uint64_t expiration;
    uint8_t purpose_hash[32];
    bool has_reference;
    uint8_t reference_hash[32];
    uint64_t revocation_sequence;
    uint8_t public_key[32];
    uint8_t signature[64];
} lxp_payer_grant;

typedef struct lxp_receive {
    uint8_t from[32];
    uint8_t to[32];
    uint8_t asset[32];
    lxp_u128 amount;
    uint8_t grant_id[32];
    uint64_t receiver_sequence;
    uint8_t idempotency_key[32];
    uint8_t context_hash[32];
    lxp_send_authorization receiver_authorization;
    lxp_payer_grant payer_grant;
} lxp_receive;

typedef struct lxp_grant_state {
    lxp_payer_grant grant;
    lxp_u128 drawn_total;
    lxp_u128 drawn_this_period;
    uint64_t window_start;
    uint64_t revoked_at_sequence;
    bool revoked;
    bool invoice_settled;
} lxp_grant_state;

typedef struct lxp_grant_store {
    lxp_grant_state grants[LXP_GRANT_STORE_CAPACITY];
    size_t count;
} lxp_grant_store;

typedef struct lxp_receive_environment {
    lx_account_registry *accounts;
    const struct lxp_transfer_asset_state *assets;
    size_t asset_count;
    lxp_grant_store *grants;
    lxp_send_store *idempotency;
    uint64_t batch_timestamp;
    uint64_t global_sequence;
    uint32_t network_id;
    uint16_t protocol_version;
} lxp_receive_environment;

struct lx_account_registry {
    lx_account accounts[LX_ACCOUNT_REGISTRY_CAPACITY];
    size_t count;
    _Atomic(struct lxp_gateway_invoice_registry *) gateway_owner;
};

enum { LXP_STATE_PROOF_MAX_DEPTH = 32 };
typedef struct lxp_state_proof {
    uint32_t leaf_index;
    uint32_t leaf_count;
    uint8_t depth;
    uint8_t siblings[LXP_STATE_PROOF_MAX_DEPTH][32];
} lxp_state_proof;

lxp_result lx_account_name_parse(const uint8_t *name, size_t name_length,
                                 lx_account_name *parsed);
lxp_result lx_account_kind_of(const uint8_t *name, size_t name_length,
                              lx_account_kind *kind);
lxp_result lx_account_id_from_string(const uint8_t *name, size_t name_length,
                                     uint8_t account_id[LX_ACCOUNT_ID_BYTES]);
lxp_result lx_account_registry_init(lx_account_registry *registry);
lxp_result lx_account_registry_root(const lx_account_registry *registry,
                                    uint8_t root[32]);
lxp_result lx_account_registry_proof(
    const lx_account_registry *registry,
    const uint8_t account_id[LX_ACCOUNT_ID_BYTES], uint8_t root[32],
    lxp_state_proof *proof);
lxp_result lx_account_lookup(lx_account_registry *registry,
                             const uint8_t *name, size_t name_length,
                             const uint8_t presented_id[LX_ACCOUNT_ID_BYTES],
                             lx_account **account);
lxp_result lx_account_open(lx_account_registry *registry,
                           const uint8_t *name, size_t name_length,
                           const uint8_t presented_id[LX_ACCOUNT_ID_BYTES],
                           uint64_t global_sequence,
                           lx_account_open_authority authority,
                           lxp_log *activity_log, lx_account **account);
lxp_result lx_account_module_value_prepare(
    lx_account_registry *registry, const uint8_t *module_name,
    size_t module_name_length, const uint8_t account_id[LX_ACCOUNT_ID_BYTES],
    const uint8_t asset_id[32], uint64_t global_sequence,
    lx_account_registration *registration, lx_account **account,
    bool *created);
lxp_result lx_account_registration_commit(
    lx_account_registry *registry, const lx_account_registration *registration,
    lx_account **account);
lxp_result lx_account_close(lx_account_registry *registry,
                            const uint8_t account_id[LX_ACCOUNT_ID_BYTES]);
lxp_result lxp_send_decode(const uint8_t *bytes, size_t length, lxp_send *send);
lxp_result lxp_send_encode(const lxp_send *send, uint8_t *bytes,
                           size_t capacity, size_t *length);
lxp_result lxp_send_authorization_message(const lxp_send *send, uint8_t *bytes,
                                          size_t capacity, size_t *length);
lxp_result lxp_send_validate(const lxp_send *send,
                             const lxp_send_environment *environment);
lxp_result lxp_send_build_transfer_set(const lxp_send *send,
                                       lx_account_registry *registry,
                                       struct lxp_transfer_leg *leg);
lxp_result lxp_send_execute(const lxp_send *send,
                            lxp_send_environment *environment,
                            lxp_send_receipt_projection *receipt);
lxp_result lxp_grant_authorization_message(const lxp_payer_grant *grant,
                                           uint8_t *bytes, size_t capacity,
                                           size_t *length);
lxp_result lxp_receive_authorization_message(const lxp_receive *receive,
                                             uint8_t *bytes, size_t capacity,
                                             size_t *length);
lxp_result lxp_receive_encode(const lxp_receive *receive, uint8_t *bytes,
                              size_t capacity, size_t *length);
lxp_result lxp_receive_decode(const uint8_t *bytes, size_t length,
                              lxp_receive *receive);
lxp_result lxp_verify_payer_grant(const lxp_payer_grant *grant,
                                  const lx_account *from);
lxp_result lxp_grant_store_put(lxp_grant_store *store,
                               const lxp_payer_grant *grant,
                               const lx_account *from);
lxp_result lxp_grant_draw_record(lxp_grant_state *state, lxp_u128 amount,
                                 uint64_t batch_timestamp);
lxp_result lxp_grant_revoke(lxp_grant_store *store, const uint8_t grant_id[32],
                            uint64_t global_sequence);
lxp_result lxp_receive_execute(const lxp_receive *receive,
                               lxp_receive_environment *environment,
                               lxp_send_receipt_projection *receipt);

#endif
