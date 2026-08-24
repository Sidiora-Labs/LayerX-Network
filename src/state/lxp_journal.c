#include "layerx/lxp_state.h"

#include <string.h>

static size_t find_cell(const lxp_state_store *store, const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->cells[i].key, key, 32U) == 0) return i;
    return store->count;
}

lxp_result lxp_state_store_init(lxp_state_store *store,
                                uint64_t first_sequence)
{
    if (store == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(store, 0, sizeof(*store));
    store->next_sequence = first_sequence;
    store->writer = pthread_self();
    return pthread_mutex_init(&store->lock, NULL) == 0 ? LXP_OK : LXP_ERR_IO;
}

lxp_result lxp_state_store_destroy(lxp_state_store *store)
{
    if (store == NULL) return LXP_ERR_NON_CANONICAL;
    return pthread_mutex_destroy(&store->lock) == 0 ? LXP_OK : LXP_ERR_IO;
}

lxp_result lxp_state_store_bind_accounts(
    lxp_state_store *store, struct lx_account_registry *accounts)
{
    lxp_result status;
    if (store == NULL || accounts == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(store);
    if (status != LXP_OK) return status;
    if (store->accounts != NULL && store->accounts != accounts)
        return LXP_ERR_CONTEXT_MISMATCH;
    store->accounts = accounts;
    return LXP_OK;
}

lxp_result lxp_state_store_require_account_root(lxp_state_store *store)
{
    lxp_result status;
    if (store == NULL || store->accounts == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(store);
    if (status != LXP_OK) return status;
    store->account_root_required = true;
    return LXP_OK;
}

lxp_result lxp_state_writer_assert_owner(const lxp_state_store *store)
{
    if (store == NULL) return LXP_ERR_NON_CANONICAL;
    return pthread_equal(store->writer, pthread_self()) != 0 ?
           LXP_OK : LXP_FATAL_INVARIANT;
}

lxp_result lxp_state_journal_open(lxp_state_store *store,
                                  uint64_t global_sequence,
                                  lxp_state_journal *journal)
{
    lxp_result status;
    if (store == NULL || journal == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(store);
    if (status != LXP_OK) return status;
    if (global_sequence != store->next_sequence) return LXP_ERR_SEQUENCE_GAP;
    journal->store = store;
    journal->count = 0U;
    journal->global_sequence = global_sequence;
    journal->open = true;
    journal->account_root_required_before = store->account_root_required;
    journal->has_idempotency = false;
    return LXP_OK;
}

lxp_result lxp_state_journal_require_account_root(
    lxp_state_journal *journal)
{
    lxp_result status;
    if (journal == NULL || !journal->open || journal->store == NULL ||
        journal->store->accounts == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(journal->store);
    if (status != LXP_OK) return status;
    journal->store->account_root_required = true;
    return LXP_OK;
}

lxp_result lxp_state_journal_set(lxp_state_journal *journal,
                                 const uint8_t key[32], lxp_u128 value)
{
    size_t i;
    lxp_result status;
    if (journal == NULL || key == NULL || !journal->open)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(journal->store);
    if (status != LXP_OK) return status;
    for (i = 0U; i < journal->count; ++i) {
        if (memcmp(journal->staged[i].key, key, 32U) == 0) {
            journal->staged[i].value = value;
            return LXP_OK;
        }
    }
    if (journal->count == LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_TOO_MANY_LEGS;
    (void)memcpy(journal->staged[journal->count].key, key, 32U);
    journal->staged[journal->count].value = value;
    ++journal->count;
    return LXP_OK;
}

lxp_result lxp_state_journal_commit(lxp_state_journal *journal)
{
    size_t new_cells = 0U;
    size_t i;
    lxp_result status;
    if (journal == NULL || !journal->open) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(journal->store);
    if (status != LXP_OK) return status;
    if (journal->global_sequence != journal->store->next_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    for (i = 0U; i < journal->count; ++i) {
        size_t location = find_cell(journal->store, journal->staged[i].key);
        if (location == journal->store->count) ++new_cells;
    }
    if (new_cells > LXP_STATE_MAX_CELLS - journal->store->count)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = lxp_idempotency_can_commit(journal);
    if (status != LXP_OK) return status;
    if (pthread_mutex_lock(&journal->store->lock) != 0) return LXP_ERR_IO;
    for (i = 0U; i < journal->count; ++i) {
        size_t location = find_cell(journal->store, journal->staged[i].key);
        if (location == journal->store->count) {
            ++journal->store->count;
            (void)memcpy(journal->store->cells[location].key,
                         journal->staged[i].key, 32U);
        }
        journal->store->cells[location].value = journal->staged[i].value;
    }
    lxp_idempotency_commit_staged(journal);
    ++journal->store->next_sequence;
    journal->open = false;
    if (pthread_mutex_unlock(&journal->store->lock) != 0)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

lxp_result lxp_state_journal_rollback(lxp_state_journal *journal)
{
    lxp_result status;
    if (journal == NULL || !journal->open) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(journal->store);
    if (status != LXP_OK) return status;
    journal->count = 0U;
    journal->has_idempotency = false;
    journal->store->account_root_required =
        journal->account_root_required_before;
    journal->open = false;
    return LXP_OK;
}

lxp_result lxp_state_store_get(lxp_state_store *store, const uint8_t key[32],
                               lxp_u128 *value, bool *found)
{
    size_t location;
    if (store == NULL || key == NULL || value == NULL || found == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&store->lock) != 0) return LXP_ERR_IO;
    location = find_cell(store, key);
    *found = location != store->count;
    if (*found) *value = store->cells[location].value;
    if (pthread_mutex_unlock(&store->lock) != 0) return LXP_FATAL_INVARIANT;
    return LXP_OK;
}
