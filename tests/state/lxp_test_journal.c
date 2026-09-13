#include "layerx/lxp_hash.h"
#include "layerx/lxp_state.h"

#include <pthread.h>
#include <stdint.h>
#include <string.h>

typedef struct verified_result {
    uint64_t sequence;
    uint8_t key[32];
    lxp_u128 value;
} verified_result;

typedef struct worker_input {
    verified_result *result;
    uint64_t sequence;
} worker_input;

static void *verify_worker(void *opaque)
{
    worker_input *input = (worker_input *)opaque;
    input->result->sequence = input->sequence;
    (void)memset(input->result->key, 0, sizeof(input->result->key));
    input->result->key[0] = (uint8_t)(input->sequence % 4U);
    input->result->value = (lxp_u128){ 0U, input->sequence + 100U };
    return NULL;
}

static void *wrong_writer(void *opaque)
{
    lxp_state_store *store = (lxp_state_store *)opaque;
    return (void *)(uintptr_t)(uint32_t)(-lxp_state_writer_assert_owner(store));
}

static int run(size_t worker_count, uint8_t digest[32])
{
    enum { COUNT = 32, MAX_WORKERS = 8 };
    lxp_state_store store;
    verified_result results[COUNT];
    worker_input inputs[COUNT];
    pthread_t threads[MAX_WORKERS];
    size_t completed = 0U;
    size_t i;
    if (lxp_state_store_init(&store, 0U) != LXP_OK) return 0;
    while (completed < COUNT) {
        size_t batch = worker_count;
        size_t j;
        if (batch == 0U) batch = 1U;
        if (batch > MAX_WORKERS) batch = MAX_WORKERS;
        if (batch > COUNT - completed) batch = COUNT - completed;
        for (j = 0U; j < batch; ++j) {
            size_t reversed = completed + batch - 1U - j;
            inputs[j].result = &results[reversed];
            inputs[j].sequence = reversed;
            if (worker_count == 0U) (void)verify_worker(&inputs[j]);
            else if (pthread_create(&threads[j], NULL, verify_worker,
                                    &inputs[j]) != 0) return 0;
        }
        if (worker_count != 0U)
            for (j = 0U; j < batch; ++j)
                if (pthread_join(threads[j], NULL) != 0) return 0;
        completed += batch;
    }
    for (i = 0U; i < COUNT; ++i) {
        lxp_state_journal journal;
        if (results[i].sequence != i ||
            lxp_state_journal_open(&store, i, &journal) != LXP_OK ||
            lxp_state_journal_set(&journal, results[i].key,
                                  results[i].value) != LXP_OK ||
            lxp_state_journal_commit(&journal) != LXP_OK) return 0;
    }
    if (lxp_hash_sha256(store.cells, store.count * sizeof(store.cells[0]),
                        digest) != LXP_OK ||
        lxp_state_store_destroy(&store) != LXP_OK) return 0;
    return 1;
}

int main(void)
{
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_u128 value;
    bool found;
    uint8_t key[32] = { 1U };
    uint8_t zero_workers[32];
    uint8_t maximum_workers[32];
    pthread_t outsider;
    void *outsider_result;
    if (lxp_state_store_init(&store, 5U) != LXP_OK ||
        lxp_state_journal_open(&store, 5U, &journal) != LXP_OK ||
        lxp_state_journal_set(&journal, key, (lxp_u128){ 0U, 9U }) != LXP_OK ||
        lxp_state_journal_rollback(&journal) != LXP_OK ||
        lxp_state_store_get(&store, key, &value, &found) != LXP_OK || found)
        return 1;
    if (pthread_create(&outsider, NULL, wrong_writer, &store) != 0 ||
        pthread_join(outsider, &outsider_result) != 0 ||
        (uintptr_t)outsider_result != (uintptr_t)(uint32_t)(-LXP_FATAL_INVARIANT))
        return 1;
    if (lxp_state_store_destroy(&store) != LXP_OK ||
        !run(0U, zero_workers) || !run(8U, maximum_workers) ||
        memcmp(zero_workers, maximum_workers, sizeof(zero_workers)) != 0)
        return 1;
    if (lxp_state_store_init(&store, UINT64_MAX - 1U) != LXP_OK ||
        lxp_state_journal_open(&store, UINT64_MAX - 1U, &journal) != LXP_OK ||
        lxp_state_journal_set(&journal, key, (lxp_u128){0U, 7U}) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        store.next_sequence != UINT64_MAX ||
        lxp_state_journal_open(&store, UINT64_MAX, &journal) != LXP_OK ||
        lxp_state_journal_set(&journal, key, (lxp_u128){0U, 8U}) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_ERR_SEQUENCE_EXHAUSTED ||
        store.next_sequence != UINT64_MAX || !journal.open ||
        lxp_state_store_get(&store, key, &value, &found) != LXP_OK ||
        !found || value.hi != 0U || value.lo != 7U ||
        lxp_state_journal_rollback(&journal) != LXP_OK ||
        lxp_state_store_destroy(&store) != LXP_OK) return 1;
    return 0;
}
