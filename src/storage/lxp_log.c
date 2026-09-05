#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"
#include "layerx/lxp_fault.h"

#include <errno.h>
#include <fcntl.h>
#include <inttypes.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

enum {
    LXP_LOG_DURABLE_MARKER_MAGIC = 0x4c585044,
    LXP_LOG_DURABLE_MARKER_VERSION = 1,
    LXP_LOG_DURABLE_MARKER_SLOT_BYTES = 64,
    LXP_LOG_DURABLE_MARKER_BYTES = 128
};

static int valid_kind(uint8_t kind)
{
    return kind >= (uint8_t)LXP_LOG_ACTIVITY &&
           kind <= (uint8_t)LXP_LOG_BATCH_BODY;
}

static void store_u32(uint8_t *out, uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void store_u64(uint8_t *out, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static uint32_t load_u32(const uint8_t *in)
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | in[3];
}

static uint64_t load_u64(const uint8_t *in)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static void encode_header(const lxp_log_record_header *header,
                          uint8_t out[LXP_LOG_HEADER_BYTES])
{
    store_u32(out, header->magic);
    out[4] = header->record_kind;
    out[5] = header->reserved[0];
    out[6] = header->reserved[1];
    out[7] = header->reserved[2];
    store_u64(out + 8U, header->global_sequence);
    store_u32(out + 16U, header->body_length);
    store_u32(out + 20U, header->body_crc32c);
    store_u64(out + 24U, header->previous_record_offset);
}

static void decode_header(const uint8_t in[LXP_LOG_HEADER_BYTES],
                          lxp_log_record_header *header)
{
    header->magic = load_u32(in);
    header->record_kind = in[4];
    header->reserved[0] = in[5];
    header->reserved[1] = in[6];
    header->reserved[2] = in[7];
    header->global_sequence = load_u64(in + 8U);
    header->body_length = load_u32(in + 16U);
    header->body_crc32c = load_u32(in + 20U);
    header->previous_record_offset = load_u64(in + 24U);
}

static lxp_result write_exact(int descriptor, const uint8_t *bytes,
                              size_t length, uint64_t offset)
{
    size_t written = 0U;
    while (written < length) {
        ssize_t count = pwrite(descriptor, bytes + written, length - written,
                               (off_t)(offset + written));
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return LXP_ERR_IO;
        written += (size_t)count;
    }
    return LXP_OK;
}

static lxp_result read_exact(int descriptor, uint8_t *bytes, size_t length,
                             uint64_t offset)
{
    size_t consumed = 0U;
    while (consumed < length) {
        ssize_t count = pread(descriptor, bytes + consumed, length - consumed,
                              (off_t)(offset + consumed));
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return LXP_ERR_LOG_TRUNCATED;
        consumed += (size_t)count;
    }
    return LXP_OK;
}

uint32_t lxp_log_crc32c(const void *bytes, size_t length)
{
    const uint8_t *input = (const uint8_t *)bytes;
    uint32_t crc = UINT32_MAX;
    size_t i;
    if (input == NULL && length != 0U) return 0U;
    for (i = 0U; i < length; ++i) {
        unsigned int bit;
        crc ^= input[i];
        for (bit = 0U; bit < 8U; ++bit)
            crc = (crc >> 1U) ^ ((crc & 1U) != 0U ?
                  UINT32_C(0x82f63b78) : 0U);
    }
    return ~crc;
}

static void durable_marker_encode(const lxp_log *log, uint64_t generation,
                                  uint8_t out[LXP_LOG_DURABLE_MARKER_SLOT_BYTES])
{
    (void)memset(out, 0, LXP_LOG_DURABLE_MARKER_SLOT_BYTES);
    store_u32(out, LXP_LOG_DURABLE_MARKER_MAGIC);
    store_u32(out + 4U, LXP_LOG_DURABLE_MARKER_VERSION);
    store_u64(out + 8U, generation);
    store_u64(out + 16U, log->write_offset);
    store_u64(out + 24U, log->previous_record_offset);
    store_u64(out + 32U, log->next_sequence);
    store_u32(out + 40U, lxp_log_crc32c(out, 40U));
}

static bool durable_marker_decode(
    const uint8_t in[LXP_LOG_DURABLE_MARKER_SLOT_BYTES], uint64_t capacity,
    uint64_t *generation, uint64_t *offset, uint64_t *previous,
    uint64_t *next)
{
    size_t i;
    if (load_u32(in) != LXP_LOG_DURABLE_MARKER_MAGIC ||
        load_u32(in + 4U) != LXP_LOG_DURABLE_MARKER_VERSION ||
        load_u32(in + 40U) != lxp_log_crc32c(in, 40U))
        return false;
    for (i = 44U; i < LXP_LOG_DURABLE_MARKER_SLOT_BYTES; ++i) {
        if (in[i] != 0U) return false;
    }
    *generation = load_u64(in + 8U);
    *offset = load_u64(in + 16U);
    *previous = load_u64(in + 24U);
    *next = load_u64(in + 32U);
    if (*offset > capacity || (*offset == 0U && *previous != 0U) ||
        (*offset != 0U && *previous >= *offset))
        return false;
    return true;
}

static lxp_result durable_marker_store(lxp_log *log, uint64_t generation)
{
    uint8_t encoded[LXP_LOG_DURABLE_MARKER_SLOT_BYTES];
    uint64_t slot = generation & UINT64_C(1);
    lxp_result status;
    durable_marker_encode(log, generation, encoded);
    status = write_exact(log->descriptor, encoded, sizeof(encoded),
                         log->capacity + slot * sizeof(encoded));
    if (status != LXP_OK || fdatasync(log->descriptor) != 0)
        return status != LXP_OK ? status : LXP_ERR_IO;
    log->durable_offset = log->write_offset;
    log->durable_previous_record_offset = log->previous_record_offset;
    log->durable_next_sequence = log->next_sequence;
    log->durable_generation = generation;
    return LXP_OK;
}

static lxp_result durable_marker_load(lxp_log *log, uint64_t physical_capacity)
{
    uint8_t slots[2][LXP_LOG_DURABLE_MARKER_SLOT_BYTES];
    uint64_t generation[2];
    uint64_t offset[2];
    uint64_t previous[2];
    uint64_t next[2];
    bool valid[2];
    bool marker_magic = false;
    size_t i;
    size_t selected;
    lxp_result status;
    if (physical_capacity < LXP_LOG_HEADER_BYTES +
            LXP_LOG_DURABLE_MARKER_BYTES)
        return LXP_OK;
    status = read_exact(log->descriptor, slots[0], sizeof(slots),
                        physical_capacity - LXP_LOG_DURABLE_MARKER_BYTES);
    if (status != LXP_OK) return status;
    for (i = 0U; i < 2U; ++i) {
        if (load_u32(slots[i]) == LXP_LOG_DURABLE_MARKER_MAGIC)
            marker_magic = true;
        valid[i] = durable_marker_decode(
            slots[i], physical_capacity - LXP_LOG_DURABLE_MARKER_BYTES,
            &generation[i], &offset[i], &previous[i], &next[i]);
    }
    if (!valid[0] && !valid[1])
        return marker_magic ? LXP_ERR_LOG_CORRUPT : LXP_OK;
    selected = valid[0] && valid[1] ?
        (generation[1] > generation[0] ? 1U : 0U) : (valid[1] ? 1U : 0U);
    log->capacity = physical_capacity - LXP_LOG_DURABLE_MARKER_BYTES;
    log->durable_generation = generation[selected];
    log->durable_offset = offset[selected];
    log->durable_previous_record_offset = previous[selected];
    log->durable_next_sequence = next[selected];
    log->has_durable_marker = true;
    return LXP_OK;
}

lxp_result lxp_log_segment_create(lxp_log *log, const char *directory,
                                  uint64_t segment_sequence,
                                  uint64_t segment_size)
{
    char path[4096];
    int descriptor;
    int length;
    int allocation;
    if (log == NULL || directory == NULL ||
        segment_size < LXP_LOG_HEADER_BYTES + LXP_LOG_DURABLE_MARKER_BYTES)
        return LXP_ERR_NON_CANONICAL;
    length = snprintf(path, sizeof(path), "%s/%020" PRIu64 ".lxp",
                      directory, segment_sequence);
    if (length < 0 || (size_t)length >= sizeof(path)) return LXP_ERR_LENGTH_LIMIT;
    descriptor = open(path, O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (descriptor < 0) return LXP_ERR_IO;
    allocation = posix_fallocate(descriptor, 0, (off_t)segment_size);
    if (allocation != 0) {
        (void)close(descriptor);
        (void)unlink(path);
        return LXP_ERR_IO;
    }
    log->descriptor = descriptor;
    log->segment_sequence = segment_sequence;
    log->capacity = segment_size - LXP_LOG_DURABLE_MARKER_BYTES;
    log->write_offset = 0U;
    log->previous_record_offset = 0U;
    log->next_sequence = 0U;
    log->durable_offset = 0U;
    log->durable_previous_record_offset = 0U;
    log->durable_next_sequence = 0U;
    log->durable_generation = 0U;
    log->has_durable_marker = true;
    if (durable_marker_store(log, 0U) != LXP_OK) {
        (void)close(descriptor);
        (void)unlink(path);
        log->descriptor = -1;
        return LXP_ERR_IO;
    }
    return LXP_OK;
}

static lxp_result log_open_descriptor(lxp_log *log, int descriptor,
                                      uint64_t physical_size)
{
    log->descriptor = descriptor;
    log->segment_sequence = 0U;
    log->capacity = physical_size;
    log->write_offset = 0U;
    log->previous_record_offset = 0U;
    log->next_sequence = 0U;
    log->durable_offset = 0U;
    log->durable_previous_record_offset = 0U;
    log->durable_next_sequence = 0U;
    log->durable_generation = 0U;
    log->has_durable_marker = false;
    {
        lxp_result status = durable_marker_load(
            log, physical_size);
        if (status != LXP_OK) {
            (void)close(descriptor);
            log->descriptor = -1;
            return status;
        }
    }
    return LXP_OK;
}

lxp_result lxp_log_open(lxp_log *log, const char *path)
{
    struct stat information;
    int descriptor;
    if (log == NULL || path == NULL) return LXP_ERR_NON_CANONICAL;
    descriptor = open(path, O_RDWR | O_CLOEXEC);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size < 0) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    return log_open_descriptor(log, descriptor,
                                (uint64_t)information.st_size);
}

lxp_result lxp_log_open_or_create(lxp_log *log, const char *path,
                                  uint64_t initial_size)
{
    char parent[4096];
    char *slash;
    struct stat information;
    int descriptor;
    int directory;
    lxp_result status;
    size_t length;
    if (log == NULL || path == NULL ||
        initial_size < LXP_LOG_HEADER_BYTES + LXP_LOG_DURABLE_MARKER_BYTES ||
        initial_size > INT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    length = strlen(path);
    if (length == 0U || length >= sizeof(parent)) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(parent, path, length + 1U);
    slash = strrchr(parent, '/');
    if (slash == NULL) (void)strcpy(parent, ".");
    else if (slash == parent) slash[1] = '\0';
    else *slash = '\0';
    directory = open(parent, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
    if (directory < 0) return LXP_ERR_IO;
    descriptor = open(path, O_RDWR | O_CREAT | O_NOFOLLOW | O_CLOEXEC, 0600);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size < 0 || !S_ISREG(information.st_mode) ||
        information.st_nlink != 1 || information.st_uid != geteuid() ||
        (information.st_mode & 0777U) != 0600U) {
        if (descriptor >= 0) (void)close(descriptor);
        (void)close(directory);
        return LXP_ERR_IO;
    }
    status = log_open_descriptor(log, descriptor,
                                  (uint64_t)information.st_size);
    if (status == LXP_OK && information.st_size == 0) {
        if (posix_fallocate(descriptor, 0, (off_t)initial_size) != 0)
            status = LXP_ERR_IO;
        if (status == LXP_OK) {
            log->capacity = initial_size - LXP_LOG_DURABLE_MARKER_BYTES;
            log->has_durable_marker = true;
            status = durable_marker_store(log, 0U);
        }
    }
    if (status == LXP_OK && fsync(directory) != 0) status = LXP_ERR_IO;
    (void)close(directory);
    if (status != LXP_OK && log->descriptor >= 0) {
        (void)close(log->descriptor);
        log->descriptor = -1;
    }
    return status;
}

lxp_result lxp_log_append(lxp_log *log, lxp_log_record_kind kind,
                          uint64_t global_sequence, const void *body,
                          uint32_t body_length, uint64_t *record_offset)
{
    lxp_log_record_header header;
    uint8_t encoded[LXP_LOG_HEADER_BYTES];
    uint64_t end;
    lxp_result status;
    if (log == NULL || log->descriptor < 0 || !valid_kind((uint8_t)kind) ||
        (body == NULL && body_length != 0U)) return LXP_ERR_NON_CANONICAL;
    end = log->write_offset + LXP_LOG_HEADER_BYTES + body_length;
    if (end < log->write_offset || end > log->capacity)
        return LXP_ERR_LENGTH_LIMIT;
    header.magic = LXP_LOG_MAGIC;
    header.record_kind = (uint8_t)kind;
    header.reserved[0] = 0U;
    header.reserved[1] = 0U;
    header.reserved[2] = 0U;
    header.global_sequence = global_sequence;
    header.body_length = body_length;
    header.body_crc32c = lxp_log_crc32c(body, body_length);
    header.previous_record_offset = log->write_offset == 0U ? 0U :
                                    log->previous_record_offset;
    encode_header(&header, encoded);
    status = write_exact(log->descriptor, encoded, sizeof(encoded),
                         log->write_offset);
    if (status != LXP_OK) return status;
    lxp_fault_inject_point(LXP_FAULT_LOG_HEADER_WRITTEN);
    status = write_exact(log->descriptor, (const uint8_t *)body, body_length,
                         log->write_offset + LXP_LOG_HEADER_BYTES);
    if (status != LXP_OK) return status;
    lxp_fault_inject_point(LXP_FAULT_LOG_BODY_WRITTEN);
    if (record_offset != NULL) *record_offset = log->write_offset;
    log->previous_record_offset = log->write_offset;
    log->write_offset = end;
    log->next_sequence = global_sequence + 1U;
    return LXP_OK;
}

lxp_result lxp_log_read(const lxp_log *log, uint64_t record_offset,
                        lxp_log_record_header *header, void *body,
                        size_t body_capacity)
{
    uint8_t encoded[LXP_LOG_HEADER_BYTES];
    lxp_result status;
    if (log == NULL || header == NULL || log->descriptor < 0 ||
        record_offset > log->capacity ||
        LXP_LOG_HEADER_BYTES > log->capacity - record_offset)
        return LXP_ERR_LOG_TRUNCATED;
    status = read_exact(log->descriptor, encoded, sizeof(encoded), record_offset);
    if (status != LXP_OK) return status;
    decode_header(encoded, header);
    if (header->magic != LXP_LOG_MAGIC || !valid_kind(header->record_kind) ||
        header->reserved[0] != 0U || header->reserved[1] != 0U ||
        header->reserved[2] != 0U) return LXP_ERR_LOG_CORRUPT;
    if ((uint64_t)header->body_length > log->capacity - record_offset -
        LXP_LOG_HEADER_BYTES) return LXP_ERR_LOG_TRUNCATED;
    if (header->body_length > body_capacity ||
        (body == NULL && header->body_length != 0U)) return LXP_ERR_LENGTH_LIMIT;
    status = read_exact(log->descriptor, (uint8_t *)body, header->body_length,
                        record_offset + LXP_LOG_HEADER_BYTES);
    if (status != LXP_OK) return status;
    return lxp_log_crc32c(body, header->body_length) == header->body_crc32c ?
           LXP_OK : LXP_ERR_LOG_CORRUPT;
}

lxp_result lxp_log_close(lxp_log *log)
{
    if (log == NULL || log->descriptor < 0) return LXP_ERR_NON_CANONICAL;
    if (close(log->descriptor) != 0) return LXP_ERR_IO;
    log->descriptor = -1;
    return LXP_OK;
}

lxp_result lxp_log_sync(lxp_log *log)
{
    if (log == NULL || log->descriptor < 0) return LXP_ERR_NON_CANONICAL;
    if (fdatasync(log->descriptor) != 0) return LXP_ERR_IO;
    if (log->has_durable_marker) {
        if (log->durable_generation == UINT64_MAX)
            return LXP_ERR_LENGTH_LIMIT;
        {
            lxp_result status = durable_marker_store(
                log, log->durable_generation + 1U);
            if (status != LXP_OK) return status;
        }
    }
    lxp_fault_inject_point(LXP_FAULT_LOG_SYNCED);
    return LXP_OK;
}

lxp_result lxp_log_write_boundary(lxp_log *log)
{
    return lxp_log_sync(log);
}

bool lxp_log_fault_point(uint32_t boundary, uint32_t abort_boundary)
{
    return abort_boundary != 0U && boundary == abort_boundary;
}

lxp_result lxp_log_durable_head(const lxp_log *log, uint64_t *global_sequence)
{
    uint64_t offset = 0U;
    uint64_t limit;
    uint64_t pending_sequence = 0U;
    uint64_t durable = UINT64_MAX;
    int have_activity = 0;
    if (log == NULL || global_sequence == NULL || log->descriptor < 0)
        return LXP_ERR_NON_CANONICAL;
    limit = log->has_durable_marker ? log->durable_offset : log->capacity;
    if (limit > log->capacity) return LXP_ERR_LOG_CORRUPT;
    while (offset + LXP_LOG_HEADER_BYTES <= limit) {
        uint8_t encoded[LXP_LOG_HEADER_BYTES];
        lxp_log_record_header header;
        uint8_t *body;
        lxp_result status = read_exact(log->descriptor, encoded,
                                       sizeof(encoded), offset);
        if (status != LXP_OK) return status;
        if (load_u32(encoded) == 0U) break;
        decode_header(encoded, &header);
        if (header.magic != LXP_LOG_MAGIC || !valid_kind(header.record_kind) ||
            header.reserved[0] != 0U || header.reserved[1] != 0U ||
            header.reserved[2] != 0U ||
            (uint64_t)header.body_length > limit - offset -
            LXP_LOG_HEADER_BYTES) return LXP_ERR_LOG_CORRUPT;
        body = header.body_length == 0U ? NULL : malloc(header.body_length);
        if (header.body_length != 0U && body == NULL) return LXP_ERR_IO;
        status = read_exact(log->descriptor, body, header.body_length,
                            offset + LXP_LOG_HEADER_BYTES);
        if (status == LXP_OK && lxp_log_crc32c(body, header.body_length) !=
            header.body_crc32c) status = LXP_ERR_LOG_CORRUPT;
        free(body);
        if (status != LXP_OK) return status;
        if (header.record_kind == (uint8_t)LXP_LOG_ACTIVITY) {
            pending_sequence = header.global_sequence;
            have_activity = 1;
        } else if (header.record_kind == (uint8_t)LXP_LOG_RECEIPT &&
                   have_activity != 0 &&
                   header.global_sequence == pending_sequence) {
            durable = pending_sequence;
            have_activity = 0;
        }
        offset += LXP_LOG_HEADER_BYTES + header.body_length;
    }
    if (log->has_durable_marker && offset != limit)
        return LXP_ERR_LOG_TRUNCATED;
    *global_sequence = durable;
    return LXP_OK;
}
