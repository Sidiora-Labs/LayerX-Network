#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"

#include "layerx/lxp_crypto.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum { PROTOCOL_RESPONSE_MAX_BYTES = 64 * 1024 * 1024 };

typedef struct json_writer {
    char *bytes;
    size_t length;
    size_t capacity;
    lxp_result status;
} json_writer;

static void json_reserve(json_writer *writer, size_t additional)
{
    size_t required;
    size_t capacity;
    char *bytes;
    if (writer->status != LXP_OK) return;
    if (additional > PROTOCOL_RESPONSE_MAX_BYTES - writer->length) {
        writer->status = LXP_ERR_LENGTH_LIMIT;
        return;
    }
    required = writer->length + additional;
    if (required <= writer->capacity) return;
    capacity = writer->capacity == 0U ? 4096U : writer->capacity;
    while (capacity < required && capacity <= PROTOCOL_RESPONSE_MAX_BYTES / 2U)
        capacity *= 2U;
    if (capacity < required) capacity = PROTOCOL_RESPONSE_MAX_BYTES;
    bytes = (char *)realloc(writer->bytes, capacity);
    if (bytes == NULL) {
        writer->status = LXP_ERR_IO;
        return;
    }
    writer->bytes = bytes;
    writer->capacity = capacity;
}

static void json_raw(json_writer *writer, const char *bytes, size_t length)
{
    json_reserve(writer, length);
    if (writer->status != LXP_OK) return;
    (void)memcpy(writer->bytes + writer->length, bytes, length);
    writer->length += length;
}

static void json_text(json_writer *writer, const char *text)
{
    json_raw(writer, text, strlen(text));
}

static void json_format(json_writer *writer, const char *format, ...)
{
    char local[128];
    va_list arguments;
    int length;
    va_start(arguments, format);
    length = vsnprintf(local, sizeof(local), format, arguments);
    va_end(arguments);
    if (length < 0 || (size_t)length >= sizeof(local)) {
        writer->status = LXP_ERR_LENGTH_LIMIT;
        return;
    }
    json_raw(writer, local, (size_t)length);
}

static void json_hex(json_writer *writer, const uint8_t *bytes, size_t length)
{
    static const char alphabet[] = "0123456789abcdef";
    size_t index;
    if ((bytes == NULL && length != 0U) ||
        length > PROTOCOL_RESPONSE_MAX_BYTES / 2U) {
        writer->status = length > PROTOCOL_RESPONSE_MAX_BYTES / 2U ?
                         LXP_ERR_LENGTH_LIMIT : LXP_ERR_NON_CANONICAL;
        return;
    }
    json_reserve(writer, length * 2U);
    if (writer->status != LXP_OK) return;
    for (index = 0U; index < length; ++index) {
        writer->bytes[writer->length++] = alphabet[bytes[index] >> 4U];
        writer->bytes[writer->length++] = alphabet[bytes[index] & 15U];
    }
}

static int hex_nibble(char value)
{
    if (value >= '0' && value <= '9') return value - '0';
    if (value >= 'a' && value <= 'f') return value - 'a' + 10;
    if (value >= 'A' && value <= 'F') return value - 'A' + 10;
    return -1;
}

static lxp_result parse_hex32(const char *text, uint8_t output[32])
{
    size_t index;
    if (text == NULL || strlen(text) != 64U) return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < 32U; ++index) {
        int high = hex_nibble(text[index * 2U]);
        int low = hex_nibble(text[index * 2U + 1U]);
        if (high < 0 || low < 0) return LXP_ERR_NON_CANONICAL;
        output[index] = (uint8_t)((unsigned int)high << 4U |
                                 (unsigned int)low);
    }
    return lxp_ct_is_zero(output, 32U) ?
           LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result parse_u64(const char *text, uint64_t *value)
{
    uint64_t parsed = 0U;
    if (text == NULL || value == NULL || *text == '\0')
        return LXP_ERR_NON_CANONICAL;
    while (*text != '\0') {
        unsigned int digit;
        if (*text < '0' || *text > '9') return LXP_ERR_NON_CANONICAL;
        digit = (unsigned int)(*text - '0');
        if (parsed > (UINT64_MAX - digit) / 10U)
            return LXP_ERR_LENGTH_LIMIT;
        parsed = parsed * 10U + digit;
        ++text;
    }
    *value = parsed;
    return LXP_OK;
}

static bool authorized(const lxp_daemon_protocol_owner *owner,
                       const uint8_t *token, size_t token_length)
{
    return owner != NULL && token != NULL &&
           token_length == owner->bearer_token_length &&
           lxp_ct_memcmp(token, owner->bearer_token, token_length) == 0;
}

static lxp_result durable_receipt_facts(
    void *context, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts)
{
    lxp_daemon_protocol_owner *owner =
        (lxp_daemon_protocol_owner *)context;
    lxp_daemon_receipt_evidence evidence;
    lxp_receipt receipt;
    size_t mark;
    lxp_result status;
    if (owner == NULL || receipt_digest == NULL || facts == NULL ||
        owner->receipt_authority == NULL || owner->scratch == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&owner->mutex) != 0) return LXP_ERR_IO;
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, receipt_digest, owner->scratch,
        &evidence);
    if (status == LXP_OK)
        status = lxp_receipt_decode(
            evidence.canonical_receipt.bytes,
            evidence.canonical_receipt.length, true, &receipt);
    if (status == LXP_OK) {
        (void)memset(facts, 0, sizeof(*facts));
        (void)memcpy(facts->receipt_digest, receipt_digest, 32U);
        facts->result_code = receipt.result_code;
        facts->global_sequence = receipt.global_sequence;
        facts->timestamp = receipt.timestamp;
        (void)memcpy(facts->asset, receipt.asset, 32U);
        facts->amount = receipt.amount;
        (void)memcpy(facts->resulting_state_root,
                     receipt.resulting_state_root, 32U);
    }
    (void)lxp_arena_reset(owner->scratch, mark);
    if (pthread_mutex_unlock(&owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result evidence_for_head(
    lxp_daemon_protocol_owner *owner, lxp_arena *arena,
    lxp_daemon_receipt_evidence *evidence)
{
    if (owner->feed_store.scanned_through_sequence == 0U ||
        lxp_ct_is_zero(owner->feed_store.head_receipt_digest, 32U) ||
        lxp_ct_memcmp(owner->feed_store.head_state_root,
                      owner->kernel->current_state_root, 32U) != 0)
        return LXP_ERR_PROJECTION_STALE;
    return lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, owner->feed_store.head_receipt_digest,
        arena, evidence);
}

static void put_batch_evidence(json_writer *writer,
                               const lxp_daemon_receipt_evidence *evidence,
                               lxp_arena *arena)
{
    lxp_codec_writer proof_writer;
    size_t mark = lxp_arena_mark(arena);
    lxp_result status = lxp_codec_writer_init(
        &proof_writer, arena, 16U + LXP_MERKLE_MAX_DEPTH * 32U);
    if (status == LXP_OK)
        status = lxp_merkle_proof_encode(
            &proof_writer, &evidence->receipt_proof);
    if (status != LXP_OK) {
        writer->status = status;
        (void)lxp_arena_reset(arena, mark);
        return;
    }
    json_text(writer, "{\"header_hex\":\"");
    json_hex(writer, evidence->canonical_header.bytes,
             evidence->canonical_header.length);
    json_text(writer, "\",\"header_signature\":\"");
    json_hex(writer, evidence->header_signature, 64U);
    json_text(writer, "\",\"receipt_proof_hex\":\"");
    json_hex(writer, proof_writer.bytes, proof_writer.length);
    json_text(writer, "\"}");
    (void)lxp_arena_reset(arena, mark);
}

static lxp_result put_receipt_document(
    lxp_daemon_protocol_owner *owner,
    const lxp_daemon_receipt_evidence *evidence, bool current,
    lxp_arena *arena, json_writer *writer)
{
    lxp_receipt receipt;
    lxp_result status = lxp_receipt_decode(
        evidence->canonical_receipt.bytes,
        evidence->canonical_receipt.length, true, &receipt);
    if (status != LXP_OK) return status;
    json_text(writer, "{\"current\":");
    json_text(writer, current ? "true" : "false");
    json_text(writer, ",\"receipt_hex\":\"");
    json_hex(writer, evidence->canonical_receipt.bytes,
             evidence->canonical_receipt.length);
    json_text(writer, "\",\"receipt_digest\":\"");
    json_hex(writer, evidence->receipt_digest, 32U);
    json_text(writer, "\",\"state_root\":\"");
    json_hex(writer, receipt.resulting_state_root, 32U);
    json_format(writer,
                "\",\"observed_sequence\":%llu,\"observed_at\":%llu,"
                "\"batch_evidence\":",
                (unsigned long long)receipt.global_sequence,
                (unsigned long long)receipt.timestamp);
    put_batch_evidence(writer, evidence, arena);
    json_text(writer, "}");
    (void)owner;
    return writer->status;
}

static lxp_result receipt_route(lxp_daemon_protocol_owner *owner,
                                const uint8_t digest[32], lxp_arena *arena,
                                json_writer *writer)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_result status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, digest, arena, &evidence);
    bool current;
    if (status != LXP_OK) return status;
    current = lxp_ct_memcmp(digest,
                            owner->feed_store.head_receipt_digest, 32U) == 0;
    return put_receipt_document(owner, &evidence, current, arena, writer);
}

static lxp_result head_route(lxp_daemon_protocol_owner *owner,
                             lxp_arena *arena, json_writer *writer)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_result status = evidence_for_head(owner, arena, &evidence);
    if (status != LXP_OK) return status;
    return put_receipt_document(owner, &evidence, true, arena, writer);
}

static lxp_result program_route(lxp_daemon_protocol_owner *owner,
                                const uint8_t program_id[32], uint64_t at,
                                lxp_arena *arena, json_writer *writer)
{
    lxp_module_ctx context;
    lxp_byte_span record;
    uint8_t digest[32];
    size_t mark;
    lxp_result status;
    if (at != owner->feed_store.scanned_through_sequence)
        return LXP_ERR_PROJECTION_STALE;
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_module_ctx_init(
        &context, owner->kernel, LXP_MODULE_PROGRAMS,
        owner->feed_store.head_timestamp, owner->kernel->epoch,
        owner->feed_store.scanned_through_sequence, UINT64_MAX,
        owner->scratch, false);
    if (status == LXP_OK) {
        context.verified_receipts = owner->verified_receipts;
        status = lxp_programs_state_record_encode(
            &context, program_id, owner->feed_store.head_receipt_digest,
            owner->scratch, &record);
    }
    if (status == LXP_OK) status = lxp_hash_sha256(record.bytes, record.length, digest);
    if (status == LXP_OK) {
        json_text(writer, "{\"program_id\":\"");
        json_hex(writer, program_id, 32U);
        json_text(writer, "\",\"record_hex\":\"");
        json_hex(writer, record.bytes, record.length);
        json_text(writer, "\",\"record_digest\":\"");
        json_hex(writer, digest, 32U);
        json_text(writer, "\",\"receipt_digest\":\"");
        json_hex(writer, owner->feed_store.head_receipt_digest, 32U);
        json_text(writer, "\"}");
        status = writer->status;
    }
    (void)lxp_arena_reset(owner->scratch, mark);
    (void)arena;
    return status;
}

static lxp_result changes_route(lxp_daemon_protocol_owner *owner,
                                uint64_t after, json_writer *writer)
{
    lx_programs_state_notice *notices;
    size_t count = 0U;
    size_t index;
    uint64_t complete = 0U;
    uint64_t scanned = 0U;
    lxp_result status;
    notices = (lx_programs_state_notice *)malloc(
        LX_PROGRAMS_STATE_FEED_MAX_NOTICES * sizeof(*notices));
    if (notices == NULL) return LXP_ERR_IO;
    status = lxp_programs_state_feed_store_page(
        &owner->feed_store, after, LX_PROGRAMS_STATE_FEED_MAX_NOTICES,
        notices, &count, &complete, &scanned);
    if (status == LXP_OK) {
        json_text(writer, "{\"records\":[");
        for (index = 0U; index < count; ++index) {
            if (index != 0U) json_text(writer, ",");
            json_format(writer,
                        "{\"sequence\":%llu,\"ordinal\":%u,"
                        "\"program_id\":\"",
                        (unsigned long long)notices[index].global_sequence,
                        notices[index].ordinal);
            json_hex(writer, notices[index].program_id, 32U);
            json_format(writer,
                        "\",\"activity_type\":%u,\"event_type\":%u,"
                        "\"receipt_digest\":\"",
                        notices[index].activity_type,
                        (unsigned int)notices[index].event_type);
            json_hex(writer, notices[index].receipt_digest, 32U);
            json_text(writer, "\"}");
        }
        json_format(writer,
                    "],\"complete_through\":{\"sequence\":%llu,"
                    "\"ordinal\":0},\"scanned_through_sequence\":%llu,"
                    "\"caught_up\":%s}",
                    (unsigned long long)complete,
                    (unsigned long long)scanned,
                    complete == scanned ? "true" : "false");
        status = writer->status;
    }
    free(notices);
    return status;
}

static lxp_result batch_route(lxp_daemon_protocol_owner *owner,
                              const uint8_t batch_id[32],
                              const uint8_t receipt_digest[32],
                              lxp_arena *arena, json_writer *writer)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_result status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, receipt_digest, arena, &evidence);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(evidence.batch_id, batch_id, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    json_text(writer, "{\"sequencer_public_key\":\"");
    json_hex(writer, owner->receipt_authority->authorization.public_key, 32U);
    json_text(writer, "\",\"batch_evidence\":");
    put_batch_evidence(writer, &evidence, arena);
    json_text(writer, "}");
    return writer->status;
}

static lxp_result route_inner(lxp_daemon_protocol_owner *owner,
                              const char *method, const char *path,
                              lxp_arena *arena, json_writer *writer)
{
    static const char receipt_prefix[] = "/v1/receipts/";
    static const char program_prefix[] = "/v1/programs/";
    static const char batch_prefix[] = "/v1/batches/";
    if (strcmp(method, "GET") != 0) return LXP_ERR_UNKNOWN_ACTIVITY;
    if (strcmp(path, "/v1/protocol/account-state/head") == 0)
        return head_route(owner, arena, writer);
    if (strncmp(path, receipt_prefix, sizeof(receipt_prefix) - 1U) == 0) {
        const char *suffix = path + sizeof(receipt_prefix) - 1U;
        const char *tail = strstr(suffix, "/account-state");
        char digest_text[65];
        uint8_t digest[32];
        if (tail == NULL || strcmp(tail, "/account-state") != 0 ||
            (size_t)(tail - suffix) != 64U)
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(digest_text, suffix, 64U); digest_text[64] = '\0';
        if (parse_hex32(digest_text, digest) != LXP_OK)
            return LXP_ERR_NON_CANONICAL;
        return receipt_route(owner, digest, arena, writer);
    }
    if (strncmp(path, program_prefix, sizeof(program_prefix) - 1U) == 0) {
        const char *suffix = path + sizeof(program_prefix) - 1U;
        if (strcmp(suffix, "account-state/changes") == 0)
            return LXP_ERR_NON_CANONICAL;
        if (strncmp(suffix, "account-state/changes?after_sequence=", 37U) == 0) {
            uint64_t after;
            lxp_result status = parse_u64(suffix + 37U, &after);
            return status == LXP_OK ? changes_route(owner, after, writer) : status;
        }
        {
            const char *tail = strstr(suffix, "/account-state?at=");
            char program_text[65];
            uint8_t program_id[32];
            uint64_t at;
            if (tail == NULL || (size_t)(tail - suffix) != 64U)
                return LXP_ERR_NON_CANONICAL;
            (void)memcpy(program_text, suffix, 64U); program_text[64] = '\0';
            if (parse_hex32(program_text, program_id) != LXP_OK ||
                parse_u64(tail + 18U, &at) != LXP_OK)
                return LXP_ERR_NON_CANONICAL;
            return program_route(owner, program_id, at, arena, writer);
        }
    }
    if (strncmp(path, batch_prefix, sizeof(batch_prefix) - 1U) == 0) {
        const char *suffix = path + sizeof(batch_prefix) - 1U;
        const char *tail = strstr(suffix, "/receipt-authority?receipt_digest=");
        char batch_text[65];
        uint8_t batch_id[32];
        uint8_t receipt_digest[32];
        if (tail == NULL || (size_t)(tail - suffix) != 64U ||
            strlen(tail + 34U) != 64U)
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(batch_text, suffix, 64U); batch_text[64] = '\0';
        if (parse_hex32(batch_text, batch_id) != LXP_OK ||
            parse_hex32(tail + 34U, receipt_digest) != LXP_OK)
            return LXP_ERR_NON_CANONICAL;
        return batch_route(owner, batch_id, receipt_digest, arena, writer);
    }
    return LXP_ERR_UNKNOWN_ACTIVITY;
}

lxp_result lxp_daemon_protocol_owner_attach(
    lxp_daemon_protocol_owner *owner, lxp_kernel *kernel,
    lx_programs_transfer_runtime *programs_runtime, lxp_log *feed_log,
    lxp_log *canonical_log, lxp_history *history,
    lxp_verified_receipt_index *verified_receipts,
    lxp_daemon_receipt_authority_store *receipt_authority,
    lxp_arena *scratch, lxp_daemon_protocol_replay_fn replay,
    void *replay_context, const uint8_t *bearer_token,
    size_t bearer_token_length)
{
    size_t index;
    lxp_result status;
    pthread_mutexattr_t mutex_attributes;
    bool mutex_initialized = false;
    if (owner == NULL || kernel == NULL || programs_runtime == NULL ||
        feed_log == NULL || canonical_log == NULL || history == NULL ||
        verified_receipts == NULL || receipt_authority == NULL ||
        replay == NULL || replay_context == NULL ||
        scratch == NULL || scratch->offset > scratch->capacity ||
        scratch->capacity - scratch->offset <
            LXP_DAEMON_PROTOCOL_SCRATCH_MIN_BYTES ||
        bearer_token == NULL || bearer_token_length < 32U ||
        bearer_token_length > LXP_DAEMON_BEARER_MAX_BYTES ||
        (kernel->module_runtime[LXP_MODULE_PROGRAMS] != NULL &&
         kernel->module_runtime[LXP_MODULE_PROGRAMS] != programs_runtime) ||
        history->log != canonical_log || receipt_authority->log == feed_log ||
        receipt_authority->log == canonical_log || feed_log == canonical_log)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < bearer_token_length; ++index)
        if (bearer_token[index] < 0x21U || bearer_token[index] > 0x7eU)
            return LXP_ERR_NON_CANONICAL;
    (void)memset(owner, 0, sizeof(*owner));
    owner->kernel = kernel;
    owner->programs_runtime = programs_runtime;
    owner->history = history;
    owner->verified_receipts = verified_receipts;
    owner->receipt_authority = receipt_authority;
    owner->scratch = scratch;
    owner->listener_descriptor = -1;
    (void)memcpy(owner->bearer_token, bearer_token, bearer_token_length);
    owner->bearer_token_length = bearer_token_length;
    if (pthread_mutexattr_init(&mutex_attributes) != 0)
        status = LXP_ERR_IO;
    else {
        status = pthread_mutexattr_settype(
            &mutex_attributes, PTHREAD_MUTEX_RECURSIVE) == 0 &&
            pthread_mutex_init(&owner->mutex, &mutex_attributes) == 0 ?
                LXP_OK : LXP_ERR_IO;
        (void)pthread_mutexattr_destroy(&mutex_attributes);
        mutex_initialized = status == LXP_OK;
    }
    if (status != LXP_OK) goto fail;
    status = lxp_verified_receipt_index_bind_fallback(
        verified_receipts, durable_receipt_facts, owner);
    if (status != LXP_OK) goto fail;
    status = lxp_programs_state_feed_store_open(
        &owner->feed_store, feed_log, canonical_log, history, scratch,
        &owner->mutex,
        kernel->state->next_sequence, kernel->current_state_root);
    if (status == LXP_OK) {
        programs_runtime->state_feed = &owner->feed_store.feed;
        status = lxp_kernel_bind_module_runtime(
            kernel, LXP_MODULE_PROGRAMS, programs_runtime);
    }
    if (status == LXP_OK) status = replay(replay_context, owner);
    if (status == LXP_OK)
        status = lxp_programs_state_feed_store_recover(
            &owner->feed_store, kernel);
    if (status == LXP_OK &&
        ((owner->feed_store.scanned_through_sequence == 0U &&
          (owner->feed_store.baseline_next_sequence !=
               kernel->state->next_sequence ||
           lxp_ct_memcmp(owner->feed_store.baseline_state_root,
                         kernel->current_state_root, 32U) != 0)) ||
         (owner->feed_store.scanned_through_sequence != 0U &&
          (owner->feed_store.scanned_through_sequence == UINT64_MAX ||
           owner->feed_store.scanned_through_sequence + 1U !=
               kernel->state->next_sequence ||
           lxp_ct_memcmp(owner->feed_store.head_state_root,
                         kernel->current_state_root, 32U) != 0))))
        status = LXP_ERR_PROJECTION_STALE;
    {
        uint64_t authority_offset = 0U;
        bool present = true;
        while (status == LXP_OK && present) {
            lxp_daemon_receipt_evidence evidence;
            lxp_receipt receipt;
            size_t mark = lxp_arena_mark(scratch);
            status = lxp_daemon_receipt_authority_scan(
                receipt_authority, &authority_offset, scratch,
                &evidence, &present);
            if (status == LXP_OK && present)
                status = lxp_receipt_decode(
                    evidence.canonical_receipt.bytes,
                    evidence.canonical_receipt.length, true, &receipt);
            if (status == LXP_OK && present)
                status = lxp_verified_receipt_index_add(
                    verified_receipts, &receipt,
                    receipt_authority->authorization.public_key, scratch);
            (void)lxp_arena_reset(scratch, mark);
        }
    }
    if (status == LXP_OK &&
        owner->feed_store.scanned_through_sequence != 0U) {
        lxp_daemon_receipt_evidence evidence;
        lxp_receipt receipt;
        size_t mark = lxp_arena_mark(scratch);
        status = lxp_daemon_receipt_authority_lookup(
            receipt_authority, owner->feed_store.head_receipt_digest,
            scratch, &evidence);
        if (status == LXP_OK)
            status = lxp_receipt_decode(
                evidence.canonical_receipt.bytes,
                evidence.canonical_receipt.length, true, &receipt);
        if (status == LXP_OK &&
            (receipt.global_sequence !=
                 owner->feed_store.scanned_through_sequence ||
             lxp_ct_memcmp(receipt.resulting_state_root,
                           owner->feed_store.head_state_root, 32U) != 0))
            status = LXP_ERR_PROJECTION_STALE;
        (void)lxp_arena_reset(scratch, mark);
    }
    if (status == LXP_OK) owner->attached = true;
    else {
fail:
        programs_runtime->state_feed = NULL;
        if (kernel->commit_observer_context == &owner->feed_store.feed) {
            kernel->observe_commit = NULL;
            kernel->commit_observer_context = NULL;
        }
        lxp_secure_zero(owner->bearer_token, sizeof(owner->bearer_token));
        owner->bearer_token_length = 0U;
        (void)lxp_verified_receipt_index_bind_fallback(
            verified_receipts, NULL, NULL);
        if (mutex_initialized) (void)pthread_mutex_destroy(&owner->mutex);
    }
    return status;
}

lxp_result lxp_daemon_protocol_owner_detach(
    lxp_daemon_protocol_owner *owner)
{
    if (owner == NULL || !owner->attached || owner->listener_started)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_kernel_clear_commit_observer(
            owner->kernel, &owner->feed_store.feed) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    owner->attached = false;
    owner->programs_runtime->state_feed = NULL;
    if (lxp_verified_receipt_index_bind_fallback(
            owner->verified_receipts, NULL, NULL) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    if (pthread_mutex_destroy(&owner->mutex) != 0) return LXP_ERR_IO;
    lxp_secure_zero(owner->bearer_token, sizeof(owner->bearer_token));
    owner->bearer_token_length = 0U;
    return LXP_OK;
}

lxp_result lxp_daemon_protocol_publish_receipt(
    lxp_daemon_protocol_owner *owner,
    const uint8_t *canonical_receipt, size_t receipt_length,
    const uint8_t *canonical_header, size_t header_length,
    const uint8_t header_signature[64],
    const lxp_merkle_proof *receipt_proof)
{
    lxp_receipt receipt;
    lxp_result status;
    size_t mark;
    if (owner == NULL || !owner->attached) return LXP_ERR_NON_CANONICAL;
    (void)pthread_mutex_lock(&owner->mutex);
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_daemon_receipt_authority_append(
        owner->receipt_authority, canonical_receipt, receipt_length,
        canonical_header, header_length, header_signature, receipt_proof,
        owner->scratch);
    if (status == LXP_OK)
        status = lxp_receipt_decode(canonical_receipt, receipt_length,
                                    true, &receipt);
    if (status == LXP_OK)
        status = lxp_verified_receipt_index_add(
            owner->verified_receipts, &receipt,
            owner->receipt_authority->authorization.public_key,
            owner->scratch);
    (void)lxp_arena_reset(owner->scratch, mark);
    (void)pthread_mutex_unlock(&owner->mutex);
    return status;
}

lxp_result lxp_daemon_protocol_route(
    lxp_daemon_protocol_owner *owner, const uint8_t *bearer_token,
    size_t bearer_token_length, const char *method, const char *path,
    lxp_arena *response_arena, lxp_daemon_protocol_response *response)
{
    json_writer writer = {NULL, 0U, 0U, LXP_OK};
    void *body = NULL;
    lxp_result status;
    if (owner == NULL || !owner->attached || method == NULL || path == NULL ||
        response_arena == NULL || response == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(response, 0, sizeof(*response));
    if (!authorized(owner, bearer_token, bearer_token_length)) {
        response->status = 401U;
        status = LXP_ERR_BAD_SIGNATURE;
    } else {
        (void)pthread_mutex_lock(&owner->mutex);
        status = route_inner(owner, method, path, response_arena, &writer);
        (void)pthread_mutex_unlock(&owner->mutex);
        response->status = status == LXP_OK ? 200U :
                           status == LXP_ERR_UNKNOWN_ACTIVITY ? 404U : 503U;
    }
    if (status != LXP_OK) {
        writer.length = 0U;
        writer.status = LXP_OK;
        json_format(&writer, "{\"error\":%d}", status);
    }
    if (writer.status == LXP_OK)
        writer.status = lxp_arena_alloc(
            response_arena, writer.length, 1U, &body);
    if (writer.status == LXP_OK) {
        (void)memcpy(body, writer.bytes, writer.length);
        response->body = (lxp_byte_span){(const uint8_t *)body, writer.length};
    }
    free(writer.bytes);
    return writer.status;
}
