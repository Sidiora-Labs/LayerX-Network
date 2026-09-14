#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_storage.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "replica prefix requirement failed at %d\n", __LINE__); \
    return 1; } } while (0)

int main(int argc, char **argv)
{
    lxp_log source = {.descriptor = -1};
    lxp_log target = {.descriptor = -1};
    lxp_log_record_header header;
    uint8_t *body;
    uint64_t valid_end, last_record, next_sequence;
    uint64_t capacity;
    uint64_t target_capacity;
    char *end = NULL;
    int descriptor;
    REQUIRE(argc == 4);
    errno = 0;
    capacity = strtoull(argv[3], &end, 10);
    REQUIRE(errno == 0 && end != argv[3] && *end == '\0');
    REQUIRE(lxp_log_open(&source, argv[1]) == LXP_OK);
    REQUIRE(lxp_log_read(&source, 0U, &header, NULL, 0U) == LXP_ERR_LENGTH_LIMIT);
    REQUIRE(header.record_kind == LXP_LOG_STATE_DIFF && header.global_sequence == 1U);
    REQUIRE(header.body_length >= 5U && header.body_length <= 1024U * 1024U);
    REQUIRE(source.capacity == LXP_LOG_HEADER_BYTES + header.body_length);
    REQUIRE(capacity > source.capacity);
    body = malloc(header.body_length);
    REQUIRE(body != NULL);
    REQUIRE(lxp_log_read(&source, 0U, &header, body, header.body_length) == LXP_OK);
    REQUIRE(memcmp(body, "LXBE1", 5U) == 0);
    descriptor = open(argv[2], O_CREAT | O_EXCL | O_RDWR | O_NOFOLLOW | O_CLOEXEC, 0600);
    REQUIRE(descriptor >= 0 && close(descriptor) == 0);
    REQUIRE(lxp_log_open_or_create(&target, argv[2], capacity) == LXP_OK);
    target_capacity = target.capacity;
    REQUIRE(target_capacity > source.capacity && target_capacity < capacity);
    REQUIRE(lxp_log_append(&target, LXP_LOG_STATE_DIFF, 1U, body, header.body_length, NULL) == LXP_OK);
    REQUIRE(lxp_log_sync(&target) == LXP_OK);
    REQUIRE(target.has_durable_marker && target.durable_offset == source.capacity);
    REQUIRE(lxp_log_close(&target) == LXP_OK);
    REQUIRE(lxp_log_open(&target, argv[2]) == LXP_OK);
    REQUIRE(target.has_durable_marker && target.capacity == target_capacity);
    REQUIRE(target.durable_offset == source.capacity && target.durable_next_sequence == 2U);
    REQUIRE(lxp_log_scan_tail(&target, &valid_end, &last_record, &next_sequence) == LXP_OK);
    REQUIRE(valid_end == source.capacity && last_record == 0U && next_sequence == 2U);
    REQUIRE(lxp_log_close(&target) == LXP_OK && lxp_log_close(&source) == LXP_OK);
    free(body);
    return 0;
}
