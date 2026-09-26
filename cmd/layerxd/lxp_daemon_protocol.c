#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_maintenance.h"
#include "layerx/lxp_daemon.h"
#include "lxp_daemon_maintenance_json.h"

#include "layerx/lxp_crypto.h"

#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum {
    PROTOCOL_RESPONSE_MAX_BYTES = 64 * 1024 * 1024,
    PROGRAM_EVENTS_TOPIC_MAX_BYTES = 64,
    PROGRAM_EVENTS_MAX_LIMIT = 256
};

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

static lxp_result protocol_read_lock(lxp_daemon_protocol_owner *owner)
{
    if (pthread_mutex_lock(&owner->publication_mutex) != 0)
        return LXP_ERR_IO;
    if (pthread_mutex_lock(&owner->mutex) != 0) {
        (void)pthread_mutex_unlock(&owner->publication_mutex);
        return LXP_ERR_IO;
    }
    return LXP_OK;
}

static lxp_result protocol_read_unlock(
    lxp_daemon_protocol_owner *owner, lxp_result status)
{
    if (pthread_mutex_unlock(&owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    if (pthread_mutex_unlock(&owner->publication_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
}

static lxp_result durable_receipt_facts(
    void *context, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts)
{
    lxp_daemon_protocol_owner *owner =
        (lxp_daemon_protocol_owner *)context;
    lxp_daemon_receipt_evidence evidence;
    lxp_receipt receipt;
    lxp_programs_occupancy_receipt maintenance;
    lxp_batch_header header;
    size_t mark;
    lxp_result status;
    if (owner == NULL || receipt_digest == NULL || facts == NULL ||
        owner->receipt_authority == NULL || owner->scratch == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&owner->mutex) != 0) return LXP_ERR_IO;
    if (pthread_mutex_lock(&owner->receipt_authority_mutex) != 0) {
        (void)pthread_mutex_unlock(&owner->mutex);
        return LXP_ERR_IO;
    }
    mark = lxp_arena_mark(owner->scratch);
    status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, receipt_digest, owner->scratch,
        &evidence);
    if (status == LXP_OK && evidence.format_version == 3U) {
        status = lxp_batch_maintenance_occupancy_decode(
            evidence.canonical_receipt.bytes,
            evidence.canonical_receipt.length, &maintenance);
        if (status == LXP_OK)
            status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                evidence.canonical_header.length, &header);
        if (status == LXP_OK) {
            (void)memset(facts, 0, sizeof(*facts));
            (void)memcpy(facts->receipt_digest, receipt_digest, 32U);
            facts->result_code = LXP_OK;
            facts->global_sequence = maintenance.global_sequence;
            facts->timestamp = header.timestamp_ms;
            (void)memcpy(facts->resulting_state_root,
                         maintenance.resulting_state_root, 32U);
        }
    } else if (status == LXP_OK) {
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
    }
    (void)lxp_arena_reset(owner->scratch, mark);
    if (pthread_mutex_unlock(&owner->receipt_authority_mutex) != 0 &&
        status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
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

static void put_batch_evidence(lxp_daemon_protocol_owner *owner, json_writer *writer,
                               const lxp_daemon_receipt_evidence *evidence,
                               lxp_arena *arena)
{
    char *identity_json = NULL;
    size_t identity_length = 0U;
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
    status = maintenance_identity_json(owner->receipt_authority, evidence,
        arena, &identity_json, &identity_length);
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
    json_text(writer, "\"");
    if (identity_length != 0U) json_raw(writer, identity_json, identity_length);
    free(identity_json);
    json_text(writer, "}");
    (void)lxp_arena_reset(arena, mark);
}

static lxp_result put_receipt_document(
    lxp_daemon_protocol_owner *owner,
    const lxp_daemon_receipt_evidence *evidence, bool current,
    lxp_arena *arena, json_writer *writer)
{
    lxp_receipt receipt;
    lxp_programs_occupancy_receipt maintenance;
    lxp_batch_header header;
    const uint8_t *state_root;
    uint64_t sequence, timestamp;
    lxp_result status;
    if (evidence->format_version == 3U) {
        status = lxp_batch_maintenance_occupancy_decode(
            evidence->canonical_receipt.bytes,
            evidence->canonical_receipt.length, &maintenance);
        if (status != LXP_OK) return status;
        state_root = maintenance.resulting_state_root;
        sequence = maintenance.global_sequence;
        status = lxp_batch_header_decode(evidence->canonical_header.bytes,
            evidence->canonical_header.length, &header);
        if (status != LXP_OK) return status;
        timestamp = header.timestamp_ms;
    } else {
        status = lxp_receipt_decode(evidence->canonical_receipt.bytes,
            evidence->canonical_receipt.length, true, &receipt);
        if (status != LXP_OK) return status;
        state_root = receipt.resulting_state_root;
        sequence = receipt.global_sequence;
        timestamp = receipt.timestamp;
    }
    if (current && (sequence != owner->feed_store.scanned_through_sequence ||
        lxp_ct_memcmp(state_root, owner->feed_store.head_state_root, 32U) != 0))
        return LXP_ERR_PROJECTION_STALE;
    json_text(writer, "{\"current\":");
    json_text(writer, current ? "true" : "false");
    json_text(writer, ",\"receipt_hex\":\"");
    json_hex(writer, evidence->canonical_receipt.bytes,
             evidence->canonical_receipt.length);
    json_text(writer, "\",\"receipt_digest\":\"");
    json_hex(writer, evidence->receipt_digest, 32U);
    json_text(writer, "\",\"state_root\":\"");
    json_hex(writer, state_root, 32U);
    json_format(writer,
                "\",\"observed_sequence\":%llu,\"observed_at\":%llu,"
                "\"batch_evidence\":",
                (unsigned long long)sequence,
                (unsigned long long)timestamp);
    put_batch_evidence(owner, writer, evidence, arena);
    json_text(writer, "}");
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
        context.protocol_version = owner->protocol_version;
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

static lxp_result put_program_artifacts(
    const uint8_t activity_id[32], const uint8_t receipt_digest[32],
    lxp_byte_span terminal_payload, lxp_byte_span call_graph,
    json_writer *writer)
{
    json_text(writer, "{\"activity_id\":\"");
    json_hex(writer, activity_id, 32U);
    json_text(writer, "\",\"receipt_digest\":\"");
    json_hex(writer, receipt_digest, 32U);
    json_text(writer, "\",\"terminal_payload\":\"");
    json_hex(writer, terminal_payload.bytes, terminal_payload.length);
    json_text(writer, "\",\"call_graph\":\"");
    json_hex(writer, call_graph.bytes, call_graph.length);
    json_text(writer, "\"}");
    return writer->status;
}

static lxp_result pending_artifacts_route(
    lxp_daemon_protocol_owner *owner, const uint8_t activity_id[32],
    const uint8_t receipt_digest[32], json_writer *writer, bool *present)
{
    lxp_result status = LXP_ERR_UNKNOWN_ACTIVITY;
    uint8_t *scratch_bytes;
    lxp_arena scratch;
    size_t index;
    if (owner == NULL || activity_id == NULL || receipt_digest == NULL ||
        writer == NULL || present == NULL)
        return LXP_ERR_NON_CANONICAL;
    *present = false;
    scratch_bytes = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
    if (scratch_bytes == NULL) return LXP_ERR_IO;
    status = lxp_arena_init(&scratch, scratch_bytes, LXP_MAX_ACTIVITY_BYTES);
    if (status != LXP_OK) {
        free(scratch_bytes);
        return status;
    }
    if (pthread_mutex_lock(&owner->receipt_mutex) != 0) {
        free(scratch_bytes);
        return LXP_ERR_IO;
    }
    for (index = 0U; index < owner->pending_receipt_count; ++index) {
        const lxp_daemon_pending_receipt *pending =
            &owner->pending_receipts[index];
        lxp_receipt receipt;
        uint8_t digest[32];
        if (lxp_ct_memcmp(pending->activity_id, activity_id, 32U) != 0)
            continue;
        *present = true;
        status = pending->bytes == NULL || pending->length == 0U ||
                pending->length > LXP_STATE_MAX_RECEIPT_BYTES ||
                (pending->terminal_payload.length != 0U &&
                 pending->terminal_payload.bytes == NULL) ||
                (pending->call_graph.length != 0U &&
                 pending->call_graph.bytes == NULL) ||
                (pending->event_list.length != 0U &&
                 pending->event_list.bytes == NULL) ?
            LXP_FATAL_INVARIANT : LXP_OK;
        if (status == LXP_OK)
            status = lxp_receipt_decode(
                pending->bytes, pending->length, true, &receipt);
        if (status == LXP_OK)
            status = lxp_receipt_digest(&receipt, &scratch, digest);
        if (status == LXP_OK &&
            lxp_ct_memcmp(digest, receipt_digest, 32U) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK &&
            (lxp_ct_memcmp(receipt.activity_id, activity_id, 32U) != 0 ||
             receipt.global_sequence != pending->global_sequence ||
             receipt.module_id != LXP_MODULE_PROGRAMS ||
             !receipt.program_outcome.present))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK)
            status = lxp_receipt_bind_program_artifacts(
                &receipt, pending->terminal_payload, pending->call_graph,
                pending->event_list);
        if (status == LXP_OK)
            status = put_program_artifacts(
                activity_id, receipt_digest, pending->terminal_payload,
                pending->call_graph, writer);
        break;
    }
    if (!*present && status == LXP_ERR_UNKNOWN_ACTIVITY) status = LXP_OK;
    if (pthread_mutex_unlock(&owner->receipt_mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    free(scratch_bytes);
    return status;
}

static lxp_result artifacts_route(lxp_daemon_protocol_owner *owner,
                                   const uint8_t activity_id[32],
                                   const uint8_t receipt_digest[32],
                                   lxp_arena *arena, json_writer *writer)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_receipt receipt;
    lxp_result status;
    if (owner->receipt_authority == NULL) return LXP_ERR_PROJECTION_STALE;
    status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, receipt_digest, arena, &evidence);
    if (status == LXP_OK)
        status = lxp_receipt_decode(evidence.canonical_receipt.bytes,
            evidence.canonical_receipt.length, true, &receipt);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(receipt.activity_id, activity_id, 32U) != 0 ||
         receipt.module_id != LXP_MODULE_PROGRAMS ||
         !receipt.program_outcome.present))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_receipt_bind_program_artifacts(
            &receipt, evidence.terminal_payload, evidence.call_graph,
            evidence.event_list);
    if (status != LXP_OK) return status;
    return put_program_artifacts(
        activity_id, receipt_digest, evidence.terminal_payload,
        evidence.call_graph, writer);
}

static lxp_result parse_artifacts_request(
    const char *method, const char *path, uint8_t activity_id[32],
    uint8_t receipt_digest[32], bool *matches)
{
    static const char prefix[] = "/v1/programs/activities/";
    static const char tail[] = "/artifacts?receipt_digest=";
    const char *activity;
    char activity_text[65];
    if (method == NULL || path == NULL || activity_id == NULL ||
        receipt_digest == NULL || matches == NULL)
        return LXP_ERR_NON_CANONICAL;
    *matches = false;
    if (strcmp(method, "GET") != 0 ||
        strncmp(path, prefix, sizeof(prefix) - 1U) != 0)
        return LXP_OK;
    *matches = true;
    activity = path + sizeof(prefix) - 1U;
    if (strlen(activity) != 64U + sizeof(tail) - 1U + 64U ||
        memcmp(activity + 64U, tail, sizeof(tail) - 1U) != 0)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(activity_text, activity, 64U);
    activity_text[64] = '\0';
    if (parse_hex32(activity_text, activity_id) != LXP_OK ||
        parse_hex32(activity + 64U + sizeof(tail) - 1U,
                    receipt_digest) != LXP_OK)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static uint32_t read_event_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | (uint32_t)bytes[3];
}

/* Writes every event of one bound event list whose topic matches, and counts
 * them; the list layout is the programs runtime's canonical event envelope. */
static lxp_result put_matching_events(
    lxp_byte_span list, uint64_t sequence, const uint8_t *topic,
    size_t topic_length, bool write, json_writer *writer, size_t *written,
    size_t *matched)
{
    static const uint8_t domain[] = "LayerX/programs/events/v1";
    size_t cursor = sizeof(domain);
    uint32_t count;
    uint32_t index;
    *matched = 0U;
    if (list.bytes == NULL || list.length < sizeof(domain) + 4U ||
        memcmp(list.bytes, domain, sizeof(domain)) != 0)
        return LXP_ERR_LOG_CORRUPT;
    count = read_event_u32(list.bytes + cursor);
    cursor += 4U;
    for (index = 0U; index < count; ++index) {
        const uint8_t *program_id;
        const uint8_t *event_topic;
        const uint8_t *data;
        uint32_t event_topic_length;
        uint32_t data_length;
        if (list.length - cursor < 32U + 32U + 8U + 1U + 4U)
            return LXP_ERR_LOG_CORRUPT;
        program_id = list.bytes + cursor;
        cursor += 32U + 32U + 8U + 1U;
        event_topic_length = read_event_u32(list.bytes + cursor);
        cursor += 4U;
        if (event_topic_length > list.length - cursor ||
            list.length - cursor - event_topic_length < 4U)
            return LXP_ERR_LOG_CORRUPT;
        event_topic = list.bytes + cursor;
        cursor += event_topic_length;
        data_length = read_event_u32(list.bytes + cursor);
        cursor += 4U;
        if (data_length > list.length - cursor) return LXP_ERR_LOG_CORRUPT;
        data = list.bytes + cursor;
        cursor += data_length;
        if (event_topic_length != topic_length ||
            memcmp(event_topic, topic, topic_length) != 0)
            continue;
        ++*matched;
        if (!write) continue;
        if (*written != 0U) json_text(writer, ",");
        json_format(writer, "{\"sequence\":%llu,\"program_id\":\"",
                    (unsigned long long)sequence);
        json_hex(writer, program_id, 32U);
        json_text(writer, "\",\"topic\":\"");
        json_hex(writer, event_topic, event_topic_length);
        json_text(writer, "\",\"data\":\"");
        json_hex(writer, data, data_length);
        json_text(writer, "\"}");
        ++*written;
    }
    return cursor == list.length ? writer->status : LXP_ERR_LOG_CORRUPT;
}

/* Pages the program events bound in the receipt authority log by topic from a
 * global sequence; next_sequence is where the following page starts and never
 * passes the durable head, the sequence after the last durable record. */
static lxp_result program_events_route(
    lxp_daemon_protocol_owner *owner, const uint8_t *topic,
    size_t topic_length, uint64_t from_sequence, size_t limit,
    lxp_arena *arena, json_writer *writer)
{
    const lxp_daemon_receipt_authority_store *store = owner->receipt_authority;
    uint64_t head;
    uint64_t next_sequence;
    uint64_t offset = 0U;
    uint64_t best_sequence = 0U;
    size_t written = 0U;
    size_t index;
    lxp_result status = LXP_OK;
    if (store == NULL || store->log == NULL) return LXP_ERR_PROJECTION_STALE;
    if (store->record_count != 0U && store->last_global_sequence == UINT64_MAX)
        return LXP_FATAL_INVARIANT;
    head = store->record_count == 0U ? 0U : store->last_global_sequence + 1U;
    next_sequence = from_sequence < head ? from_sequence : head;
    for (index = 0U; index < store->cache_count; ++index) {
        const lxp_daemon_receipt_authority_entry *entry = &store->cache[index];
        if (entry->global_sequence <= from_sequence &&
            entry->global_sequence >= best_sequence) {
            best_sequence = entry->global_sequence;
            offset = entry->record_offset;
        }
    }
    json_text(writer, "{\"events\":[");
    while (status == LXP_OK && next_sequence < head) {
        lxp_daemon_receipt_evidence evidence;
        uint64_t record_offset = offset;
        size_t mark = lxp_arena_mark(arena);
        size_t matched = 0U;
        bool present = false;
        status = lxp_daemon_receipt_authority_scan(
            store, &offset, arena, &evidence, &present);
        if (status != LXP_OK || !present) {
            (void)lxp_arena_reset(arena, mark);
            break;
        }
        if (evidence.global_sequence >= from_sequence) {
            lxp_receipt receipt;
            if (evidence.event_list.length != 0U) {
                status = lxp_receipt_decode(evidence.canonical_receipt.bytes,
                    evidence.canonical_receipt.length, true, &receipt);
                if (status == LXP_OK &&
                    (receipt.result_code != LXP_OK ||
                     !receipt.program_outcome.present ||
                     receipt.program_outcome.terminal_kind !=
                         LXP_PROGRAM_TERMINAL_SUCCESS))
                    evidence.event_list = (lxp_byte_span){NULL, 0U};
            }
            if (status == LXP_OK && evidence.event_list.length != 0U) {
                status = put_matching_events(evidence.event_list,
                    evidence.global_sequence, topic, topic_length, false,
                    writer, &written, &matched);
                if (status == LXP_OK && matched != 0U && written != 0U &&
                    matched > limit - written) {
                    offset = record_offset;
                    (void)lxp_arena_reset(arena, mark);
                    break;
                }
                if (status == LXP_OK && matched != 0U)
                    status = put_matching_events(evidence.event_list,
                        evidence.global_sequence, topic, topic_length, true,
                        writer, &written, &matched);
            }
            if (status == LXP_OK) next_sequence = evidence.global_sequence + 1U;
        }
        (void)lxp_arena_reset(arena, mark);
        if (written >= limit) break;
    }
    if (status != LXP_OK) return status;
    if (next_sequence > head) return LXP_FATAL_INVARIANT;
    json_format(writer, "],\"next_sequence\":%llu}",
                (unsigned long long)next_sequence);
    return writer->status;
}

static lxp_result parse_program_events_request(
    const char *suffix, uint8_t topic[PROGRAM_EVENTS_TOPIC_MAX_BYTES],
    size_t *topic_length, uint64_t *from_sequence, size_t *limit)
{
    const char *sequence_text;
    const char *limit_text;
    char number[21];
    uint64_t parsed_limit;
    size_t hex_length;
    size_t index;
    sequence_text = strchr(suffix, '/');
    if (sequence_text == NULL) return LXP_ERR_NON_CANONICAL;
    limit_text = strchr(sequence_text + 1U, '/');
    if (limit_text == NULL || strchr(limit_text + 1U, '/') != NULL)
        return LXP_ERR_NON_CANONICAL;
    hex_length = (size_t)(sequence_text - suffix);
    if (hex_length == 0U || (hex_length & 1U) != 0U ||
        hex_length > 2U * PROGRAM_EVENTS_TOPIC_MAX_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < hex_length; index += 2U) {
        int high = hex_nibble(suffix[index]);
        int low = hex_nibble(suffix[index + 1U]);
        if (high < 0 || low < 0 ||
            (suffix[index] >= 'A' && suffix[index] <= 'F') ||
            (suffix[index + 1U] >= 'A' && suffix[index + 1U] <= 'F'))
            return LXP_ERR_NON_CANONICAL;
        topic[index / 2U] = (uint8_t)((unsigned int)high << 4U |
                                      (unsigned int)low);
    }
    *topic_length = hex_length / 2U;
    if ((size_t)(limit_text - sequence_text - 1) >= sizeof(number))
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(number, sequence_text + 1U,
                 (size_t)(limit_text - sequence_text - 1));
    number[limit_text - sequence_text - 1] = '\0';
    if (parse_u64(number, from_sequence) != LXP_OK ||
        parse_u64(limit_text + 1U, &parsed_limit) != LXP_OK ||
        parsed_limit == 0U || parsed_limit > PROGRAM_EVENTS_MAX_LIMIT)
        return LXP_ERR_NON_CANONICAL;
    *limit = (size_t)parsed_limit;
    return LXP_OK;
}

static lxp_result idempotency_receipt_route(
    lxp_daemon_protocol_owner *owner, const uint8_t key[32],
    lxp_arena *arena, json_writer *writer)
{
    lxp_receipt_query query;
    lxp_byte_span canonical = {NULL, 0U};
    lxp_receipt receipt;
    lxp_daemon_receipt_evidence evidence;
    uint8_t digest[32];
    lxp_result status;
    if (owner->history == NULL || owner->receipt_authority == NULL)
        return LXP_ERR_PROJECTION_STALE;
    (void)memset(&query, 0, sizeof(query));
    query.kind = LXP_RECEIPT_BY_IDEMPOTENCY_KEY;
    query.maximum_response_bytes = LXP_MAX_ACTIVITY_BYTES;
    (void)memcpy(query.identifier, key, 32U);
    status = lxp_receipt_lookup(owner->history, &query, arena, &canonical);
    if (status == LXP_OK)
        status = lxp_receipt_decode(canonical.bytes, canonical.length, true, &receipt);
    if (status == LXP_OK && receipt.module_id != LXP_MODULE_PROGRAMS)
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK) status = lxp_receipt_digest(&receipt, arena, digest);
    if (status == LXP_OK)
        status = lxp_daemon_receipt_authority_lookup(
            owner->receipt_authority, digest, arena, &evidence);
    if (status == LXP_OK &&
        (evidence.canonical_receipt.length != canonical.length ||
         lxp_ct_memcmp(evidence.canonical_receipt.bytes, canonical.bytes,
                       canonical.length) != 0))
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status != LXP_OK) return status;
    json_text(writer, "{\"activity_id\":\"");
    json_hex(writer, receipt.activity_id, 32U);
    json_text(writer, "\",\"receipt\":\"");
    json_hex(writer, canonical.bytes, canonical.length);
    json_text(writer, "\"}");
    return writer->status;
}

static lxp_result batch_route(lxp_daemon_protocol_owner *owner,
                              const uint8_t batch_id[32],
                              const uint8_t receipt_digest[32],
                              lxp_arena *arena, json_writer *writer)
{
    lxp_daemon_receipt_evidence evidence;
    lxp_batch_header header;
    lxp_sequencer_authorization authorization;
    lxp_result status = lxp_daemon_receipt_authority_lookup(
        owner->receipt_authority, receipt_digest, arena, &evidence);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(evidence.batch_id, batch_id, 32U) != 0)
        return LXP_ERR_ROOT_MISMATCH;
    status = lxp_batch_header_decode(evidence.canonical_header.bytes, evidence.canonical_header.length, &header);
    if (status == LXP_OK) status = lxp_daemon_receipt_authority_header_authorization(
        owner->receipt_authority, &header, &authorization);
    if (status != LXP_OK) return status;
    json_text(writer, "{\"sequencer_public_key\":\"");
    json_hex(writer, authorization.public_key, 32U);
    json_text(writer, "\",\"batch_evidence\":");
    put_batch_evidence(owner, writer, &evidence, arena);
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
    uint8_t artifact_activity_id[32];
    uint8_t artifact_receipt_digest[32];
    bool artifact_request = false;
    lxp_result artifact_status;
    if (strcmp(method, "GET") != 0) return LXP_ERR_UNKNOWN_ACTIVITY;
    artifact_status = parse_artifacts_request(
        method, path, artifact_activity_id, artifact_receipt_digest,
        &artifact_request);
    if (artifact_status != LXP_OK) return artifact_status;
    if (artifact_request)
        return artifacts_route(owner, artifact_activity_id,
                               artifact_receipt_digest, arena, writer);
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
        static const char idempotency_prefix[] = "receipts/by-idempotency/";
        static const char events_prefix[] = "events/";
        if (strncmp(suffix, events_prefix, sizeof(events_prefix) - 1U) == 0) {
            uint8_t topic[PROGRAM_EVENTS_TOPIC_MAX_BYTES];
            size_t topic_length = 0U;
            uint64_t from_sequence = 0U;
            size_t limit = 0U;
            lxp_result status = parse_program_events_request(
                suffix + sizeof(events_prefix) - 1U, topic, &topic_length,
                &from_sequence, &limit);
            return status == LXP_OK ?
                program_events_route(owner, topic, topic_length,
                                     from_sequence, limit, arena, writer) :
                status;
        }
        if (strncmp(suffix, idempotency_prefix, sizeof(idempotency_prefix) - 1U) == 0) {
            const char *key_text = suffix + sizeof(idempotency_prefix) - 1U;
            uint8_t key[32];
            size_t index;
            if (strlen(key_text) != 64U) return LXP_ERR_NON_CANONICAL;
            for (index = 0U; index < 64U; ++index) {
                if (hex_nibble(key_text[index]) < 0 ||
                    (key_text[index] >= 'A' && key_text[index] <= 'F'))
                    return LXP_ERR_NON_CANONICAL;
            }
            for (index = 0U; index < 32U; ++index)
                key[index] = (uint8_t)((unsigned int)hex_nibble(key_text[index * 2U]) << 4U |
                                      (unsigned int)hex_nibble(key_text[index * 2U + 1U]));
            return idempotency_receipt_route(owner, key, arena, writer);
        }
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
        static const char authority_suffix[] = "/receipt-authority?receipt_digest=";
        const char *tail = strstr(suffix, authority_suffix);
        char batch_text[65];
        uint8_t batch_id[32];
        uint8_t receipt_digest[32];
        if (tail == NULL || (size_t)(tail - suffix) != 64U ||
            strlen(tail + sizeof(authority_suffix) - 1U) != 64U)
            return LXP_ERR_NON_CANONICAL;
        (void)memcpy(batch_text, suffix, 64U); batch_text[64] = '\0';
        if (parse_hex32(batch_text, batch_id) != LXP_OK ||
            parse_hex32(tail + sizeof(authority_suffix) - 1U, receipt_digest) != LXP_OK)
            return LXP_ERR_NON_CANONICAL;
        return batch_route(owner, batch_id, receipt_digest, arena, writer);
    }
    return LXP_ERR_UNKNOWN_ACTIVITY;
}

lxp_result lxp_daemon_protocol_owner_attach(
    lxp_daemon_protocol_owner *owner, lxp_kernel *kernel,
    lxp_identity_store *identities, uint32_t network_id,
    uint64_t bootstrap_sealed_timestamp,
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
    bool publication_mutex_initialized = false;
    bool receipt_authority_mutex_initialized = false;
    bool receipt_mutex_initialized = false;
    const char *stage = "input validation";
    if (owner == NULL || kernel == NULL || identities == NULL ||
        network_id == 0U ||
        programs_runtime == NULL ||
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
        receipt_authority->log == canonical_log || feed_log == canonical_log) {
        (void)fprintf(stderr, "layerxd: protocol owner input invalid (scratch available %zu)\n",
                      scratch != NULL && scratch->offset <= scratch->capacity ?
                      scratch->capacity - scratch->offset : 0U);
        return LXP_ERR_NON_CANONICAL;
    }
    for (index = 0U; index < bearer_token_length; ++index)
        if (bearer_token[index] < 0x21U || bearer_token[index] > 0x7eU)
            return LXP_ERR_NON_CANONICAL;
    (void)memset(owner, 0, sizeof(*owner));
    owner->kernel = kernel;
    owner->identities = identities;
    owner->network_id = network_id;
    owner->protocol_version = LXP_PROTOCOL_VERSION;
    owner->programs_runtime = programs_runtime;
    owner->history = history;
    owner->verified_receipts = verified_receipts;
    owner->receipt_authority = receipt_authority;
    owner->scratch = scratch;
    owner->listener_descriptor = -1;
    (void)memcpy(owner->bearer_token, bearer_token, bearer_token_length);
    owner->bearer_token_length = bearer_token_length;
    if (pthread_mutex_init(&owner->publication_mutex, NULL) != 0) {
        status = LXP_ERR_IO;
        goto fail;
    }
    publication_mutex_initialized = true;
    if (pthread_mutex_init(&owner->receipt_authority_mutex, NULL) != 0) {
        status = LXP_ERR_IO;
        goto fail;
    }
    receipt_authority_mutex_initialized = true;
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
    if (pthread_mutex_init(&owner->receipt_mutex, NULL) != 0) {
        status = LXP_ERR_IO;
        goto fail;
    }
    receipt_mutex_initialized = true;
    status = lxp_verified_receipt_index_bind_fallback(
        verified_receipts, durable_receipt_facts, owner);
    if (status != LXP_OK) goto fail;
    stage = "feed store open";
    status = lxp_programs_state_feed_store_open(
        &owner->feed_store, feed_log, canonical_log, history, scratch,
        &owner->mutex);
    if (status == LXP_OK) {
        stage = "module runtime binding";
        programs_runtime->state_feed = &owner->feed_store.feed;
        status = lxp_kernel_bind_module_runtime(
            kernel, LXP_MODULE_PROGRAMS, programs_runtime);
    }
    if (status == LXP_OK)
        status = lxp_programs_state_feed_store_bind_maintenance(&owner->feed_store, kernel);
    if (status == LXP_OK) stage = "canonical replay";
    if (status == LXP_OK) status = replay(replay_context, owner);
    if (status == LXP_OK) stage = "feed recovery";
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
            if (status == LXP_OK && present && evidence.format_version != 3U)
                status = lxp_receipt_decode(
                    evidence.canonical_receipt.bytes,
                    evidence.canonical_receipt.length, true, &receipt);
            if (status == LXP_OK && present && evidence.format_version != 3U) {
                lxp_batch_header header;
                lxp_sequencer_authorization authorization;
                status = lxp_batch_header_decode(evidence.canonical_header.bytes,
                    evidence.canonical_header.length, &header);
                if (status == LXP_OK) status = lxp_daemon_receipt_authority_header_authorization(
                    receipt_authority, &header, &authorization);
                if (status == LXP_OK) status = lxp_verified_receipt_index_add(
                    verified_receipts, &receipt, authorization.public_key, scratch);
            }
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
        if (status == LXP_OK && evidence.format_version == 3U) {
            lxp_programs_occupancy_receipt maintenance;
            status = lxp_batch_maintenance_occupancy_decode(
                evidence.canonical_receipt.bytes, evidence.canonical_receipt.length,
                &maintenance);
            if (status == LXP_OK &&
                (maintenance.global_sequence != owner->feed_store.scanned_through_sequence ||
                 lxp_ct_memcmp(maintenance.resulting_state_root,
                               owner->feed_store.head_state_root, 32U) != 0))
                status = LXP_ERR_PROJECTION_STALE;
        } else {
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
        }
        (void)lxp_arena_reset(scratch, mark);
    }
    if (status == LXP_OK && receipt_authority->record_count != 0U) {
        if (receipt_authority->last_global_sequence == UINT64_MAX ||
            kernel->state->next_sequence !=
                receipt_authority->last_global_sequence + 1U ||
            receipt_authority->last_sealed_timestamp == 0U)
            status = LXP_ERR_PROJECTION_STALE;
        else
            owner->latest_sealed_timestamp =
                receipt_authority->last_sealed_timestamp;
    } else if (status == LXP_OK) {
        if (kernel->state->next_sequence != 1U ||
            bootstrap_sealed_timestamp == 0U)
            status = LXP_ERR_PROJECTION_STALE;
        else
            owner->latest_sealed_timestamp = bootstrap_sealed_timestamp;
    }
    if (status == LXP_OK) {
        owner->published_receipt_log = *canonical_log;
        owner->published_receipt_log.capacity = canonical_log->write_offset;
        owner->published_batch_number = receipt_authority->last_batch_number;
        owner->attached = true;
    }
    else {
fail:
        (void)fprintf(stderr, "layerxd: protocol owner %s failed with result %d\n", stage, (int)status);
        programs_runtime->state_feed = NULL;
        if (kernel->commit_observer_context == &owner->feed_store.feed) {
            kernel->observe_commit = NULL;
            kernel->commit_observer_context = NULL;
        }
        lxp_secure_zero(owner->bearer_token, sizeof(owner->bearer_token));
        owner->bearer_token_length = 0U;
        (void)lxp_verified_receipt_index_bind_fallback(
            verified_receipts, NULL, NULL);
        if (receipt_mutex_initialized) (void)pthread_mutex_destroy(&owner->receipt_mutex);
        if (mutex_initialized) (void)pthread_mutex_destroy(&owner->mutex);
        if (receipt_authority_mutex_initialized)
            (void)pthread_mutex_destroy(&owner->receipt_authority_mutex);
        if (publication_mutex_initialized)
            (void)pthread_mutex_destroy(&owner->publication_mutex);
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
    if (pthread_mutex_destroy(&owner->receipt_mutex) != 0) return LXP_ERR_IO;
    if (pthread_mutex_destroy(&owner->mutex) != 0) return LXP_ERR_IO;
    if (pthread_mutex_destroy(&owner->receipt_authority_mutex) != 0)
        return LXP_ERR_IO;
    if (pthread_mutex_destroy(&owner->publication_mutex) != 0)
        return LXP_ERR_IO;
    lxp_secure_zero(owner->bearer_token, sizeof(owner->bearer_token));
    owner->bearer_token_length = 0U;
    return LXP_OK;
}

lxp_result lxp_daemon_protocol_owner_bind_evidence(
    lxp_daemon_protocol_owner *owner,
    lxp_daemon_evidence_store *evidence_store)
{
    lxp_result status = LXP_OK;
    if (owner == NULL || evidence_store == NULL || !owner->attached ||
        !evidence_store->initialized || evidence_store->log == NULL ||
        evidence_store->network_id != owner->network_id ||
        !evidence_store->authorization.authorized ||
        !owner->receipt_authority->authorization.authorized ||
        evidence_store->authorization.first_batch_number !=
            owner->receipt_authority->authorization.first_batch_number ||
        evidence_store->authorization.last_batch_number !=
            owner->receipt_authority->authorization.last_batch_number ||
        lxp_ct_memcmp(evidence_store->authorization.sequencer_id,
                      owner->receipt_authority->authorization.sequencer_id,
                      32U) != 0 ||
        lxp_ct_memcmp(evidence_store->authorization.public_key,
                      owner->receipt_authority->authorization.public_key,
                      32U) != 0 ||
        evidence_store->log == owner->receipt_authority->log ||
        evidence_store->log == owner->history->log)
        return LXP_ERR_NON_CANONICAL;
    if (pthread_mutex_lock(&owner->mutex) != 0) return LXP_ERR_IO;
    if (owner->evidence_store != NULL &&
        owner->evidence_store != evidence_store)
        status = LXP_ERR_CONTEXT_MISMATCH;
    else {
        owner->evidence_store = evidence_store;
        if (pthread_mutex_lock(&owner->receipt_mutex) != 0)
            status = LXP_ERR_IO;
        else {
            (void)memcpy(owner->published_checkpoint_id,
                         evidence_store->latest_checkpoint_id, 32U);
            if (pthread_mutex_unlock(&owner->receipt_mutex) != 0)
                status = LXP_FATAL_INVARIANT;
        }
    }
    if (pthread_mutex_unlock(&owner->mutex) != 0 && status == LXP_OK)
        status = LXP_FATAL_INVARIANT;
    return status;
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
    if (status == LXP_OK) {
        lxp_batch_header header;
        lxp_sequencer_authorization authorization;
        status = lxp_batch_header_decode(canonical_header, header_length, &header);
        if (status == LXP_OK) status = lxp_daemon_receipt_authority_header_authorization(
            owner->receipt_authority, &header, &authorization);
        if (status == LXP_OK) status = lxp_verified_receipt_index_add(
            owner->verified_receipts, &receipt, authorization.public_key, owner->scratch);
    }
    if (status == LXP_OK) owner->latest_sealed_timestamp = receipt.timestamp;
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
    uint8_t artifact_activity_id[32];
    uint8_t artifact_receipt_digest[32];
    bool artifact_request = false;
    bool pending_artifact = false;
    bool read_locked = false;
    lxp_result status;
    if (owner == NULL || !owner->attached || method == NULL || path == NULL ||
        response_arena == NULL || response == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(response, 0, sizeof(*response));
    if (!authorized(owner, bearer_token, bearer_token_length)) {
        response->status = 401U;
        status = LXP_ERR_BAD_SIGNATURE;
    } else {
        status = parse_artifacts_request(
            method, path, artifact_activity_id, artifact_receipt_digest,
            &artifact_request);
        if (status == LXP_OK && artifact_request)
            status = pending_artifacts_route(
                owner, artifact_activity_id, artifact_receipt_digest,
                &writer, &pending_artifact);
        if (status == LXP_OK && !pending_artifact) {
            status = protocol_read_lock(owner);
            if (status == LXP_OK) {
                read_locked = true;
                status = route_inner(
                    owner, method, path, response_arena, &writer);
            }
            if (read_locked)
                status = protocol_read_unlock(owner, status);
        }
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
