#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_fault.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/types.h>
#include <unistd.h>

enum { LXP_SNAPSHOT_STORE_HEADER_BYTES = 116 };

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

lxp_result lxp_snapshot_store_write(const char *directory,
                                    const lxp_snapshot_manifest_record *manifest,
                                    const uint8_t *snapshot,
                                    size_t snapshot_length)
{
    uint8_t header[LXP_SNAPSHOT_STORE_HEADER_BYTES] = {'L','X','S','2'};
    lxp_snapshot_manifest_record expected;
    char temporary[4096];
    char final[4096];
    int descriptor;
    int directory_descriptor;
    int length;
    lxp_result status;
    if (directory == NULL || manifest == NULL ||
        (snapshot == NULL && snapshot_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    status = lxp_snapshot_manifest_build(
        snapshot, snapshot_length, manifest->global_sequence,
        manifest->canonical_state_root, manifest->receipt_state_root,
        &expected);
    if (status != LXP_OK) return status;
    if (memcmp(expected.snapshot_digest, manifest->snapshot_digest, 32U) != 0)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    length = snprintf(final, sizeof(final), "%s/%020llu.lxs", directory,
                      (unsigned long long)manifest->global_sequence);
    if (length < 0 || (size_t)length >= sizeof(final))
        return LXP_ERR_LENGTH_LIMIT;
    length = snprintf(temporary, sizeof(temporary), "%s.tmp", final);
    if (length < 0 || (size_t)length >= sizeof(temporary) ||
        access(final, F_OK) == 0) return LXP_ERR_NON_CANONICAL;
    put_u64(header + 4U, manifest->global_sequence);
    (void)memcpy(header + 12U, manifest->canonical_state_root, 32U);
    (void)memcpy(header + 44U, manifest->receipt_state_root, 32U);
    (void)memcpy(header + 76U, manifest->snapshot_digest, 32U);
    put_u64(header + 108U, (uint64_t)snapshot_length);
    descriptor = open(temporary, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC,
                      0600);
    if (descriptor < 0) return LXP_ERR_IO;
    status = write_all(descriptor, header, sizeof(header));
    if (status == LXP_OK)
        lxp_fault_inject_point(LXP_FAULT_CHECKPOINT_HEADER_WRITTEN);
    if (status == LXP_OK) {
        status = write_all(descriptor, snapshot, snapshot_length);
        if (status == LXP_OK)
            lxp_fault_inject_point(LXP_FAULT_CHECKPOINT_BODY_WRITTEN);
    }
    if (status == LXP_OK) {
        if (fdatasync(descriptor) != 0) status = LXP_ERR_IO;
        else lxp_fault_inject_point(LXP_FAULT_CHECKPOINT_FILE_SYNCED);
    }
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status == LXP_OK) {
        if (rename(temporary, final) != 0) status = LXP_ERR_IO;
        else lxp_fault_inject_point(LXP_FAULT_CHECKPOINT_RENAMED);
    }
    directory_descriptor = status == LXP_OK ?
        open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC) : -1;
    if (status == LXP_OK) {
        if (directory_descriptor < 0 || fsync(directory_descriptor) != 0)
            status = LXP_ERR_IO;
        else
            lxp_fault_inject_point(LXP_FAULT_CHECKPOINT_DIRECTORY_SYNCED);
    }
    if (directory_descriptor >= 0) (void)close(directory_descriptor);
    if (status != LXP_OK) (void)unlink(temporary);
    return status;
}

lxp_result lxp_snapshot_store_read(const char *path, lxp_arena *arena,
                                   lxp_snapshot_manifest_record *manifest,
                                   lxp_byte_span *snapshot)
{
    uint8_t header[LXP_SNAPSHOT_STORE_HEADER_BYTES];
    struct stat information;
    void *memory;
    uint64_t length;
    int descriptor;
    lxp_result status;
    if (path == NULL || arena == NULL || manifest == NULL || snapshot == NULL)
        return LXP_ERR_NON_CANONICAL;
    descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0 || fstat(descriptor, &information) != 0 ||
        !S_ISREG(information.st_mode) || information.st_nlink != 1) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    status = read_all(descriptor, header, sizeof(header));
    length = status == LXP_OK ? get_u64(header + 108U) : 0U;
    if (status == LXP_OK && (memcmp(header, "LXS2", 4U) != 0 ||
        length > SIZE_MAX ||
        length > UINT64_MAX - sizeof(header) || information.st_size < 0 ||
        (uint64_t)information.st_size != sizeof(header) + length))
        status = LXP_ERR_SNAPSHOT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_arena_alloc(arena, (size_t)length, 1U, &memory);
    if (status == LXP_OK)
        status = read_all(descriptor, (uint8_t *)memory, (size_t)length);
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status != LXP_OK) return status;
    manifest->global_sequence = get_u64(header + 4U);
    (void)memcpy(manifest->canonical_state_root, header + 12U, 32U);
    (void)memcpy(manifest->receipt_state_root, header + 44U, 32U);
    (void)memcpy(manifest->snapshot_digest, header + 76U, 32U);
    snapshot->bytes = (const uint8_t *)memory;
    snapshot->length = (size_t)length;
    {
        lxp_snapshot_manifest_record expected;
        status = lxp_snapshot_manifest_build(
            snapshot->bytes, snapshot->length, manifest->global_sequence,
            manifest->canonical_state_root, manifest->receipt_state_root,
            &expected);
        if (status != LXP_OK) return status;
        return memcmp(expected.snapshot_digest, manifest->snapshot_digest,
                      32U) == 0 ? LXP_OK : LXP_ERR_SNAPSHOT_MISMATCH;
    }
}
