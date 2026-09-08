#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_protocol.h"
#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <time.h>
#include <unistd.h>

int registry_read_node(const char *path, const char *actor, uint32_t network);

static uint64_t load(const uint8_t *p, size_t n)
{
    uint64_t v = 0U;
    for (size_t i = 0U; i < n; ++i) v = (v << 8U) | p[i];
    return v;
}

static void store(uint8_t *p, uint64_t v, size_t n)
{
    for (size_t i = 0U; i < n; ++i) p[i] = (uint8_t)(v >> (8U * (n - i - 1U)));
}

static int64_t now_ms(void)
{
    struct timespec now;
    if (clock_gettime(CLOCK_MONOTONIC, &now) != 0) return -1;
    return (int64_t)now.tv_sec * 1000 + now.tv_nsec / 1000000;
}

static int transfer(int fd, uint8_t *p, size_t n, int writing, int64_t deadline)
{
    while (n > 0U) {
        int64_t now = now_ms();
        if (now < 0 || now >= deadline) return 1;
        struct pollfd pending = {fd, writing ? POLLOUT : POLLIN, 0};
        int ready = poll(&pending, 1U, (int)(deadline - now));
        if (ready < 0 && errno == EINTR) continue;
        if (ready <= 0 || (pending.revents & (POLLERR | POLLNVAL))) return 1;
        ssize_t amount = writing ? send(fd, p, n, MSG_NOSIGNAL) : recv(fd, p, n, 0);
        if (amount < 0 && (errno == EINTR || errno == EAGAIN)) continue;
        if (amount <= 0) return 1;
        p += (size_t)amount;
        n -= (size_t)amount;
    }
    return 0;
}

static int exchange(int fd, uint16_t minor, uint16_t tag, uint64_t correlation,
                    const uint8_t *request, size_t length, uint8_t response[8192],
                    size_t *response_length, uint16_t *response_minor, int64_t deadline)
{
    uint8_t frame[8192] = {0};
    if (length > sizeof(frame) - 26U) return 1;
    store(frame, 22U + length, 4U);
    store(frame + 4U, 1U, 2U);
    store(frame + 6U, minor, 2U);
    store(frame + 8U, tag, 2U);
    store(frame + 10U, correlation, 8U);
    store(frame + 18U, length, 4U);
    if (length) (void)memcpy(frame + 22U, request, length);
    if (transfer(fd, frame, length + 26U, 1, deadline) ||
        transfer(fd, frame, 4U, 0, deadline)) return 1;
    size_t size = (size_t)load(frame, 4U);
    if (size < 22U || size > sizeof(frame) || transfer(fd, frame, size, 0, deadline)) return 1;
    size_t payload = (size_t)load(frame + 14U, 4U);
    if (payload != size - 22U || load(frame, 2U) != 1U ||
        load(frame + 4U, 2U) != (uint64_t)tag + 1U ||
        load(frame + 6U, 8U) != correlation || load(frame + 18U + payload, 4U) != 0U) return 1;
    *response_minor = (uint16_t)load(frame + 2U, 2U);
    *response_length = payload;
    (void)memcpy(response, frame + 18U, payload);
    return 0;
}

int registry_read_node(const char *path, const char *actor, uint32_t network)
{
    struct sockaddr_un address = {0};
    uint8_t response[8192], request[4U + LXP_MAX_DID_LENGTH];
    size_t size = 0U, actor_length = strlen(actor);
    uint16_t minor;
    if (path[0] != '/' || strlen(path) >= sizeof(address.sun_path) ||
        actor_length == 0U || actor_length > LXP_MAX_DID_LENGTH) return 1;
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK, 0);
    if (fd < 0) return 1;
    int result = 1;
    int64_t start = now_ms();
    if (start < 0) goto done;
    int64_t deadline = start + 10000;
    address.sun_family = AF_UNIX;
    (void)memcpy(address.sun_path, path, strlen(path) + 1U);
    if (connect(fd, (struct sockaddr *)&address, sizeof(address)) != 0) goto done;
    if (exchange(fd, 0U, 1U, 0U, NULL, 0U, response, &size, &minor, deadline) ||
        size < 93U || load(response, 2U) != 1U || load(response + 2U, 2U) < 1U ||
        load(response + 2U, 2U) != minor || load(response + 4U, 2U) != 3U ||
        load(response + 6U, 4U) != network || response[10] != 1U) goto done;
    uint64_t head = load(response + 11U, 8U);
    size_t count = (size_t)load(response + 91U, 2U), cursor = 93U;
    char previous[65] = {0};
    int preparation = 0;
    if (count > 64U) goto done;
    for (size_t i = 0U; i < count; ++i) {
        if (size - cursor < 2U) goto done;
        size_t length = (size_t)load(response + cursor, 2U); cursor += 2U;
        if (length == 0U || length > 64U || length > size - cursor) goto done;
        char name[65] = {0};
        for (size_t j = 0U; j < length; ++j)
            if (response[cursor + j] < 33U || response[cursor + j] > 126U) goto done;
        (void)memcpy(name, response + cursor, length);
        if (strcmp(previous, name) >= 0) goto done;
        (void)memcpy(previous, name, sizeof(name));
        if (strcmp(name, "preparation_state") == 0) preparation = 1;
        cursor += length;
    }
    if (cursor != size || !preparation) goto done;
    store(request, 1U, 2U);
    store(request + 2U, actor_length, 2U);
    (void)memcpy(request + 4U, actor, actor_length);
    uint16_t received_minor;
    if (exchange(fd, minor, 26U, 1U, request, actor_length + 4U,
                 response, &size, &received_minor, deadline) || received_minor != minor ||
        size > 4096U || size < actor_length + 74U || load(response, 2U) != 1U ||
        load(response + 2U, 2U) != actor_length ||
        memcmp(response + 4U, actor, actor_length) != 0) goto done;
    cursor = 4U + actor_length;
    if (load(response + cursor, 4U) != network ||
        load(response + cursor + 12U, 8U) == 0U ||
        load(response + cursor + 20U, 8U) < head ||
        load(response + cursor + 60U, 8U) == 0U) goto done;
    int nonzero = 0;
    for (size_t i = 0U; i < 32U; ++i) nonzero |= response[cursor + 28U + i];
    if (!nonzero) goto done;
    cursor += 68U;
    count = (size_t)load(response + cursor, 2U); cursor += 2U;
    if (count == 0U || count > 9U) goto done;
    uint16_t ids[9], ordinals[9][64];
    size_t counts[9];
    for (size_t i = 0U; i < count; ++i) {
        if (size - cursor < 4U) goto done;
        ids[i] = (uint16_t)load(response + cursor, 2U);
        counts[i] = (size_t)load(response + cursor + 2U, 2U); cursor += 4U;
        if (ids[i] == 0U || ids[i] > 9U || (i && ids[i - 1U] >= ids[i]) ||
            counts[i] == 0U || counts[i] > 64U || counts[i] > (size - cursor) / 4U) goto done;
        for (size_t j = 0U; j < counts[i]; ++j) {
            uint32_t type = (uint32_t)load(response + cursor, 4U); cursor += 4U;
            ordinals[i][j] = (uint16_t)(type & 65535U);
            if ((type >> 16U) != ids[i] || ordinals[i][j] == 0U ||
                (j && ordinals[i][j - 1U] >= ordinals[i][j])) goto done;
        }
    }
    if (cursor != size) goto done;
    (void)printf("{\"modules\":[");
    for (size_t i = 0U; i < count; ++i) {
        (void)printf("%s{\"module\":%u,\"ordinals\":[", i ? "," : "", ids[i]);
        for (size_t j = 0U; j < counts[i]; ++j)
            (void)printf("%s%u", j ? "," : "", ordinals[i][j]);
        (void)printf("]}");
    }
    (void)puts("]}");
    result = fflush(stdout) == 0 && !ferror(stdout) ? 0 : 1;
done:
    if (close(fd) != 0) result = 1;
    return result;
}
