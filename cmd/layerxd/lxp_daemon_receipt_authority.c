#include "layerx/lxp_daemon.h"

#include "layerx/lxp_crypto.h"

#include <stdlib.h>
#include <string.h>

enum {
    AUTHORITY_VERSION = 1,
    AUTHORITY_FIXED_BYTES = 5 + 32 + 32 + 8 + 2 + 64 + 1 + 4 + 4 + 4
};

static const uint8_t authority_magic[5] = {'L', 'X', 'B', 'E', '1'};

static uint16_t read_u16(const uint8_t bytes[2])
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t read_u32(const uint8_t bytes[4])
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t read_u64(const uint8_t bytes[8])
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index) value = (value << 8U) | bytes[index];
    return value;
}

static void write_u16(uint8_t bytes[2], uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
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

static lxp_result validate_evidence(
    const lxp_daemon_receipt_authority_store *store,
    const uint8_t *receipt_bytes, size_t receipt_length,
    const uint8_t *header_bytes, size_t header_length,
    const uint8_t header_signature[64], const lxp_merkle_proof *proof,
    lxp_arena *arena, lxp_receipt *receipt, lxp_batch_header *header,
    uint8_t receipt_digest[32])
{
    uint8_t leaf[32];
    size_t mark;
    lxp_result status;
    if (store == NULL || receipt_bytes == NULL || header_bytes == NULL ||
        header_signature == NULL || proof == NULL || arena == NULL ||
        receipt == NULL || header == NULL || receipt_digest == NULL ||
        receipt_length == 0U || receipt_length > LXP_MAX_ACTIVITY_BYTES ||
        header_length != LXP_BATCH_HEADER_ENCODED_SIZE)
        return LXP_ERR_NON_CANONICAL;
    mark = lxp_arena_mark(arena);
    status = lxp_receipt_decode(receipt_bytes, receipt_length, true, receipt);
    if (status == LXP_OK)
        status = lxp_receipt_verify(receipt,
                                    store->authorization.public_key, arena);
    if (status == LXP_OK)
        status = lxp_receipt_digest(receipt, arena, receipt_digest);
    if (status == LXP_OK)
        status = lxp_batch_header_decode(header_bytes, header_length, header);
    if (status == LXP_OK)
        status = lxp_batch_verify_signature(
            header, header_signature, 64U, &store->authorization, arena);
    if (status == LXP_OK)
        status = lxp_merkle_leaf_hash(receipt_bytes, receipt_length, leaf);
    if (status == LXP_OK)
        status = lxp_merkle_proof_verify(
            leaf, proof, header->receipt_merkle_root);
    if (status == LXP_OK &&
        (header->first_sequence == 0U ||
         header->last_sequence < header->first_sequence ||
         header->last_sequence - header->first_sequence >= UINT32_MAX ||
         receipt->global_sequence < header->first_sequence ||
         receipt->global_sequence > header->last_sequence ||
         receipt->protocol_version != header->protocol_version ||
         receipt->timestamp != header->timestamp_ms ||
         proof->leaf_index !=
             receipt->global_sequence - header->first_sequence ||
         proof->leaf_count !=
             header->last_sequence - header->first_sequence + 1U ||
         (receipt->global_sequence == header->first_sequence &&
          lxp_ct_memcmp(receipt->previous_state_root,
                        header->previous_state_root, 32U) != 0) ||
         (receipt->global_sequence == header->last_sequence &&
          lxp_ct_memcmp(receipt->resulting_state_root,
                        header->resulting_state_root, 32U) != 0) ||
         lxp_ct_is_zero(receipt->batch_id, 32U)))
        status = LXP_ERR_ROOT_MISMATCH;
    (void)lxp_arena_reset(arena, mark);
    return status;
}

static lxp_result decode_body(
    const uint8_t *body, size_t length, lxp_daemon_receipt_evidence *evidence)
{
    uint16_t header_length;
    uint32_t receipt_length;
    uint32_t leaf_index;
    uint32_t leaf_count;
    uint8_t depth;
    size_t proof_bytes;
    size_t offset = 0U;
    if (body == NULL || evidence == NULL || length < AUTHORITY_FIXED_BYTES ||
        memcmp(body, authority_magic, sizeof(authority_magic)) != 0)
        return LXP_ERR_LOG_CORRUPT;
    (void)memset(evidence, 0, sizeof(*evidence));
    offset += sizeof(authority_magic);
    (void)memcpy(evidence->receipt_digest, body + offset, 32U); offset += 32U;
    (void)memcpy(evidence->batch_id, body + offset, 32U); offset += 32U;
    evidence->global_sequence = read_u64(body + offset); offset += 8U;
    header_length = read_u16(body + offset); offset += 2U;
    if (header_length != LXP_BATCH_HEADER_ENCODED_SIZE ||
        header_length > length - offset)
        return LXP_ERR_LOG_CORRUPT;
    evidence->canonical_header =
        (lxp_byte_span){body + offset, header_length};
    offset += header_length;
    if (64U > length - offset) return LXP_ERR_LOG_CORRUPT;
    (void)memcpy(evidence->header_signature, body + offset, 64U); offset += 64U;
    if (9U > length - offset) return LXP_ERR_LOG_CORRUPT;
    depth = body[offset++];
    leaf_index = read_u32(body + offset); offset += 4U;
    leaf_count = read_u32(body + offset); offset += 4U;
    if (depth > LXP_MERKLE_MAX_DEPTH) return LXP_ERR_LOG_CORRUPT;
    proof_bytes = (size_t)depth * 32U;
    if (proof_bytes + 4U > length - offset) return LXP_ERR_LOG_CORRUPT;
    evidence->receipt_proof.depth = depth;
    evidence->receipt_proof.leaf_index = leaf_index;
    evidence->receipt_proof.leaf_count = leaf_count;
    (void)memcpy(evidence->receipt_proof.siblings, body + offset, proof_bytes);
    offset += proof_bytes;
    receipt_length = read_u32(body + offset); offset += 4U;
    if (receipt_length == 0U || receipt_length != length - offset)
        return LXP_ERR_LOG_CORRUPT;
    evidence->canonical_receipt =
        (lxp_byte_span){body + offset, receipt_length};
    return LXP_OK;
}

static lxp_result cache_insert(
    lxp_daemon_receipt_authority_store *store,
    const lxp_daemon_receipt_evidence *evidence,
    uint64_t record_offset, uint32_t body_length)
{
    size_t index;
    size_t slot;
    for (index = 0U; index < store->cache_count; ++index) {
        const lxp_daemon_receipt_authority_entry *entry = &store->cache[index];
        if (lxp_ct_memcmp(entry->receipt_digest,
                          evidence->receipt_digest, 32U) == 0)
            return entry->global_sequence == evidence->global_sequence &&
                           lxp_ct_memcmp(entry->batch_id,
                           evidence->batch_id, 32U) == 0 ?
                   LXP_OK : LXP_FATAL_INVARIANT;
    }
    slot = store->cache_count < LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS ?
               store->cache_count++ : store->cache_next;
    if (store->cache_count == LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS)
        store->cache_next =
            (slot + 1U) % LXP_DAEMON_AUTHORITY_CACHE_RECEIPTS;
    (void)memcpy(store->cache[slot].receipt_digest,
                 evidence->receipt_digest, 32U);
    (void)memcpy(store->cache[slot].batch_id,
                 evidence->batch_id, 32U);
    store->cache[slot].global_sequence = evidence->global_sequence;
    store->cache[slot].record_offset = record_offset;
    store->cache[slot].body_length = body_length;
    return LXP_OK;
}

static lxp_result replay_authority(void *context,
                                   const lxp_log_record_header *header,
                                   const uint8_t *body)
{
    lxp_daemon_receipt_authority_store *store =
        (lxp_daemon_receipt_authority_store *)context;
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header batch;
    bool same_batch;
    if (store == NULL || header == NULL || body == NULL ||
        header->record_kind != (uint8_t)LXP_LOG_STATE_DIFF)
        return LXP_ERR_LOG_CORRUPT;
    uint64_t record_offset;
    lxp_result status;
    if (decode_body(body, header->body_length, &evidence) != LXP_OK ||
        lxp_batch_header_decode(evidence.canonical_header.bytes,
                                evidence.canonical_header.length,
                                &batch) != LXP_OK ||
        evidence.global_sequence != header->global_sequence ||
        evidence.global_sequence < batch.first_sequence ||
        evidence.global_sequence > batch.last_sequence)
        return LXP_ERR_LOG_CORRUPT;
    same_batch = store->record_count != 0U &&
                 batch.batch_number == store->last_batch_number;
    if ((store->record_count == 0U &&
         evidence.global_sequence != batch.first_sequence) ||
        (store->record_count != 0U &&
         (store->last_global_sequence == UINT64_MAX ||
          evidence.global_sequence != store->last_global_sequence + 1U)) ||
        (same_batch &&
         (evidence.canonical_header.length !=
              sizeof(store->active_canonical_header) ||
          lxp_ct_memcmp(evidence.canonical_header.bytes,
                        store->active_canonical_header,
                        sizeof(store->active_canonical_header)) != 0 ||
          lxp_ct_memcmp(evidence.header_signature,
                        store->active_header_signature, 64U) != 0 ||
          evidence.global_sequence > store->active_batch_last_sequence)) ||
        (store->record_count != 0U && !same_batch &&
         (store->last_global_sequence != store->active_batch_last_sequence ||
          store->last_batch_number == UINT64_MAX ||
          batch.batch_number != store->last_batch_number + 1U ||
          evidence.global_sequence != batch.first_sequence)))
        return LXP_ERR_LOG_CORRUPT;
    record_offset = store->replay_offset;
    store->replay_offset += LXP_LOG_HEADER_BYTES + header->body_length;
    status = cache_insert(store, &evidence, record_offset,
                          header->body_length);
    if (status == LXP_OK) {
        if (!same_batch) {
            (void)memcpy(store->active_canonical_header,
                         evidence.canonical_header.bytes,
                         sizeof(store->active_canonical_header));
            (void)memcpy(store->active_header_signature,
                         evidence.header_signature, 64U);
            store->active_batch_last_sequence = batch.last_sequence;
        }
        ++store->record_count;
        store->last_global_sequence = evidence.global_sequence;
        store->last_batch_number = batch.batch_number;
    }
    return status;
}

lxp_result lxp_daemon_receipt_authority_open(
    lxp_daemon_receipt_authority_store *store, lxp_log *log,
    const lxp_sequencer_authorization *authorization)
{
    if (store == NULL || log == NULL || authorization == NULL ||
        authorization->authorized == 0U ||
        authorization->first_batch_number == 0U ||
        authorization->last_batch_number < authorization->first_batch_number ||
        lxp_ct_is_zero(authorization->sequencer_id, 32U) ||
        lxp_ct_is_zero(authorization->public_key, 32U))
        return LXP_ERR_NON_CANONICAL;
    (void)memset(store, 0, sizeof(*store));
    store->log = log;
    store->authorization = *authorization;
    {
        lxp_result status = lxp_log_recover_complete_records(
            log, replay_authority, store);
        store->replay_offset = 0U;
        return status;
    }
}

lxp_result lxp_daemon_receipt_authority_append(
    lxp_daemon_receipt_authority_store *store,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof, lxp_arena *arena)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_receipt receipt;
    lxp_batch_header header;
    uint8_t *body;
    uint64_t record_offset;
    size_t body_length;
    size_t offset = 0U;
    lxp_result status;
    if (store == NULL || store->log == NULL || receipt_proof == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = validate_evidence(
        store, canonical_receipt, receipt_length, canonical_header,
        header_length, header_signature, receipt_proof, arena, &receipt,
        &header, evidence.receipt_digest);
    if (status != LXP_OK) return status;
    (void)memcpy(evidence.batch_id, receipt.batch_id, 32U);
    evidence.global_sequence = receipt.global_sequence;
    {
        lxp_daemon_receipt_evidence existing;
        size_t mark = lxp_arena_mark(arena);
        status = lxp_daemon_receipt_authority_lookup(
            store, evidence.receipt_digest, arena, &existing);
        if (status == LXP_OK &&
            (existing.canonical_receipt.length != receipt_length ||
             lxp_ct_memcmp(existing.canonical_receipt.bytes,
                           canonical_receipt, receipt_length) != 0 ||
             existing.canonical_header.length != header_length ||
             lxp_ct_memcmp(existing.canonical_header.bytes,
                           canonical_header, header_length) != 0 ||
             lxp_ct_memcmp(existing.header_signature,
                           header_signature, 64U) != 0 ||
             existing.receipt_proof.depth != receipt_proof->depth ||
             existing.receipt_proof.leaf_index !=
                 receipt_proof->leaf_index ||
             existing.receipt_proof.leaf_count !=
                 receipt_proof->leaf_count ||
             lxp_ct_memcmp(existing.receipt_proof.siblings,
                           receipt_proof->siblings,
                           (size_t)receipt_proof->depth * 32U) != 0))
            status = LXP_FATAL_INVARIANT;
        (void)lxp_arena_reset(arena, mark);
        if (status == LXP_OK) return LXP_OK;
        if (status != LXP_ERR_UNKNOWN_ACTIVITY) return status;
    }
    if ((store->record_count != 0U &&
         (receipt.global_sequence != store->last_global_sequence + 1U ||
          (header.batch_number == store->last_batch_number ?
               (canonical_header == NULL ||
                header_length != sizeof(store->active_canonical_header) ||
                lxp_ct_memcmp(canonical_header,
                              store->active_canonical_header,
                              sizeof(store->active_canonical_header)) != 0 ||
                lxp_ct_memcmp(header_signature,
                              store->active_header_signature, 64U) != 0 ||
                receipt.global_sequence >
                    store->active_batch_last_sequence) :
               (store->last_global_sequence !=
                    store->active_batch_last_sequence ||
                store->last_batch_number == UINT64_MAX ||
                header.batch_number != store->last_batch_number + 1U ||
                receipt.global_sequence != header.first_sequence)))) ||
        (store->record_count == 0U &&
         (receipt.global_sequence == 0U || header.batch_number == 0U ||
          receipt.global_sequence != header.first_sequence)))
        return LXP_ERR_SEQUENCE_GAP;
    body_length = AUTHORITY_FIXED_BYTES + header_length +
                  (size_t)receipt_proof->depth * 32U + receipt_length;
    if (body_length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    body = (uint8_t *)malloc(body_length);
    if (body == NULL) return LXP_ERR_IO;
    (void)memcpy(body + offset, authority_magic, sizeof(authority_magic));
    offset += sizeof(authority_magic);
    (void)memcpy(body + offset, evidence.receipt_digest, 32U); offset += 32U;
    (void)memcpy(body + offset, evidence.batch_id, 32U); offset += 32U;
    write_u64(body + offset, evidence.global_sequence); offset += 8U;
    write_u16(body + offset, (uint16_t)header_length); offset += 2U;
    (void)memcpy(body + offset, canonical_header, header_length);
    offset += header_length;
    (void)memcpy(body + offset, header_signature, 64U); offset += 64U;
    body[offset++] = receipt_proof->depth;
    write_u32(body + offset, receipt_proof->leaf_index); offset += 4U;
    write_u32(body + offset, receipt_proof->leaf_count); offset += 4U;
    (void)memcpy(body + offset, receipt_proof->siblings,
                 (size_t)receipt_proof->depth * 32U);
    offset += (size_t)receipt_proof->depth * 32U;
    write_u32(body + offset, (uint32_t)receipt_length); offset += 4U;
    (void)memcpy(body + offset, canonical_receipt, receipt_length);
    offset += receipt_length;
    status = offset == body_length ?
        lxp_log_append(store->log, LXP_LOG_STATE_DIFF,
                       receipt.global_sequence, body, (uint32_t)body_length,
                       &record_offset) : LXP_FATAL_INVARIANT;
    if (status == LXP_OK) status = lxp_log_write_boundary(store->log);
    if (status == LXP_OK) {
        evidence.canonical_receipt =
            (lxp_byte_span){canonical_receipt, receipt_length};
        evidence.canonical_header =
            (lxp_byte_span){canonical_header, header_length};
        evidence.receipt_proof = *receipt_proof;
        (void)memcpy(evidence.header_signature, header_signature, 64U);
        status = cache_insert(store, &evidence, record_offset,
                              (uint32_t)body_length);
    }
    if (status == LXP_OK) {
        if (store->record_count == 0U ||
            header.batch_number != store->last_batch_number) {
            (void)memcpy(store->active_canonical_header, canonical_header,
                         sizeof(store->active_canonical_header));
            (void)memcpy(store->active_header_signature, header_signature,
                         64U);
            store->active_batch_last_sequence = header.last_sequence;
        }
        ++store->record_count;
        store->last_global_sequence = receipt.global_sequence;
        store->last_batch_number = header.batch_number;
    }
    free(body);
    return status;
}

lxp_result lxp_daemon_receipt_authority_lookup(
    const lxp_daemon_receipt_authority_store *store,
    const uint8_t receipt_digest[32], lxp_arena *arena,
    lxp_daemon_receipt_evidence *evidence)
{
    size_t index;
    if (store == NULL || store->log == NULL || receipt_digest == NULL ||
        arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < store->cache_count; ++index) {
        const lxp_daemon_receipt_authority_entry *entry = &store->cache[index];
        if (lxp_ct_memcmp(entry->receipt_digest, receipt_digest, 32U) == 0) {
            lxp_log_record_header header;
            void *body = NULL;
            lxp_result status = lxp_arena_alloc(
                arena, entry->body_length, 1U, &body);
            if (status == LXP_OK)
                status = lxp_log_read(store->log, entry->record_offset,
                                      &header, body, entry->body_length);
            if (status == LXP_OK)
                status = decode_body((const uint8_t *)body,
                                     entry->body_length, evidence);
            if (status == LXP_OK) {
                lxp_receipt receipt;
                lxp_batch_header batch;
                uint8_t digest[32];
                status = validate_evidence(
                    store, evidence->canonical_receipt.bytes,
                    evidence->canonical_receipt.length,
                    evidence->canonical_header.bytes,
                    evidence->canonical_header.length,
                    evidence->header_signature, &evidence->receipt_proof,
                    arena, &receipt, &batch, digest);
                if (status == LXP_OK &&
                    (lxp_ct_memcmp(digest,
                                   evidence->receipt_digest, 32U) != 0 ||
                     lxp_ct_memcmp(receipt.batch_id,
                                   evidence->batch_id, 32U) != 0 ||
                     receipt.global_sequence != evidence->global_sequence))
                    status = LXP_ERR_LOG_CORRUPT;
            }
            if (status == LXP_OK &&
                lxp_ct_memcmp(evidence->receipt_digest,
                              receipt_digest, 32U) != 0)
                status = LXP_ERR_LOG_CORRUPT;
            return status;
        }
    }
    {
        uint64_t offset = 0U;
        bool present = false;
        lxp_result status = LXP_OK;
        while (status == LXP_OK) {
            size_t mark = lxp_arena_mark(arena);
            status = lxp_daemon_receipt_authority_scan(
                store, &offset, arena, evidence, &present);
            if (status != LXP_OK || !present ||
                lxp_ct_memcmp(evidence->receipt_digest,
                              receipt_digest, 32U) == 0)
                return status != LXP_OK ? status :
                       present ? LXP_OK : LXP_ERR_UNKNOWN_ACTIVITY;
            (void)lxp_arena_reset(arena, mark);
        }
        return status;
    }
}

lxp_result lxp_daemon_receipt_authority_scan(
    const lxp_daemon_receipt_authority_store *store, uint64_t *record_offset,
    lxp_arena *arena, lxp_daemon_receipt_evidence *evidence,
    bool *present)
{
    lxp_log_record_header record;
    lxp_receipt receipt;
    lxp_batch_header batch;
    uint8_t digest[32];
    void *body = NULL;
    lxp_result status;
    if (store == NULL || store->log == NULL || record_offset == NULL ||
        arena == NULL || evidence == NULL || present == NULL ||
        *record_offset > store->log->write_offset)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    if (*record_offset == store->log->write_offset) return LXP_OK;
    status = lxp_log_read(store->log, *record_offset, &record, NULL, 0U);
    if (status != LXP_OK && status != LXP_ERR_LENGTH_LIMIT) return status;
    if (record.record_kind != (uint8_t)LXP_LOG_STATE_DIFF ||
        record.body_length < AUTHORITY_FIXED_BYTES)
        return LXP_ERR_LOG_CORRUPT;
    status = lxp_arena_alloc(arena, record.body_length, 1U, &body);
    if (status == LXP_OK)
        status = lxp_log_read(store->log, *record_offset, &record, body,
                              record.body_length);
    if (status == LXP_OK)
        status = decode_body((const uint8_t *)body, record.body_length,
                             evidence);
    if (status == LXP_OK)
        status = validate_evidence(
            store, evidence->canonical_receipt.bytes,
            evidence->canonical_receipt.length,
            evidence->canonical_header.bytes,
            evidence->canonical_header.length, evidence->header_signature,
            &evidence->receipt_proof, arena, &receipt, &batch, digest);
    if (status == LXP_OK &&
        (record.global_sequence != evidence->global_sequence ||
         lxp_ct_memcmp(digest, evidence->receipt_digest, 32U) != 0 ||
         lxp_ct_memcmp(receipt.batch_id, evidence->batch_id, 32U) != 0))
        status = LXP_ERR_LOG_CORRUPT;
    if (status != LXP_OK) return status;
    if ((uint64_t)record.body_length > UINT64_MAX - LXP_LOG_HEADER_BYTES ||
        *record_offset > UINT64_MAX - LXP_LOG_HEADER_BYTES -
                             (uint64_t)record.body_length)
        return LXP_ERR_OVERFLOW;
    *record_offset += LXP_LOG_HEADER_BYTES + record.body_length;
    *present = true;
    return LXP_OK;
}
