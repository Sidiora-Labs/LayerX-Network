#define _POSIX_C_SOURCE 200809L

#include "lxp_daemon_artifact.h"

#include "layerx/lxp_crypto.h"

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

lxp_result lxp_daemon_artifact_read(
    const char *path, size_t maximum_length, size_t exact_length,
    uint8_t **bytes, size_t *length)
{
    struct stat initial;
    struct stat final;
    uint8_t *memory;
    size_t file_length;
    size_t offset = 0U;
    int descriptor;
    lxp_result status = LXP_OK;
    if (path == NULL || maximum_length == 0U || bytes == NULL ||
        length == NULL || exact_length > maximum_length)
        return LXP_ERR_NON_CANONICAL;
    *bytes = NULL;
    *length = 0U;
    descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0 || fstat(descriptor, &initial) != 0 ||
        !S_ISREG(initial.st_mode) || initial.st_nlink != 1 ||
        initial.st_size <= 0 ||
        (uint64_t)initial.st_size > maximum_length ||
        (exact_length != 0U &&
         (uint64_t)initial.st_size != exact_length)) {
        if (descriptor >= 0) (void)close(descriptor);
        return LXP_ERR_IO;
    }
    file_length = (size_t)initial.st_size;
    memory = (uint8_t *)malloc(file_length);
    if (memory == NULL) {
        (void)close(descriptor);
        return LXP_ERR_IO;
    }
    while (offset < file_length) {
        ssize_t count = read(descriptor, memory + offset,
                             file_length - offset);
        if (count > 0) offset += (size_t)count;
        else if (count < 0 && errno == EINTR) continue;
        else {
            status = LXP_ERR_IO;
            break;
        }
    }
    if (status == LXP_OK &&
        (fstat(descriptor, &final) != 0 || !S_ISREG(final.st_mode) ||
         final.st_nlink != 1 || final.st_dev != initial.st_dev ||
         final.st_ino != initial.st_ino || final.st_size != initial.st_size))
        status = LXP_ERR_IO;
    if (close(descriptor) != 0 && status == LXP_OK) status = LXP_ERR_IO;
    if (status != LXP_OK) {
        lxp_secure_zero(memory, file_length);
        free(memory);
        return status;
    }
    *bytes = memory;
    *length = file_length;
    return LXP_OK;
}
