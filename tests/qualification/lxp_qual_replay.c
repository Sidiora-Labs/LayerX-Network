#include "layerx/lxp_qualification.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_protocol.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

enum {
    QUAL_FORMAT_VERSION = 1,
    QUAL_HEADER_BYTES = 252,
    QUAL_HEADER_PREFIX_BYTES = 28,
    QUAL_DIGEST_COUNT = 7,
    QUAL_ROOT_HEADER_BYTES = 48,
    QUAL_ROOT_ENTRY_BYTES = 72
};

static const uint8_t corpus_magic[8] = {
    'L', 'X', 'P', 'Q', 'R', 'P', '0', '1'
};
static const uint8_t root_magic[8] = {
    'L', 'X', 'P', 'Q', 'R', 'L', '0', '1'
};

static const uint32_t activity_types[LXP_QUAL_ACTIVITY_TYPE_COUNT] = {
    UINT32_C(0x00010001), UINT32_C(0x00010002), UINT32_C(0x00010003),
    UINT32_C(0x00010004), UINT32_C(0x00010005), UINT32_C(0x00010006),
    UINT32_C(0x00010007), UINT32_C(0x00010008),
    UINT32_C(0x00020001), UINT32_C(0x00020002), UINT32_C(0x00020003),
    UINT32_C(0x00020004), UINT32_C(0x00020005), UINT32_C(0x00020006),
    UINT32_C(0x00020007),
    UINT32_C(0x00030001), UINT32_C(0x00030002), UINT32_C(0x00030003),
    UINT32_C(0x00030004), UINT32_C(0x00030005), UINT32_C(0x00030006),
    UINT32_C(0x00030007),
    UINT32_C(0x00040001), UINT32_C(0x00040002), UINT32_C(0x00040003),
    UINT32_C(0x00040004), UINT32_C(0x00040005), UINT32_C(0x00040006),
    UINT32_C(0x00040007),
    UINT32_C(0x00050001), UINT32_C(0x00050002), UINT32_C(0x00050003),
    UINT32_C(0x00050004), UINT32_C(0x00050005), UINT32_C(0x00050006),
    UINT32_C(0x00050007), UINT32_C(0x00050008), UINT32_C(0x00050009),
    UINT32_C(0x0005000a), UINT32_C(0x0005000b), UINT32_C(0x0005000c),
    UINT32_C(0x0005000d),
    UINT32_C(0x00060001), UINT32_C(0x00060002), UINT32_C(0x00060003),
    UINT32_C(0x00060004), UINT32_C(0x00060005), UINT32_C(0x00060006),
    UINT32_C(0x00060007), UINT32_C(0x00060008), UINT32_C(0x00060009),
    UINT32_C(0x0006000a), UINT32_C(0x0006000b)
};

typedef struct qual_header {
    uint64_t activity_count;
    uint32_t batch_size;
    uint32_t activity_type_count;
    uint8_t activity_digest[32];
    uint8_t receipt_digest[32];
    uint8_t event_digest[32];
    uint8_t batch_digest[32];
    uint8_t root_ledger_digest[32];
    uint8_t terminal_root[32];
    uint8_t corpus_digest[32];
} qual_header;

typedef struct generated_record {
    uint8_t state_root[32];
    uint8_t receipt[LXP_QUAL_RECEIPT_BYTES];
    uint8_t event[LXP_QUAL_EVENT_BYTES];
    uint8_t batch_header[LXP_QUAL_BATCH_HEADER_BYTES];
    uint8_t batch_root[32];
} generated_record;

typedef struct digest_set {
    lxp_hash_context activity;
    lxp_hash_context receipt;
    lxp_hash_context event;
    lxp_hash_context batch;
    lxp_hash_context ledger;
    lxp_hash_context corpus;
} digest_set;

static void store_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void store_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static uint32_t load_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | (uint32_t)in[3];
}

static uint64_t load_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static lxp_result write_bytes(FILE *file, const void *bytes, size_t length)
{
    if (file == NULL || (bytes == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return fwrite(bytes, 1U, length, file) == length ? LXP_OK : LXP_ERR_IO;
}

static lxp_result read_bytes(FILE *file, void *bytes, size_t length)
{
    if (file == NULL || (bytes == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return fread(bytes, 1U, length, file) == length ? LXP_OK : LXP_ERR_IO;
}

static lxp_result stream_init(lxp_hash_context *context, const char *domain)
{
    lxp_hash_init(context);
    return lxp_hash_update(context, domain, strlen(domain) + 1U);
}

static lxp_result digests_init(digest_set *digests)
{
    lxp_result status;
    if (digests == NULL) return LXP_ERR_NON_CANONICAL;
    status = stream_init(&digests->activity, "LXP/qual/activity-stream/v1");
    if (status == LXP_OK)
        status = stream_init(&digests->receipt, "LXP/qual/receipt-stream/v1");
    if (status == LXP_OK)
        status = stream_init(&digests->event, "LXP/qual/event-stream/v1");
    if (status == LXP_OK)
        status = stream_init(&digests->batch, "LXP/qual/batch-stream/v1");
    if (status == LXP_OK)
        status = stream_init(&digests->ledger, "LXP/qual/root-ledger/v1");
    if (status == LXP_OK)
        status = stream_init(&digests->corpus, "LXP/qual/corpus/v1");
    return status;
}

static lxp_result write_corpus_part(FILE *file, lxp_hash_context *corpus,
                                    const void *bytes, size_t length)
{
    lxp_result status = write_bytes(file, bytes, length);
    if (status == LXP_OK) status = lxp_hash_update(corpus, bytes, length);
    return status;
}

static lxp_result write_root_part(FILE *file, lxp_hash_context *ledger,
                                  const void *bytes, size_t length)
{
    lxp_result status = write_bytes(file, bytes, length);
    if (status == LXP_OK) status = lxp_hash_update(ledger, bytes, length);
    return status;
}

static lxp_result header_prefix(uint8_t out[QUAL_HEADER_PREFIX_BYTES],
                                uint64_t activity_count, uint32_t batch_size)
{
    if (out == NULL || activity_count == 0U || batch_size == 0U ||
        batch_size > LXP_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(out, corpus_magic, sizeof(corpus_magic));
    store_u32(out + 8U, QUAL_FORMAT_VERSION);
    store_u64(out + 12U, activity_count);
    store_u32(out + 20U, batch_size);
    store_u32(out + 24U, LXP_QUAL_ACTIVITY_TYPE_COUNT);
    return LXP_OK;
}

static lxp_result encode_activity(uint64_t sequence, uint32_t activity_type,
                                  lxp_arena *arena, lxp_byte_span *encoded)
{
    uint8_t actor[] = {'q'};
    uint8_t authority[] = {1U};
    uint8_t payload[12];
    uint8_t signature[64];
    uint8_t idempotency_input[20];
    lxp_activity activity;
    lxp_result status;
    if (arena == NULL || encoded == NULL) return LXP_ERR_NON_CANONICAL;
    store_u32(payload, activity_type);
    store_u64(payload + 4U, sequence);
    (void)memcpy(idempotency_input, "LXPQ-IDEMPOTENCY", 16U);
    store_u32(idempotency_input + 16U, activity_type);
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_LEGACY;
    activity.network_id = UINT32_C(77);
    activity.activity_type = activity_type;
    activity.actor_did.bytes = actor;
    activity.actor_did.length = sizeof(actor);
    activity.authority.bytes = authority;
    activity.authority.length = sizeof(authority);
    activity.account_sequence = sequence;
    activity.timestamp_bound.not_before = UINT64_C(1700000000000);
    activity.timestamp_bound.not_after = UINT64_C(1700000100000);
    activity.fee_limit.hi = 0U;
    activity.fee_limit.lo = 1U;
    activity.payload.bytes = payload;
    activity.payload.length = sizeof(payload);
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, idempotency_input,
                             sizeof(idempotency_input),
                             activity.idempotency_key);
    if (status == LXP_OK)
        status = lxp_hash_payload(payload, sizeof(payload),
                                  activity.payload_hash);
    if (status != LXP_OK) return status;
    (void)memcpy(signature, activity.payload_hash, 32U);
    (void)memcpy(signature + 32U, activity.idempotency_key, 32U);
    activity.signature.bytes = signature;
    activity.signature.length = sizeof(signature);
    return lxp_activity_encode(&activity, arena, encoded);
}

static lxp_result compute_record(uint64_t sequence, uint8_t boundary,
                                 const uint8_t *activity_bytes,
                                 size_t activity_length,
                                 const uint8_t previous_state[32],
                                 const uint8_t previous_batch[32],
                                 generated_record *record)
{
    lxp_activity activity;
    uint8_t activity_id[32];
    uint8_t state_input[72];
    lxp_result status;
    if (record == NULL || previous_state == NULL || previous_batch == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_activity_decode(activity_bytes, activity_length, &activity);
    if (status == LXP_OK)
        status = lxp_activity_id(activity_bytes, activity_length, activity_id);
    if (status != LXP_OK) return status;
    (void)memcpy(state_input, previous_state, 32U);
    store_u64(state_input + 32U, sequence);
    (void)memcpy(state_input + 40U, activity_id, 32U);
    status = lxp_hash_domain(LXP_DOMAIN_STATE_ROOT_CHAIN, state_input,
                             sizeof(state_input), record->state_root);
    if (status != LXP_OK) return status;
    record->receipt[0] = 0U;
    record->receipt[1] = 1U;
    store_u64(record->receipt + 2U, sequence);
    (void)memcpy(record->receipt + 10U, activity_id, 32U);
    (void)memcpy(record->receipt + 42U, previous_state, 32U);
    (void)memcpy(record->receipt + 74U, record->state_root, 32U);
    store_u32(record->event, activity.activity_type);
    (void)memcpy(record->event + 4U, activity_id, 32U);
    (void)memset(record->batch_header, 0, sizeof(record->batch_header));
    (void)memset(record->batch_root, 0, sizeof(record->batch_root));
    if (boundary != 0U) {
        (void)memcpy(record->batch_header, previous_batch, 32U);
        (void)memcpy(record->batch_header + 32U, record->state_root, 32U);
        store_u64(record->batch_header + 64U, sequence);
        status = lxp_hash_domain(LXP_DOMAIN_BATCH_HEADER,
                                 record->batch_header,
                                 sizeof(record->batch_header),
                                 record->batch_root);
    }
    return status;
}

static lxp_result write_initial_headers(FILE *corpus, FILE *ledger,
                                        const uint8_t prefix[QUAL_HEADER_PREFIX_BYTES],
                                        uint64_t batch_count)
{
    uint8_t corpus_header[QUAL_HEADER_BYTES] = {0};
    uint8_t root_header[QUAL_ROOT_HEADER_BYTES] = {0};
    if (batch_count > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(corpus_header, prefix, QUAL_HEADER_PREFIX_BYTES);
    (void)memcpy(root_header, root_magic, sizeof(root_magic));
    store_u32(root_header + 8U, QUAL_FORMAT_VERSION);
    store_u32(root_header + 12U, (uint32_t)batch_count);
    if (write_bytes(corpus, corpus_header, sizeof(corpus_header)) != LXP_OK ||
        write_bytes(ledger, root_header, sizeof(root_header)) != LXP_OK)
        return LXP_ERR_IO;
    return LXP_OK;
}

static lxp_result finalize_header(FILE *corpus, const uint8_t prefix[QUAL_HEADER_PREFIX_BYTES],
                                  const lxp_qual_replay_result *result)
{
    uint8_t header[QUAL_HEADER_BYTES] = {0};
    size_t offset = QUAL_HEADER_PREFIX_BYTES;
    if (corpus == NULL || prefix == NULL || result == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(header, prefix, QUAL_HEADER_PREFIX_BYTES);
    (void)memcpy(header + offset, result->activity_digest, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->receipt_digest, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->event_digest, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->batch_digest, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->root_ledger_digest, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->terminal_root, 32U);
    offset += 32U;
    (void)memcpy(header + offset, result->corpus_digest, 32U);
    if (offset + 32U != sizeof(header) || fseek(corpus, 0L, SEEK_SET) != 0)
        return LXP_ERR_IO;
    return write_bytes(corpus, header, sizeof(header));
}

static lxp_result finalize_root_header(FILE *ledger, uint64_t batch_count,
                                       const lxp_qual_replay_result *result)
{
    uint8_t header[QUAL_ROOT_HEADER_BYTES] = {0};
    if (ledger == NULL || result == NULL || batch_count > UINT32_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(header, root_magic, sizeof(root_magic));
    store_u32(header + 8U, QUAL_FORMAT_VERSION);
    store_u32(header + 12U, (uint32_t)batch_count);
    (void)memcpy(header + 16U, result->corpus_digest, 32U);
    if (fseek(ledger, 0L, SEEK_SET) != 0) return LXP_ERR_IO;
    return write_bytes(ledger, header, sizeof(header));
}

static lxp_result finalize_digests(digest_set *digests,
                                   lxp_qual_replay_result *result)
{
    lxp_result status;
    status = lxp_hash_final(&digests->activity, result->activity_digest);
    if (status == LXP_OK)
        status = lxp_hash_final(&digests->receipt, result->receipt_digest);
    if (status == LXP_OK)
        status = lxp_hash_final(&digests->event, result->event_digest);
    if (status == LXP_OK)
        status = lxp_hash_final(&digests->batch, result->batch_digest);
    if (status == LXP_OK)
        status = lxp_hash_final(&digests->ledger,
                                result->root_ledger_digest);
    if (status == LXP_OK)
        status = lxp_hash_final(&digests->corpus, result->corpus_digest);
    return status;
}

static lxp_result update_output_digests(digest_set *digests,
                                        const uint8_t length_bytes[4],
                                        const lxp_byte_span *encoded,
                                        const generated_record *record,
                                        uint8_t boundary)
{
    lxp_result status;
    status = lxp_hash_update(&digests->activity, length_bytes, 4U);
    if (status == LXP_OK)
        status = lxp_hash_update(&digests->activity, encoded->bytes,
                                 encoded->length);
    if (status == LXP_OK)
        status = lxp_hash_update(&digests->receipt, record->receipt,
                                 sizeof(record->receipt));
    if (status == LXP_OK)
        status = lxp_hash_update(&digests->event, record->event,
                                 sizeof(record->event));
    if (status == LXP_OK && boundary != 0U)
        status = lxp_hash_update(&digests->batch, record->batch_header,
                                 sizeof(record->batch_header));
    if (status == LXP_OK && boundary != 0U)
        status = lxp_hash_update(&digests->batch, record->batch_root,
                                 sizeof(record->batch_root));
    return status;
}

static lxp_result write_record(FILE *corpus, FILE *ledger,
                               digest_set *digests, uint64_t sequence,
                               uint8_t boundary, const lxp_byte_span *encoded,
                               const generated_record *record)
{
    uint8_t sequence_bytes[8];
    uint8_t length_bytes[4];
    uint8_t root_entry[QUAL_ROOT_ENTRY_BYTES];
    lxp_result status;
    if (encoded->length > UINT32_MAX) return LXP_ERR_LENGTH_LIMIT;
    store_u64(sequence_bytes, sequence);
    store_u32(length_bytes, (uint32_t)encoded->length);
#define CORPUS_PART(bytes, length) do { \
    status = write_corpus_part(corpus, &digests->corpus, (bytes), (length)); \
    if (status != LXP_OK) return status; \
} while (0)
    CORPUS_PART(sequence_bytes, sizeof(sequence_bytes));
    CORPUS_PART(&boundary, sizeof(boundary));
    CORPUS_PART(length_bytes, sizeof(length_bytes));
    CORPUS_PART(encoded->bytes, encoded->length);
    CORPUS_PART(record->state_root, sizeof(record->state_root));
    CORPUS_PART(record->receipt, sizeof(record->receipt));
    CORPUS_PART(record->event, sizeof(record->event));
    if (boundary != 0U) {
        CORPUS_PART(record->batch_header, sizeof(record->batch_header));
        CORPUS_PART(record->batch_root, sizeof(record->batch_root));
    }
#undef CORPUS_PART
    status = update_output_digests(digests, length_bytes, encoded, record,
                                   boundary);
    if (status != LXP_OK) return status;
    if (boundary != 0U) {
        store_u64(root_entry, sequence);
        (void)memcpy(root_entry + 8U, record->state_root, 32U);
        (void)memcpy(root_entry + 40U, record->batch_root, 32U);
        status = write_root_part(ledger, &digests->ledger, root_entry,
                                 sizeof(root_entry));
    }
    return status;
}

lxp_result lxp_qual_corpus_generate(const char *corpus_path,
                                    const char *root_ledger_path,
                                    uint64_t activity_count,
                                    uint32_t batch_size)
{
    FILE *corpus = NULL;
    FILE *ledger = NULL;
    uint8_t *arena_storage = NULL;
    lxp_arena arena;
    uint8_t prefix[QUAL_HEADER_PREFIX_BYTES];
    uint8_t type_bytes[4];
    uint8_t previous_state[32] = {0};
    uint8_t previous_batch[32] = {0};
    generated_record record;
    digest_set digests;
    lxp_qual_replay_result result;
    uint64_t sequence;
    uint64_t batch_count;
    size_t i;
    lxp_result status;
    if (corpus_path == NULL || root_ledger_path == NULL ||
        activity_count < LXP_QUAL_ACTIVITY_TYPE_COUNT || batch_size == 0U ||
        batch_size > LXP_MAX_BATCH_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    batch_count = activity_count / batch_size;
    if (activity_count % batch_size != 0U) ++batch_count;
    status = header_prefix(prefix, activity_count, batch_size);
    if (status != LXP_OK) return status;
    corpus = fopen(corpus_path, "w+b");
    ledger = fopen(root_ledger_path, "w+b");
    arena_storage = malloc(LXP_MAX_ACTIVITY_BYTES);
    if (corpus == NULL || ledger == NULL || arena_storage == NULL) {
        status = LXP_ERR_IO;
        goto cleanup;
    }
    (void)setvbuf(corpus, NULL, _IOFBF, 1024U * 1024U);
    (void)setvbuf(ledger, NULL, _IOFBF, 64U * 1024U);
    status = lxp_arena_init(&arena, arena_storage, LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK) status = digests_init(&digests);
    if (status == LXP_OK)
        status = write_initial_headers(corpus, ledger, prefix, batch_count);
    if (status == LXP_OK)
        status = lxp_hash_update(&digests.corpus, prefix, sizeof(prefix));
    for (i = 0U; status == LXP_OK && i < LXP_QUAL_ACTIVITY_TYPE_COUNT; ++i) {
        store_u32(type_bytes, activity_types[i]);
        status = write_corpus_part(corpus, &digests.corpus, type_bytes,
                                   sizeof(type_bytes));
    }
    for (sequence = 1U; status == LXP_OK && sequence <= activity_count;
         ++sequence) {
        uint8_t boundary =
            (sequence % batch_size == 0U || sequence == activity_count) ? 1U : 0U;
        uint32_t activity_type = activity_types[(sequence - 1U) %
                                                LXP_QUAL_ACTIVITY_TYPE_COUNT];
        lxp_byte_span encoded;
        status = lxp_arena_reset(&arena, 0U);
        if (status == LXP_OK)
            status = encode_activity(sequence, activity_type, &arena,
                                     &encoded);
        if (status == LXP_OK)
            status = compute_record(sequence, boundary, encoded.bytes,
                                    encoded.length, previous_state,
                                    previous_batch, &record);
        if (status == LXP_OK)
            status = write_record(corpus, ledger, &digests, sequence,
                                  boundary, &encoded, &record);
        if (status == LXP_OK) {
            (void)memcpy(previous_state, record.state_root, 32U);
            if (boundary != 0U)
                (void)memcpy(previous_batch, record.batch_root, 32U);
        }
    }
    (void)memset(&result, 0, sizeof(result));
    result.activity_count = activity_count;
    result.batch_count = batch_count;
    (void)memcpy(result.terminal_root, previous_state, 32U);
    if (status == LXP_OK) status = finalize_digests(&digests, &result);
    if (status == LXP_OK) status = finalize_header(corpus, prefix, &result);
    if (status == LXP_OK)
        status = finalize_root_header(ledger, batch_count, &result);
    if (status == LXP_OK && (fflush(corpus) != 0 || fflush(ledger) != 0))
        status = LXP_ERR_IO;
cleanup:
    if (corpus != NULL && fclose(corpus) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (ledger != NULL && fclose(ledger) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (arena_storage != NULL) {
        (void)memset(arena_storage, 0, LXP_MAX_ACTIVITY_BYTES);
        free(arena_storage);
    }
    return status;
}

static lxp_result parse_header(FILE *file, qual_header *header,
                               uint8_t prefix[QUAL_HEADER_PREFIX_BYTES])
{
    uint8_t bytes[QUAL_HEADER_BYTES];
    size_t offset = QUAL_HEADER_PREFIX_BYTES;
    if (read_bytes(file, bytes, sizeof(bytes)) != LXP_OK)
        return LXP_ERR_IO;
    if (memcmp(bytes, corpus_magic, sizeof(corpus_magic)) != 0 ||
        load_u32(bytes + 8U) != QUAL_FORMAT_VERSION)
        return LXP_ERR_NON_CANONICAL;
    header->activity_count = load_u64(bytes + 12U);
    header->batch_size = load_u32(bytes + 20U);
    header->activity_type_count = load_u32(bytes + 24U);
    if (header->activity_count < LXP_QUAL_ACTIVITY_TYPE_COUNT ||
        header->batch_size == 0U ||
        header->batch_size > LXP_MAX_BATCH_ACTIVITIES ||
        header->activity_type_count != LXP_QUAL_ACTIVITY_TYPE_COUNT)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(prefix, bytes, QUAL_HEADER_PREFIX_BYTES);
    (void)memcpy(header->activity_digest, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->receipt_digest, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->event_digest, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->batch_digest, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->root_ledger_digest, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->terminal_root, bytes + offset, 32U);
    offset += 32U;
    (void)memcpy(header->corpus_digest, bytes + offset, 32U);
    return offset + 32U == sizeof(bytes) ? LXP_OK : LXP_ERR_NON_CANONICAL;
}

static lxp_result read_root_header(FILE *ledger, uint64_t expected_batches,
                                   const uint8_t expected_corpus[32])
{
    uint8_t header[QUAL_ROOT_HEADER_BYTES];
    if (expected_batches > UINT32_MAX ||
        read_bytes(ledger, header, sizeof(header)) != LXP_OK)
        return LXP_ERR_IO;
    if (memcmp(header, root_magic, sizeof(root_magic)) != 0 ||
        load_u32(header + 8U) != QUAL_FORMAT_VERSION ||
        load_u32(header + 12U) != (uint32_t)expected_batches ||
        memcmp(header + 16U, expected_corpus, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

lxp_result lxp_qual_root_ledger(const char *root_ledger_path,
                               uint64_t expected_batch_count,
                               const uint8_t expected_corpus_digest[32],
                               const uint8_t expected_digest[32])
{
    FILE *ledger;
    lxp_hash_context digest;
    uint8_t entry[QUAL_ROOT_ENTRY_BYTES];
    uint8_t actual[32];
    uint64_t i;
    lxp_result status;
    if (root_ledger_path == NULL || expected_corpus_digest == NULL ||
        expected_digest == NULL || expected_batch_count == 0U)
        return LXP_ERR_NON_CANONICAL;
    ledger = fopen(root_ledger_path, "rb");
    if (ledger == NULL) return LXP_ERR_IO;
    status = read_root_header(ledger, expected_batch_count,
                              expected_corpus_digest);
    if (status == LXP_OK)
        status = stream_init(&digest, "LXP/qual/root-ledger/v1");
    for (i = 0U; status == LXP_OK && i < expected_batch_count; ++i) {
        status = read_bytes(ledger, entry, sizeof(entry));
        if (status == LXP_OK)
            status = lxp_hash_update(&digest, entry, sizeof(entry));
    }
    if (status == LXP_OK && fgetc(ledger) != EOF)
        status = LXP_ERR_TRAILING_BYTES;
    if (status == LXP_OK) status = lxp_hash_final(&digest, actual);
    if (status == LXP_OK && memcmp(actual, expected_digest, 32U) != 0)
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (fclose(ledger) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    return status;
}

static lxp_result read_corpus_part(FILE *file, lxp_hash_context *corpus,
                                   void *bytes, size_t length)
{
    lxp_result status = read_bytes(file, bytes, length);
    if (status == LXP_OK) status = lxp_hash_update(corpus, bytes, length);
    return status;
}

static lxp_result compare_record(FILE *corpus, FILE *ledger,
                                 digest_set *digests, lxp_arena *arena,
                                 uint8_t *activity_bytes, uint64_t sequence,
                                 uint32_t batch_size,
                                 uint8_t previous_state[32],
                                 uint8_t previous_batch[32],
                                 uint64_t *batch_count,
                                 bool seen_types[LXP_QUAL_ACTIVITY_TYPE_COUNT])
{
    uint8_t sequence_bytes[8];
    uint8_t boundary;
    uint8_t length_bytes[4];
    uint8_t expected_state[32];
    uint8_t expected_receipt[LXP_QUAL_RECEIPT_BYTES];
    uint8_t expected_event[LXP_QUAL_EVENT_BYTES];
    uint8_t expected_header[LXP_QUAL_BATCH_HEADER_BYTES];
    uint8_t expected_batch[32];
    uint8_t ledger_entry[QUAL_ROOT_ENTRY_BYTES];
    uint32_t activity_length;
    lxp_activity decoded;
    lxp_byte_span reencoded;
    generated_record computed;
    size_t type_index;
    lxp_result status;
#define READ_PART(bytes, length) do { \
    status = read_corpus_part(corpus, &digests->corpus, (bytes), (length)); \
    if (status != LXP_OK) return status; \
} while (0)
    READ_PART(sequence_bytes, sizeof(sequence_bytes));
    READ_PART(&boundary, sizeof(boundary));
    READ_PART(length_bytes, sizeof(length_bytes));
    activity_length = load_u32(length_bytes);
    if (load_u64(sequence_bytes) != sequence || activity_length == 0U ||
        activity_length > LXP_MAX_ACTIVITY_BYTES || boundary > 1U ||
        boundary != ((sequence % batch_size == 0U) ? 1U : 0U))
        return LXP_ERR_NON_CANONICAL;
    READ_PART(activity_bytes, activity_length);
    READ_PART(expected_state, sizeof(expected_state));
    READ_PART(expected_receipt, sizeof(expected_receipt));
    READ_PART(expected_event, sizeof(expected_event));
    if (boundary != 0U) {
        READ_PART(expected_header, sizeof(expected_header));
        READ_PART(expected_batch, sizeof(expected_batch));
    } else {
        (void)memset(expected_header, 0, sizeof(expected_header));
        (void)memset(expected_batch, 0, sizeof(expected_batch));
    }
#undef READ_PART
    status = lxp_activity_decode(activity_bytes, activity_length, &decoded);
    if (status != LXP_OK || decoded.account_sequence != sequence)
        return status == LXP_OK ? LXP_ERR_SEQUENCE_GAP : status;
    for (type_index = 0U; type_index < LXP_QUAL_ACTIVITY_TYPE_COUNT;
         ++type_index) {
        if (decoded.activity_type == activity_types[type_index]) {
            seen_types[type_index] = true;
            break;
        }
    }
    if (type_index == LXP_QUAL_ACTIVITY_TYPE_COUNT)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_arena_reset(arena, 0U);
    if (status == LXP_OK)
        status = lxp_activity_encode(&decoded, arena, &reencoded);
    if (status != LXP_OK || reencoded.length != activity_length ||
        memcmp(reencoded.bytes, activity_bytes, activity_length) != 0)
        return status == LXP_OK ? LXP_FATAL_REPLAY_DIVERGENCE : status;
    status = compute_record(sequence, boundary, activity_bytes,
                            activity_length, previous_state, previous_batch,
                            &computed);
    if (status != LXP_OK) return status;
    if (memcmp(computed.state_root, expected_state, 32U) != 0 ||
        memcmp(computed.receipt, expected_receipt,
               sizeof(expected_receipt)) != 0 ||
        memcmp(computed.event, expected_event, sizeof(expected_event)) != 0 ||
        memcmp(computed.batch_header, expected_header,
               sizeof(expected_header)) != 0 ||
        memcmp(computed.batch_root, expected_batch, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = update_output_digests(digests, length_bytes, &reencoded,
                                   &computed, boundary);
    if (status != LXP_OK) return status;
    (void)memcpy(previous_state, computed.state_root, 32U);
    if (boundary != 0U) {
        (void)memcpy(previous_batch, computed.batch_root, 32U);
        status = read_bytes(ledger, ledger_entry, sizeof(ledger_entry));
        if (status != LXP_OK || load_u64(ledger_entry) != sequence ||
            memcmp(ledger_entry + 8U, computed.state_root, 32U) != 0 ||
            memcmp(ledger_entry + 40U, computed.batch_root, 32U) != 0)
            return status == LXP_OK ? LXP_FATAL_REPLAY_DIVERGENCE : status;
        status = lxp_hash_update(&digests->ledger, ledger_entry,
                                 sizeof(ledger_entry));
        if (status != LXP_OK) return status;
        ++*batch_count;
    }
    return LXP_OK;
}

static bool all_types_seen(const bool seen[LXP_QUAL_ACTIVITY_TYPE_COUNT])
{
    size_t i;
    for (i = 0U; i < LXP_QUAL_ACTIVITY_TYPE_COUNT; ++i)
        if (!seen[i]) return false;
    return true;
}

lxp_result lxp_qual_replay_matrix(const char *corpus_path,
                                  const char *root_ledger_path,
                                  lxp_qual_replay_result *result)
{
    FILE *corpus = NULL;
    FILE *ledger = NULL;
    uint8_t *arena_storage = NULL;
    uint8_t *activity_bytes = NULL;
    lxp_arena arena;
    qual_header header = {0};
    uint8_t prefix[QUAL_HEADER_PREFIX_BYTES];
    uint8_t type_bytes[4];
    uint8_t previous_state[32] = {0};
    uint8_t previous_batch[32] = {0};
    bool seen_types[LXP_QUAL_ACTIVITY_TYPE_COUNT] = {false};
    digest_set digests;
    uint64_t expected_batches = 0U;
    uint64_t sequence;
    size_t i;
    lxp_result status;
    if (corpus_path == NULL || root_ledger_path == NULL || result == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(result, 0, sizeof(*result));
    corpus = fopen(corpus_path, "rb");
    ledger = fopen(root_ledger_path, "rb");
    arena_storage = malloc(LXP_MAX_ACTIVITY_BYTES);
    activity_bytes = malloc(LXP_MAX_ACTIVITY_BYTES);
    if (corpus == NULL || ledger == NULL || arena_storage == NULL ||
        activity_bytes == NULL) {
        status = LXP_ERR_IO;
        goto cleanup;
    }
    (void)setvbuf(corpus, NULL, _IOFBF, 1024U * 1024U);
    status = parse_header(corpus, &header, prefix);
    if (status == LXP_OK) {
        expected_batches = header.activity_count / header.batch_size;
        if (header.activity_count % header.batch_size != 0U)
            ++expected_batches;
    }
    if (status == LXP_OK)
        status = read_root_header(ledger, expected_batches,
                                  header.corpus_digest);
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_storage,
                                LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK) status = digests_init(&digests);
    if (status == LXP_OK)
        status = lxp_hash_update(&digests.corpus, prefix, sizeof(prefix));
    for (i = 0U; status == LXP_OK && i < LXP_QUAL_ACTIVITY_TYPE_COUNT; ++i) {
        status = read_corpus_part(corpus, &digests.corpus, type_bytes,
                                  sizeof(type_bytes));
        if (status == LXP_OK && load_u32(type_bytes) != activity_types[i])
            status = LXP_ERR_NON_CANONICAL;
    }
    for (sequence = 1U; status == LXP_OK && sequence <= header.activity_count;
         ++sequence) {
        status = compare_record(corpus, ledger, &digests, &arena,
                                activity_bytes, sequence, header.batch_size,
                                previous_state, previous_batch,
                                &result->batch_count, seen_types);
        if (status != LXP_OK) result->first_divergent_sequence = sequence;
    }
    if (status == LXP_OK && (!all_types_seen(seen_types) ||
        result->batch_count != expected_batches))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK && (fgetc(corpus) != EOF || fgetc(ledger) != EOF))
        status = LXP_ERR_TRAILING_BYTES;
    result->activity_count = header.activity_count;
    (void)memcpy(result->terminal_root, previous_state, 32U);
    if (status == LXP_OK) status = finalize_digests(&digests, result);
    if (status == LXP_OK &&
        (memcmp(result->activity_digest, header.activity_digest, 32U) != 0 ||
         memcmp(result->receipt_digest, header.receipt_digest, 32U) != 0 ||
         memcmp(result->event_digest, header.event_digest, 32U) != 0 ||
         memcmp(result->batch_digest, header.batch_digest, 32U) != 0 ||
         memcmp(result->root_ledger_digest, header.root_ledger_digest, 32U) != 0 ||
         memcmp(result->terminal_root, header.terminal_root, 32U) != 0 ||
         memcmp(result->corpus_digest, header.corpus_digest, 32U) != 0))
        status = LXP_FATAL_REPLAY_DIVERGENCE;
    if (status == LXP_OK)
        status = lxp_qual_root_ledger(root_ledger_path, expected_batches,
                                      header.corpus_digest,
                                      header.root_ledger_digest);
cleanup:
    if (corpus != NULL && fclose(corpus) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (ledger != NULL && fclose(ledger) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (arena_storage != NULL) {
        (void)memset(arena_storage, 0, LXP_MAX_ACTIVITY_BYTES);
        free(arena_storage);
    }
    if (activity_bytes != NULL) {
        (void)memset(activity_bytes, 0, LXP_MAX_ACTIVITY_BYTES);
        free(activity_bytes);
    }
    return status;
}
