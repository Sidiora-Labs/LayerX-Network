#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"

#include <errno.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>
#include <unistd.h>

static uint32_t read_u32(const uint8_t *in)
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static uint64_t read_u64(const uint8_t *in)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static void decode(const uint8_t in[LXP_LOG_HEADER_BYTES],
                   lxp_log_record_header *header)
{
    header->magic = read_u32(in);
    header->record_kind = in[4];
    header->reserved[0] = in[5];
    header->reserved[1] = in[6];
    header->reserved[2] = in[7];
    header->global_sequence = read_u64(in + 8U);
    header->body_length = read_u32(in + 16U);
    header->body_crc32c = read_u32(in + 20U);
    header->previous_record_offset = read_u64(in + 24U);
}

static lxp_result pread_exact(int descriptor, uint8_t *bytes, size_t length,
                              uint64_t offset)
{
    size_t consumed = 0U;
    while (consumed < length) {
        ssize_t count = pread(descriptor, bytes + consumed, length - consumed,
                              (off_t)(offset + consumed));
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) return LXP_ERR_IO;
        if (count == 0) return LXP_ERR_LOG_TRUNCATED;
        consumed += (size_t)count;
    }
    return LXP_OK;
}

static int kind_valid(uint8_t kind)
{
    return kind >= (uint8_t)LXP_LOG_ACTIVITY &&
           kind <= (uint8_t)LXP_LOG_BATCH_BODY;
}

static lxp_result load_record(const lxp_log *log, uint64_t offset,
                              lxp_log_record_header *header, uint8_t **body)
{
    uint8_t encoded[LXP_LOG_HEADER_BYTES];
    lxp_result status;
    *body = NULL;
    if (offset > log->capacity ||
        LXP_LOG_HEADER_BYTES > log->capacity - offset)
        return LXP_ERR_LOG_TRUNCATED;
    status = pread_exact(log->descriptor, encoded, sizeof(encoded), offset);
    if (status != LXP_OK) return status;
    if (read_u32(encoded) == 0U) return LXP_ERR_LOG_TRUNCATED;
    decode(encoded, header);
    if (header->magic != LXP_LOG_MAGIC || !kind_valid(header->record_kind) ||
        header->reserved[0] != 0U || header->reserved[1] != 0U ||
        header->reserved[2] != 0U) return LXP_ERR_LOG_CORRUPT;
    if ((uint64_t)header->body_length > log->capacity - offset -
        LXP_LOG_HEADER_BYTES) return LXP_ERR_LOG_TRUNCATED;
    if (header->body_length != 0U) {
        *body = malloc(header->body_length);
        if (*body == NULL) return LXP_ERR_IO;
        status = pread_exact(log->descriptor, *body, header->body_length,
                             offset + LXP_LOG_HEADER_BYTES);
        if (status != LXP_OK) {
            free(*body);
            *body = NULL;
            return status;
        }
    }
    if (lxp_log_crc32c(*body, header->body_length) != header->body_crc32c) {
        free(*body);
        *body = NULL;
        return LXP_ERR_LOG_CORRUPT;
    }
    return LXP_OK;
}

lxp_result lxp_log_scan_tail(const lxp_log *log, uint64_t *valid_end,
                             uint64_t *last_record_offset,
                             uint64_t *next_sequence)
{
    uint64_t offset = 0U;
    uint64_t last = 0U;
    uint64_t next = 0U;
    if (log == NULL || valid_end == NULL || last_record_offset == NULL ||
        next_sequence == NULL || log->descriptor < 0)
        return LXP_ERR_NON_CANONICAL;
    while (offset + LXP_LOG_HEADER_BYTES <= log->capacity) {
        uint8_t prefix[4];
        lxp_log_record_header header;
        uint8_t *body;
        lxp_result status = pread_exact(log->descriptor, prefix,
                                        sizeof(prefix), offset);
        if (status != LXP_OK) break;
        if (read_u32(prefix) == 0U) {
            *valid_end = offset;
            *last_record_offset = last;
            *next_sequence = next;
            return LXP_OK;
        }
        status = load_record(log, offset, &header, &body);
        free(body);
        if (status != LXP_OK) {
            *valid_end = offset;
            *last_record_offset = last;
            *next_sequence = next;
            return status;
        }
        if ((offset != 0U && header.previous_record_offset != last) ||
            (next != 0U && header.global_sequence + 1U < next)) {
            *valid_end = offset;
            *last_record_offset = last;
            *next_sequence = next;
            return LXP_ERR_LOG_CORRUPT;
        }
        last = offset;
        if (header.global_sequence == UINT64_MAX) return LXP_ERR_LOG_CORRUPT;
        if (header.global_sequence + 1U > next) next = header.global_sequence + 1U;
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    *valid_end = offset;
    *last_record_offset = last;
    *next_sequence = next;
    return offset == log->capacity ? LXP_OK : LXP_ERR_LOG_TRUNCATED;
}

lxp_result lxp_log_truncate_partial(lxp_log *log, uint64_t valid_end)
{
    static const uint8_t zeros[4096] = { 0U };
    uint64_t offset = valid_end;
    if (log == NULL || log->descriptor < 0 || valid_end > log->capacity)
        return LXP_ERR_NON_CANONICAL;
    while (offset < log->capacity) {
        size_t count = log->capacity - offset > sizeof(zeros) ? sizeof(zeros) :
                       (size_t)(log->capacity - offset);
        ssize_t written = pwrite(log->descriptor, zeros, count, (off_t)offset);
        if (written < 0 && errno == EINTR) continue;
        if (written <= 0) return LXP_ERR_IO;
        offset += (size_t)written;
    }
    log->write_offset = valid_end;
    return lxp_log_sync(log);
}

static lxp_result find_recovery_window(const lxp_log *log, uint64_t valid_end,
                                       uint64_t durable, uint64_t *start,
                                       uint64_t *end, uint64_t *last_offset,
                                       bool complete_records)
{
    uint64_t offset = 0U;
    uint64_t checkpoint = 0U;
    uint64_t durable_end = 0U;
    uint64_t last = 0U;
    uint64_t complete_last = 0U;
    bool have_activity = false;
    bool have_non_checkpoint = false;
    while (offset < valid_end) {
        lxp_log_record_header header;
        uint8_t *body;
        lxp_result status = load_record(log, offset, &header, &body);
        free(body);
        if (status != LXP_OK) return status;
        complete_last = offset;
        if (header.record_kind == (uint8_t)LXP_LOG_ACTIVITY)
            have_activity = true;
        if (header.record_kind != (uint8_t)LXP_LOG_CHECKPOINT)
            have_non_checkpoint = true;
        if (header.record_kind == (uint8_t)LXP_LOG_CHECKPOINT &&
            durable != UINT64_MAX && header.global_sequence <= durable)
            checkpoint = offset;
        if (durable != UINT64_MAX && header.global_sequence <= durable) {
            durable_end = offset + LXP_LOG_HEADER_BYTES + header.body_length;
            last = offset;
        }
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    *start = checkpoint;
    if (!complete_records && !have_activity && have_non_checkpoint)
        return LXP_ERR_LOG_CORRUPT;
    *end = complete_records || !have_activity ? valid_end : durable_end;
    *last_offset = complete_records || !have_activity ? complete_last : last;
    return LXP_OK;
}

static lxp_result recover(lxp_log *log, lxp_log_replay_fn replay,
                          void *context, bool complete_records)
{
    uint64_t valid_end;
    uint64_t last;
    uint64_t scanned_next;
    uint64_t durable;
    uint64_t start;
    uint64_t recovery_end;
    lxp_result status;
    if (log == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_log_scan_tail(log, &valid_end, &last, &scanned_next);
    if (status == LXP_ERR_LOG_TRUNCATED) {
        status = lxp_log_truncate_partial(log, valid_end);
        if (status != LXP_OK) return status;
    } else if (status != LXP_OK) return status;
    status = lxp_log_durable_head(log, &durable);
    if (status != LXP_OK) return status;
    status = find_recovery_window(log, valid_end, durable, &start,
                                  &recovery_end, &last, complete_records);
    if (status != LXP_OK) return status;
    if (valid_end != recovery_end) {
        status = lxp_log_truncate_partial(log, recovery_end);
        if (status != LXP_OK) return status;
    }
    if (replay != NULL) {
        uint64_t offset = start;
        while (offset < recovery_end) {
            lxp_log_record_header header;
            uint8_t *body;
            status = load_record(log, offset, &header, &body);
            if (status == LXP_OK) status = replay(context, &header, body);
            free(body);
            if (status != LXP_OK) return status;
            offset += LXP_LOG_HEADER_BYTES + header.body_length;
        }
    }
    log->write_offset = recovery_end;
    log->previous_record_offset = last;
    log->next_sequence = durable == UINT64_MAX && recovery_end == valid_end ?
                         scanned_next : durable == UINT64_MAX ? 0U :
                         durable + 1U;
    return LXP_OK;
}

lxp_result lxp_log_recover(lxp_log *log, lxp_log_replay_fn replay,
                           void *context)
{
    return recover(log, replay, context, false);
}

lxp_result lxp_log_recover_complete_records(lxp_log *log,
                                            lxp_log_replay_fn replay,
                                            void *context)
{
    return recover(log, replay, context, true);
}

uint64_t lxp_log_resume_sequence(const lxp_log *log)
{
    return log == NULL ? 0U : log->next_sequence;
}
