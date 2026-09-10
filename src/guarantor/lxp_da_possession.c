#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_da.h"
#include "layerx/lxp_guarantor.h"

#include "layerx/lxp_crypto.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

enum {
    LXP_DA_STORE_HEADER_BYTES = 56,
    LXP_DA_STORE_CHUNK_HEADER_BYTES = 49
};

static void put_u32(uint8_t out[4], uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static uint32_t get_u32(const uint8_t in[4])
{
    return ((uint32_t)in[0] << 24U) | ((uint32_t)in[1] << 16U) |
           ((uint32_t)in[2] << 8U) | (uint32_t)in[3];
}

static void put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) out[7U - i] = (uint8_t)(value >> (i * 8U));
}

static uint64_t get_u64(const uint8_t in[8])
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | in[i];
    return value;
}

static lxp_result write_all(int descriptor, const uint8_t *bytes,
                            size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t count = write(descriptor, bytes + offset, length - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return LXP_ERR_IO;
        offset += (size_t)count;
    }
    return LXP_OK;
}

static lxp_result read_all(int descriptor, uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t count = read(descriptor, bytes + offset, length - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) return LXP_ERR_IO;
        offset += (size_t)count;
    }
    return LXP_OK;
}

static lxp_result bundle_classes(const lxp_da_bundle *bundle)
{
    uint64_t offsets[LXP_DA_CLASS_COUNT] = {0U, 0U, 0U, 0U, 0U};
    uint8_t seen[LXP_DA_CLASS_COUNT] = {0U, 0U, 0U, 0U, 0U};
    size_t total = 0U;
    size_t previous = 0U;
    size_t i;
    if (bundle == NULL || bundle->chunks == NULL ||
        bundle->chunk_count < LXP_DA_CLASS_COUNT ||
        bundle->chunk_count > LXP_DA_MAX_CHUNKS)
        return LXP_ERR_DA_MISSING;
    for (i = 0U; i < bundle->chunk_count; ++i) {
        const lxp_da_chunk *chunk = &bundle->chunks[i];
        size_t class_index;
        if (chunk->availability_class < LXP_DA_ACTIVITIES ||
            chunk->availability_class > LXP_DA_RECOVERY_METADATA)
            return LXP_ERR_DA_MISSING;
        class_index = (size_t)chunk->availability_class - 1U;
        if (class_index < previous || chunk->chunk_index != i ||
            chunk->batch_number != bundle->batch_number ||
            chunk->class_offset != offsets[class_index] ||
            chunk->length != chunk->bytes.length ||
            (chunk->bytes.bytes == NULL && chunk->length != 0U) ||
            (chunk->length == 0U &&
             (seen[class_index] != 0U ||
              (i + 1U < bundle->chunk_count &&
               bundle->chunks[i + 1U].availability_class ==
                   chunk->availability_class))))
            return LXP_ERR_DA_MISSING;
        if (chunk->length > SIZE_MAX - total)
            return LXP_ERR_LENGTH_LIMIT;
        offsets[class_index] += chunk->length;
        total += chunk->length;
        seen[class_index] = 1U;
        previous = class_index;
    }
    for (i = 0U; i < LXP_DA_CLASS_COUNT; ++i)
        if (seen[i] == 0U) return LXP_ERR_DA_MISSING;
    return total == bundle->total_bytes ? LXP_OK : LXP_ERR_DA_MISSING;
}

static lxp_result store_path(const lxp_da_store *store, uint64_t batch_number,
                             char path[LXP_DA_STORE_PATH_BYTES])
{
    int length;
    if (store == NULL || store->directory[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    length = snprintf(path, LXP_DA_STORE_PATH_BYTES, "%s/%020llu.lxda",
                      store->directory, (unsigned long long)batch_number);
    return length < 0 || length >= LXP_DA_STORE_PATH_BYTES ?
        LXP_ERR_LENGTH_LIMIT : LXP_OK;
}

lxp_result lxp_da_store_init(lxp_da_store *store, const char *directory)
{
    struct stat information;
    size_t length;
    if (store == NULL || directory == NULL ||
        stat(directory, &information) != 0 || !S_ISDIR(information.st_mode))
        return LXP_ERR_IO;
    length = strlen(directory);
    if (length == 0U || length >= sizeof(store->directory))
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(store, 0, sizeof(*store));
    (void)memcpy(store->directory, directory, length);
    return LXP_OK;
}

lxp_result lxp_da_store_bundle(const lxp_da_store *store,
                               const lxp_da_bundle *bundle,
                               lxp_arena *arena)
{
    uint8_t header[LXP_DA_STORE_HEADER_BYTES] = {'L','X','D','1'};
    uint8_t chunk_header[LXP_DA_STORE_CHUNK_HEADER_BYTES];
    uint8_t root[32];
    char final[LXP_DA_STORE_PATH_BYTES];
    char temporary[LXP_DA_STORE_PATH_BYTES];
    int descriptor = -1;
    int directory_descriptor = -1;
    int length;
    size_t i;
    lxp_result status;
    if (store == NULL || arena == NULL) return LXP_ERR_NON_CANONICAL;
    status = bundle_classes(bundle);
    if (status == LXP_OK) status = lxp_da_bundle_root(bundle, arena, root);
    if (status == LXP_OK)
        status = store_path(store, bundle->batch_number, final);
    length = status == LXP_OK ? snprintf(
        temporary, sizeof(temporary), "%s.tmp", final) : -1;
    if (status == LXP_OK && (length < 0 || (size_t)length >= sizeof(temporary)))
        status = LXP_ERR_LENGTH_LIMIT;
    if (status != LXP_OK) return status;
    put_u64(header + 4U, bundle->batch_number);
    (void)memcpy(header + 12U, root, 32U);
    put_u32(header + 44U, (uint32_t)bundle->chunk_count);
    put_u64(header + 48U, (uint64_t)bundle->total_bytes);
    if (unlink(temporary) != 0 && errno != ENOENT)
        return LXP_ERR_IO;
    descriptor = open(temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
                      0600);
    if (descriptor < 0) return LXP_ERR_IO;
    status = write_all(descriptor, header, sizeof(header));
    for (i = 0U; status == LXP_OK && i < bundle->chunk_count; ++i) {
        const lxp_da_chunk *chunk = &bundle->chunks[i];
        put_u32(chunk_header, chunk->chunk_index);
        chunk_header[4] = (uint8_t)chunk->availability_class;
        put_u64(chunk_header + 5U, chunk->class_offset);
        put_u32(chunk_header + 13U, chunk->length);
        (void)memcpy(chunk_header + 17U, chunk->chunk_hash, 32U);
        status = write_all(descriptor, chunk_header, sizeof(chunk_header));
        if (status == LXP_OK)
            status = write_all(descriptor, chunk->bytes.bytes,
                               chunk->bytes.length);
    }
    if (status == LXP_OK &&
        !lxp_durability_group_defer_descriptor(descriptor) &&
        fdatasync(descriptor) != 0)
        status = LXP_ERR_IO;
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    descriptor = -1;
    if (status == LXP_OK && rename(temporary, final) != 0) status = LXP_ERR_IO;
    directory_descriptor = status == LXP_OK ?
        open(store->directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC) : -1;
    if (status == LXP_OK &&
        (directory_descriptor < 0 ||
         (!lxp_durability_group_defer_descriptor(directory_descriptor) &&
          fsync(directory_descriptor) != 0)))
        status = LXP_ERR_IO;
    if (directory_descriptor >= 0) (void)close(directory_descriptor);
    if (status != LXP_OK) (void)unlink(temporary);
    return status;
}

lxp_result lxp_da_store_read_bundle(const lxp_da_store *store,
                                    uint64_t batch_number,
                                    lxp_arena *arena,
                                    lxp_da_bundle *bundle,
                                    uint8_t root[32])
{
    char path[LXP_DA_STORE_PATH_BYTES];
    struct stat information;
    uint8_t *file;
    lxp_da_chunk *chunks;
    void *memory;
    size_t size;
    size_t cursor = LXP_DA_STORE_HEADER_BYTES;
    size_t total = 0U;
    uint32_t count;
    uint64_t stored_total;
    int descriptor;
    size_t i;
    lxp_result status;
    if (arena == NULL || bundle == NULL || root == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = store_path(store, batch_number, path);
    if (status != LXP_OK) return status;
    descriptor = open(path, O_RDONLY | O_CLOEXEC);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        information.st_size < LXP_DA_STORE_HEADER_BYTES) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_DA_MISSING;
    }
    size = (size_t)information.st_size;
    if ((off_t)size != information.st_size ||
        size > LXP_DA_STORE_HEADER_BYTES +
            LXP_DA_MAX_CHUNKS * LXP_DA_STORE_CHUNK_HEADER_BYTES +
            LXP_MAX_BATCH_BODY_BYTES) {
        (void)close(descriptor);
        return LXP_ERR_DA_MISSING;
    }
    status = lxp_arena_alloc(arena, size, 1U, &memory);
    file = status == LXP_OK ? (uint8_t *)memory : NULL;
    if (status == LXP_OK) status = read_all(descriptor, file, size);
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status != LXP_OK) return status;
    count = get_u32(file + 44U);
    stored_total = get_u64(file + 48U);
    if (memcmp(file, "LXD1", 4U) != 0 || get_u64(file + 4U) != batch_number ||
        count < LXP_DA_CLASS_COUNT || count > LXP_DA_MAX_CHUNKS ||
        stored_total > LXP_MAX_BATCH_BODY_BYTES)
        return LXP_ERR_DA_MISSING;
    status = lxp_arena_alloc(arena, (size_t)count * sizeof(*chunks),
                             _Alignof(lxp_da_chunk), &memory);
    if (status != LXP_OK) return status;
    chunks = (lxp_da_chunk *)memory;
    for (i = 0U; i < count; ++i) {
        uint32_t chunk_length;
        if (cursor > size || size - cursor < LXP_DA_STORE_CHUNK_HEADER_BYTES)
            return LXP_ERR_DA_MISSING;
        chunk_length = get_u32(file + cursor + 13U);
        if (chunk_length > LXP_DA_MAX_CHUNK_BYTES ||
            size - cursor - LXP_DA_STORE_CHUNK_HEADER_BYTES < chunk_length ||
            chunk_length > SIZE_MAX - total)
            return LXP_ERR_DA_MISSING;
        chunks[i].batch_number = batch_number;
        chunks[i].chunk_index = get_u32(file + cursor);
        chunks[i].availability_class =
            (lxp_da_class)file[cursor + 4U];
        chunks[i].class_offset = get_u64(file + cursor + 5U);
        chunks[i].length = chunk_length;
        (void)memcpy(chunks[i].chunk_hash, file + cursor + 17U, 32U);
        cursor += LXP_DA_STORE_CHUNK_HEADER_BYTES;
        chunks[i].bytes = (lxp_byte_span){file + cursor, chunk_length};
        cursor += chunk_length;
        total += chunk_length;
    }
    if (cursor != size || total != stored_total) return LXP_ERR_DA_MISSING;
    bundle->chunks = chunks;
    bundle->chunk_count = count;
    bundle->batch_number = batch_number;
    bundle->total_bytes = total;
    status = bundle_classes(bundle);
    if (status != LXP_OK) return status;
    (void)memcpy(root, file + 12U, 32U);
    return LXP_OK;
}

lxp_result lxp_da_possession_verify(
    const lxp_da_store *store,
    const struct lxp_guarantor_attestation *attestation,
    const uint8_t expected_data_availability_root[32], lxp_arena *arena)
{
    lxp_da_bundle bundle;
    uint8_t stored_root[32];
    uint8_t rebuilt_root[32];
    size_t mark;
    lxp_result status;
    if (store == NULL || attestation == NULL ||
        expected_data_availability_root == NULL || arena == NULL ||
        !attestation->replayed || !attestation->da_possessed ||
        attestation->availability_class_mask !=
            LXP_GUARANTOR_AVAILABILITY_ALL ||
        lxp_ct_memcmp(attestation->data_availability_root,
                      expected_data_availability_root, 32U) != 0)
        return LXP_ERR_INVALID_ATTESTATION;
    mark = lxp_arena_mark(arena);
    status = lxp_da_store_read_bundle(store, attestation->batch_number, arena,
                                      &bundle, stored_root);
    if (status == LXP_OK)
        status = lxp_da_bundle_root(&bundle, arena, rebuilt_root);
    if (status == LXP_OK &&
        (lxp_ct_memcmp(stored_root, rebuilt_root, 32U) != 0 ||
         lxp_ct_memcmp(stored_root, expected_data_availability_root, 32U) != 0))
        status = LXP_ERR_DA_MISSING;
    (void)lxp_arena_reset(arena, mark);
    return status;
}

lxp_result lxp_da_possession_attest(
    const lxp_da_store *store, const struct lxp_guarantor_ctx *ctx,
    const struct lxp_checkpoint_certificate *checkpoint,
    uint64_t attested_at_ms, lxp_arena *arena,
    struct lxp_guarantor_attestation *attestation)
{
    lxp_guarantor_attestation probe;
    lxp_guarantor_ctx verified;
    lxp_result status;
    if (store == NULL || ctx == NULL || checkpoint == NULL || arena == NULL ||
        attestation == NULL || !ctx->ready_to_sign)
        return LXP_ERR_ATTESTATION_THRESHOLD;
    (void)memset(&probe, 0, sizeof(probe));
    probe.batch_number = checkpoint->header.batch_number;
    (void)memcpy(probe.data_availability_root,
                 checkpoint->header.data_availability_root, 32U);
    probe.replayed = true;
    probe.da_possessed = true;
    probe.availability_class_mask = LXP_GUARANTOR_AVAILABILITY_ALL;
    status = lxp_da_possession_verify(
        store, &probe, checkpoint->header.data_availability_root, arena);
    if (status != LXP_OK) return status;
    verified = *ctx;
    verified.possesses_availability = true;
    status = lxp_guarantor_attest(&verified, checkpoint, true, true,
                                  attested_at_ms, arena, attestation);
    if (status == LXP_OK)
        status = lxp_da_possession_verify(
            store, attestation, checkpoint->header.data_availability_root,
            arena);
    return status;
}
