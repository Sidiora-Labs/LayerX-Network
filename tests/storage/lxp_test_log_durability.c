#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_storage.h"

#include <stdint.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/wait.h>
#include <unistd.h>

static int run_child(const char *directory, uint32_t abort_boundary)
{
    const uint8_t activity[] = { 1U, 2U };
    const uint8_t receipt[] = { 3U, 4U };
    const uint8_t state_diff[] = { 5U, 6U };
    const uint8_t batch[] = { 7U, 8U };
    lxp_log log;
    if (lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK)
        return 1;
    if (lxp_log_append(&log, LXP_LOG_ACTIVITY, 11U, activity,
                       (uint32_t)sizeof(activity), NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK) return 1;
    if (lxp_log_fault_point(1U, abort_boundary)) _exit(81);
    if (lxp_log_append(&log, LXP_LOG_RECEIPT, 11U, receipt,
                       (uint32_t)sizeof(receipt), NULL) != LXP_OK ||
        lxp_log_append(&log, LXP_LOG_STATE_DIFF, 11U, state_diff,
                       (uint32_t)sizeof(state_diff), NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK) return 1;
    if (lxp_log_fault_point(2U, abort_boundary)) _exit(82);
    if (lxp_log_append(&log, LXP_LOG_BATCH_HEADER, 11U, batch,
                       (uint32_t)sizeof(batch), NULL) != LXP_OK ||
        lxp_log_write_boundary(&log) != LXP_OK) return 1;
    if (lxp_log_fault_point(3U, abort_boundary)) _exit(83);
    return lxp_log_close(&log) == LXP_OK ? 0 : 1;
}

int main(void)
{
    uint32_t boundary;
    for (boundary = 1U; boundary <= 3U; ++boundary) {
        char directory[] = "/tmp/lxp-durable-XXXXXX";
        char path[128];
        pid_t child;
        int child_status;
        lxp_log log;
        uint64_t durable;
        if (mkdtemp(directory) == NULL) return 1;
        child = fork();
        if (child < 0) return 1;
        if (child == 0) _exit(run_child(directory, boundary));
        if (waitpid(child, &child_status, 0) != child ||
            !WIFEXITED(child_status) || WEXITSTATUS(child_status) == 0)
            return 1;
        if (snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0)
            return 1;
        if (lxp_log_open(&log, path) != LXP_OK ||
            lxp_log_durable_head(&log, &durable) != LXP_OK) return 1;
        if ((boundary == 1U && durable != UINT64_MAX) ||
            (boundary > 1U && durable != 11U)) return 1;
        if (lxp_log_close(&log) != LXP_OK || unlink(path) != 0 ||
            rmdir(directory) != 0) return 1;
    }
    {
        char directory[] = "/tmp/lxp-durable-group-XXXXXX";
        char first_path[128];
        char second_path[128];
        const uint8_t first[] = { 1U, 2U };
        const uint8_t second[] = { 3U, 4U };
        const uint8_t later[] = { 5U, 6U };
        lxp_durability_group group;
        lxp_log first_log;
        lxp_log second_log;
        uint64_t durable;
        int descriptor;
        int directory_descriptor;
        uint8_t corrupt = 0xffU;
        off_t first_pair = (off_t)(2U *
            (LXP_LOG_HEADER_BYTES + sizeof(first)));
        off_t later_body = first_pair + (off_t)LXP_LOG_HEADER_BYTES;
        if (mkdtemp(directory) == NULL ||
            lxp_log_segment_create(&first_log, directory, 0U, 4096U) != LXP_OK ||
            lxp_log_segment_create(&second_log, directory, 1U, 4096U) != LXP_OK ||
            lxp_durability_group_begin(&group) != LXP_OK ||
            lxp_log_append(&first_log, LXP_LOG_ACTIVITY, 11U, first,
                           (uint32_t)sizeof(first), NULL) != LXP_OK ||
            lxp_log_append(&first_log, LXP_LOG_RECEIPT, 11U, first,
                           (uint32_t)sizeof(first), NULL) != LXP_OK ||
            lxp_log_write_boundary(&first_log) != LXP_OK ||
            lxp_log_append(&second_log, LXP_LOG_ACTIVITY, 12U, second,
                           (uint32_t)sizeof(second), NULL) != LXP_OK ||
            lxp_log_append(&second_log, LXP_LOG_RECEIPT, 12U, second,
                           (uint32_t)sizeof(second), NULL) != LXP_OK ||
            lxp_log_write_boundary(&second_log) != LXP_OK)
            return 1;
        directory_descriptor = open(directory, O_RDONLY | O_DIRECTORY | O_CLOEXEC);
        if (directory_descriptor < 0 ||
            !lxp_durability_group_defer_descriptor(directory_descriptor) ||
            close(directory_descriptor) != 0 ||
            lxp_durability_group_commit(&group) != LXP_OK)
            return 1;
        if (lxp_log_durable_head(&first_log, &durable) != LXP_OK ||
            durable != 11U)
            return 1;
        if (lxp_log_durable_head(&second_log, &durable) != LXP_OK ||
            durable != 12U)
            return 1;
        if (
            lxp_durability_group_begin(&group) != LXP_OK ||
            lxp_log_append(&first_log, LXP_LOG_ACTIVITY, 13U, later,
                           (uint32_t)sizeof(later), NULL) != LXP_OK ||
            lxp_log_append(&first_log, LXP_LOG_RECEIPT, 13U, later,
                           (uint32_t)sizeof(later), NULL) != LXP_OK ||
            lxp_log_write_boundary(&first_log) != LXP_OK ||
            lxp_log_append(&second_log, LXP_LOG_ACTIVITY, 14U, later,
                           (uint32_t)sizeof(later), NULL) != LXP_OK ||
            lxp_log_append(&second_log, LXP_LOG_RECEIPT, 14U, later,
                           (uint32_t)sizeof(later), NULL) != LXP_OK ||
            lxp_log_write_boundary(&second_log) != LXP_OK ||
            lxp_durability_group_commit(&group) != LXP_OK ||
            snprintf(first_path, sizeof(first_path), "%s/%020u.lxp",
                     directory, 0U) < 0 ||
            snprintf(second_path, sizeof(second_path), "%s/%020u.lxp",
                     directory, 1U) < 0 ||
            lxp_log_close(&first_log) != LXP_OK ||
            lxp_log_close(&second_log) != LXP_OK)
            return 1;
        descriptor = open(first_path, O_WRONLY | O_CLOEXEC);
        if (descriptor < 0 || pwrite(descriptor, &corrupt, 1U, later_body) != 1 ||
            fdatasync(descriptor) != 0 || close(descriptor) != 0)
            return 1;
        lxp_log_set_prepared_recovery(true);
        if (
            lxp_log_open(&first_log, first_path) != LXP_OK ||
            lxp_log_recover(&first_log, NULL, NULL) != LXP_OK ||
            lxp_log_durable_head(&first_log, &durable) != LXP_OK ||
            durable != 11U ||
            first_log.write_offset != (uint64_t)first_pair ||
            lxp_log_close(&first_log) != LXP_OK ||
            unlink(first_path) != 0 || unlink(second_path) != 0 ||
            rmdir(directory) != 0)
            return 1;
        lxp_log_set_prepared_recovery(false);
    }
    {
        char directory[] = "/tmp/lxp-durable-failure-XXXXXX";
        char path[128];
        const uint8_t bytes[] = {1U, 2U};
        lxp_log log;
        lxp_durability_group group;
        uint64_t durable;
        if (mkdtemp(directory) == NULL ||
            lxp_log_segment_create(&log, directory, 0U, 4096U) != LXP_OK ||
            lxp_durability_group_begin(&group) != LXP_OK ||
            lxp_log_append(&log, LXP_LOG_ACTIVITY, 1U, bytes, sizeof(bytes), NULL) != LXP_OK ||
            lxp_log_append(&log, LXP_LOG_RECEIPT, 1U, bytes, sizeof(bytes), NULL) != LXP_OK ||
            lxp_log_write_boundary(&log) != LXP_OK ||
            group.descriptor_count != 1U || close(group.descriptors[0]) != 0 ||
            lxp_durability_group_commit(&group) != LXP_ERR_IO || group.active ||
            log.durable_offset != 0U || log.durable_generation != 1U ||
            lxp_log_durable_head(&log, &durable) != LXP_OK || durable != UINT64_MAX ||
            lxp_log_close(&log) != LXP_OK ||
            snprintf(path, sizeof(path), "%s/%020u.lxp", directory, 0U) < 0 ||
            unlink(path) != 0 || rmdir(directory) != 0)
            return 1;
    }
    return 0;
}
