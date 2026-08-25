#ifndef LAYERX_LX_ASSET_H
#define LAYERX_LX_ASSET_H

#include "layerx/lxp_module.h"
#include "layerx/lxp_merkle.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LX_ASSET_REGISTRY_CAPACITY = 64,
    LX_ASSET_RESERVE_LINE_CAPACITY = 128,
    LX_ASSET_SYMBOL_MAX = 16,
    LX_ASSET_CUSTODY_REFERENCE_MAX = 128,
    LX_ASSET_REGISTER = 0x00010001,
    LX_ASSET_PAUSE = 0x00010002,
    LX_ASSET_UNPAUSE = 0x00010003,
    LX_ASSET_ACCOUNT_OPEN = 0x00010004,
    LX_ASSET_SEND = 0x00010005,
    LX_ASSET_RECEIVE = 0x00010006,
    LX_ASSET_GRANT_ISSUE = 0x00010007,
    LX_ASSET_GRANT_REVOKE = 0x00010008
};

typedef enum lx_asset_custody_kind {
    LX_ASSET_CUSTODY_PAXEER = 1
} lx_asset_custody_kind;

typedef struct lx_asset_record {
    uint8_t asset_id[32];
    char symbol[LX_ASSET_SYMBOL_MAX + 1U];
    uint8_t symbol_length;
    uint8_t decimals;
    lx_asset_custody_kind custody_kind;
    uint8_t custody_reference[LX_ASSET_CUSTODY_REFERENCE_MAX];
    uint16_t custody_reference_length;
    bool paused;
    lxp_u128 total_units;
} lx_asset_record;

typedef struct lx_asset_registry {
    lx_asset_record assets[LX_ASSET_REGISTRY_CAPACITY];
    size_t count;
    uint64_t next_sequence;
    lxp_u128 fees_charged;
} lx_asset_registry;

typedef struct lx_asset_transfer_request {
    lx_account *from;
    lx_account *to;
    const lx_asset_record *asset;
    lxp_u128 amount;
    lxp_transfer_context context;
    const lxp_payer_grant *payer_grant;
    bool direct_balance_write;
} lx_asset_transfer_request;

enum {
    LX_CHECKPOINT_CAPACITY = 64,
    LX_DEPOSIT_NULLIFIER_CAPACITY = 128
};

typedef struct lx_finalized_checkpoint {
    uint8_t checkpoint_id[32];
    uint8_t state_root[32];
    uint8_t deposit_root[32];
    uint8_t custody_reference[32];
    uint32_t network_id;
    uint16_t protocol_version;
    bool finalized;
} lx_finalized_checkpoint;

typedef struct lx_checkpoint_registry lx_checkpoint_registry;

typedef struct lx_paxeer_deposit_root_registration {
    uint8_t checkpoint_id[32];
    uint8_t checkpoint_state_root[32];
    uint8_t deposit_root[32];
    uint8_t custody_reference[32];
    uint32_t network_id;
    uint16_t protocol_version;
    uint8_t signature[64];
} lx_paxeer_deposit_root_registration;

typedef struct lx_deposit_proof {
    uint8_t deposit_id[32];
    uint8_t custody_reference[32];
    uint8_t asset_id[32];
    lxp_u128 amount;
    uint8_t checkpoint_id[32];
    lxp_merkle_proof inclusion_proof;
    uint32_t network_id;
    uint16_t protocol_version;
} lx_deposit_proof;

typedef struct lx_withdrawal_request {
    uint32_t network_id;
    uint8_t withdrawal_id[32];
    uint8_t account_id[32];
    uint8_t asset_id[32];
    lxp_u128 amount;
    uint8_t payout_recipient[32];
    uint8_t checkpoint_id[32];
} lx_withdrawal_request;

typedef struct lx_withdrawal_record {
    uint8_t nullifier[32];
    lx_withdrawal_request request;
    bool settled;
} lx_withdrawal_record;

typedef struct lx_withdrawal_store {
    lx_withdrawal_record records[LX_DEPOSIT_NULLIFIER_CAPACITY];
    size_t count;
} lx_withdrawal_store;

typedef struct lx_asset_custody_attestation {
    uint8_t asset_id[32];
    lxp_u128 custody_amount;
    lxp_u128 settled_out;
    uint8_t checkpoint_id[32];
    uint8_t state_root[32];
    bool finalized;
} lx_asset_custody_attestation;

typedef struct lx_asset_reserve_report_record {
    uint8_t asset_id[32];
    lxp_u128 agent_main;
    lxp_u128 escrow;
    lxp_u128 budget;
    lxp_u128 stream;
    lxp_u128 margin;
    lxp_u128 liquidity;
    lxp_u128 insurance;
    lxp_u128 fees;
    lxp_u128 withdrawals;
    lxp_u128 other_system;
    lxp_u128 reserve;
    lxp_u128 raw_total;
    lxp_u128 circulating;
    lxp_u128 effective_total;
    lxp_u128 expected_backing;
    struct {
        uint8_t account_id[32];
        lx_account_kind kind;
        lxp_u128 balance;
    } escrow_lines[LX_ASSET_RESERVE_LINE_CAPACITY];
    size_t escrow_line_count;
} lx_asset_reserve_report_record;

const lxp_module_iface *lx_asset_module_iface(void);
lxp_result lx_asset_registry_init(lx_asset_registry *registry,
                                  uint64_t next_sequence);
lxp_result lx_asset_register(lx_asset_registry *registry,
                             const lx_asset_record *record,
                             uint64_t sequence, lxp_u128 fee);
lxp_result lx_asset_lookup(lx_asset_registry *registry,
                           const uint8_t asset_id[32],
                           lx_asset_record **record);
lxp_result lx_asset_pause(lx_asset_registry *registry,
                          const uint8_t asset_id[32]);
lxp_result lx_asset_unpause(lx_asset_registry *registry,
                            const uint8_t asset_id[32]);
lxp_result lx_asset_amount_decode(const uint8_t *bytes, size_t length,
                                  lxp_u128 *amount);
lxp_result lx_asset_record_encode(const lx_asset_record *record,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length);
lxp_result lx_asset_record_decode(const uint8_t *bytes, size_t length,
                                  lx_asset_record *record);
lxp_result lx_asset_transfer_state(const lx_asset_record *record,
                                   lxp_transfer_asset_state *state);
lxp_result lx_asset_balance_get(const lx_account_registry *accounts,
                                const uint8_t account_id[32],
                                const uint8_t asset_id[32], lxp_u128 *balance);
lxp_result lx_asset_account_open(lx_asset_registry *assets,
                                 lx_account_registry *accounts,
                                 const uint8_t asset_id[32],
                                 const uint8_t *name, size_t name_length,
                                 uint64_t global_sequence,
                                 lx_account_open_authority authority,
                                 lxp_log *activity_log, lx_account **account);
lxp_result lx_asset_total_units(lx_asset_registry *assets,
                                const lx_account_registry *accounts,
                                const uint8_t asset_id[32], lxp_u128 *total);
lxp_result lx_asset_state_root(const lx_asset_registry *assets,
                               const lx_account_registry *accounts,
                               uint8_t root[32]);
lxp_result lx_asset_validate(const lx_asset_transfer_request *request);
lxp_result lx_asset_send_execute(lxp_module_ctx *ctx,
                                 const lx_asset_transfer_request *request,
                                 lxp_receipt *receipt);
lxp_result lx_asset_receive_execute(lxp_module_ctx *ctx,
                                    const lx_asset_transfer_request *request,
                                    lxp_receipt *receipt);
lxp_result lx_paxeer_deposit_leaf_hash(const lx_deposit_proof *proof,
                                       uint8_t leaf_hash[32]);
lxp_result lx_paxeer_deposit_root_message(
    const lx_paxeer_deposit_root_registration *registration,
    uint8_t *message, size_t capacity, size_t *message_length);
lxp_result lx_checkpoint_registry_create(
    const uint8_t paxeer_checkpoint_authority[32],
    uint32_t network_id, uint16_t protocol_version,
    lx_checkpoint_registry **registry);
lxp_result lx_checkpoint_registry_destroy(
    lx_checkpoint_registry **registry);
lxp_result lx_checkpoint_registry_register_deposit_root(
    lx_checkpoint_registry *registry,
    const lx_paxeer_deposit_root_registration *registration);
lxp_result lx_bridge_verify_deposit(const lx_deposit_proof *proof,
                                    const lx_checkpoint_registry *checkpoints,
                                    uint32_t network_id,
                                    uint16_t protocol_version);
lxp_result lx_asset_deposit_credit(lxp_module_ctx *ctx,
                                   const lx_asset_transfer_request *request,
                                   const lx_deposit_proof *proof,
                                   const lx_checkpoint_registry *checkpoints,
                                   uint32_t network_id,
                                   uint16_t protocol_version,
                                   lxp_receipt *receipt);
lxp_result lx_withdrawal_nullifier(const lx_withdrawal_request *request,
                                   uint8_t nullifier[32]);
bool lx_asset_nullifier_seen(const lx_withdrawal_store *store,
                             const uint8_t nullifier[32]);
lxp_result lx_asset_withdraw_request(lxp_module_ctx *ctx,
                                     const lx_asset_transfer_request *transfer,
                                     const lx_withdrawal_request *withdrawal,
                                     lx_withdrawal_store *store,
                                     lxp_receipt *receipt);
lxp_result lx_asset_withdraw_settle(lxp_module_ctx *ctx,
                                    lx_account *withdrawals,
                                    lx_account *reserve,
                                    const lx_asset_record *asset,
                                    const lx_finalized_checkpoint *checkpoint,
                                    const uint8_t nullifier[32],
                                    lx_withdrawal_store *store,
                                    lxp_transfer_context context,
                                    lxp_receipt *receipt);
lxp_result lx_asset_reserve_report(
    const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestation,
    lx_asset_reserve_report_record *report);
lxp_result lx_asset_reserve_reconcile(
    const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestation,
    lx_asset_reserve_report_record *report);
lxp_result lx_asset_reserve_report_encode(
    const lx_asset_reserve_report_record *report,
    uint8_t *bytes, size_t capacity, size_t *length);
lxp_result lx_asset_supply_check(
    const lx_asset_registry *assets, const lx_account_registry *accounts,
    const lx_asset_custody_attestation *attestations,
    size_t attestation_count);

#endif
