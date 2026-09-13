#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"

#include <fcntl.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <unistd.h>

int main(void)
{
    char directory[] = "/tmp/lxp-log-XXXXXX";
    char path[128];
    const uint8_t body[] = { 0x01U, 0x02U, 0x80U, 0xffU };
    uint8_t decoded[sizeof(body)];
    lxp_log log;
    lxp_log_record_header header;
    struct stat information;
    uint64_t offset;
    uint8_t corrupt = 0U;
    size_t kind;
    if (mkdtemp(directory) == NULL) return 1;
    if (lxp_log_segment_create(&log, directory, 42U, 4096U) != LXP_OK)
        return 1;
    if (snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 42U) < 0)
        return 1;
    if (fstat(log.descriptor, &information) != 0 || information.st_size != 4096)
        return 1;
    for (kind = LXP_LOG_ACTIVITY; kind <= LXP_LOG_BATCH_BODY; ++kind) {
        if (lxp_log_append(&log, (lxp_log_record_kind)kind, kind, body,
                           (uint32_t)sizeof(body), &offset) != LXP_OK) return 1;
        if (lxp_log_read(&log, offset, &header, decoded, sizeof(decoded)) !=
            LXP_OK || header.record_kind != kind ||
            memcmp(body, decoded, sizeof(body)) != 0) return 1;
    }
    {
        uint64_t before = log.write_offset;
        uint64_t sequence_before = log.next_sequence;
        if (lxp_log_append(&log, LXP_LOG_ACTIVITY, UINT64_MAX, body,
                           (uint32_t)sizeof(body), NULL) !=
                LXP_ERR_SEQUENCE_EXHAUSTED || log.write_offset != before ||
            log.next_sequence != sequence_before) return 1;
    }
    if (lxp_log_append(&log, (lxp_log_record_kind)10, 10U, body,
                       (uint32_t)sizeof(body), NULL) != LXP_ERR_NON_CANONICAL)
        return 1;
    if (pread(log.descriptor, &corrupt, 1U,
              (off_t)(offset + LXP_LOG_HEADER_BYTES)) != 1) return 1;
    corrupt ^= 1U;
    if (pwrite(log.descriptor, &corrupt, 1U,
               (off_t)(offset + LXP_LOG_HEADER_BYTES)) != 1) return 1;
    if (lxp_log_read(&log, offset, &header, decoded, sizeof(decoded)) !=
        LXP_ERR_LOG_CORRUPT) return 1;
    if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
        rmdir(directory) != 0) return 1;
    return 0;
}
