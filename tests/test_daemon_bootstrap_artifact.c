#define _POSIX_C_SOURCE 200809L

#include "../cmd/layerxd/lxp_daemon_artifact.h"

#include "layerx/lxp_crypto.h"

#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define REQUIRE(condition) do { if (!(condition)) return 1; } while (0)

static int write_exact(int descriptor, const uint8_t *bytes, size_t length)
{
    size_t offset = 0U;
    while (offset < length) {
        ssize_t written = write(descriptor, bytes + offset, length - offset);
        if (written > 0) offset += (size_t)written;
        else if (written < 0 && errno == EINTR) continue;
        else return 1;
    }
    return 0;
}

int main(void)
{
    static const uint8_t expected[4] = {1U, 2U, 3U, 4U};
    char path[] = "/tmp/layerx-bootstrap-artifact-XXXXXX";
    char link_path[] = "/tmp/layerx-bootstrap-artifact-link-XXXXXX";
    char hardlink_path[] = "/tmp/layerx-bootstrap-artifact-hard-XXXXXX";
    uint8_t *bytes = NULL;
    size_t length = 0U;
    int descriptor = mkstemp(path);
    int link_descriptor = mkstemp(link_path);
    int hardlink_descriptor = mkstemp(hardlink_path);
    REQUIRE(descriptor >= 0 && link_descriptor >= 0 &&
            hardlink_descriptor >= 0);
    REQUIRE(close(link_descriptor) == 0 && close(hardlink_descriptor) == 0);
    REQUIRE(unlink(link_path) == 0 && unlink(hardlink_path) == 0);
    REQUIRE(write_exact(descriptor, expected, sizeof(expected)) == 0);
    REQUIRE(close(descriptor) == 0);
    REQUIRE(lxp_daemon_artifact_read(
        path, sizeof(expected), sizeof(expected), &bytes, &length) == LXP_OK);
    REQUIRE(length == sizeof(expected) &&
            memcmp(bytes, expected, sizeof(expected)) == 0);
    lxp_secure_zero(bytes, length);
    free(bytes);
    bytes = NULL;
    length = 0U;
    REQUIRE(lxp_daemon_artifact_read(
        path, sizeof(expected), sizeof(expected) - 1U,
        &bytes, &length) != LXP_OK);
    REQUIRE(bytes == NULL && length == 0U);
    REQUIRE(symlink(path, link_path) == 0);
    REQUIRE(lxp_daemon_artifact_read(
        link_path, sizeof(expected), sizeof(expected),
        &bytes, &length) != LXP_OK);
    REQUIRE(bytes == NULL && length == 0U);
    REQUIRE(link(path, hardlink_path) == 0);
    REQUIRE(lxp_daemon_artifact_read(
        path, sizeof(expected), sizeof(expected),
        &bytes, &length) != LXP_OK);
    REQUIRE(bytes == NULL && length == 0U);
    REQUIRE(unlink(hardlink_path) == 0 && unlink(link_path) == 0 &&
            unlink(path) == 0);
    return 0;
}
