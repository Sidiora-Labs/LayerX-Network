#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_history.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_storage.h"
#include "layerx/lxp_kernel.h"

#include <stdlib.h>
#include <string.h>

enum {
    FEED_RECORD_VERSION = 1,
    FEED_NOTICE_RECORD = 1,
    FEED_HEAD_RECORD = 2,
    FEED_BASELINE_RECORD = 3,
    FEED_PENDING_BYTES = 109,
    FEED_COMPLETE_BYTES = 77,
    FEED_NOTICE_BYTES = 84,
    FEED_HEAD_BYTES = 114,
    FEED_BASELINE_BYTES = 42
};

static const uint8_t pending_magic[5] = {'L', 'X', 'P', 'P', '1'};
static const uint8_t complete_magic[5] = {'L', 'X', 'P', 'C', '1'};

static uint32_t read_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

static uint64_t read_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index) value = (value << 8U) | bytes[index];
    return value;
}

static void write_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void write_u64(uint8_t bytes[8], uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static bool same_notice(const lx_programs_state_notice *left,
                        const lx_programs_state_notice *right)
{
    return left->global_sequence == right->global_sequence &&
           left->ordinal == right->ordinal &&
           left->activity_type == right->activity_type &&
           left->event_type == right->event_type &&
           lxp_ct_memcmp(left->program_id, right->program_id, 32U) == 0 &&
           lxp_ct_memcmp(left->receipt_digest,
                         right->receipt_digest, 32U) == 0;
}

static lxp_result store_lock(void *context)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    if (store == NULL || store->coordination_mutex == NULL)
        return LXP_ERR_NON_CANONICAL;
    return pthread_mutex_lock(store->coordination_mutex) == 0 ?
               LXP_OK : LXP_ERR_IO;
}

static lxp_result store_unlock(void *context)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    if (store == NULL || store->coordination_mutex == NULL)
        return LXP_ERR_NON_CANONICAL;
    return pthread_mutex_unlock(store->coordination_mutex) == 0 ?
               LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result notice_validate(const lx_programs_state_feed_store *store,
                                  const lx_programs_state_notice *notice)
{
    uint64_t expected_sequence;
    if (store == NULL || notice == NULL ||
        !store->baseline_present ||
        notice->ordinal == UINT32_MAX ||
        lxp_ct_is_zero(notice->program_id, 32U) ||
        lxp_ct_is_zero(notice->receipt_digest, 32U))
        return LXP_ERR_NON_CANONICAL;
    expected_sequence = store->scanned_through_sequence == 0U ?
        store->baseline_next_sequence : store->scanned_through_sequence + 1U;
    if (store->scanned_through_sequence == UINT64_MAX ||
        notice->global_sequence != expected_sequence ||
        notice->ordinal != store->next_notice_ordinal ||
        (store->notice_group_open &&
         notice->global_sequence != store->open_notice_sequence))
        return LXP_ERR_UNSORTED_SEQUENCE;
    if (store->notice_group_open &&
        lxp_ct_memcmp(notice->receipt_digest,
                      store->open_notice_receipt_digest, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static void notice_commit(lx_programs_state_feed_store *store,
                          const lx_programs_state_notice *notice)
{
    size_t slot;
    slot = store->notice_count < LX_PROGRAMS_STATE_FEED_CACHE_NOTICES ?
               store->notice_count++ : store->notice_next;
    if (store->notice_count == LX_PROGRAMS_STATE_FEED_CACHE_NOTICES)
        store->notice_next =
            (slot + 1U) % LX_PROGRAMS_STATE_FEED_CACHE_NOTICES;
    store->notices[slot] = *notice;
    ++store->notice_record_count;
    if (!store->notice_group_open)
        (void)memcpy(store->open_notice_receipt_digest,
                     notice->receipt_digest, 32U);
    store->notice_group_open = true;
    store->open_notice_sequence = notice->global_sequence;
    store->next_notice_ordinal = notice->ordinal + 1U;
}

static lxp_result notice_accept(lx_programs_state_feed_store *store,
                                const lx_programs_state_notice *notice)
{
    lxp_result status = notice_validate(store, notice);
    if (status == LXP_OK) notice_commit(store, notice);
    return status;
}

static lxp_result decode_notice(const uint8_t *body, size_t length,
                                lx_programs_state_notice *notice)
{
    if (body == NULL || notice == NULL || length != FEED_NOTICE_BYTES ||
        body[0] != FEED_RECORD_VERSION || body[1] != FEED_NOTICE_RECORD)
        return LXP_ERR_LOG_CORRUPT;
    (void)memset(notice, 0, sizeof(*notice));
    notice->global_sequence = read_u64(body + 2U);
    notice->ordinal = read_u32(body + 10U);
    (void)memcpy(notice->program_id, body + 14U, 32U);
    notice->activity_type = read_u32(body + 46U);
    notice->event_type = (uint16_t)(((uint16_t)body[50U] << 8U) | body[51U]);
    (void)memcpy(notice->receipt_digest, body + 52U, 32U);
    return LXP_OK;
}

static void encode_notice(const lx_programs_state_notice *notice,
                          uint8_t body[FEED_NOTICE_BYTES])
{
    (void)memset(body, 0, FEED_NOTICE_BYTES);
    body[0] = FEED_RECORD_VERSION;
    body[1] = FEED_NOTICE_RECORD;
    write_u64(body + 2U, notice->global_sequence);
    write_u32(body + 10U, notice->ordinal);
    (void)memcpy(body + 14U, notice->program_id, 32U);
    write_u32(body + 46U, notice->activity_type);
    body[50U] = (uint8_t)(notice->event_type >> 8U);
    body[51U] = (uint8_t)notice->event_type;
    (void)memcpy(body + 52U, notice->receipt_digest, 32U);
}

static lxp_result notice_lookup(
    const lx_programs_state_feed_store *store, uint64_t global_sequence,
    uint32_t ordinal, lx_programs_state_notice *notice, bool *present)
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    if (store == NULL || store->log == NULL || notice == NULL ||
        present == NULL)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        uint8_t body[FEED_HEAD_BYTES];
        status = lxp_log_read(store->log, offset, &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.record_kind != (uint8_t)LXP_LOG_STATE_DIFF ||
            header.body_length > sizeof(body))
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_log_read(store->log, offset, &header, body,
                              sizeof(body));
        if (status == LXP_OK && header.body_length == FEED_NOTICE_BYTES &&
            body[0] == FEED_RECORD_VERSION &&
            body[1] == FEED_NOTICE_RECORD) {
            lx_programs_state_notice candidate;
            status = decode_notice(body, header.body_length, &candidate);
            if (status == LXP_OK &&
                candidate.global_sequence == global_sequence &&
                candidate.ordinal == ordinal) {
                *notice = candidate;
                *present = true;
                return LXP_OK;
            }
        }
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    return status;
}

static lxp_result head_matches(
    const lx_programs_state_feed_store *store, const lxp_receipt *receipt,
    const uint8_t receipt_digest[32], const uint8_t activity_id[32])
{
    uint64_t offset = 0U;
    lxp_result status = LXP_OK;
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        uint8_t body[FEED_HEAD_BYTES];
        status = lxp_log_read(store->log, offset, &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.record_kind != (uint8_t)LXP_LOG_STATE_DIFF ||
            header.body_length > sizeof(body))
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_log_read(store->log, offset, &header, body,
                              sizeof(body));
        if (status == LXP_OK && header.body_length == FEED_HEAD_BYTES &&
            body[0] == FEED_RECORD_VERSION && body[1] == FEED_HEAD_RECORD &&
            read_u64(body + 2U) == receipt->global_sequence)
            return lxp_ct_memcmp(body + 10U, receipt_digest, 32U) == 0 &&
                           lxp_ct_memcmp(body + 42U,
                                         receipt->resulting_state_root,
                                         32U) == 0 &&
                           read_u64(body + 74U) == receipt->timestamp &&
                           lxp_ct_memcmp(body + 82U, activity_id, 32U) == 0 ?
                       LXP_OK : LXP_FATAL_INVARIANT;
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    return status == LXP_OK ? LXP_ERR_PROJECTION_STALE : status;
}

static lxp_result canonical_group_matches(
    const lx_programs_state_feed_store *store, uint64_t *feed_offset,
    const lxp_receipt *receipt, const uint8_t receipt_digest[32],
    const uint8_t activity_id[32])
{
    uint32_t expected_ordinal = 0U;
    bool first_record;
    lxp_result status = LXP_OK;
    if (store == NULL || store->log == NULL || feed_offset == NULL ||
        receipt == NULL || receipt_digest == NULL || activity_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    first_record = *feed_offset == 0U;
    while (status == LXP_OK && *feed_offset < store->log->write_offset) {
        lxp_log_record_header header;
        uint8_t body[FEED_HEAD_BYTES];
        status = lxp_log_read(store->log, *feed_offset, &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.record_kind != (uint8_t)LXP_LOG_STATE_DIFF ||
            header.body_length > sizeof(body))
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_log_read(store->log, *feed_offset, &header, body,
                              sizeof(body));
        if (status != LXP_OK) break;
        *feed_offset += LXP_LOG_HEADER_BYTES + header.body_length;
        if (first_record) {
            first_record = false;
            if (header.body_length != FEED_BASELINE_BYTES ||
                body[0] != FEED_RECORD_VERSION ||
                body[1] != FEED_BASELINE_RECORD ||
                read_u64(body + 2U) != store->baseline_next_sequence ||
                header.global_sequence != store->baseline_next_sequence ||
                lxp_ct_memcmp(body + 10U,
                              store->baseline_state_root, 32U) != 0)
                return LXP_ERR_LOG_CORRUPT;
            continue;
        }
        if (header.body_length == FEED_NOTICE_BYTES &&
            body[0] == FEED_RECORD_VERSION &&
            body[1] == FEED_NOTICE_RECORD) {
            lx_programs_state_notice notice;
            status = decode_notice(body, header.body_length, &notice);
            if (status != LXP_OK ||
                notice.global_sequence != receipt->global_sequence ||
                header.global_sequence != receipt->global_sequence ||
                notice.ordinal != expected_ordinal ||
                lxp_ct_memcmp(notice.receipt_digest,
                              receipt_digest, 32U) != 0)
                return LXP_ERR_LOG_CORRUPT;
            if (expected_ordinal == UINT32_MAX)
                return LXP_ERR_LOG_CORRUPT;
            ++expected_ordinal;
            continue;
        }
        if (header.body_length == FEED_HEAD_BYTES &&
            body[0] == FEED_RECORD_VERSION && body[1] == FEED_HEAD_RECORD &&
            read_u64(body + 2U) == receipt->global_sequence &&
            header.global_sequence == receipt->global_sequence)
            return lxp_ct_memcmp(body + 10U, receipt_digest, 32U) == 0 &&
                           lxp_ct_memcmp(body + 42U,
                                         receipt->resulting_state_root,
                                         32U) == 0 &&
                           read_u64(body + 74U) == receipt->timestamp &&
                           lxp_ct_memcmp(body + 82U, activity_id, 32U) == 0 ?
                       LXP_OK : LXP_ERR_LOG_CORRUPT;
        return LXP_ERR_LOG_CORRUPT;
    }
    return status == LXP_OK ? LXP_ERR_PROJECTION_STALE : status;
}

static lxp_result replay_feed(void *context,
                              const lxp_log_record_header *header,
                              const uint8_t *body)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    if (store == NULL || header == NULL || body == NULL ||
        header->record_kind != (uint8_t)LXP_LOG_STATE_DIFF)
        return LXP_ERR_LOG_CORRUPT;
    if (header->body_length == FEED_BASELINE_BYTES &&
        body[0] == FEED_RECORD_VERSION && body[1] == FEED_BASELINE_RECORD) {
        uint64_t next_sequence = read_u64(body + 2U);
        if (store->baseline_present || store->notice_record_count != 0U ||
            store->scanned_through_sequence != 0U || next_sequence == 0U ||
            header->global_sequence != next_sequence ||
            lxp_ct_is_zero(body + 10U, 32U))
            return LXP_ERR_LOG_CORRUPT;
        store->baseline_next_sequence = next_sequence;
        (void)memcpy(store->baseline_state_root, body + 10U, 32U);
        store->baseline_present = true;
        return LXP_OK;
    }
    if (!store->baseline_present) return LXP_ERR_LOG_CORRUPT;
    if (header->body_length == FEED_NOTICE_BYTES &&
        body[1] == FEED_NOTICE_RECORD) {
        lx_programs_state_notice notice;
        lxp_result status = decode_notice(body, header->body_length, &notice);
        if (status != LXP_OK || notice.global_sequence != header->global_sequence)
            return status != LXP_OK ? status : LXP_ERR_LOG_CORRUPT;
        status = notice_accept(store, &notice);
        return status == LXP_OK ? LXP_OK : LXP_ERR_LOG_CORRUPT;
    }
    if (header->body_length == FEED_HEAD_BYTES && body[0] == FEED_RECORD_VERSION &&
        body[1] == FEED_HEAD_RECORD) {
        uint64_t sequence = read_u64(body + 2U);
        if (sequence != header->global_sequence ||
            store->scanned_through_sequence == UINT64_MAX ||
            sequence != (store->scanned_through_sequence == 0U ?
                store->baseline_next_sequence :
                store->scanned_through_sequence + 1U) ||
            (store->notice_group_open &&
             store->open_notice_sequence != sequence) ||
            (store->notice_group_open &&
             lxp_ct_memcmp(store->open_notice_receipt_digest,
                           body + 10U, 32U) != 0) ||
            lxp_ct_is_zero(body + 10U, 32U) ||
            lxp_ct_is_zero(body + 42U, 32U) || read_u64(body + 74U) == 0U ||
            lxp_ct_is_zero(body + 82U, 32U))
            return LXP_ERR_LOG_CORRUPT;
        store->scanned_through_sequence = sequence;
        (void)memcpy(store->head_receipt_digest, body + 10U, 32U);
        (void)memcpy(store->head_state_root, body + 42U, 32U);
        store->head_timestamp = read_u64(body + 74U);
        store->notice_group_open = false;
        store->open_notice_sequence = 0U;
        (void)memset(store->open_notice_receipt_digest, 0,
                     sizeof(store->open_notice_receipt_digest));
        store->next_notice_ordinal = 0U;
        return LXP_OK;
    }
    return LXP_ERR_LOG_CORRUPT;
}

static lxp_result store_append(
    void *context, uint64_t global_sequence, uint32_t ordinal,
    const uint8_t program_id[32], uint32_t activity_type,
    uint16_t event_type, const lxp_receipt *receipt)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    lx_programs_state_notice notice;
    uint8_t body[FEED_NOTICE_BYTES];
    uint64_t offset;
    size_t mark;
    lxp_result status;
    if (store == NULL || store->log == NULL || store->scratch == NULL ||
        receipt == NULL || receipt->global_sequence != global_sequence)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&notice, 0, sizeof(notice));
    notice.global_sequence = global_sequence;
    notice.ordinal = ordinal;
    (void)memcpy(notice.program_id, program_id, 32U);
    notice.activity_type = activity_type;
    notice.event_type = event_type;
    mark = lxp_arena_mark(store->scratch);
    status = lxp_receipt_digest(receipt, store->scratch,
                                notice.receipt_digest);
    (void)lxp_arena_reset(store->scratch, mark);
    if (status != LXP_OK) return status;
    {
        lx_programs_state_notice existing;
        bool present;
        status = notice_lookup(store, global_sequence, ordinal,
                               &existing, &present);
        if (status != LXP_OK) return status;
        if (present)
            return same_notice(&existing, &notice) ?
                       LXP_OK : LXP_FATAL_INVARIANT;
    }
    status = notice_validate(store, &notice);
    if (status != LXP_OK) return status;
    encode_notice(&notice, body);
    status = lxp_log_append(store->log, LXP_LOG_STATE_DIFF,
                            global_sequence, body, sizeof(body), &offset);
    if (status == LXP_OK) status = lxp_log_write_boundary(store->log);
    if (status == LXP_OK) notice_commit(store, &notice);
    return status;
}

static lxp_result store_begin(void *context, const lxp_activity *activity,
                              const lxp_receipt *receipt)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    lxp_receipt_query query;
    lxp_byte_span prior = {NULL, 0U};
    lxp_byte_span encoded_activity;
    lxp_byte_span encoded_receipt;
    uint8_t pending[FEED_PENDING_BYTES];
    uint8_t activity_id[32];
    uint64_t offset;
    size_t mark;
    lxp_result status;
    if (store == NULL || store->canonical_log == NULL ||
        store->history == NULL || store->scratch == NULL ||
        activity == NULL || receipt == NULL ||
        receipt->global_sequence < store->baseline_next_sequence)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(store->scratch);
    (void)memset(&query, 0, sizeof(query));
    query.kind = LXP_RECEIPT_BY_GLOBAL_SEQUENCE;
    query.global_sequence = receipt->global_sequence;
    query.maximum_response_bytes = LXP_MAX_ACTIVITY_BYTES * 4U;
    status = lxp_receipt_lookup(store->history, &query, store->scratch,
                                &prior);
    if (status == LXP_OK) {
        status = lxp_receipt_encode(receipt, true, store->scratch,
                                    &encoded_receipt);
        if (status == LXP_OK &&
            (prior.length != encoded_receipt.length ||
             memcmp(prior.bytes, encoded_receipt.bytes, prior.length) != 0))
            status = LXP_FATAL_INVARIANT;
        (void)lxp_arena_reset(store->scratch, mark);
        return status;
    }
    if (status != LXP_ERR_UNKNOWN_ACTIVITY) {
        (void)lxp_arena_reset(store->scratch, mark);
        return status;
    }
    if (receipt->global_sequence !=
            (store->scanned_through_sequence == 0U ?
                 store->baseline_next_sequence :
                 store->scanned_through_sequence + 1U) ||
        lxp_ct_memcmp(
            receipt->previous_state_root,
            store->scanned_through_sequence == 0U ?
                store->baseline_state_root : store->head_state_root,
            32U) != 0) {
        (void)lxp_arena_reset(store->scratch, mark);
        return LXP_ERR_NON_CANONICAL;
    }
    status = lxp_activity_encode(activity, store->scratch,
                                 &encoded_activity);
    if (status == LXP_OK)
        status = lxp_receipt_encode(receipt, true, store->scratch,
                                    &encoded_receipt);
    if (status == LXP_OK)
        status = lxp_activity_id(encoded_activity.bytes,
                                 encoded_activity.length, activity_id);
    if (status == LXP_OK &&
        lxp_ct_memcmp(activity_id, receipt->activity_id, 32U) != 0)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK &&
        (encoded_activity.length > UINT32_MAX ||
         encoded_receipt.length > UINT32_MAX))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) {
        (void)memset(pending, 0, sizeof(pending));
        (void)memcpy(pending, pending_magic, sizeof(pending_magic));
        write_u64(pending + 5U, receipt->global_sequence);
        (void)memcpy(pending + 13U, receipt->activity_id, 32U);
        (void)memcpy(pending + 45U, receipt->previous_state_root, 32U);
        (void)memcpy(pending + 77U, receipt->resulting_state_root, 32U);
        status = lxp_log_append(
            store->canonical_log, LXP_LOG_CHECKPOINT,
            receipt->global_sequence, pending, sizeof(pending), &offset);
    }
    if (status == LXP_OK)
        status = lxp_log_append(
            store->canonical_log, LXP_LOG_ACTIVITY,
            receipt->global_sequence, encoded_activity.bytes,
            (uint32_t)encoded_activity.length, &offset);
    if (status == LXP_OK)
        status = lxp_log_append(
            store->canonical_log, LXP_LOG_RECEIPT,
            receipt->global_sequence, encoded_receipt.bytes,
            (uint32_t)encoded_receipt.length, &offset);
    if (status == LXP_OK)
        status = lxp_log_write_boundary(store->canonical_log);
    (void)lxp_arena_reset(store->scratch, mark);
    if (status == LXP_OK)
        status = lxp_history_index_rebuild(store->history);
    return status;
}

static lxp_result canonical_complete_append(
    lx_programs_state_feed_store *store, uint64_t sequence,
    const uint8_t receipt_digest[32], const uint8_t state_root[32])
{
    uint8_t complete[FEED_COMPLETE_BYTES];
    uint64_t offset;
    lxp_result status;
    if (store == NULL || store->canonical_log == NULL || sequence == 0U ||
        receipt_digest == NULL || state_root == NULL ||
        lxp_ct_is_zero(receipt_digest, 32U) ||
        lxp_ct_is_zero(state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(complete, 0, sizeof(complete));
    (void)memcpy(complete, complete_magic, sizeof(complete_magic));
    write_u64(complete + 5U, sequence);
    (void)memcpy(complete + 13U, receipt_digest, 32U);
    (void)memcpy(complete + 45U, state_root, 32U);
    status = lxp_log_append(store->canonical_log, LXP_LOG_CHECKPOINT,
                            sequence, complete, sizeof(complete), &offset);
    if (status == LXP_OK)
        status = lxp_log_write_boundary(store->canonical_log);
    return status;
}

static lxp_result store_advance(void *context, const lxp_activity *activity,
                                const lxp_receipt *receipt)
{
    lx_programs_state_feed_store *store =
        (lx_programs_state_feed_store *)context;
    uint8_t body[FEED_HEAD_BYTES];
    uint8_t digest[32];
    uint8_t activity_id[32];
    lxp_byte_span encoded_activity;
    uint64_t offset;
    size_t mark;
    lxp_result status;
    if (store == NULL || store->log == NULL || store->scratch == NULL ||
        activity == NULL || receipt == NULL || receipt->global_sequence == 0U ||
        receipt->global_sequence < store->baseline_next_sequence ||
        (receipt->global_sequence > store->scanned_through_sequence &&
         receipt->global_sequence !=
             (store->scanned_through_sequence == 0U ?
                  store->baseline_next_sequence :
                  store->scanned_through_sequence + 1U)))
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(store->scratch);
    status = lxp_activity_encode(activity, store->scratch,
                                 &encoded_activity);
    if (status == LXP_OK)
        status = lxp_activity_id(encoded_activity.bytes,
                                 encoded_activity.length, activity_id);
    (void)lxp_arena_reset(store->scratch, mark);
    if (status != LXP_OK) return status;
    mark = lxp_arena_mark(store->scratch);
    status = lxp_receipt_digest(receipt, store->scratch, digest);
    (void)lxp_arena_reset(store->scratch, mark);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(activity_id, receipt->activity_id, 32U) != 0)
        return LXP_FATAL_INVARIANT;
    if (receipt->global_sequence <= store->scanned_through_sequence)
        return head_matches(store, receipt, digest, activity_id);
    if ((store->notice_group_open &&
         (store->open_notice_sequence != receipt->global_sequence ||
          lxp_ct_memcmp(store->open_notice_receipt_digest,
                        digest, 32U) != 0)) ||
        lxp_ct_is_zero(receipt->resulting_state_root, 32U) ||
        receipt->timestamp == 0U || lxp_ct_is_zero(activity_id, 32U))
        return LXP_FATAL_INVARIANT;
    (void)memset(body, 0, sizeof(body));
    body[0] = FEED_RECORD_VERSION;
    body[1] = FEED_HEAD_RECORD;
    write_u64(body + 2U, receipt->global_sequence);
    (void)memcpy(body + 10U, digest, 32U);
    (void)memcpy(body + 42U, receipt->resulting_state_root, 32U);
    write_u64(body + 74U, receipt->timestamp);
    (void)memcpy(body + 82U, activity_id, 32U);
    status = lxp_log_append(store->log, LXP_LOG_STATE_DIFF,
                            receipt->global_sequence, body, sizeof(body),
                            &offset);
    if (status == LXP_OK) status = lxp_log_write_boundary(store->log);
    if (status != LXP_OK) return status;
    status = canonical_complete_append(
        store, receipt->global_sequence, digest,
        receipt->resulting_state_root);
    if (status != LXP_OK) return status;
    store->scanned_through_sequence = receipt->global_sequence;
    (void)memcpy(store->head_receipt_digest, digest, 32U);
    (void)memcpy(store->head_state_root, receipt->resulting_state_root, 32U);
    store->head_timestamp = receipt->timestamp;
    store->notice_group_open = false;
    store->open_notice_sequence = 0U;
    (void)memset(store->open_notice_receipt_digest, 0,
                 sizeof(store->open_notice_receipt_digest));
    store->next_notice_ordinal = 0U;
    return LXP_OK;
}

static lxp_result recover_canonical(lx_programs_state_feed_store *store,
                                    lxp_kernel *kernel)
{
    uint64_t offset = 0U;
    uint64_t feed_validation_offset = 0U;
    uint64_t projected_through_sequence = store->scanned_through_sequence;
    uint64_t activity_sequence = 0U;
    uint8_t *activity_bytes = NULL;
    size_t activity_length = 0U;
    bool pending = false;
    bool completed = false;
    uint64_t pending_sequence = 0U;
    uint64_t expected_sequence = store->baseline_next_sequence;
    uint8_t pending_activity_id[32] = {0};
    uint8_t pending_previous_root[32] = {0};
    uint8_t pending_resulting_root[32] = {0};
    uint8_t pending_receipt_digest[32] = {0};
    lxp_result status = LXP_OK;
    while (status == LXP_OK && offset < store->canonical_log->write_offset) {
        lxp_log_record_header header;
        uint8_t *body = NULL;
        status = lxp_log_read(store->canonical_log, offset, &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.body_length > LXP_MAX_ACTIVITY_BYTES) {
            status = LXP_ERR_LENGTH_LIMIT;
            break;
        }
        if (header.body_length != 0U) {
            body = (uint8_t *)malloc(header.body_length);
            if (body == NULL) {
                status = LXP_ERR_IO;
                break;
            }
            status = lxp_log_read(store->canonical_log, offset, &header,
                                  body, header.body_length);
            if (status != LXP_OK) {
                free(body);
                break;
            }
        }
        if (header.global_sequence < store->baseline_next_sequence) {
            free(body);
            offset += LXP_LOG_HEADER_BYTES + header.body_length;
            continue;
        }
        if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
            header.body_length == FEED_PENDING_BYTES &&
            memcmp(body, pending_magic, sizeof(pending_magic)) == 0) {
            uint64_t sequence = read_u64(body + 5U);
            if (sequence != header.global_sequence || pending || completed ||
                sequence != expected_sequence) {
                status = LXP_ERR_LOG_CORRUPT;
            } else {
                pending = true;
                completed = false;
                pending_sequence = sequence;
                (void)memcpy(pending_activity_id, body + 13U, 32U);
                (void)memcpy(pending_previous_root, body + 45U, 32U);
                (void)memcpy(pending_resulting_root, body + 77U, 32U);
                (void)memset(pending_receipt_digest, 0,
                             sizeof(pending_receipt_digest));
            }
        } else if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
                   header.body_length == FEED_COMPLETE_BYTES &&
                   memcmp(body, complete_magic, sizeof(complete_magic)) == 0) {
            uint64_t sequence = read_u64(body + 5U);
            if (!pending || completed || sequence != pending_sequence ||
                sequence != header.global_sequence ||
                lxp_ct_is_zero(pending_receipt_digest, 32U) ||
                lxp_ct_memcmp(body + 13U,
                              pending_receipt_digest, 32U) != 0 ||
                lxp_ct_memcmp(body + 45U, pending_resulting_root, 32U) != 0)
                status = LXP_ERR_LOG_CORRUPT;
            else {
                if (expected_sequence == UINT64_MAX)
                    status = LXP_ERR_LOG_CORRUPT;
                else
                    ++expected_sequence;
                pending = false;
                completed = false;
            }
        } else if (header.global_sequence >= store->baseline_next_sequence &&
            header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
            if (!pending || header.global_sequence != pending_sequence ||
                activity_bytes != NULL) {
                status = LXP_ERR_LOG_CORRUPT;
            } else {
                activity_bytes = body;
                activity_length = header.body_length;
                activity_sequence = header.global_sequence;
                body = NULL;
            }
        } else if (header.global_sequence >= store->baseline_next_sequence &&
                   header.record_kind == (uint8_t)LXP_LOG_RECEIPT) {
            lxp_activity activity;
            lxp_receipt receipt;
            uint8_t activity_id[32];
            if (activity_bytes == NULL ||
                activity_sequence != header.global_sequence) {
                status = LXP_ERR_LOG_CORRUPT;
            } else {
                status = lxp_activity_decode(activity_bytes, activity_length,
                                             &activity);
                if (status == LXP_OK)
                    status = lxp_activity_id(activity_bytes, activity_length,
                                             activity_id);
                if (status == LXP_OK)
                    status = lxp_receipt_decode(body, header.body_length,
                                                true, &receipt);
                if (status == LXP_OK &&
                    (receipt.global_sequence != activity_sequence ||
                     lxp_ct_memcmp(receipt.activity_id,
                                   activity_id, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.activity_id,
                                   pending_activity_id, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.previous_state_root,
                                   pending_previous_root, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.resulting_state_root,
                                   pending_resulting_root, 32U) != 0))
                    status = LXP_ERR_LOG_CORRUPT;
                if (status == LXP_OK) {
                    size_t mark = lxp_arena_mark(store->scratch);
                    status = lxp_receipt_digest(
                        &receipt, store->scratch,
                        pending_receipt_digest);
                    (void)lxp_arena_reset(store->scratch, mark);
                }
                if (status == LXP_OK &&
                    receipt.global_sequence <= projected_through_sequence) {
                    status = canonical_group_matches(
                        store, &feed_validation_offset, &receipt,
                        pending_receipt_digest, activity_id);
                } else if (status == LXP_OK) {
                    status = lxp_kernel_restore_commit_observer_pending(
                        kernel, &activity, &receipt);
                    if (status == LXP_OK)
                        status = lxp_kernel_recover_commit_observer(
                            kernel, &activity, &receipt);
                }
                free(activity_bytes);
                activity_bytes = NULL;
                activity_length = 0U;
                activity_sequence = 0U;
            }
        } else if (header.global_sequence >=
                   store->baseline_next_sequence) {
            status = LXP_ERR_LOG_CORRUPT;
        }
        free(body);
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    if (activity_bytes != NULL) {
        free(activity_bytes);
        if (status == LXP_OK) status = LXP_ERR_LOG_CORRUPT;
    }
    if (status == LXP_OK && pending && !completed) {
        if (pending_sequence != store->scanned_through_sequence ||
            lxp_ct_memcmp(pending_receipt_digest,
                          store->head_receipt_digest, 32U) != 0 ||
            lxp_ct_memcmp(pending_resulting_root,
                          store->head_state_root, 32U) != 0)
            status = LXP_ERR_PROJECTION_STALE;
        else
            status = canonical_complete_append(
                store, pending_sequence, pending_receipt_digest,
                pending_resulting_root);
        if (status == LXP_OK) {
            if (expected_sequence == UINT64_MAX)
                status = LXP_ERR_LOG_CORRUPT;
            else
                ++expected_sequence;
        }
    }
    if (status == LXP_OK &&
        expected_sequence !=
            (store->scanned_through_sequence == 0U ?
                 store->baseline_next_sequence :
                 store->scanned_through_sequence + 1U))
        status = LXP_ERR_PROJECTION_STALE;
    return status;
}

static lxp_result state_feed_store_anchor(
    lx_programs_state_feed_store *store, uint64_t next_sequence,
    const uint8_t state_root[32])
{
    uint8_t body[FEED_BASELINE_BYTES];
    uint64_t offset;
    lxp_result status;
    if (store == NULL || store->log == NULL || state_root == NULL ||
        next_sequence == 0U || lxp_ct_is_zero(state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    if (store->baseline_present) return LXP_OK;
    if (store->log->write_offset != 0U ||
        store->notice_record_count != 0U ||
        store->scanned_through_sequence != 0U)
        return LXP_ERR_LOG_CORRUPT;
    (void)memset(body, 0, sizeof(body));
    body[0] = FEED_RECORD_VERSION;
    body[1] = FEED_BASELINE_RECORD;
    write_u64(body + 2U, next_sequence);
    (void)memcpy(body + 10U, state_root, 32U);
    status = lxp_log_append(store->log, LXP_LOG_STATE_DIFF, next_sequence,
                            body, sizeof(body), &offset);
    if (status == LXP_OK) status = lxp_log_write_boundary(store->log);
    if (status == LXP_OK) {
        store->baseline_next_sequence = next_sequence;
        (void)memcpy(store->baseline_state_root, state_root, 32U);
        store->baseline_present = true;
    }
    return status;
}

lxp_result lxp_programs_state_feed_store_open(
    lx_programs_state_feed_store *store, lxp_log *log,
    lxp_log *canonical_log, lxp_history *history, lxp_arena *scratch,
    pthread_mutex_t *coordination_mutex,
    uint64_t baseline_next_sequence, const uint8_t baseline_state_root[32])
{
    lxp_result status;
    if (store == NULL || log == NULL || canonical_log == NULL ||
        history == NULL || history->log != canonical_log ||
        log == canonical_log || scratch == NULL || coordination_mutex == NULL ||
        baseline_state_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(store, 0, sizeof(*store));
    store->log = log;
    store->canonical_log = canonical_log;
    store->history = history;
    store->scratch = scratch;
    store->coordination_mutex = coordination_mutex;
    status = lxp_log_recover(log, replay_feed, store);
    store->feed.begin = store_begin;
    store->feed.append = store_append;
    store->feed.advance = store_advance;
    store->feed.lock = store_lock;
    store->feed.unlock = store_unlock;
    store->feed.context = store;
    if (status == LXP_OK)
        status = state_feed_store_anchor(
            store, baseline_next_sequence, baseline_state_root);
    return status;
}

lxp_result lxp_programs_state_feed_store_recover(
    lx_programs_state_feed_store *store, lxp_kernel *kernel)
{
    lxp_result status;
    bool locked = false;
    if (store == NULL || kernel == NULL ||
        kernel->commit_observer_context != &store->feed ||
        kernel->observe_commit == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = store_lock(store);
    if (status == LXP_OK) {
        locked = true;
        status = recover_canonical(store, kernel);
    }
    {
        lxp_result unlock_status = locked ? store_unlock(store) : LXP_OK;
        return status == LXP_OK ? unlock_status : status;
    }
}

lxp_result lxp_programs_state_feed_store_page(
    const lx_programs_state_feed_store *store, uint64_t after_sequence,
    size_t maximum, lx_programs_state_notice *notices, size_t *notice_count,
    uint64_t *complete_through, uint64_t *scanned_through)
{
    uint64_t offset = 0U;
    uint64_t expected_sequence = 0U;
    uint64_t group_sequence = 0U;
    uint64_t disk_scanned_through = 0U;
    uint8_t group_receipt_digest[32] = {0};
    uint32_t expected_ordinal = 0U;
    size_t group_start = 0U;
    size_t count = 0U;
    uint64_t complete = after_sequence;
    bool baseline = false;
    bool group_open = false;
    bool group_overflow = false;
    bool stopped_at_capacity = false;
    lxp_result lock_status;
    lxp_result status = LXP_OK;
    if (store == NULL || notices == NULL || notice_count == NULL ||
        complete_through == NULL || scanned_through == NULL || maximum == 0U ||
        maximum > LX_PROGRAMS_STATE_FEED_MAX_NOTICES)
        return LXP_ERR_NON_CANONICAL;
    lock_status = store_lock((void *)store);
    if (lock_status != LXP_OK) return lock_status;
    if (after_sequence > store->scanned_through_sequence) {
        (void)store_unlock((void *)store);
        return LXP_ERR_NON_CANONICAL;
    }
    while (status == LXP_OK && offset < store->log->write_offset) {
        lxp_log_record_header header;
        uint8_t body[FEED_HEAD_BYTES];
        status = lxp_log_read(store->log, offset, &header, NULL, 0U);
        if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) break;
        if (header.record_kind != (uint8_t)LXP_LOG_STATE_DIFF ||
            header.body_length > sizeof(body)) {
            status = LXP_ERR_LOG_CORRUPT;
            break;
        }
        status = lxp_log_read(store->log, offset, &header, body,
                              sizeof(body));
        if (status != LXP_OK) break;
        if (header.body_length == FEED_BASELINE_BYTES &&
            body[0] == FEED_RECORD_VERSION &&
            body[1] == FEED_BASELINE_RECORD) {
            expected_sequence = read_u64(body + 2U);
            if (baseline || expected_sequence == 0U ||
                expected_sequence != header.global_sequence)
                status = LXP_ERR_LOG_CORRUPT;
            baseline = true;
        } else if (header.body_length == FEED_NOTICE_BYTES &&
                   body[0] == FEED_RECORD_VERSION &&
                   body[1] == FEED_NOTICE_RECORD) {
            lx_programs_state_notice notice;
            status = decode_notice(body, header.body_length, &notice);
            if (status == LXP_OK &&
                (!baseline || notice.global_sequence != expected_sequence ||
                 notice.global_sequence != header.global_sequence ||
                 notice.ordinal != expected_ordinal))
                status = LXP_ERR_LOG_CORRUPT;
            if (status == LXP_OK) {
                if (!group_open) {
                    group_open = true;
                    group_sequence = notice.global_sequence;
                    (void)memcpy(group_receipt_digest,
                                 notice.receipt_digest, 32U);
                    group_start = count;
                } else if (group_sequence != notice.global_sequence ||
                           lxp_ct_memcmp(group_receipt_digest,
                                         notice.receipt_digest, 32U) != 0) {
                    status = LXP_ERR_LOG_CORRUPT;
                    break;
                }
                if (expected_ordinal == UINT32_MAX) {
                    status = LXP_ERR_LOG_CORRUPT;
                    break;
                }
                ++expected_ordinal;
                if (notice.global_sequence > after_sequence) {
                    if (count == maximum)
                        group_overflow = true;
                    else if (!group_overflow)
                        notices[count++] = notice;
                }
            }
        } else if (header.body_length == FEED_HEAD_BYTES &&
                   body[0] == FEED_RECORD_VERSION &&
                   body[1] == FEED_HEAD_RECORD) {
            uint64_t sequence = read_u64(body + 2U);
            if (!baseline || sequence != expected_sequence ||
                sequence != header.global_sequence ||
                (group_open && group_sequence != sequence) ||
                (group_open &&
                 lxp_ct_memcmp(group_receipt_digest, body + 10U, 32U) != 0) ||
                lxp_ct_is_zero(body + 10U, 32U) ||
                lxp_ct_is_zero(body + 42U, 32U) ||
                read_u64(body + 74U) == 0U ||
                lxp_ct_is_zero(body + 82U, 32U)) {
                status = LXP_ERR_LOG_CORRUPT;
                break;
            }
            if (sequence > after_sequence) {
                if (group_overflow) {
                    count = group_start;
                    stopped_at_capacity = true;
                    break;
                }
                complete = sequence;
            }
            if (sequence == UINT64_MAX) {
                status = LXP_ERR_LOG_CORRUPT;
                break;
            }
            expected_sequence = sequence + 1U;
            expected_ordinal = 0U;
            disk_scanned_through = sequence;
            group_sequence = 0U;
            (void)memset(group_receipt_digest, 0,
                         sizeof(group_receipt_digest));
            group_open = false;
            group_overflow = false;
        } else {
            status = LXP_ERR_LOG_CORRUPT;
        }
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    if (status == LXP_OK && !stopped_at_capacity &&
        offset == store->log->write_offset) {
        if (!baseline || group_open)
            status = group_open ? LXP_ERR_PROJECTION_STALE :
                                  LXP_ERR_LOG_CORRUPT;
        else if (disk_scanned_through != store->scanned_through_sequence)
            status = LXP_ERR_LOG_CORRUPT;
        else
            complete = store->scanned_through_sequence;
    }
    if (status == LXP_OK) {
        *notice_count = count;
        *complete_through = complete;
        *scanned_through = store->scanned_through_sequence;
    }
    {
        lxp_result unlock_status = store_unlock((void *)store);
        return status == LXP_OK ? unlock_status : status;
    }
}
