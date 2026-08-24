#ifndef LAYERX_LXP_TRANSFER_H
#define LAYERX_LXP_TRANSFER_H

#include "layerx/lxp_ledger.h"
#include "layerx/lxp_protocol.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

typedef struct lxp_transfer_asset_state {
    uint8_t asset_id[32];
    bool registered;
    bool paused;
} lxp_transfer_asset_state;

typedef struct lxp_transfer_leg {
    lx_account *from;
    lx_account *to;
    uint8_t asset_id[32];
    lxp_u128 amount;
    uint16_t reason;
    uint8_t supply_mode;
} lxp_transfer_leg;

enum {
    LXP_TRANSFER_CONSERVED = 0,
    LXP_TRANSFER_CREDIT_ONLY = 1,
    LXP_TRANSFER_DEBIT_ONLY = 2
};

/* Generic per-source authorization resolved before the balance primitive is
 * entered. The kernel sees only an ordinary source account and an existing
 * authorization kind; program-specific derivation evidence is verified by the
 * owning module before it constructs this table. */
typedef struct lxp_transfer_source_authority {
    uint8_t authorized_from[32];
    lxp_authorization_kind debit_authority_kind;
    bool protocol_system_capability;
} lxp_transfer_source_authority;

typedef struct lxp_transfer_context {
    const lxp_transfer_asset_state *assets;
    size_t asset_count;
    uint8_t authorized_from[32];
    uint64_t actor_sequence;
    bool idempotency_seen;
    uint64_t batch_timestamp;
    uint64_t expires_at;
    bool has_client_balance;
    bool protocol_system_capability;
    lx_account *sequence_account;
    bool inject_failure;
    size_t failure_after_leg;
    uint16_t origin_module_id;
    lxp_authorization_kind debit_authority_kind;
    const lxp_transfer_source_authority *source_authorities;
    size_t source_authority_count;
} lxp_transfer_context;

typedef struct lxp_transfer_result {
    lxp_u128 from_balance_before;
    lxp_u128 from_balance_after;
    lxp_u128 to_balance_before;
    lxp_u128 to_balance_after;
} lxp_transfer_result;

typedef struct lxp_ledger_journal_entry {
    lx_account *account;
    lxp_u128 balance_before;
    uint8_t asset_id[32];
    bool has_asset;
    uint64_t next_sequence;
} lxp_ledger_journal_entry;

typedef struct lxp_ledger_journal {
    lxp_ledger_journal_entry entries[LXP_MAX_TRANSFER_SET_LEGS * 2U];
    size_t count;
    bool open;
} lxp_ledger_journal;

typedef struct lxp_transfer_set_result {
    lxp_transfer_result legs[LXP_MAX_TRANSFER_SET_LEGS];
    size_t leg_count;
    size_t failed_leg;
    lxp_result failure;
    uint8_t transfer_set_root[32];
    bool receipt_emitted;
} lxp_transfer_set_result;

typedef struct lxp_transfer_set {
    lxp_transfer_leg legs[LXP_MAX_TRANSFER_SET_LEGS];
    size_t leg_count;
    lxp_transfer_context context;
} lxp_transfer_set;

lxp_result lxp_ledger_bootstrap_balance(lx_account *account,
                                        const uint8_t asset_id[32],
                                        lxp_u128 balance,
                                        uint64_t next_sequence);
lxp_result lxp_ledger_restore_account_snapshot(lx_account *account,
                                               lxp_u128 balance,
                                               const uint8_t asset_id[32],
                                               bool has_asset,
                                               uint64_t next_sequence);
lxp_result lxp_state_balance_get(const lx_account *account,
                                 const uint8_t asset_id[32],
                                 lxp_u128 *balance);
lxp_result lxp_precondition_check(const lxp_transfer_leg *legs,
                                  size_t leg_count,
                                  const lxp_transfer_context *context);
lxp_result lxp_balance_apply_leg(lxp_transfer_leg *leg,
                                 lxp_transfer_result *result);
lxp_result lxp_apply_transfer(lxp_transfer_leg *leg,
                              lxp_transfer_context *context,
                              lxp_transfer_result *result);
lxp_result lxp_journal_open(lxp_transfer_leg *legs, size_t leg_count,
                            lxp_ledger_journal *journal);
lxp_result lxp_journal_commit(lxp_ledger_journal *journal);
lxp_result lxp_journal_rollback(lxp_ledger_journal *journal);
lxp_result lxp_conservation_check(const lxp_transfer_leg *legs,
                                  size_t leg_count);
lxp_result lxp_transfer_set_root(const lxp_transfer_leg *legs,
                                 size_t leg_count, uint8_t root[32]);
lxp_result lxp_apply_transfer_set(lxp_transfer_leg *legs, size_t leg_count,
                                  lxp_transfer_context *context,
                                  lxp_transfer_set_result *result);

#endif
