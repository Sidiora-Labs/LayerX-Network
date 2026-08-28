#include "layerx/lxp_state.h"
#include "layerx/lxp_ledger.h"

#include <stdlib.h>
#include <string.h>

struct lxp_state_snapshot {
    lxp_state_store store;
    lx_account_registry accounts;
    uint64_t base_sequence;
    uint64_t lineage;
    uint64_t generation;
    bool store_initialized;
};

typedef struct lxp_state_cell_delta {
    lxp_state_cell before;
    lxp_state_cell after;
    bool before_present;
    bool after_present;
} lxp_state_cell_delta;

typedef struct lxp_state_idempotency_delta {
    lxp_idempotency_key_state before;
    lxp_idempotency_key_state after;
    bool before_present;
    bool after_present;
} lxp_state_idempotency_delta;

typedef struct lxp_state_account_delta {
    lx_account before;
    lx_account after;
    bool before_present;
    bool after_present;
} lxp_state_account_delta;

struct lxp_state_transition {
    lxp_state_cell_delta cells[LXP_STATE_MAX_CELLS * 2U];
    size_t cell_count;
    lxp_state_idempotency_delta idempotency[
        LXP_STATE_MAX_IDEMPOTENCY * 2U];
    size_t idempotency_count;
    lxp_state_account_delta accounts[LX_ACCOUNT_REGISTRY_CAPACITY * 2U];
    size_t account_count;
    bool before_account_root_required;
    bool after_account_root_required;
    bool has_accounts;
    uint64_t lineage;
    uint64_t before_generation;
};

struct lxp_state_publication_guard {
    lxp_state_store *live;
    const lxp_state_snapshot *settled;
    bool gateway_excluded;
    bool state_locked;
    bool published;
};

static atomic_uint_fast64_t snapshot_lineage = ATOMIC_VAR_INIT(UINT64_C(1));

static uint64_t next_snapshot_lineage(void)
{
    uint_fast64_t current = atomic_load_explicit(&snapshot_lineage,
                                                 memory_order_relaxed);
    while (current != 0U && current != UINT64_MAX) {
        if (atomic_compare_exchange_weak_explicit(
                &snapshot_lineage, &current, current + 1U,
                memory_order_relaxed, memory_order_relaxed))
            return (uint64_t)current;
    }
    return 0U;
}

static size_t find_cell(const lxp_state_store *store, const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->cells[i].key, key, 32U) == 0) return i;
    return store->count;
}

static bool account_record_canonical(const lx_account *account)
{
    return lx_account_validate_canonical(account) == LXP_OK;
}

static bool store_canonical(const lxp_state_store *store)
{
    size_t i;
    size_t j;
    if (store == NULL || store->count > LXP_STATE_MAX_CELLS ||
        store->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        (store->account_root_required && store->accounts == NULL) ||
        (store->accounts != NULL &&
         store->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY))
        return false;
    for (i = 0U; i < store->count; ++i)
        for (j = 0U; j < i; ++j)
            if (memcmp(store->cells[i].key, store->cells[j].key, 32U) == 0)
                return false;
    for (i = 0U; i < store->idempotency_count; ++i) {
        if (store->idempotency[i].receipt_length >
            LXP_STATE_MAX_RECEIPT_BYTES)
            return false;
        for (j = 0U; j < i; ++j)
            if (memcmp(store->idempotency[i].key_hash,
                       store->idempotency[j].key_hash, 32U) == 0)
                return false;
    }
    if (store->accounts == NULL) return true;
    for (i = 0U; i < store->accounts->count; ++i) {
        if (!account_record_canonical(&store->accounts->accounts[i]))
            return false;
        for (j = 0U; j < i; ++j)
            if (memcmp(store->accounts->accounts[i].id,
                       store->accounts->accounts[j].id, 32U) == 0)
                return false;
    }
    return true;
}

static bool snapshot_canonical(const lxp_state_snapshot *snapshot)
{
    if (snapshot == NULL || !snapshot->store_initialized ||
        snapshot->lineage == 0U ||
        (snapshot->store.accounts != NULL &&
         snapshot->store.accounts != &snapshot->accounts))
        return false;
    return store_canonical(&snapshot->store);
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

static lxp_result snapshot_copy_store(const lxp_state_store *source,
                                      lxp_state_snapshot *snapshot)
{
    lxp_result status;
    if (source->count > LXP_STATE_MAX_CELLS ||
        source->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY)
        return LXP_FATAL_INVARIANT;
    status = lxp_state_store_init(&snapshot->store, source->next_sequence);
    if (status != LXP_OK) return status;
    snapshot->store_initialized = true;
    snapshot->store.count = source->count;
    snapshot->store.idempotency_count = source->idempotency_count;
    snapshot->store.account_root_required = source->account_root_required;
    if (source->count != 0U)
        (void)memcpy(snapshot->store.cells, source->cells,
                     source->count * sizeof(source->cells[0]));
    if (source->idempotency_count != 0U)
        (void)memcpy(snapshot->store.idempotency, source->idempotency,
                     source->idempotency_count *
                         sizeof(source->idempotency[0]));
    if (source->accounts != NULL) {
        status = lx_account_registry_snapshot(source->accounts,
                                              &snapshot->accounts);
        if (status != LXP_OK) return status;
        snapshot->store.accounts = &snapshot->accounts;
    }
    snapshot->base_sequence = source->next_sequence;
    return LXP_OK;
}

lxp_result lxp_state_snapshot_create(lxp_state_store *source,
                                     lxp_state_snapshot **snapshot)
{
    lxp_state_snapshot *created;
    uint64_t lineage;
    lxp_result status;
    if (source == NULL || snapshot == NULL) return LXP_ERR_NON_CANONICAL;
    *snapshot = NULL;
    status = lxp_state_writer_assert_owner(source);
    if (status != LXP_OK) return status;
    created = calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_IO;
    if (pthread_mutex_lock(&source->lock) != 0) {
        free(created);
        return LXP_ERR_IO;
    }
    status = store_canonical(source) ? snapshot_copy_store(source, created) :
                                       LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_unlock(&source->lock) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK) {
        lxp_state_snapshot_destroy(created);
        return status;
    }
    lineage = next_snapshot_lineage();
    if (lineage == 0U) {
        lxp_state_snapshot_destroy(created);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    created->lineage = lineage;
    *snapshot = created;
    return LXP_OK;
}

lxp_result lxp_state_snapshot_clone(lxp_state_snapshot *source,
                                    lxp_state_snapshot **snapshot)
{
    lxp_state_snapshot *created;
    lxp_result status;
    if (!snapshot_canonical(source) || snapshot == NULL)
        return LXP_ERR_NON_CANONICAL;
    *snapshot = NULL;
    created = calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_IO;
    if (pthread_mutex_lock(&source->store.lock) != 0) {
        free(created);
        return LXP_ERR_IO;
    }
    status = snapshot_canonical(source) ?
                 snapshot_copy_store(&source->store, created) :
                 LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_unlock(&source->store.lock) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK) {
        lxp_state_snapshot_destroy(created);
        return status;
    }
    created->lineage = source->lineage;
    created->generation = source->generation;
    *snapshot = created;
    return LXP_OK;
}

void lxp_state_snapshot_destroy(lxp_state_snapshot *snapshot)
{
    if (snapshot == NULL) return;
    if (snapshot->store_initialized)
        (void)lxp_state_store_destroy(&snapshot->store);
    (void)memset(snapshot, 0, sizeof(*snapshot));
    free(snapshot);
}

lxp_state_store *lxp_state_snapshot_store_for_prepare(
    lxp_state_snapshot *snapshot)
{
    return snapshot != NULL && snapshot->store_initialized ?
               &snapshot->store : NULL;
}

const lxp_state_store *lxp_state_snapshot_store(
    const lxp_state_snapshot *snapshot)
{
    return snapshot != NULL && snapshot->store_initialized ?
               &snapshot->store : NULL;
}

lx_account_registry *lxp_state_snapshot_accounts_for_prepare(
    lxp_state_snapshot *snapshot)
{
    return snapshot != NULL && snapshot->store_initialized &&
                   snapshot->store.accounts != NULL ?
               &snapshot->accounts : NULL;
}

const lx_account_registry *lxp_state_snapshot_accounts(
    const lxp_state_snapshot *snapshot)
{
    return snapshot != NULL && snapshot->store_initialized &&
                   snapshot->store.accounts != NULL ?
               &snapshot->accounts : NULL;
}

uint64_t lxp_state_snapshot_base_sequence(
    const lxp_state_snapshot *snapshot)
{
    return snapshot != NULL && snapshot->store_initialized ?
               snapshot->base_sequence : UINT64_MAX;
}

lxp_result lxp_state_snapshot_seal_level(lxp_state_snapshot *snapshot)
{
    if (snapshot == NULL || !snapshot->store_initialized)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&snapshot->store.lock) != 0) return LXP_ERR_IO;
    if (!snapshot_canonical(snapshot) || snapshot->generation == UINT64_MAX) {
        (void)pthread_mutex_unlock(&snapshot->store.lock);
        return LXP_ERR_NON_CANONICAL;
    }
    ++snapshot->generation;
    if (pthread_mutex_unlock(&snapshot->store.lock) != 0)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static size_t find_idempotency(const lxp_state_store *store,
                               const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < store->idempotency_count; ++i)
        if (memcmp(store->idempotency[i].key_hash, key, 32U) == 0) return i;
    return store->idempotency_count;
}

static size_t find_account(const lx_account_registry *accounts,
                           const uint8_t id[32])
{
    size_t i;
    for (i = 0U; i < accounts->count; ++i)
        if (memcmp(accounts->accounts[i].id, id, 32U) == 0) return i;
    return accounts->count;
}

static bool cell_equal(const lxp_state_cell *left,
                       const lxp_state_cell *right)
{
    return memcmp(left->key, right->key, 32U) == 0 &&
           left->value.hi == right->value.hi &&
           left->value.lo == right->value.lo;
}

static bool idempotency_equal(const lxp_idempotency_key_state *left,
                              const lxp_idempotency_key_state *right)
{
    return left->receipt_length <= LXP_STATE_MAX_RECEIPT_BYTES &&
           right->receipt_length <= LXP_STATE_MAX_RECEIPT_BYTES &&
           memcmp(left->key_hash, right->key_hash, 32U) == 0 &&
           left->receipt_length == right->receipt_length &&
           memcmp(left->receipt, right->receipt, left->receipt_length) == 0;
}

static bool account_equal(const lx_account *left, const lx_account *right)
{
    if (left->name_length > LX_ACCOUNT_NAME_MAX ||
        right->name_length > LX_ACCOUNT_NAME_MAX)
        return false;
    return memcmp(left->id, right->id, sizeof(left->id)) == 0 &&
           left->name_length == right->name_length &&
           memcmp(left->name, right->name, left->name_length) == 0 &&
           left->kind == right->kind &&
           left->balance.hi == right->balance.hi &&
           left->balance.lo == right->balance.lo &&
           memcmp(left->asset_id, right->asset_id,
                  sizeof(left->asset_id)) == 0 &&
           left->has_asset == right->has_asset &&
           left->next_sequence == right->next_sequence &&
           left->created_at_sequence == right->created_at_sequence &&
           left->frozen == right->frozen &&
           left->has_open_reference == right->has_open_reference &&
           memcmp(left->authority_key, right->authority_key,
                  sizeof(left->authority_key)) == 0 &&
           left->has_authority_key == right->has_authority_key;
}

lxp_result lxp_state_transition_create(
    const lxp_state_snapshot *base, const lxp_state_snapshot *prepared,
    lxp_state_transition **transition)
{
    lxp_state_transition *created;
    const lxp_state_store *before;
    const lxp_state_store *after;
    if (!snapshot_canonical(base) || !snapshot_canonical(prepared) ||
        transition == NULL || base->lineage != prepared->lineage)
        return LXP_ERR_NON_CANONICAL;
    before = &base->store;
    after = &prepared->store;
    if ((before->accounts == NULL) != (after->accounts == NULL) ||
        before->count > LXP_STATE_MAX_CELLS ||
        after->count > LXP_STATE_MAX_CELLS ||
        before->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        after->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        (before->accounts != NULL &&
         (before->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
          after->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)))
        return LXP_FATAL_INVARIANT;
    *transition = NULL;
    created = calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_IO;
    if (after->next_sequence != before->next_sequence) {
        free(created);
        return LXP_ERR_CONTEXT_MISMATCH;
    }
    created->lineage = base->lineage;
    created->before_generation = base->generation;
    created->before_account_root_required = before->account_root_required;
    created->after_account_root_required = after->account_root_required;
    if (created->before_account_root_required &&
        !created->after_account_root_required) {
        free(created);
        return LXP_ERR_CONTEXT_MISMATCH;
    }
    created->has_accounts = before->accounts != NULL;
    {
        size_t i;
        for (i = 0U; i < before->count; ++i) {
            size_t location = find_cell(after, before->cells[i].key);
            if (location != after->count &&
                cell_equal(&before->cells[i], &after->cells[location]))
                continue;
            created->cells[created->cell_count].before = before->cells[i];
            created->cells[created->cell_count].before_present = true;
            if (location != after->count) {
                created->cells[created->cell_count].after =
                    after->cells[location];
                created->cells[created->cell_count].after_present = true;
            }
            ++created->cell_count;
        }
        for (i = 0U; i < after->count; ++i) {
            if (find_cell(before, after->cells[i].key) != before->count)
                continue;
            created->cells[created->cell_count].after = after->cells[i];
            created->cells[created->cell_count].after_present = true;
            ++created->cell_count;
        }
        for (i = 0U; i < before->idempotency_count; ++i) {
            size_t location = find_idempotency(
                after, before->idempotency[i].key_hash);
            if (location != after->idempotency_count && idempotency_equal(
                    &before->idempotency[i], &after->idempotency[location]))
                continue;
            created->idempotency[created->idempotency_count].before =
                before->idempotency[i];
            created->idempotency[created->idempotency_count].before_present =
                true;
            if (location != after->idempotency_count) {
                created->idempotency[created->idempotency_count].after =
                    after->idempotency[location];
                created->idempotency[created->idempotency_count].after_present =
                    true;
            }
            ++created->idempotency_count;
        }
        for (i = 0U; i < after->idempotency_count; ++i) {
            if (find_idempotency(before, after->idempotency[i].key_hash) !=
                before->idempotency_count)
                continue;
            created->idempotency[created->idempotency_count].after =
                after->idempotency[i];
            created->idempotency[created->idempotency_count].after_present =
                true;
            ++created->idempotency_count;
        }
    }
    if (before->accounts != NULL) {
        size_t i;
        for (i = 0U; i < before->accounts->count; ++i) {
            size_t location = find_account(after->accounts,
                                            before->accounts->accounts[i].id);
            if (location != after->accounts->count && account_equal(
                    &before->accounts->accounts[i],
                    &after->accounts->accounts[location]))
                continue;
            created->accounts[created->account_count].before =
                before->accounts->accounts[i];
            created->accounts[created->account_count].before_present = true;
            if (location != after->accounts->count) {
                created->accounts[created->account_count].after =
                    after->accounts->accounts[location];
                created->accounts[created->account_count].after_present = true;
            }
            ++created->account_count;
        }
        for (i = 0U; i < after->accounts->count; ++i) {
            if (find_account(before->accounts, after->accounts->accounts[i].id)
                != before->accounts->count)
                continue;
            created->accounts[created->account_count].after =
                after->accounts->accounts[i];
            created->accounts[created->account_count].after_present = true;
            ++created->account_count;
        }
    }
    *transition = created;
    return LXP_OK;
}

void lxp_state_transition_destroy(lxp_state_transition *transition)
{
    if (transition == NULL) return;
    (void)memset(transition, 0, sizeof(*transition));
    free(transition);
}

lxp_result lxp_state_transition_apply_snapshot(
    const lxp_state_transition *transition, lxp_state_snapshot *snapshot)
{
    lxp_state_store *store;
    if (transition == NULL || snapshot == NULL ||
        !snapshot->store_initialized ||
        transition->lineage != snapshot->lineage ||
        transition->before_generation != snapshot->generation ||
        snapshot->generation == UINT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    store = &snapshot->store;
    if (pthread_mutex_lock(&store->lock) != 0) return LXP_ERR_IO;
    {
        size_t i;
        size_t cell_additions = 0U;
        size_t cell_deletions = 0U;
        size_t idempotency_additions = 0U;
        size_t idempotency_deletions = 0U;
        size_t account_additions = 0U;
        size_t account_deletions = 0U;
        if (!snapshot_canonical(snapshot) ||
            (store->accounts != NULL) != transition->has_accounts ||
            (transition->before_account_root_required &&
             !store->account_root_required)) {
            (void)pthread_mutex_unlock(&store->lock);
            return LXP_ERR_CONTEXT_MISMATCH;
        }
        for (i = 0U; i < transition->cell_count; ++i) {
            const lxp_state_cell_delta *delta = &transition->cells[i];
            const uint8_t *key = delta->before_present ? delta->before.key :
                                                       delta->after.key;
            size_t location = find_cell(store, key);
            if ((location != store->count) != delta->before_present ||
                (delta->before_present &&
                 !cell_equal(&store->cells[location], &delta->before))) {
                (void)pthread_mutex_unlock(&store->lock);
                return LXP_ERR_CONTEXT_MISMATCH;
            }
            if (!delta->before_present && delta->after_present)
                ++cell_additions;
            if (delta->before_present && !delta->after_present)
                ++cell_deletions;
        }
        for (i = 0U; i < transition->idempotency_count; ++i) {
            const lxp_state_idempotency_delta *delta =
                &transition->idempotency[i];
            const uint8_t *key = delta->before_present ?
                                     delta->before.key_hash :
                                     delta->after.key_hash;
            size_t location = find_idempotency(store, key);
            if ((location != store->idempotency_count) !=
                    delta->before_present ||
                (delta->before_present && !idempotency_equal(
                    &store->idempotency[location], &delta->before))) {
                (void)pthread_mutex_unlock(&store->lock);
                return LXP_ERR_CONTEXT_MISMATCH;
            }
            if (!delta->before_present && delta->after_present)
                ++idempotency_additions;
            if (delta->before_present && !delta->after_present)
                ++idempotency_deletions;
        }
        for (i = 0U; i < transition->account_count; ++i) {
            const lxp_state_account_delta *delta = &transition->accounts[i];
            const uint8_t *id = delta->before_present ? delta->before.id :
                                                       delta->after.id;
            size_t location = find_account(store->accounts, id);
            if ((location != store->accounts->count) != delta->before_present ||
                (delta->before_present && !account_equal(
                    &store->accounts->accounts[location], &delta->before))) {
                (void)pthread_mutex_unlock(&store->lock);
                return LXP_ERR_CONTEXT_MISMATCH;
            }
            if (!delta->before_present && delta->after_present)
                ++account_additions;
            if (delta->before_present && !delta->after_present)
                ++account_deletions;
        }
        if (cell_additions >
                LXP_STATE_MAX_CELLS - store->count + cell_deletions ||
            idempotency_additions >
                LXP_STATE_MAX_IDEMPOTENCY - store->idempotency_count +
                    idempotency_deletions ||
            account_additions >
                LX_ACCOUNT_REGISTRY_CAPACITY -
                    (store->accounts != NULL ? store->accounts->count : 0U) +
                    account_deletions) {
            (void)pthread_mutex_unlock(&store->lock);
            return LXP_ERR_ARENA_EXHAUSTED;
        }
        for (i = 0U; i < transition->cell_count; ++i) {
            const lxp_state_cell_delta *delta = &transition->cells[i];
            const uint8_t *key = delta->before_present ? delta->before.key :
                                                       delta->after.key;
            size_t location = find_cell(store, key);
            if (!delta->after_present) {
                if (location + 1U < store->count)
                    (void)memmove(&store->cells[location],
                                  &store->cells[location + 1U],
                                  (store->count - location - 1U) *
                                      sizeof(store->cells[0]));
                --store->count;
            } else {
                if (location == store->count) ++store->count;
                store->cells[location] = delta->after;
            }
        }
        for (i = 0U; i < transition->idempotency_count; ++i) {
            const lxp_state_idempotency_delta *delta =
                &transition->idempotency[i];
            const uint8_t *key = delta->before_present ?
                                     delta->before.key_hash :
                                     delta->after.key_hash;
            size_t location = find_idempotency(store, key);
            if (!delta->after_present) {
                if (location + 1U < store->idempotency_count)
                    (void)memmove(&store->idempotency[location],
                                  &store->idempotency[location + 1U],
                                  (store->idempotency_count - location - 1U) *
                                      sizeof(store->idempotency[0]));
                --store->idempotency_count;
            } else {
                if (location == store->idempotency_count)
                    ++store->idempotency_count;
                store->idempotency[location] = delta->after;
            }
        }
        for (i = 0U; i < transition->account_count; ++i) {
            const lxp_state_account_delta *delta = &transition->accounts[i];
            const uint8_t *id = delta->before_present ? delta->before.id :
                                                       delta->after.id;
            size_t location = find_account(store->accounts, id);
            if (!delta->after_present) {
                if (location + 1U < store->accounts->count)
                    (void)memmove(&store->accounts->accounts[location],
                                  &store->accounts->accounts[location + 1U],
                                  (store->accounts->count - location - 1U) *
                                      sizeof(store->accounts->accounts[0]));
                --store->accounts->count;
            } else {
                if (location == store->accounts->count)
                    ++store->accounts->count;
                store->accounts->accounts[location] = delta->after;
            }
        }
        store->account_root_required = store->account_root_required ||
                                       transition->after_account_root_required;
        snapshot->base_sequence = store->next_sequence;
    }
    if (pthread_mutex_unlock(&store->lock) != 0)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static bool snapshot_matches_store(const lxp_state_snapshot *snapshot,
                                   const lxp_state_store *store)
{
    size_t i;
    if (!snapshot_canonical(snapshot) || !store_canonical(store) ||
        snapshot->store.count != store->count ||
        snapshot->store.idempotency_count != store->idempotency_count ||
        snapshot->store.next_sequence != store->next_sequence ||
        snapshot->store.account_root_required != store->account_root_required ||
        (snapshot->store.accounts != NULL) != (store->accounts != NULL))
        return false;
    for (i = 0U; i < store->count; ++i)
        if (!cell_equal(&snapshot->store.cells[i], &store->cells[i]))
            return false;
    for (i = 0U; i < store->idempotency_count; ++i)
        if (!idempotency_equal(&snapshot->store.idempotency[i],
                               &store->idempotency[i]))
            return false;
    if (store->accounts == NULL) return true;
    if (snapshot->accounts.count != store->accounts->count) return false;
    for (i = 0U; i < store->accounts->count; ++i)
        if (!account_equal(&snapshot->accounts.accounts[i],
                           &store->accounts->accounts[i]))
            return false;
    return true;
}

lxp_result lxp_state_publication_guard_begin(
    const lxp_state_snapshot *base, const lxp_state_snapshot *settled,
    lxp_state_store *live, lxp_state_publication_guard **guard)
{
    lxp_state_publication_guard *created;
    lxp_result status;
    if (guard == NULL) return LXP_ERR_NON_CANONICAL;
    *guard = NULL;
    if (!snapshot_canonical(base) ||
        !snapshot_canonical(settled) ||
        live == NULL || base->lineage != settled->lineage ||
        settled->generation < base->generation ||
        (base->store.accounts != NULL) !=
            (settled->store.accounts != NULL) ||
        settled->store.count > LXP_STATE_MAX_CELLS ||
        settled->store.idempotency_count > LXP_STATE_MAX_IDEMPOTENCY ||
        (settled->store.accounts != NULL &&
         settled->accounts.count > LX_ACCOUNT_REGISTRY_CAPACITY))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_writer_assert_owner(live);
    if (status != LXP_OK) return status;
    created = (lxp_state_publication_guard *)calloc(1U, sizeof(*created));
    if (created == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    created->live = live;
    created->settled = settled;
    if (live->accounts != NULL) {
        bool expected = false;
        if (!atomic_compare_exchange_strong_explicit(
                &live->accounts->gateway_transition, &expected, true,
                memory_order_acq_rel, memory_order_acquire)) {
            free(created);
            return LXP_ERR_CONTEXT_MISMATCH;
        }
        created->gateway_excluded = true;
        if (atomic_load_explicit(&live->accounts->gateway_acquirers,
                                 memory_order_acquire) != 0U) {
            atomic_store_explicit(&live->accounts->gateway_transition, false,
                                  memory_order_release);
            free(created);
            return LXP_ERR_CONTEXT_MISMATCH;
        }
    }
    if (pthread_mutex_lock(&live->lock) != 0) {
        if (created->gateway_excluded)
            atomic_store_explicit(&live->accounts->gateway_transition, false,
                                  memory_order_release);
        free(created);
        return LXP_ERR_IO;
    }
    created->state_locked = true;
    if (!snapshot_matches_store(base, live)) {
        if (pthread_mutex_unlock(&live->lock) != 0) abort();
        if (created->gateway_excluded)
            atomic_store_explicit(&live->accounts->gateway_transition, false,
                                  memory_order_release);
        free(created);
        return LXP_ERR_CONTEXT_MISMATCH;
    }
    *guard = created;
    return LXP_OK;
}

void lxp_state_snapshot_publish_guarded(lxp_state_publication_guard *guard)
{
    lxp_state_store *live;
    const lxp_state_snapshot *settled;
    if (guard == NULL || !guard->state_locked || guard->published) abort();
    live = guard->live;
    settled = guard->settled;
    live->count = settled->store.count;
    (void)memcpy(live->cells, settled->store.cells, sizeof(live->cells));
    live->idempotency_count = settled->store.idempotency_count;
    (void)memcpy(live->idempotency, settled->store.idempotency,
                 sizeof(live->idempotency));
    live->next_sequence = settled->store.next_sequence;
    live->account_root_required = settled->store.account_root_required;
    if (live->accounts != NULL) {
        live->accounts->count = settled->accounts.count;
        (void)memcpy(live->accounts->accounts, settled->accounts.accounts,
                     sizeof(live->accounts->accounts));
    }
    guard->published = true;
}

lxp_result lxp_state_publication_guard_end(
    lxp_state_publication_guard *guard)
{
    lxp_result status;
    if (guard == NULL || !guard->state_locked || guard->live == NULL)
        abort();
    if (pthread_mutex_unlock(&guard->live->lock) != 0) abort();
    status = LXP_OK;
    if (guard->gateway_excluded)
        atomic_store_explicit(&guard->live->accounts->gateway_transition,
                              false, memory_order_release);
    (void)memset(guard, 0, sizeof(*guard));
    free(guard);
    return status;
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
