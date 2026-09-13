#define _POSIX_C_SOURCE 200809L
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_fault.h"
#include <errno.h>
#include <limits.h>
#include <stdatomic.h>
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <time.h>

static lxp_daemon_apply_batch_fn actual_apply;
static unsigned long boundary;
static unsigned long occurrence;
static int gate;
static atomic_bool armed;
static atomic_uint observed;

void __real_lxp_fault_inject_point(lxp_fault_boundary);

void __wrap_lxp_fault_inject_point(lxp_fault_boundary point)
{
    if (atomic_load_explicit(&armed, memory_order_acquire) &&
        (unsigned long)point == boundary &&
        atomic_fetch_add_explicit(&observed, 1U, memory_order_relaxed) + 1U == occurrence)
        _exit(128 + (int)point);
    __real_lxp_fault_inject_point(point);
}

lxp_result __real_lxp_daemon_start_protocol_batch(
    lxp_daemon *, const lxp_daemon_configuration *, lxp_daemon_apply_batch_fn,
    void *, lxp_daemon_protocol_owner *, const char *, uint16_t);

static lxp_result crash_apply(void *context, uint64_t sequence,
    const lxp_daemon_activity *activities, size_t count, size_t *consumed)
{
    if (!atomic_load_explicit(&armed, memory_order_acquire)) {
        char ready;
        if (read(gate, &ready, 1U) != 1 || ready != 'G' || close(gate) != 0)
            return LXP_ERR_IO;
        atomic_store_explicit(&armed, true, memory_order_release);
    }
    lxp_result status = actual_apply(context, sequence, activities, count, consumed);
    if (status == LXP_OK)
        for (unsigned attempt = 0U; attempt < 2000U; ++attempt) {
            struct timespec pause = {0, 10000000L};
            (void)nanosleep(&pause, NULL);
        }
    fprintf(stderr, "batch returned before crash boundary: %d\n", (int)status);
    _exit(1);
}

lxp_result __wrap_lxp_daemon_start_protocol_batch(
    lxp_daemon *daemon, const lxp_daemon_configuration *configuration,
    lxp_daemon_apply_batch_fn apply, void *context,
    lxp_daemon_protocol_owner *owner, const char *address, uint16_t port)
{
    actual_apply = apply;
    return __real_lxp_daemon_start_protocol_batch(daemon, configuration,
        crash_apply, context, owner, address, port);
}

static bool number(const char *name, unsigned long *value)
{
    const char *text = getenv(name);
    char *end;
    if (text == NULL || *text == '\0') return false;
    errno = 0;
    *value = strtoul(text, &end, 10);
    return errno == 0 && *end == '\0';
}

int main(int argc, char **argv)
{
    unsigned long descriptor;
    if (!number("LXP_TEST_CRASH_BOUNDARY", &boundary) ||
        !number("LXP_TEST_CRASH_OCCURRENCE", &occurrence) || occurrence > UINT32_MAX ||
        !number("LXP_TEST_APPLY_GATE_FD", &descriptor) || descriptor > INT_MAX)
        return 2;
    gate = (int)descriptor;
    lxp_result status = lxp_daemon_main(argc, argv);
    fprintf(stderr, "crash workload returned without reaching its boundary: %d\n", (int)status);
    return 1;
}
