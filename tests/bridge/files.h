#ifndef LAYERX_BRIDGE_TEST_FILES_H
#define LAYERX_BRIDGE_TEST_FILES_H

#include <fcntl.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

static int read_file(const char *path, size_t maximum, bool private_file,
                      uint8_t **bytes, size_t *length)
{
    struct stat information;
    size_t offset = 0U;
    int descriptor = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    if (descriptor < 0) return 1;
    if (fstat(descriptor, &information) != 0 || !S_ISREG(information.st_mode) ||
        information.st_nlink != 1 || information.st_size <= 0 ||
        (uint64_t)information.st_size > maximum ||
        (private_file && (information.st_mode & 077U) != 0U)) {
        (void)close(descriptor);
        return 1;
    }
    *length = (size_t)information.st_size;
    *bytes = malloc(*length);
    if (*bytes == NULL) {
        (void)close(descriptor);
        return 1;
    }
    while (offset < *length) {
        ssize_t count = read(descriptor, *bytes + offset, *length - offset);
        if (count <= 0) {
            (void)close(descriptor);
            return 1;
        }
        offset += (size_t)count;
    }
    return close(descriptor) == 0 ? 0 : 1;
}

#endif
