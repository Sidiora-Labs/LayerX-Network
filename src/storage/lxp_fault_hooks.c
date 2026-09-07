#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_fault.h"

#include <stdbool.h>
#include <stddef.h>
#include <sys/types.h>
#include <sys/wait.h>
#include <unistd.h>

enum { LXP_FAULT_EXIT_BASE = 128 };

typedef struct lxp_fault_state {
    lxp_fault_boundary boundary;
    uint32_t occurrence;
    uint32_t observed;
    bool armed;
} lxp_fault_state;

static _Thread_local lxp_fault_state active_fault;

static bool boundary_valid(lxp_fault_boundary boundary)
{
    return boundary >= LXP_FAULT_LOG_HEADER_WRITTEN &&
           boundary <= LXP_FAULT_BATCH_WAL_DIRECTORY_SYNCED;
}

lxp_result lxp_fault_arm(lxp_fault_boundary boundary, uint32_t occurrence)
{
    if (!boundary_valid(boundary) || occurrence == 0U)
        return LXP_ERR_NON_CANONICAL;
    active_fault.boundary = boundary;
    active_fault.occurrence = occurrence;
    active_fault.observed = 0U;
    active_fault.armed = true;
    return LXP_OK;
}

void lxp_fault_disarm(void)
{
    active_fault.boundary = (lxp_fault_boundary)0;
    active_fault.occurrence = 0U;
    active_fault.observed = 0U;
    active_fault.armed = false;
}

void lxp_fault_inject_point(lxp_fault_boundary boundary)
{
    if (!active_fault.armed || boundary != active_fault.boundary) return;
    if (active_fault.observed == UINT32_MAX) _exit(LXP_FAULT_EXIT_BASE);
    active_fault.observed += 1U;
    if (active_fault.observed == active_fault.occurrence)
        _exit(LXP_FAULT_EXIT_BASE + (int)boundary);
}

lxp_result lxp_fault_crash_at_boundary(lxp_fault_boundary boundary,
                                       uint32_t occurrence,
                                       lxp_fault_workload_fn workload,
                                       void *context,
                                       int *child_exit_status)
{
    pid_t child;
    int status;
    int expected;
    if (!boundary_valid(boundary) || occurrence == 0U || workload == NULL ||
        child_exit_status == NULL) return LXP_ERR_NON_CANONICAL;
    child = fork();
    if (child < 0) return LXP_ERR_IO;
    if (child == 0) {
        lxp_result result = lxp_fault_arm(boundary, occurrence);
        if (result == LXP_OK) result = workload(context);
        _exit(result == LXP_OK ? 0 : 1);
    }
    if (waitpid(child, &status, 0) != child || !WIFEXITED(status))
        return LXP_ERR_IO;
    *child_exit_status = WEXITSTATUS(status);
    expected = LXP_FAULT_EXIT_BASE + (int)boundary;
    return *child_exit_status == expected ? LXP_OK : LXP_FATAL_INVARIANT;
}
