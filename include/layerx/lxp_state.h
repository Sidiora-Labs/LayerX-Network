#ifndef LAYERX_LXP_STATE_H
#define LAYERX_LXP_STATE_H

#include "layerx/lxp_protocol.h"
#include "layerx/lxp_result.h"
#include "layerx/lxp_u128.h"

#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_STATE_MAX_CELLS = 512,
    LXP_STATE_MAX_IDEMPOTENCY = 512,
    LXP_STATE_MAX_RECEIPT_BYTES = 4096
};

typedef struct lxp_state_cell {
    uint8_t key[32];
    lxp_u128 value;
} lxp_state_cell;

typedef struct lxp_idempotency_key_state {
    uint8_t key_hash[32];
    uint32_t receipt_length;
    uint8_t receipt[LXP_STATE_MAX_RECEIPT_BYTES];
} lxp_idempotency_key_state;
#define lxp_idempotency_key_state lxp_idempotency_key_state

struct lx_account_registry;

typedef struct lxp_state_store {
    lxp_state_cell cells[LXP_STATE_MAX_CELLS];
    size_t count;
    lxp_idempotency_key_state idempotency[LXP_STATE_MAX_IDEMPOTENCY];
    size_t idempotency_count;
    uint64_t next_sequence;
    struct lx_account_registry *accounts;
    bool account_root_required;
    pthread_t writer;
    pthread_mutex_t lock;
} lxp_state_store;

typedef struct lxp_state_journal {
    lxp_state_store *store;
    lxp_state_cell staged[LXP_MAX_TRANSFER_SET_LEGS];
    size_t count;
    uint64_t global_sequence;
    bool open;
    bool account_root_required_before;
    bool has_idempotency;
    lxp_idempotency_key_state staged_idempotency;
} lxp_state_journal;
#define lxp_state_journal lxp_state_journal

lxp_result lxp_state_store_init(lxp_state_store *store,
                                uint64_t first_sequence);
lxp_result lxp_state_store_destroy(lxp_state_store *store);
lxp_result lxp_state_store_bind_accounts(
    lxp_state_store *store, struct lx_account_registry *accounts);
lxp_result lxp_state_store_require_account_root(lxp_state_store *store);
lxp_result lxp_state_writer_assert_owner(const lxp_state_store *store);
lxp_result lxp_state_journal_open(lxp_state_store *store,
                                  uint64_t global_sequence,
                                  lxp_state_journal *journal);
lxp_result lxp_state_journal_set(lxp_state_journal *journal,
                                 const uint8_t key[32], lxp_u128 value);
lxp_result lxp_state_journal_require_account_root(
    lxp_state_journal *journal);
lxp_result lxp_state_journal_commit(lxp_state_journal *journal);
lxp_result lxp_state_journal_rollback(lxp_state_journal *journal);
lxp_result lxp_state_store_get(lxp_state_store *store, const uint8_t key[32],
                               lxp_u128 *value, bool *found);
lxp_result lxp_idempotency_lookup(lxp_state_store *store,
                                  const uint8_t *actor_did,
                                  size_t actor_did_length,
                                  const uint8_t idempotency_key[32],
                                  const uint8_t **receipt,
                                  size_t *receipt_length);
lxp_result lxp_idempotency_record(lxp_state_journal *journal,
                                  const uint8_t *actor_did,
                                  size_t actor_did_length,
                                  const uint8_t idempotency_key[32],
                                  const uint8_t *receipt,
                                  size_t receipt_length);
lxp_result lxp_idempotency_can_commit(const lxp_state_journal *journal);
void lxp_idempotency_commit_staged(lxp_state_journal *journal);

struct lxp_kernel;
lxp_result lxp_state_subtree_root(const struct lxp_kernel *kernel,
                                  uint16_t module_id, uint8_t root[32]);
lxp_result lxp_state_root(const struct lxp_kernel *kernel, uint8_t root[32]);
lxp_result lxp_state_root_chain(const uint8_t previous_root[32],
                                const uint8_t state_root[32],
                                uint64_t global_sequence, uint8_t root[32]);
lxp_result lxp_state_supply_check(const struct lxp_kernel *kernel);

#endif
