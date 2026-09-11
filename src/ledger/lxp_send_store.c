#include "layerx/lxp_ledger.h"

#include "layerx/lxp_fee.h"

#include <stdlib.h>
#include <string.h>

static bool history_locate(const lxp_send_history *history,
                           const uint8_t idempotency_key[32], size_t *index)
{
    size_t low = 0U;
    size_t high = history->count;
    while (low < high) {
        size_t middle = low + (high - low) / 2U;
        int comparison = memcmp(history->records[middle].idempotency_key,
                                idempotency_key, 32U);
        if (comparison == 0) {
            *index = middle;
            return true;
        }
        if (comparison < 0) low = middle + 1U;
        else high = middle;
    }
    *index = low;
    return false;
}

static lxp_result history_charge(lxp_meter_ctx *meter, size_t added)
{
    if (meter == NULL) return LXP_FATAL_INVARIANT;
    if (added == 0U) return LXP_OK;
    if (added > (size_t)INT64_MAX) return LXP_ERR_LENGTH_LIMIT;
    return lxp_meter_charge_storage(meter, (int64_t)added);
}

static lxp_result history_reserve(lxp_send_history *history, size_t additional)
{
    lxp_send_store_record *grown;
    size_t capacity;
    size_t required;
    if (additional > SIZE_MAX - history->count) return LXP_ERR_OVERFLOW;
    required = history->count + additional;
    if (required <= history->capacity) return LXP_OK;
    capacity = history->capacity == 0U ?
        (size_t)LXP_SEND_HISTORY_INITIAL_CAPACITY : history->capacity;
    while (capacity < required) {
        if (capacity > SIZE_MAX / 2U) return LXP_ERR_OVERFLOW;
        capacity *= 2U;
    }
    if (capacity > SIZE_MAX / sizeof(*grown)) return LXP_ERR_OVERFLOW;
    grown = (lxp_send_store_record *)realloc(history->records,
                                             capacity * sizeof(*grown));
    if (grown == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    history->records = grown;
    history->capacity = capacity;
    return LXP_OK;
}

static lxp_result history_admitted(const lxp_send_history *history,
                                   const lxp_send_store_record *record,
                                   bool *present)
{
    size_t index = 0U;
    if (!history_locate(history, record->idempotency_key, &index)) {
        *present = false;
        return LXP_OK;
    }
    if (memcmp(&history->records[index], record, sizeof(*record)) != 0)
        return LXP_FATAL_INVARIANT;
    *present = true;
    return LXP_OK;
}

static void history_insert(lxp_send_history *history,
                           const lxp_send_store_record *record)
{
    size_t index = 0U;
    if (history_locate(history, record->idempotency_key, &index)) return;
    if (index < history->count)
        (void)memmove(&history->records[index + 1U], &history->records[index],
                      (history->count - index) * sizeof(history->records[0]));
    history->records[index] = *record;
    ++history->count;
    history->stored_bytes += (uint64_t)LXP_SEND_HISTORY_RECORD_BYTES;
}

lxp_result lxp_send_store_init(lxp_send_store *store, lxp_meter_ctx *meter)
{
    if (store == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(store, 0, sizeof(*store));
    store->meter = meter;
    return LXP_OK;
}

void lxp_send_store_release(lxp_send_store *store)
{
    if (store == NULL) return;
    if (store->history != NULL) {
        free(store->history->records);
        (void)memset(store->history, 0, sizeof(*store->history));
        free(store->history);
    }
    (void)memset(store, 0, sizeof(*store));
}

lxp_result lxp_send_store_lookup(const lxp_send_store *store,
                                 const uint8_t idempotency_key[32],
                                 const uint8_t activity_hash[32],
                                 lxp_send_receipt_projection *projection)
{
    size_t index = 0U;
    size_t i;
    if (store == NULL || idempotency_key == NULL || activity_hash == NULL ||
        projection == NULL) return LXP_ERR_NON_CANONICAL;
    if (store->count > (size_t)LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    for (i = 0U; i < store->count; ++i) {
        if (memcmp(store->records[i].activity_hash, activity_hash, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
        if (memcmp(store->records[i].idempotency_key, idempotency_key,
                   32U) == 0) {
            *projection = store->records[i].receipt;
            projection->replayed = true;
            return LXP_ERR_IDEMPOTENT_REPLAY;
        }
    }
    if (store->history == NULL ||
        !history_locate(store->history, idempotency_key, &index))
        return LXP_OK;
    if (memcmp(store->history->records[index].activity_hash, activity_hash,
               32U) == 0) return LXP_ERR_SEQUENCE_REUSED;
    *projection = store->history->records[index].receipt;
    projection->replayed = true;
    return LXP_ERR_IDEMPOTENT_REPLAY;
}

lxp_result lxp_send_store_admit(lxp_send_store *store)
{
    lxp_send_history *history;
    size_t admitted = 0U;
    size_t i;
    lxp_result status;
    if (store == NULL) return LXP_ERR_NON_CANONICAL;
    if (store->count > (size_t)LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    if (store->count < (size_t)LXP_SEND_STORE_CAPACITY) return LXP_OK;
    if (store->meter == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    if (store->history == NULL) {
        history = (lxp_send_history *)calloc(1U, sizeof(*history));
        if (history == NULL) return LXP_ERR_ARENA_EXHAUSTED;
        store->history = history;
    }
    for (i = 0U; i < store->count; ++i) {
        bool present = false;
        status = history_admitted(store->history, &store->records[i],
                                  &present);
        if (status != LXP_OK) return status;
        if (!present) ++admitted;
    }
    status = history_reserve(store->history, admitted);
    if (status != LXP_OK) return status;
    if (admitted > SIZE_MAX / (size_t)LXP_SEND_HISTORY_RECORD_BYTES)
        return LXP_ERR_OVERFLOW;
    status = history_charge(store->meter,
                            admitted * (size_t)LXP_SEND_HISTORY_RECORD_BYTES);
    if (status != LXP_OK) return status;
    for (i = 0U; i < store->count; ++i)
        history_insert(store->history, &store->records[i]);
    (void)memset(store->records, 0, sizeof(store->records));
    store->count = 0U;
    return LXP_OK;
}

lxp_result lxp_send_store_append(lxp_send_store *store,
                                 const lxp_send_store_record *record)
{
    if (store == NULL || record == NULL) return LXP_ERR_NON_CANONICAL;
    if (store->count >= (size_t)LXP_SEND_STORE_CAPACITY)
        return LXP_FATAL_INVARIANT;
    store->records[store->count] = *record;
    ++store->count;
    return LXP_OK;
}

lxp_result lxp_send_store_total(const lxp_send_store *store, size_t *total)
{
    size_t history_count;
    if (store == NULL || total == NULL) return LXP_ERR_NON_CANONICAL;
    if (store->count > (size_t)LXP_SEND_STORE_CAPACITY)
        return LXP_ERR_LENGTH_LIMIT;
    history_count = store->history == NULL ? 0U : store->history->count;
    if (history_count > SIZE_MAX - store->count) return LXP_ERR_OVERFLOW;
    *total = history_count + store->count;
    return LXP_OK;
}
