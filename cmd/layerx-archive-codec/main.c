#define _GNU_SOURCE
#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_activity.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_batch_identity.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_maintenance.h"
#include "layerx/lxp_protocol.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_replica.h"
#include "layerx/lxp_result.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_state.h"
#include "layerx/programs.h"

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <poll.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

enum {
    CODEC_USAGE_EXIT = 64,
    CODEC_MISSING_BATCH_EXIT = 3,
    CODEC_LNI_MAJOR = 1,
    CODEC_LNI_MINOR = 5,
    CODEC_LNI_NODE_INFO_REQUEST = 1,
    CODEC_LNI_NODE_INFO_RESPONSE = 2,
    CODEC_LNI_SUBMIT_REQUEST = 3,
    CODEC_LNI_SUBMIT_RESPONSE = 4,
    CODEC_LNI_ERROR_RESPONSE = 25,
    CODEC_LNI_ENVELOPE_BYTES = 22,
    CODEC_LNI_NODE_INFO_BYTES = 93,
    CODEC_LNI_TIMEOUT_MS = 30000,
    CODEC_VERIFY_ARENA_BYTES = 64 * 1024 * 1024,
    CODEC_EXPORT_ARENA_BYTES = 36 * 1024 * 1024,
    CODEC_GENESIS_EXTRA_ARENA_BYTES = 16 * 1024 * 1024,
    CODEC_GENESIS_MAX_SNAPSHOT_BYTES =
        LXP_SNAPSHOT_MAX_BLOB_TOTAL_BYTES + 16 * 1024 * 1024
};

typedef struct activity_metadata {
    lxp_byte_span actor;
    uint8_t activity_id[32];
    uint8_t actor_account[32];
    uint16_t module;
    uint16_t ordinal;
} activity_metadata;

typedef struct receipt_metadata {
    uint64_t sequence;
    lxp_result result_code;
    uint8_t from[32];
    uint8_t to[32];
} receipt_metadata;

typedef struct lni_envelope {
    uint16_t major;
    uint16_t minor;
    uint16_t tag;
    uint64_t correlation_id;
    const uint8_t *payload;
    size_t payload_length;
    const uint8_t *proof;
    size_t proof_length;
    uint8_t *owned;
} lni_envelope;

static int usage(void)
{
    (void)fprintf(stderr,
        "usage: layerx-archive-codec export LOG BATCH\n"
        "       layerx-archive-codec verify NETWORK SEQUENCER_ID PUBLIC_KEY FIRST_BATCH LAST_BATCH\n"
        "       layerx-archive-codec genesis NETWORK PUBLIC_KEY MANIFEST SNAPSHOT\n"
        "       layerx-archive-codec activity\n"
        "       layerx-archive-codec submit SOCKET\n");
    return CODEC_USAGE_EXIT;
}

static int failure(const char *stage, lxp_result status)
{
    (void)fprintf(stderr, "layerx-archive-codec: %s refused (%s, %" PRId32 ")\n",
                  stage, lxp_result_name(status), status);
    return 1;
}

static int parse_u64(const char *text, uint64_t *value)
{
    char *end = NULL;
    unsigned long long parsed;
    if (text == NULL || value == NULL || text[0] == '\0' || text[0] == '-')
        return 0;
    errno = 0;
    parsed = strtoull(text, &end, 10);
    if (errno != 0 || end == text || *end != '\0') return 0;
    *value = (uint64_t)parsed;
    return (unsigned long long)*value == parsed;
}

static int parse_u32(const char *text, uint32_t *value)
{
    uint64_t parsed;
    if (!parse_u64(text, &parsed) || parsed > UINT32_MAX) return 0;
    *value = (uint32_t)parsed;
    return 1;
}

static int hex_nibble(unsigned char byte)
{
    if (byte >= (unsigned char)'0' && byte <= (unsigned char)'9')
        return (int)(byte - (unsigned char)'0');
    if (byte >= (unsigned char)'a' && byte <= (unsigned char)'f')
        return (int)(byte - (unsigned char)'a') + 10;
    if (byte >= (unsigned char)'A' && byte <= (unsigned char)'F')
        return (int)(byte - (unsigned char)'A') + 10;
    return -1;
}

static int parse_hex32(const char *text, uint8_t value[32])
{
    size_t index;
    if (text == NULL || value == NULL || strlen(text) != 64U) return 0;
    for (index = 0U; index < 32U; ++index) {
        int high = hex_nibble((unsigned char)text[index * 2U]);
        int low = hex_nibble((unsigned char)text[index * 2U + 1U]);
        if (high < 0 || low < 0) return 0;
        value[index] = (uint8_t)(((unsigned int)high << 4U) |
                                 (unsigned int)low);
    }
    return 1;
}

static void print_hex(const uint8_t *bytes, size_t length)
{
    static const char digits[] = "0123456789abcdef";
    size_t index;
    for (index = 0U; index < length; ++index) {
        (void)fputc(digits[bytes[index] >> 4U], stdout);
        (void)fputc(digits[bytes[index] & 15U], stdout);
    }
}

static void print_json_bytes(const uint8_t *bytes, size_t length)
{
    static const char digits[] = "0123456789abcdef";
    size_t index;
    (void)fputc('"', stdout);
    for (index = 0U; index < length; ++index) {
        uint8_t byte = bytes[index];
        if (byte == (uint8_t)'"' || byte == (uint8_t)'\\') {
            (void)fputc('\\', stdout);
            (void)fputc((int)byte, stdout);
        } else if (byte >= 0x20U && byte <= 0x7eU) {
            (void)fputc((int)byte, stdout);
        } else {
            (void)fputs("\\u00", stdout);
            (void)fputc(digits[byte >> 4U], stdout);
            (void)fputc(digits[byte & 15U], stdout);
        }
    }
    (void)fputc('"', stdout);
}

static lxp_result read_stdin_bounded(size_t maximum, uint8_t **bytes,
                                     size_t *length)
{
    uint8_t *memory;
    size_t count = 0U;
    int extra;
    if (bytes == NULL || length == NULL || maximum == 0U)
        return LXP_ERR_NON_CANONICAL;
    memory = (uint8_t *)malloc(maximum);
    if (memory == NULL) return LXP_ERR_IO;
    while (count < maximum) {
        size_t received = fread(memory + count, 1U, maximum - count, stdin);
        count += received;
        if (received == 0U) break;
    }
    if (ferror(stdin)) {
        free(memory);
        return LXP_ERR_IO;
    }
    extra = fgetc(stdin);
    if (extra != EOF) {
        free(memory);
        return LXP_ERR_LENGTH_LIMIT;
    }
    if (ferror(stdin)) {
        free(memory);
        return LXP_ERR_IO;
    }
    if (count == 0U) {
        free(memory);
        return LXP_ERR_TRUNCATED;
    }
    *bytes = memory;
    *length = count;
    return LXP_OK;
}

static lxp_result read_file_bounded(const char *path, size_t maximum,
                                    uint8_t **bytes, size_t *length)
{
    struct stat information;
    uint8_t *memory = NULL;
    size_t consumed = 0U;
    int descriptor;
    if (path == NULL || bytes == NULL || length == NULL || maximum == 0U)
        return LXP_ERR_NON_CANONICAL;
    descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        !S_ISREG(information.st_mode) || information.st_nlink != 1 ||
        information.st_size <= 0 || (uint64_t)information.st_size > maximum) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    memory = (uint8_t *)malloc((size_t)information.st_size);
    if (memory == NULL) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    while (consumed < (size_t)information.st_size) {
        ssize_t result = read(descriptor, memory + consumed,
                              (size_t)information.st_size - consumed);
        if (result < 0 && errno == EINTR) continue;
        if (result <= 0) {
            free(memory);
            (void)close(descriptor);
            return LXP_ERR_IO;
        }
        consumed += (size_t)result;
    }
    if (close(descriptor) != 0) {
        free(memory);
        return LXP_ERR_IO;
    }
    *bytes = memory;
    *length = consumed;
    return LXP_OK;
}

static lxp_result snapshot_file_size(const char *path, size_t *length)
{
    struct stat information;
    if (path == NULL || length == NULL || lstat(path, &information) != 0 ||
        !S_ISREG(information.st_mode) || information.st_nlink != 1 ||
        information.st_size <= 0 ||
        (uint64_t)information.st_size > CODEC_GENESIS_MAX_SNAPSHOT_BYTES)
        return LXP_ERR_IO;
    *length = (size_t)information.st_size;
    return LXP_OK;
}

static lxp_result actor_account_id(const lxp_activity *activity,
                                   uint8_t account_id[32])
{
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    static const uint8_t prefix[] = "agent:";
    static const uint8_t suffix[] = ":main";
    size_t length;
    if (activity == NULL || account_id == NULL ||
        activity->actor_did.bytes == NULL || activity->actor_did.length == 0U ||
        activity->actor_did.length > sizeof(name) -
            (sizeof(prefix) - 1U) - (sizeof(suffix) - 1U))
        return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    length = sizeof(prefix) - 1U;
    (void)memcpy(name, prefix, length);
    (void)memcpy(name + length, activity->actor_did.bytes,
                 activity->actor_did.length);
    length += activity->actor_did.length;
    (void)memcpy(name + length, suffix, sizeof(suffix) - 1U);
    length += sizeof(suffix) - 1U;
    return lx_account_id_from_string(name, length, account_id);
}

static lxp_result validate_activity(const uint8_t *bytes, size_t length,
                                    lxp_arena *arena, lxp_activity *activity,
                                    uint8_t activity_id[32],
                                    uint8_t actor_account[32])
{
    lxp_byte_span reencoded;
    lxp_result status;
    if (bytes == NULL || length == 0U || length > LXP_MAX_ACTIVITY_BYTES ||
        arena == NULL || activity == NULL || activity_id == NULL ||
        actor_account == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_activity_decode(bytes, length, activity);
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(activity, activity->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK)
        status = lxp_activity_encode(activity, arena, &reencoded);
    if (status == LXP_OK &&
        (reencoded.length != length ||
         lxp_ct_memcmp(reencoded.bytes, bytes, length) != 0))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK)
        status = lxp_activity_id(bytes, length, activity_id);
    if (status == LXP_OK)
        status = actor_account_id(activity, actor_account);
    return status;
}

static lxp_result prepare_readonly_log(lxp_log *log)
{
    uint64_t valid_end;
    uint64_t last_record_offset;
    uint64_t next_sequence;
    lxp_result status;
    if (log == NULL || log->descriptor < 0)
        return LXP_ERR_NON_CANONICAL;
    if (log->has_durable_marker)
        return lxp_log_recover_complete_records(log, NULL, NULL);
    status = lxp_log_scan_tail(log, &valid_end, &last_record_offset,
                               &next_sequence);
    if (status != LXP_OK) return status;
    log->write_offset = valid_end;
    log->previous_record_offset = last_record_offset;
    log->next_sequence = next_sequence;
    return LXP_OK;
}

static int command_export(const char *path, const char *batch_text)
{
    uint8_t *arena_memory;
    uint64_t batch_number;
    lxp_arena arena;
    lxp_batch_body body;
    lxp_byte_span encoded;
    lxp_log log;
    lxp_result status;
    bool opened = false;
    if (!parse_u64(batch_text, &batch_number) || batch_number == 0U)
        return usage();
    arena_memory = (uint8_t *)malloc(CODEC_EXPORT_ARENA_BYTES);
    if (arena_memory == NULL) return failure("export memory", LXP_ERR_IO);
    status = lxp_arena_init(&arena, arena_memory, CODEC_EXPORT_ARENA_BYTES);
    if (status == LXP_OK) {
        status = lxp_log_open_readonly(&log, path);
        opened = status == LXP_OK;
    }
    if (status == LXP_OK) status = prepare_readonly_log(&log);
    if (status == LXP_OK)
        status = lxp_da_log_read_body(&log, batch_number, &arena, &body);
    if (status == LXP_OK)
        status = lxp_batch_body_encode(&body, &arena, &encoded);
    if (opened) {
        lxp_result close_status = lxp_log_close(&log);
        if (status == LXP_OK) status = close_status;
    }
    if (status == LXP_OK &&
        fwrite(encoded.bytes, 1U, encoded.length, stdout) != encoded.length)
        status = LXP_ERR_IO;
    if (status == LXP_OK && fflush(stdout) != 0) status = LXP_ERR_IO;
    lxp_secure_zero(arena_memory, CODEC_EXPORT_ARENA_BYTES);
    free(arena_memory);
    if (status == LXP_ERR_DA_MISSING) {
        (void)fprintf(stderr,
            "layerx-archive-codec: export batch %" PRIu64 " is not available\n",
            batch_number);
        return CODEC_MISSING_BATCH_EXIT;
    }
    return status == LXP_OK ? 0 : failure("export", status);
}

static int account_seen(const uint8_t accounts[3][32], size_t count,
                        const uint8_t candidate[32])
{
    size_t index;
    for (index = 0U; index < count; ++index)
        if (lxp_ct_memcmp(accounts[index], candidate, 32U) == 0) return 1;
    return 0;
}

static void print_activity_accounts(const activity_metadata *activity,
                                    const receipt_metadata *receipt)
{
    uint8_t accounts[3][32];
    size_t count = 0U;
    size_t index;
    (void)memcpy(accounts[count++], activity->actor_account, 32U);
    if (!lxp_ct_is_zero(receipt->from, 32U) &&
        !account_seen((const uint8_t (*)[32])accounts, count, receipt->from))
        (void)memcpy(accounts[count++], receipt->from, 32U);
    if (!lxp_ct_is_zero(receipt->to, 32U) &&
        !account_seen((const uint8_t (*)[32])accounts, count, receipt->to))
        (void)memcpy(accounts[count++], receipt->to, 32U);
    (void)fputc('[', stdout);
    for (index = 0U; index < count; ++index) {
        if (index != 0U) (void)fputc(',', stdout);
        (void)fputc('"', stdout);
        print_hex(accounts[index], 32U);
        (void)fputc('"', stdout);
    }
    (void)fputc(']', stdout);
}

static lxp_result verify_batch(const uint8_t *canonical, size_t canonical_length,
                               uint32_t network_id,
                               const uint8_t sequencer_id[32],
                               const uint8_t public_key[32],
                               uint64_t first_batch, uint64_t last_batch,
                               lxp_arena *arena)
{
    lxp_batch_body body;
    lxp_byte_span reencoded;
    lxp_byte_span header_encoded;
    lxp_byte_span *activities = NULL;
    lxp_byte_span *receipts = NULL;
    lxp_byte_span *receipt_events = NULL;
    lxp_byte_span *body_events = NULL;
    lxp_byte_span *oracles = NULL;
    size_t activity_count = 0U;
    size_t receipt_count = 0U;
    size_t receipt_event_count = 0U;
    size_t body_event_count = 0U;
    size_t oracle_count = 0U;
    size_t index;
    size_t mark;
    bool maintenance_present;
    bool maintenance_known = false;
    bool maintenance_envelope = false;
    lxp_batch_roots roots;
    lxp_sequencer_authorization authorization;
    lxp_programs_occupancy_receipt maintenance;
    activity_metadata *activity_meta = NULL;
    receipt_metadata *receipt_meta = NULL;
    uint8_t availability_root[32];
    uint8_t batch_id[32];
    uint64_t committed_last_sequence;
    lxp_result status;

    status = lxp_batch_body_decode(canonical, canonical_length, &body);
    if (status == LXP_OK) {
        mark = lxp_arena_mark(arena);
        status = lxp_batch_body_encode(&body, arena, &reencoded);
        if (status == LXP_OK &&
            (reencoded.length != canonical_length ||
             lxp_ct_memcmp(reencoded.bytes, canonical, canonical_length) != 0))
            status = LXP_ERR_NON_CANONICAL;
        (void)lxp_arena_reset(arena, mark);
    }
    if (status == LXP_OK &&
        (body.header.network_id != network_id ||
         body.header.batch_number < first_batch ||
         body.header.batch_number > last_batch ||
         lxp_ct_memcmp(body.header.sequencer_id, sequencer_id, 32U) != 0))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK &&
        (!lxp_protocol_version_supported(body.header.protocol_version) ||
         body.header.epoch == 0U || body.header.batch_number == 0U ||
         body.header.first_sequence == 0U ||
         body.header.last_sequence < body.header.first_sequence ||
         body.header.timestamp_ms == 0U))
        status = LXP_ERR_NON_CANONICAL;
    (void)memset(&authorization, 0, sizeof(authorization));
    if (status == LXP_OK) {
        (void)memcpy(authorization.sequencer_id, sequencer_id, 32U);
        (void)memcpy(authorization.public_key, public_key, 32U);
        authorization.first_batch_number = first_batch;
        authorization.last_batch_number = last_batch;
        authorization.authorized = 1U;
        mark = lxp_arena_mark(arena);
        status = lxp_batch_verify_signature(
            &body.header, body.sequencer_signature,
            sizeof(body.sequencer_signature), &authorization, arena);
        (void)lxp_arena_reset(arena, mark);
    }
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body.activities, arena,
                                           &activities, &activity_count);
    if (status == LXP_OK)
        status = lxp_da_receipt_section_decode(
            body.receipts, arena, &receipts, &receipt_count,
            &receipt_events, &receipt_event_count);
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body.events, arena,
                                           &body_events, &body_event_count);
    if (status == LXP_OK)
        status = lxp_replay_section_decode(&body.oracle_inputs, arena,
                                           &oracles, &oracle_count);
    if (status == LXP_OK && receipt_event_count != body_event_count)
        status = LXP_ERR_CONTEXT_MISMATCH;
    for (index = 0U; status == LXP_OK && index < body_event_count; ++index)
        if (receipt_events[index].length != body_events[index].length ||
            lxp_ct_memcmp(receipt_events[index].bytes,
                          body_events[index].bytes,
                          body_events[index].length) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK &&
        (receipt_count < activity_count ||
         receipt_count > activity_count + 1U))
        status = LXP_ERR_BATCH_GAP;
    maintenance_present = status == LXP_OK &&
        receipt_count == activity_count + 1U;
    if (status == LXP_OK && maintenance_present) {
        if (body.header.last_sequence - body.header.first_sequence !=
            (uint64_t)activity_count)
            status = LXP_ERR_BATCH_GAP;
    } else if (status == LXP_OK &&
               (activity_count == 0U ||
                body.header.last_sequence - body.header.first_sequence !=
                    (uint64_t)(activity_count - 1U))) {
        status = LXP_ERR_BATCH_GAP;
    }
    if (status == LXP_OK) {
        lxp_batch_root_inputs inputs = {
            activities, activity_count,
            receipts, receipt_count,
            body_events, body_event_count,
            oracles, oracle_count,
            NULL, 0U
        };
        status = lxp_batch_roots_compute(&inputs, arena, &roots);
    }
    if (status == LXP_OK)
        status = lxp_batch_availability_root(&body, arena,
                                             availability_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(roots.activity_merkle_root,
                       body.header.activity_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.receipt_merkle_root,
                       body.header.receipt_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.event_merkle_root,
                       body.header.event_merkle_root, 32U) != 0 ||
         lxp_ct_memcmp(roots.oracle_root,
                       body.header.oracle_root, 32U) != 0 ||
         lxp_ct_memcmp(availability_root,
                       body.header.data_availability_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_batch_identity_committed_last_sequence(
            body.header.first_sequence, body.header.last_sequence,
            maintenance_present, &committed_last_sequence);
    if (status == LXP_OK)
        status = lxp_batch_identity_committed(
            body.header.previous_state_root,
            body.header.activity_merkle_root,
            body.header.first_sequence, committed_last_sequence,
            body.header.batch_number, batch_id);
    if (status == LXP_OK && activity_count != 0U) {
        activity_meta = (activity_metadata *)calloc(
            activity_count, sizeof(*activity_meta));
        receipt_meta = (receipt_metadata *)calloc(
            activity_count, sizeof(*receipt_meta));
        if (activity_meta == NULL || receipt_meta == NULL)
            status = LXP_ERR_IO;
    }
    for (index = 0U; status == LXP_OK && index < activity_count; ++index) {
        lxp_activity decoded;
        mark = lxp_arena_mark(arena);
        status = validate_activity(
            activities[index].bytes, activities[index].length, arena,
            &decoded, activity_meta[index].activity_id,
            activity_meta[index].actor_account);
        if (status == LXP_OK &&
            (decoded.network_id != body.header.network_id ||
             decoded.protocol_version != body.header.protocol_version))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK) {
            activity_meta[index].actor = decoded.actor_did;
            activity_meta[index].module =
                lxp_activity_module_id(decoded.activity_type);
            activity_meta[index].ordinal =
                lxp_activity_type_ordinal(decoded.activity_type);
        }
        (void)lxp_arena_reset(arena, mark);
    }
    for (index = 0U; status == LXP_OK && index < activity_count; ++index) {
        lxp_receipt decoded;
        lxp_byte_span encoded;
        mark = lxp_arena_mark(arena);
        status = lxp_receipt_decode(receipts[index].bytes,
                                    receipts[index].length, true, &decoded);
        if (status == LXP_OK)
            status = lxp_receipt_verify(&decoded, public_key, arena);
        if (status == LXP_OK)
            status = lxp_receipt_encode(&decoded, true, arena, &encoded);
        if (status == LXP_OK &&
            (encoded.length != receipts[index].length ||
             lxp_ct_memcmp(encoded.bytes, receipts[index].bytes,
                           encoded.length) != 0))
            status = LXP_ERR_NON_CANONICAL;
        if (status == LXP_OK &&
            (decoded.protocol_version != body.header.protocol_version ||
             decoded.global_sequence != body.header.first_sequence + index ||
             decoded.module_id != activity_meta[index].module ||
             lxp_ct_memcmp(decoded.activity_id,
                           activity_meta[index].activity_id, 32U) != 0 ||
             lxp_ct_memcmp(decoded.activity_root,
                           body.header.activity_merkle_root, 32U) != 0 ||
             lxp_ct_memcmp(decoded.batch_id, batch_id, 32U) != 0 ||
             (index == 0U &&
              lxp_ct_memcmp(decoded.previous_state_root,
                            body.header.previous_state_root, 32U) != 0)))
            status = LXP_ERR_CONTEXT_MISMATCH;
        if (status == LXP_OK && index != 0U) {
            lxp_receipt previous;
            status = lxp_receipt_decode(receipts[index - 1U].bytes,
                                        receipts[index - 1U].length,
                                        true, &previous);
            if (status == LXP_OK &&
                lxp_ct_memcmp(previous.resulting_state_root,
                              decoded.previous_state_root, 32U) != 0)
                status = LXP_ERR_CONTEXT_MISMATCH;
        }
        if (status == LXP_OK) {
            receipt_meta[index].sequence = decoded.global_sequence;
            receipt_meta[index].result_code = decoded.result_code;
            (void)memcpy(receipt_meta[index].from, decoded.from, 32U);
            (void)memcpy(receipt_meta[index].to, decoded.to, 32U);
        }
        (void)lxp_arena_reset(arena, mark);
    }
    (void)memset(&maintenance, 0, sizeof(maintenance));
    if (status == LXP_OK && maintenance_present) {
        lxp_byte_span encoded = receipts[activity_count];
        maintenance_envelope = lxp_batch_maintenance_is_envelope(encoded);
        status = lxp_batch_maintenance_occupancy_decode(
            encoded.bytes, encoded.length, &maintenance);
        if (status == LXP_OK) {
            const uint8_t *expected_previous = body.header.previous_state_root;
            lxp_receipt previous;
            maintenance_known = true;
            if (activity_count != 0U) {
                status = lxp_receipt_decode(
                    receipts[activity_count - 1U].bytes,
                    receipts[activity_count - 1U].length, true, &previous);
                if (status == LXP_OK)
                    expected_previous = previous.resulting_state_root;
            }
            if (status == LXP_OK &&
                (maintenance.batch_number != body.header.batch_number ||
                 maintenance.global_sequence != body.header.last_sequence ||
                 lxp_ct_memcmp(maintenance.previous_state_root,
                               expected_previous, 32U) != 0 ||
                 lxp_ct_memcmp(maintenance.resulting_state_root,
                               body.header.resulting_state_root, 32U) != 0))
                status = LXP_ERR_CONTEXT_MISMATCH;
        } else if (!maintenance_envelope) {
            maintenance_known = false;
            status = LXP_OK;
        }
    }
    if (status == LXP_OK && !maintenance_present && activity_count != 0U) {
        lxp_receipt last;
        status = lxp_receipt_decode(receipts[activity_count - 1U].bytes,
                                    receipts[activity_count - 1U].length,
                                    true, &last);
        if (status == LXP_OK &&
            lxp_ct_memcmp(last.resulting_state_root,
                          body.header.resulting_state_root, 32U) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
    }
    if (status == LXP_OK) {
        uint8_t header_storage[LXP_BATCH_HEADER_ENCODED_SIZE];
        lxp_arena header_arena;
        status = lxp_arena_init(&header_arena, header_storage,
                                sizeof(header_storage));
        if (status == LXP_OK)
            status = lxp_batch_header_encode(&body.header, &header_arena,
                                             &header_encoded);
        if (status == LXP_OK) {
            (void)fputs("{\"batch_number\":\"", stdout);
            (void)fprintf(stdout, "%" PRIu64, body.header.batch_number);
            (void)fputs("\",\"batch_id\":\"", stdout);
            print_hex(batch_id, 32U);
            (void)fputs("\",\"first_sequence\":\"", stdout);
            (void)fprintf(stdout, "%" PRIu64, body.header.first_sequence);
            (void)fputs("\",\"last_sequence\":\"", stdout);
            (void)fprintf(stdout, "%" PRIu64, body.header.last_sequence);
            (void)fputs("\",\"previous_state_root\":\"", stdout);
            print_hex(body.header.previous_state_root, 32U);
            (void)fputs("\",\"resulting_state_root\":\"", stdout);
            print_hex(body.header.resulting_state_root, 32U);
            (void)fprintf(stdout,
                "\",\"network_id\":%" PRIu32
                ",\"protocol_version\":%" PRIu16
                ",\"epoch\":\"%" PRIu64
                "\",\"timestamp_ms\":\"%" PRIu64
                "\",\"sequencer_id\":\"",
                body.header.network_id, body.header.protocol_version,
                body.header.epoch, body.header.timestamp_ms);
            print_hex(body.header.sequencer_id, 32U);
            (void)fputs("\",\"header_hex\":\"", stdout);
            print_hex(header_encoded.bytes, header_encoded.length);
            (void)fputs("\",\"signature_hex\":\"", stdout);
            print_hex(body.sequencer_signature,
                      sizeof(body.sequencer_signature));
            (void)fputs("\",\"activities\":[", stdout);
            for (index = 0U; index < activity_count; ++index) {
                if (index != 0U) (void)fputc(',', stdout);
                (void)fputs("{\"activity_id\":\"", stdout);
                print_hex(activity_meta[index].activity_id, 32U);
                (void)fputs("\",\"sequence\":\"", stdout);
                (void)fprintf(stdout, "%" PRIu64,
                              receipt_meta[index].sequence);
                (void)fputs("\",\"actor\":", stdout);
                print_json_bytes(activity_meta[index].actor.bytes,
                                 activity_meta[index].actor.length);
                (void)fprintf(stdout,
                    ",\"module\":%" PRIu16 ",\"ordinal\":%" PRIu16,
                    activity_meta[index].module,
                    activity_meta[index].ordinal);
                (void)fputs(",\"canonical_hex\":\"", stdout);
                print_hex(activities[index].bytes, activities[index].length);
                (void)fputs("\",\"receipt_hex\":\"", stdout);
                print_hex(receipts[index].bytes, receipts[index].length);
                (void)fprintf(stdout, "\",\"result_code\":%" PRId32
                              ",\"accounts\":",
                              receipt_meta[index].result_code);
                print_activity_accounts(&activity_meta[index],
                                        &receipt_meta[index]);
                (void)fputc('}', stdout);
            }
            (void)fputs("],\"maintenance\":[", stdout);
            if (maintenance_present) {
                (void)fputs("{\"sequence\":\"", stdout);
                (void)fprintf(stdout, "%" PRIu64,
                    maintenance_known ? maintenance.global_sequence :
                                        body.header.last_sequence);
                (void)fputs("\",\"kind\":\"", stdout);
                (void)fputs(maintenance_known ?
                    (maintenance_envelope ? "batch_maintenance" :
                                            "occupancy_maintenance") :
                    "unknown_receipt", stdout);
                (void)fputs("\",\"receipt_hex\":\"", stdout);
                print_hex(receipts[activity_count].bytes,
                          receipts[activity_count].length);
                (void)fputs("\",\"result_code\":null}", stdout);
            }
            (void)fputs("]}\n", stdout);
            if (ferror(stdout) || fflush(stdout) != 0) status = LXP_ERR_IO;
        }
    }
    free(receipt_meta);
    free(activity_meta);
    return status;
}

static int command_verify(int argc, char **argv)
{
    uint32_t network_id;
    uint8_t sequencer_id[32];
    uint8_t public_key[32];
    uint64_t first_batch;
    uint64_t last_batch;
    uint8_t *canonical = NULL;
    size_t canonical_length = 0U;
    uint8_t *arena_memory = NULL;
    lxp_arena arena;
    lxp_result status;
    if (argc != 7 || !parse_u32(argv[2], &network_id) || network_id == 0U ||
        !parse_hex32(argv[3], sequencer_id) ||
        !parse_hex32(argv[4], public_key) ||
        !parse_u64(argv[5], &first_batch) || first_batch == 0U ||
        !parse_u64(argv[6], &last_batch) || last_batch < first_batch)
        return usage();
    status = read_stdin_bounded(LXP_MAX_BATCH_BODY_BYTES,
                                &canonical, &canonical_length);
    if (status == LXP_OK) {
        arena_memory = (uint8_t *)malloc(CODEC_VERIFY_ARENA_BYTES);
        if (arena_memory == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_memory,
                                CODEC_VERIFY_ARENA_BYTES);
    if (status == LXP_OK)
        status = verify_batch(canonical, canonical_length, network_id,
                              sequencer_id, public_key, first_batch,
                              last_batch, &arena);
    if (arena_memory != NULL) {
        lxp_secure_zero(arena_memory, CODEC_VERIFY_ARENA_BYTES);
        free(arena_memory);
    }
    if (canonical != NULL) {
        lxp_secure_zero(canonical, canonical_length);
        free(canonical);
    }
    return status == LXP_OK ? 0 : failure("verify", status);
}

static int command_genesis(int argc, char **argv)
{
    uint32_t network_id;
    uint8_t public_key[32];
    uint8_t *manifest_bytes = NULL;
    size_t manifest_length = 0U;
    size_t snapshot_file_bytes = 0U;
    size_t arena_capacity = 0U;
    uint8_t *arena_memory = NULL;
    lxp_genesis_manifest *manifest = NULL;
    lxp_snapshot_manifest_record snapshot_manifest;
    lxp_byte_span snapshot;
    lx_account_registry *accounts = NULL;
    lxp_state_store *state = NULL;
    lxp_state_journal *journal = NULL;
    lxp_kernel *kernel = NULL;
    lxp_genesis_module_plan plan;
    lxp_arena arena;
    uint8_t computed_state_root[32];
    uint8_t computed_receipt_root[32];
    uint8_t live_state_root[32];
    uint8_t manifest_commitment[32];
    bool accounts_open = false;
    bool state_open = false;
    bool snapshot_loaded = false;
    lxp_result status;
    size_t index;
    if (argc != 6 || !parse_u32(argv[2], &network_id) || network_id == 0U ||
        !parse_hex32(argv[3], public_key))
        return usage();
    status = read_file_bounded(argv[4], LXP_GENESIS_MAX_ENCODED_BYTES,
                               &manifest_bytes, &manifest_length);
    if (status == LXP_OK)
        status = snapshot_file_size(argv[5], &snapshot_file_bytes);
    if (status == LXP_OK &&
        snapshot_file_bytes > SIZE_MAX - CODEC_GENESIS_EXTRA_ARENA_BYTES)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) {
        arena_capacity = snapshot_file_bytes + CODEC_GENESIS_EXTRA_ARENA_BYTES;
        arena_memory = (uint8_t *)malloc(arena_capacity);
        manifest = (lxp_genesis_manifest *)malloc(sizeof(*manifest));
        accounts = (lx_account_registry *)malloc(sizeof(*accounts));
        state = (lxp_state_store *)malloc(sizeof(*state));
        journal = (lxp_state_journal *)calloc(1U, sizeof(*journal));
        kernel = (lxp_kernel *)malloc(sizeof(*kernel));
        if (arena_memory == NULL || manifest == NULL || accounts == NULL ||
            state == NULL || journal == NULL || kernel == NULL)
            status = LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_memory, arena_capacity);
    if (status == LXP_OK)
        status = lxp_genesis_parse(manifest_bytes, manifest_length,
                                   LXP_GENESIS_INPUT_MANIFEST, manifest);
    if (status == LXP_OK &&
        (manifest->network_id != network_id ||
         lxp_ct_memcmp(manifest->signer_public_key, public_key, 32U) != 0))
        status = LXP_ERR_AUTH_SCOPE;
    if (status == LXP_OK)
        status = lxp_genesis_verify_signature(manifest, &arena);
    if (status == LXP_OK)
        status = lxp_programs_metering_genesis_validate(manifest);
    if (status == LXP_OK)
        status = lxp_programs_fee_genesis_validate(manifest);
    if (status == LXP_OK)
        status = lxp_genesis_state_root(manifest, &arena,
                                        computed_state_root);
    if (status == LXP_OK)
        status = lxp_genesis_receipt_state_root(
            network_id, computed_state_root, computed_receipt_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(computed_state_root,
                       manifest->genesis_state_root, 32U) != 0 ||
         lxp_ct_memcmp(computed_receipt_root,
                       manifest->genesis_receipt_state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_genesis_manifest_commitment(
            manifest, &arena, manifest_commitment);
    if (status == LXP_OK) {
        status = lx_account_registry_init(accounts);
        accounts_open = status == LXP_OK;
    }
    if (status == LXP_OK) {
        status = lxp_state_store_init(state, 1U);
        state_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_store_bind_accounts(state, accounts);
    if (status == LXP_OK)
        status = lxp_kernel_create(kernel, state, journal, manifest, 1U);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_resolve(manifest, &plan);
    if (status == LXP_OK)
        status = lxp_genesis_module_plan_register(&plan, kernel);
    if (status == LXP_OK)
        status = lxp_snapshot_store_read(argv[5], &arena,
                                         &snapshot_manifest, &snapshot);
    if (status == LXP_OK &&
        (snapshot_manifest.global_sequence != 0U ||
         lxp_ct_memcmp(snapshot_manifest.canonical_state_root,
                       manifest->genesis_state_root, 32U) != 0 ||
         lxp_ct_memcmp(snapshot_manifest.receipt_state_root,
                       manifest->genesis_receipt_state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) {
        status = lxp_snapshot_load(snapshot.bytes, snapshot.length,
                                   &snapshot_manifest, kernel);
        snapshot_loaded = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_state_root(kernel, live_state_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(live_state_root,
                       snapshot_manifest.canonical_state_root, 32U) != 0 ||
         lxp_ct_memcmp(kernel->current_state_root,
                       snapshot_manifest.receipt_state_root, 32U) != 0))
        status = LXP_ERR_ROOT_MISMATCH;
    if (status == LXP_OK) {
        (void)fprintf(stdout,
            "{\"network_id\":%" PRIu32
            ",\"protocol_version\":%" PRIu16
            ",\"genesis_timestamp_ms\":\"%" PRIu64
            "\",\"state_root\":\"",
            manifest->network_id, manifest->protocol_version,
            manifest->genesis_timestamp_ms);
        print_hex(manifest->genesis_state_root, 32U);
        (void)fputs("\",\"receipt_state_root\":\"", stdout);
        print_hex(manifest->genesis_receipt_state_root, 32U);
        (void)fputs("\",\"snapshot_digest\":\"", stdout);
        print_hex(snapshot_manifest.snapshot_digest, 32U);
        (void)fprintf(stdout, "\",\"global_sequence\":\"%" PRIu64
                      "\",\"signer_public_key\":\"",
                      snapshot_manifest.global_sequence);
        print_hex(manifest->signer_public_key, 32U);
        (void)fputs("\",\"manifest_commitment\":\"", stdout);
        print_hex(manifest_commitment, 32U);
        (void)fputs("\"}\n", stdout);
        if (ferror(stdout) || fflush(stdout) != 0) status = LXP_ERR_IO;
    }
    if (snapshot_loaded)
        for (index = 0U; index < kernel->blob_count; ++index)
            free(kernel->blobs[index].bytes);
    if (state_open) {
        lxp_result close_status = lxp_state_store_destroy(state);
        if (status == LXP_OK) status = close_status;
    }
    if (accounts_open) lx_account_registry_release(accounts);
    if (manifest_bytes != NULL) {
        lxp_secure_zero(manifest_bytes, manifest_length);
        free(manifest_bytes);
    }
    if (arena_memory != NULL) {
        lxp_secure_zero(arena_memory, arena_capacity);
        free(arena_memory);
    }
    if (manifest != NULL) lxp_secure_zero(manifest, sizeof(*manifest));
    free(kernel);
    free(journal);
    free(state);
    free(accounts);
    free(manifest);
    return status == LXP_OK ? 0 : failure("genesis", status);
}

static int command_activity(void)
{
    uint8_t *canonical = NULL;
    size_t canonical_length = 0U;
    uint8_t *arena_memory = NULL;
    lxp_arena arena;
    lxp_activity activity;
    uint8_t activity_id[32];
    uint8_t actor_account[32];
    lxp_result status = read_stdin_bounded(
        LXP_MAX_ACTIVITY_BYTES, &canonical, &canonical_length);
    if (status == LXP_OK) {
        arena_memory = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
        if (arena_memory == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_memory,
                                LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK)
        status = validate_activity(canonical, canonical_length, &arena,
                                   &activity, activity_id, actor_account);
    if (status == LXP_OK) {
        (void)fputs("{\"activity_id\":\"", stdout);
        print_hex(activity_id, 32U);
        (void)fputs("\",\"actor\":", stdout);
        print_json_bytes(activity.actor_did.bytes, activity.actor_did.length);
        (void)fputs(",\"actor_account\":\"", stdout);
        print_hex(actor_account, 32U);
        (void)fprintf(stdout,
            "\",\"network_id\":%" PRIu32
            ",\"protocol_version\":%" PRIu16
            ",\"activity_type\":%" PRIu32
            ",\"module\":%" PRIu16
            ",\"ordinal\":%" PRIu16
            ",\"account_sequence\":\"%" PRIu64
            "\",\"idempotency_key\":\"",
            activity.network_id, activity.protocol_version,
            activity.activity_type,
            lxp_activity_module_id(activity.activity_type),
            lxp_activity_type_ordinal(activity.activity_type),
            activity.account_sequence);
        print_hex(activity.idempotency_key, 32U);
        (void)fputs("\",\"canonical_hex\":\"", stdout);
        print_hex(canonical, canonical_length);
        (void)fputs("\"}\n", stdout);
        if (ferror(stdout) || fflush(stdout) != 0) status = LXP_ERR_IO;
    }
    if (arena_memory != NULL) {
        lxp_secure_zero(arena_memory, LXP_MAX_ACTIVITY_BYTES);
        free(arena_memory);
    }
    if (canonical != NULL) {
        lxp_secure_zero(canonical, canonical_length);
        free(canonical);
    }
    return status == LXP_OK ? 0 : failure("activity", status);
}

static void store_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void store_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void store_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
}

static uint16_t load_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t load_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) |
           ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t load_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static int64_t deadline_after(uint32_t milliseconds)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0 || now.tv_sec < 0)
        return -1;
    if ((uint64_t)now.tv_sec > (uint64_t)INT64_MAX / 1000U)
        return -1;
    return (int64_t)((uint64_t)now.tv_sec * 1000U +
        (uint64_t)now.tv_nsec / 1000000U + milliseconds);
}

static lxp_result wait_descriptor(int descriptor, short events,
                                  int64_t deadline)
{
    struct pollfd watched;
    for (;;) {
        struct timespec now;
        int64_t current;
        int64_t remaining;
        int result;
        if (clock_gettime(CLOCK_MONOTONIC, &now) != 0 || now.tv_sec < 0)
            return LXP_ERR_IO;
        current = (int64_t)((uint64_t)now.tv_sec * 1000U +
            (uint64_t)now.tv_nsec / 1000000U);
        remaining = deadline - current;
        if (remaining <= 0) return LXP_ERR_TRUNCATED;
        watched.fd = descriptor;
        watched.events = events;
        watched.revents = 0;
        result = poll(&watched, 1U,
                      remaining > INT32_MAX ? INT32_MAX : (int)remaining);
        if (result < 0 && errno == EINTR) continue;
        if (result <= 0) return result == 0 ? LXP_ERR_TRUNCATED : LXP_ERR_IO;
        if ((watched.revents & (POLLERR | POLLHUP | POLLNVAL)) != 0 &&
            (watched.revents & events) == 0)
            return LXP_ERR_IO;
        if ((watched.revents & events) != 0) return LXP_OK;
    }
}

static lxp_result socket_write_exact(int descriptor, const uint8_t *bytes,
                                     size_t length, int64_t deadline)
{
    size_t written = 0U;
    while (written < length) {
        ssize_t result;
        lxp_result status = wait_descriptor(descriptor, POLLOUT, deadline);
        if (status != LXP_OK) return status;
        result = send(descriptor, bytes + written, length - written,
                      MSG_NOSIGNAL);
        if (result < 0 && (errno == EINTR || errno == EAGAIN ||
                           errno == EWOULDBLOCK))
            continue;
        if (result <= 0) return LXP_ERR_IO;
        written += (size_t)result;
    }
    return LXP_OK;
}

static lxp_result socket_read_exact(int descriptor, uint8_t *bytes,
                                    size_t length, int64_t deadline)
{
    size_t consumed = 0U;
    while (consumed < length) {
        ssize_t result;
        lxp_result status = wait_descriptor(descriptor, POLLIN, deadline);
        if (status != LXP_OK) return status;
        result = recv(descriptor, bytes + consumed, length - consumed, 0);
        if (result < 0 && (errno == EINTR || errno == EAGAIN ||
                           errno == EWOULDBLOCK))
            continue;
        if (result <= 0) return LXP_ERR_TRUNCATED;
        consumed += (size_t)result;
    }
    return LXP_OK;
}

static lxp_result connect_lni(const char *path, int *connected)
{
    struct sockaddr_un address;
    int descriptor;
    int flags;
    int result;
    int64_t deadline;
    if (path == NULL || connected == NULL || path[0] == '\0' ||
        strlen(path) >= sizeof(address.sun_path))
        return LXP_ERR_LENGTH_LIMIT;
    descriptor = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (descriptor < 0) return LXP_ERR_IO;
    flags = fcntl(descriptor, F_GETFL, 0);
    if (flags < 0 || fcntl(descriptor, F_SETFL, flags | O_NONBLOCK) != 0) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    (void)memset(&address, 0, sizeof(address));
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, path, strlen(path) + 1U);
    result = connect(descriptor, (const struct sockaddr *)&address,
                     sizeof(address));
    if (result != 0 && errno != EINPROGRESS) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    deadline = deadline_after(CODEC_LNI_TIMEOUT_MS);
    if (deadline < 0) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    if (result != 0) {
        int socket_error = 0;
        socklen_t error_length = sizeof(socket_error);
        lxp_result status = wait_descriptor(descriptor, POLLOUT, deadline);
        if (status != LXP_OK ||
            getsockopt(descriptor, SOL_SOCKET, SO_ERROR,
                       &socket_error, &error_length) != 0 ||
            socket_error != 0) {
            (void)close(descriptor);
            return status == LXP_OK ? LXP_ERR_IO : status;
        }
    }
    *connected = descriptor;
    return LXP_OK;
}

static lxp_result lni_send(int descriptor, uint16_t tag,
                           uint64_t correlation_id,
                           const uint8_t *payload, size_t payload_length)
{
    uint8_t prefix[4];
    uint8_t *frame;
    size_t length;
    size_t offset = 0U;
    int64_t deadline;
    lxp_result status;
    if ((payload == NULL && payload_length != 0U) ||
        payload_length > UINT32_MAX ||
        payload_length > LXP_DAEMON_LNI_MAX_FRAME_BYTES -
            CODEC_LNI_ENVELOPE_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    length = CODEC_LNI_ENVELOPE_BYTES + payload_length;
    frame = (uint8_t *)malloc(length);
    if (frame == NULL) return LXP_ERR_IO;
    store_u16(frame + offset, CODEC_LNI_MAJOR); offset += 2U;
    store_u16(frame + offset, CODEC_LNI_MINOR); offset += 2U;
    store_u16(frame + offset, tag); offset += 2U;
    store_u64(frame + offset, correlation_id); offset += 8U;
    store_u32(frame + offset, (uint32_t)payload_length); offset += 4U;
    if (payload_length != 0U) {
        (void)memcpy(frame + offset, payload, payload_length);
        offset += payload_length;
    }
    store_u32(frame + offset, 0U); offset += 4U;
    store_u32(prefix, (uint32_t)length);
    deadline = deadline_after(CODEC_LNI_TIMEOUT_MS);
    status = deadline < 0 ? LXP_ERR_IO :
        socket_write_exact(descriptor, prefix, sizeof(prefix), deadline);
    if (status == LXP_OK)
        status = socket_write_exact(descriptor, frame, offset, deadline);
    lxp_secure_zero(frame, length);
    free(frame);
    return status;
}

static void lni_envelope_release(lni_envelope *envelope)
{
    if (envelope != NULL) {
        if (envelope->owned != NULL) {
            lxp_secure_zero(envelope->owned,
                            CODEC_LNI_ENVELOPE_BYTES +
                            envelope->payload_length + envelope->proof_length);
            free(envelope->owned);
        }
        (void)memset(envelope, 0, sizeof(*envelope));
    }
}

static lxp_result lni_receive(int descriptor, lni_envelope *envelope)
{
    uint8_t prefix[4];
    uint8_t *frame;
    uint32_t length;
    uint32_t payload_length;
    uint32_t proof_length;
    size_t offset = 0U;
    int64_t deadline = deadline_after(CODEC_LNI_TIMEOUT_MS);
    lxp_result status;
    if (envelope == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(envelope, 0, sizeof(*envelope));
    status = deadline < 0 ? LXP_ERR_IO :
        socket_read_exact(descriptor, prefix, sizeof(prefix), deadline);
    if (status != LXP_OK) return status;
    length = load_u32(prefix);
    if (length < CODEC_LNI_ENVELOPE_BYTES ||
        length > LXP_DAEMON_LNI_MAX_FRAME_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    frame = (uint8_t *)malloc(length);
    if (frame == NULL) return LXP_ERR_IO;
    status = socket_read_exact(descriptor, frame, length, deadline);
    if (status != LXP_OK) {
        free(frame);
        return status;
    }
    envelope->major = load_u16(frame + offset); offset += 2U;
    envelope->minor = load_u16(frame + offset); offset += 2U;
    envelope->tag = load_u16(frame + offset); offset += 2U;
    envelope->correlation_id = load_u64(frame + offset); offset += 8U;
    payload_length = load_u32(frame + offset); offset += 4U;
    if ((size_t)payload_length > (size_t)length - offset - 4U) {
        free(frame);
        return LXP_ERR_MALFORMED_ENVELOPE;
    }
    envelope->payload = frame + offset;
    envelope->payload_length = payload_length;
    offset += payload_length;
    proof_length = load_u32(frame + offset); offset += 4U;
    if ((size_t)proof_length != (size_t)length - offset ||
        envelope->major != CODEC_LNI_MAJOR ||
        envelope->minor != CODEC_LNI_MINOR) {
        free(frame);
        return LXP_ERR_MALFORMED_ENVELOPE;
    }
    envelope->proof = frame + offset;
    envelope->proof_length = proof_length;
    envelope->owned = frame;
    return LXP_OK;
}

static lxp_result lni_handshake(int descriptor, const lxp_activity *activity)
{
    lni_envelope response;
    size_t cursor = CODEC_LNI_NODE_INFO_BYTES;
    uint16_t capability_count;
    size_t index;
    bool durable = false;
    bool submit = false;
    lxp_result status = lni_send(descriptor, CODEC_LNI_NODE_INFO_REQUEST,
                                 0U, NULL, 0U);
    if (status == LXP_OK) status = lni_receive(descriptor, &response);
    if (status != LXP_OK) return status;
    if (response.tag != CODEC_LNI_NODE_INFO_RESPONSE ||
        response.correlation_id != 0U || response.proof_length != 0U ||
        response.payload_length < CODEC_LNI_NODE_INFO_BYTES ||
        load_u16(response.payload) != CODEC_LNI_MAJOR ||
        load_u16(response.payload + 2U) != CODEC_LNI_MINOR ||
        load_u16(response.payload + 4U) != activity->protocol_version ||
        load_u32(response.payload + 6U) != activity->network_id ||
        response.payload[10U] != 1U) {
        lni_envelope_release(&response);
        return LXP_ERR_AUTH_SCOPE;
    }
    capability_count = load_u16(response.payload + 91U);
    for (index = 0U; index < capability_count; ++index) {
        uint16_t capability_length;
        if (cursor > response.payload_length - 2U) {
            lni_envelope_release(&response);
            return LXP_ERR_MALFORMED_ENVELOPE;
        }
        capability_length = load_u16(response.payload + cursor);
        cursor += 2U;
        if ((size_t)capability_length > response.payload_length - cursor) {
            lni_envelope_release(&response);
            return LXP_ERR_MALFORMED_ENVELOPE;
        }
        if ((size_t)capability_length ==
                sizeof("authenticated_durable_submit") - 1U &&
            memcmp(response.payload + cursor, "authenticated_durable_submit",
                   capability_length) == 0)
            durable = true;
        if ((size_t)capability_length == sizeof("submit") - 1U &&
            memcmp(response.payload + cursor, "submit",
                   capability_length) == 0)
            submit = true;
        cursor += capability_length;
    }
    if (cursor != response.payload_length || !durable || !submit)
        status = LXP_ERR_MODULE_DISABLED;
    lni_envelope_release(&response);
    return status;
}

static lxp_result lni_submit(int descriptor, const uint8_t *canonical,
                             size_t canonical_length,
                             const uint8_t activity_id[32], bool *refused,
                             lxp_result *refusal_result)
{
    lni_envelope response;
    if (refused == NULL || refusal_result == NULL)
        return LXP_ERR_NON_CANONICAL;
    *refused = false;
    *refusal_result = LXP_OK;
    lxp_result status = lni_send(descriptor, CODEC_LNI_SUBMIT_REQUEST,
                                 1U, canonical, canonical_length);
    if (status == LXP_OK) status = lni_receive(descriptor, &response);
    if (status != LXP_OK) return status;
    if (response.tag == CODEC_LNI_ERROR_RESPONSE &&
        response.correlation_id == 1U && response.payload_length == 5U &&
        response.proof_length == 0U) {
        lxp_result refusal = (lxp_result)load_u32(response.payload + 1U);
        if (response.payload[0] < 1U || response.payload[0] > 6U ||
            refusal == LXP_OK ||
            lxp_result_domain(refusal) == LXP_RESULT_DOMAIN_UNKNOWN) {
            lni_envelope_release(&response);
            return LXP_ERR_MALFORMED_ENVELOPE;
        }
        *refused = true;
        *refusal_result = refusal;
        lni_envelope_release(&response);
        return LXP_OK;
    }
    if (response.tag != CODEC_LNI_SUBMIT_RESPONSE ||
        response.correlation_id != 1U ||
        response.payload_length != canonical_length ||
        lxp_ct_memcmp(response.payload, canonical, canonical_length) != 0 ||
        response.proof_length != 32U ||
        lxp_ct_memcmp(response.proof, activity_id, 32U) != 0)
        status = LXP_ERR_MALFORMED_ENVELOPE;
    lni_envelope_release(&response);
    return status;
}

static int command_submit(const char *socket_path)
{
    uint8_t *canonical = NULL;
    size_t canonical_length = 0U;
    uint8_t *arena_memory = NULL;
    lxp_arena arena;
    lxp_activity activity;
    uint8_t activity_id[32];
    uint8_t actor_account[32];
    bool refused = false;
    lxp_result refusal_result = LXP_OK;
    int descriptor = -1;
    lxp_result status = read_stdin_bounded(
        LXP_MAX_ACTIVITY_BYTES, &canonical, &canonical_length);
    if (status == LXP_OK) {
        arena_memory = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
        if (arena_memory == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, arena_memory,
                                LXP_MAX_ACTIVITY_BYTES);
    if (status == LXP_OK)
        status = validate_activity(canonical, canonical_length, &arena,
                                   &activity, activity_id, actor_account);
    if (status == LXP_OK) status = connect_lni(socket_path, &descriptor);
    if (status == LXP_OK) status = lni_handshake(descriptor, &activity);
    if (status == LXP_OK)
        status = lni_submit(descriptor, canonical, canonical_length,
                            activity_id, &refused, &refusal_result);
    if (descriptor >= 0 && close(descriptor) != 0 && status == LXP_OK)
        status = LXP_ERR_IO;
    if (status == LXP_OK) {
        (void)fputs(refused ?
            "{\"state\":\"refused\",\"activity_id\":\"" :
            "{\"state\":\"acknowledged\",\"activity_id\":\"", stdout);
        print_hex(activity_id, 32U);
        (void)fputs("\",\"idempotency_key\":\"", stdout);
        print_hex(activity.idempotency_key, 32U);
        if (refused)
            (void)fprintf(stdout,
                "\",\"error\":{\"code\":\"native_refusal\","
                "\"result_code\":%" PRId32 "}}\n", refusal_result);
        else
            (void)fputs("\"}\n", stdout);
        if (ferror(stdout) || fflush(stdout) != 0) status = LXP_ERR_IO;
    }
    if (arena_memory != NULL) {
        lxp_secure_zero(arena_memory, LXP_MAX_ACTIVITY_BYTES);
        free(arena_memory);
    }
    if (canonical != NULL) {
        lxp_secure_zero(canonical, canonical_length);
        free(canonical);
    }
    return status == LXP_OK ? 0 : failure("submit", status);
}

int lxp_archive_codec_main(int argc, char **argv)
{
    if (argc < 2) return usage();
    if (strcmp(argv[1], "export") == 0)
        return argc == 4 ? command_export(argv[2], argv[3]) : usage();
    if (strcmp(argv[1], "verify") == 0)
        return command_verify(argc, argv);
    if (strcmp(argv[1], "genesis") == 0)
        return command_genesis(argc, argv);
    if (strcmp(argv[1], "activity") == 0)
        return argc == 2 ? command_activity() : usage();
    if (strcmp(argv[1], "submit") == 0)
        return argc == 3 ? command_submit(argv[2]) : usage();
    return usage();
}

int main(int argc, char **argv)
{
    return lxp_archive_codec_main(argc, argv);
}
