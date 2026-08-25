#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"
#include "layerx/lxp_hash.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

typedef struct apply_state {
    uint64_t expected_sequence;
    uint8_t root[32];
} apply_state;

static lxp_result apply_activity(
    void *context, uint64_t global_sequence,
    const uint8_t *activity, size_t activity_length)
{
    apply_state *state = (apply_state *)context;
    uint8_t preimage[32U + 8U + LXP_DAEMON_ACTIVITY_BYTES];
    size_t i;
    if (state == NULL || activity == NULL ||
        global_sequence != state->expected_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    (void)memcpy(preimage, state->root, 32U);
    for (i = 0U; i < 8U; ++i)
        preimage[39U - i] = (uint8_t)(global_sequence >> (i * 8U));
    (void)memcpy(preimage + 40U, activity, activity_length);
    if (lxp_hash_domain(
            LXP_DOMAIN_STATE_LEAF, preimage,
            40U + activity_length, state->root) != LXP_OK)
        return LXP_ERR_BAD_SIGNATURE;
    ++state->expected_sequence;
    return LXP_OK;
}

static int write_config(
    char path[64], const char *role, uint64_t start_sequence,
    size_t workers, bool serial_execution)
{
    int descriptor = mkstemp(path);
    FILE *file;
    int result;
    if (descriptor < 0) return 1;
    file = fdopen(descriptor, "wb");
    if (file == NULL) {
        (void)close(descriptor);
        return 1;
    }
    result = fprintf(
        file,
        "role=%s\n"
        "network_id=42\n"
        "start_sequence=%llu\n"
        "verify_workers=%zu\n"
        "network_workers=%zu\n"
        "projection_workers=%zu\n"
        "checkpoint_workers=%zu\n"
        "serial_execution=%s\n",
        role, (unsigned long long)start_sequence,
        workers, workers, workers, workers,
        serial_execution ? "true" : "false");
    return result < 0 || fclose(file) != 0;
}

static int write_negative_sequence_config(char path[64])
{
    int descriptor = mkstemp(path);
    FILE *file;
    int result;
    if (descriptor < 0) return 1;
    file = fdopen(descriptor, "wb");
    if (file == NULL) {
        (void)close(descriptor);
        return 1;
    }
    result = fprintf(
        file,
        "role=sequencer\n"
        "network_id=42\n"
        "start_sequence=-1\n"
        "verify_workers=0\n"
        "network_workers=0\n"
        "projection_workers=0\n"
        "checkpoint_workers=0\n"
        "serial_execution=true\n");
    return result < 0 || fclose(file) != 0;
}

static int submit_range(
    lxp_daemon *daemon, uint64_t first, uint64_t count)
{
    uint64_t i;
    for (i = 0U; i < count; ++i) {
        uint8_t activity[24];
        size_t j;
        for (j = 0U; j < sizeof(activity); ++j)
            activity[j] = (uint8_t)((first + i + j) & UINT64_C(0xff));
        if (lxp_daemon_submit(
                daemon, activity, sizeof(activity)) != LXP_OK)
            return 1;
    }
    return 0;
}

static int run_window(
    const lxp_daemon_configuration *config,
    uint64_t count, uint8_t root[32])
{
    static lxp_daemon daemon;
    apply_state state;
    (void)memset(&state, 0, sizeof(state));
    state.expected_sequence = config->start_sequence;
    if (lxp_daemon_start(
            &daemon, config, apply_activity, &state) != LXP_OK ||
        submit_range(&daemon, config->start_sequence, count) != 0 ||
        lxp_daemon_shutdown(&daemon) != LXP_OK ||
        state.expected_sequence != config->start_sequence + count ||
        daemon.next_sequence != state.expected_sequence)
        return 1;
    (void)memcpy(root, state.root, 32U);
    return 0;
}

int main(void)
{
    char parallel_path[64] = "/tmp/layerxd-parallel-XXXXXX";
    char serial_path[64] = "/tmp/layerxd-serial-XXXXXX";
    char invalid_path[64] = "/tmp/layerxd-invalid-XXXXXX";
    char negative_path[64] = "/tmp/layerxd-negative-XXXXXX";
    lxp_daemon_configuration parallel;
    lxp_daemon_configuration serial;
    lxp_daemon_configuration invalid;
    lxp_daemon_role_kind role;
    static lxp_daemon daemon;
    apply_state durable;
    uint8_t parallel_root[32];
    uint8_t serial_root[32];
    uint64_t restart_sequence;

    if (write_config(
            parallel_path, "sequencer", 0U, 2U, false) != 0 ||
        write_config(serial_path, "sequencer", 0U, 0U, true) != 0 ||
        write_config(
            invalid_path, "sequencer,guarantor", 0U, 0U, true) != 0 ||
        write_negative_sequence_config(negative_path) != 0 ||
        lxp_daemon_config_load(parallel_path, &parallel) != LXP_OK ||
        lxp_daemon_config_load(serial_path, &serial) != LXP_OK ||
        lxp_daemon_role(&parallel, &role) != LXP_OK ||
        role != LXP_DAEMON_SEQUENCER ||
        parallel.role != LXP_DAEMON_SEQUENCER ||
        parallel.serial_execution || !serial.serial_execution ||
        lxp_daemon_config_load(invalid_path, &invalid) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_daemon_config_load(negative_path, &invalid) !=
            LXP_ERR_NON_CANONICAL ||
        run_window(&parallel, 5000U, parallel_root) != 0 ||
        run_window(&serial, 5000U, serial_root) != 0 ||
        memcmp(parallel_root, serial_root, 32U) != 0)
        return 1;

    (void)memset(&durable, 0, sizeof(durable));
    if (lxp_daemon_start(
            &daemon, &parallel, apply_activity, &durable) != LXP_OK ||
        submit_range(&daemon, 0U, 6000U) != 0 ||
        lxp_daemon_shutdown(&daemon) != LXP_OK ||
        durable.expected_sequence != 6000U)
        return 1;
    restart_sequence = daemon.next_sequence;
    parallel.start_sequence = restart_sequence;
    if (restart_sequence != 6000U ||
        lxp_daemon_start(
            &daemon, &parallel, apply_activity, &durable) != LXP_OK ||
        submit_range(&daemon, restart_sequence, 4000U) != 0 ||
        lxp_daemon_shutdown(&daemon) != LXP_OK ||
        durable.expected_sequence != 10000U ||
        daemon.next_sequence != 10000U ||
        unlink(parallel_path) != 0 || unlink(serial_path) != 0 ||
        unlink(invalid_path) != 0 || unlink(negative_path) != 0)
        return 1;
    return 0;
}
