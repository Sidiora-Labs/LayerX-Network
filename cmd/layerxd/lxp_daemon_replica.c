#define _POSIX_C_SOURCE 200809L

#include "layerx/lxp_daemon.h"
#include "layerx/lxp_da.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_replica.h"

#include "../layerx-guarantor/runtime.h"

#include <errno.h>
#include <signal.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define LXP_REPLICA_ARENA_BYTES (128U * 1024U * 1024U)
#define LXP_REPLICA_LOG_BYTES (UINT64_C(64) * 1024U * 1024U)
#define LXP_REPLICA_DEFAULT_POLL_MS 250U

static volatile sig_atomic_t replica_stop_requested;

static void replica_signal(int signal_number)
{
    (void)signal_number;
    replica_stop_requested = 1;
}

static const char *replica_environment(const char *name)
{
    const char *value = getenv(name);
    return value != NULL && value[0] != '\0' ? value : NULL;
}

static lxp_result replica_parse_u64(const char *text, uint64_t *value)
{
    uint64_t parsed = 0U;
    size_t index = 0U;
    if (text == NULL || value == NULL || text[0] == '\0')
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; text[index] != '\0'; ++index) {
        uint64_t digit;
        if (text[index] < '0' || text[index] > '9')
            return LXP_ERR_NON_CANONICAL;
        digit = (uint64_t)(text[index] - '0');
        if (parsed > (UINT64_MAX - digit) / 10U)
            return LXP_ERR_LENGTH_LIMIT;
        parsed = parsed * 10U + digit;
    }
    *value = parsed;
    return LXP_OK;
}

static lxp_result replica_decode_hex(const char *text, size_t text_length,
                                     uint8_t *bytes, size_t length)
{
    size_t index;
    if (text == NULL || bytes == NULL || text_length != length * 2U)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < length; ++index) {
        uint8_t value = 0U;
        size_t nibble;
        for (nibble = 0U; nibble < 2U; ++nibble) {
            char digit = text[index * 2U + nibble];
            uint8_t decoded;
            if (digit >= '0' && digit <= '9')
                decoded = (uint8_t)(digit - '0');
            else if (digit >= 'a' && digit <= 'f')
                decoded = (uint8_t)(digit - 'a' + 10);
            else if (digit >= 'A' && digit <= 'F')
                decoded = (uint8_t)(digit - 'A' + 10);
            else
                return LXP_ERR_NON_CANONICAL;
            value = (uint8_t)((value << 4U) | decoded);
        }
        bytes[index] = value;
    }
    return LXP_OK;
}

static lxp_result replica_authorization(
    lxp_sequencer_authorization *authorization)
{
    const char *sequencer_id = replica_environment("LAYERX_NODE_SEQUENCER_ID");
    const char *public_key =
        replica_environment("LAYERX_NODE_SEQUENCER_PUBLIC_KEY");
    const char *first_batch = replica_environment("LAYERX_NODE_FIRST_BATCH");
    const char *last_batch = replica_environment("LAYERX_NODE_LAST_BATCH");
    lxp_result status;
    if (authorization == NULL || sequencer_id == NULL || public_key == NULL ||
        first_batch == NULL || last_batch == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(authorization, 0, sizeof(*authorization));
    status = replica_decode_hex(sequencer_id, strlen(sequencer_id),
                                authorization->sequencer_id, 32U);
    if (status == LXP_OK)
        status = replica_decode_hex(public_key, strlen(public_key),
                                    authorization->public_key, 32U);
    if (status == LXP_OK)
        status = replica_parse_u64(first_batch,
                                   &authorization->first_batch_number);
    if (status == LXP_OK)
        status = replica_parse_u64(last_batch,
                                   &authorization->last_batch_number);
    if (status == LXP_OK &&
        authorization->last_batch_number < authorization->first_batch_number)
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) authorization->authorized = 1U;
    return status;
}

static lxp_result replica_roster(const uint8_t replica_id[32],
                                 uint8_t (*replica_ids)[32],
                                 size_t *replica_count, size_t *threshold)
{
    const char *set = replica_environment("LAYERX_REPLICA_SET");
    const char *configured = replica_environment("LAYERX_REPLICA_ACK_THRESHOLD");
    size_t count = 0U;
    uint64_t value = 1U;
    lxp_result status = LXP_OK;
    if (replica_id == NULL || replica_ids == NULL || replica_count == NULL ||
        threshold == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (set == NULL) {
        (void)memcpy(replica_ids[0], replica_id, 32U);
        count = 1U;
    } else {
        const char *cursor = set;
        while (status == LXP_OK && *cursor != '\0') {
            const char *comma = strchr(cursor, ',');
            size_t length = comma == NULL ? strlen(cursor)
                                          : (size_t)(comma - cursor);
            if (count == (size_t)LXP_MAX_BATCH_REPLICAS)
                return LXP_ERR_LENGTH_LIMIT;
            status = replica_decode_hex(cursor, length, replica_ids[count],
                                        32U);
            if (status != LXP_OK) return status;
            count += 1U;
            cursor = comma == NULL ? cursor + length : comma + 1U;
        }
        if (count == 0U) return LXP_ERR_NON_CANONICAL;
    }
    if (configured != NULL) {
        status = replica_parse_u64(configured, &value);
        if (status != LXP_OK) return status;
    }
    if (value == 0U || value > (uint64_t)count) return LXP_ERR_NON_CANONICAL;
    *replica_count = count;
    *threshold = (size_t)value;
    return LXP_OK;
}

static void replica_wait(uint64_t poll_ms)
{
    struct timespec interval;
    interval.tv_sec = (time_t)(poll_ms / 1000U);
    interval.tv_nsec = (long)((poll_ms % 1000U) * 1000000U);
    (void)nanosleep(&interval, NULL);
}

static lxp_result replica_await_body(lxp_log *log, const char *path,
                                     bool *opened, uint64_t batch_number,
                                     uint64_t poll_ms, lxp_arena *arena,
                                     lxp_batch_body *body)
{
    lxp_result status;
    for (;;) {
        (void)lxp_arena_reset(arena, 0U);
        status = lxp_da_log_read_body(log, batch_number, arena, body);
        if (status != LXP_ERR_DA_MISSING) return status;
        if (replica_stop_requested != 0) return LXP_ERR_DA_MISSING;
        replica_wait(poll_ms);
        status = lxp_log_close(log);
        *opened = false;
        if (status != LXP_OK) return status;
        status = lxp_log_open(log, path);
        if (status != LXP_OK) return status;
        *opened = true;
    }
}

lxp_result lxp_daemon_replica_serve(const char *configuration_path)
{
    lxp_daemon_configuration configuration;
    lxp_sequencer_authorization authorization;
    lxp_replica replica;
    lxp_log availability_log;
    lxp_log replica_log;
    lxp_arena arena;
    struct sigaction action;
    gp_runtime *runtime = NULL;
    lxp_replay_engine *engine = NULL;
    uint8_t (*replica_ids)[32] = NULL;
    uint8_t *memory = NULL;
    uint8_t replica_id[32];
    const char *availability_path;
    const char *replica_log_path;
    const char *state_directory;
    const char *identifier;
    const char *text;
    uint64_t first_batch = 1U;
    uint64_t batch_limit = 0U;
    uint64_t poll_ms = LXP_REPLICA_DEFAULT_POLL_MS;
    uint64_t batch;
    size_t replica_count = 0U;
    size_t threshold = 0U;
    bool availability_open = false;
    bool replica_open = false;
    lxp_result status;
    if (configuration_path == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(&replica, 0, sizeof(replica));
    status = lxp_daemon_config_load(configuration_path, &configuration);
    if (status == LXP_OK && configuration.role != LXP_DAEMON_REPLICA)
        status = LXP_ERR_NON_CANONICAL;
    if (status != LXP_OK) return status;
    state_directory = replica_environment("LAYERX_REPLICA_STATE_DIRECTORY");
    availability_path = replica_environment("LAYERX_REPLICA_AVAILABILITY_LOG");
    replica_log_path = replica_environment("LAYERX_REPLICA_LOG");
    identifier = replica_environment("LAYERX_REPLICA_ID");
    if (state_directory == NULL || availability_path == NULL ||
        replica_log_path == NULL || identifier == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = replica_decode_hex(identifier, strlen(identifier), replica_id,
                                32U);
    text = replica_environment("LAYERX_REPLICA_FIRST_BATCH");
    if (status == LXP_OK && text != NULL)
        status = replica_parse_u64(text, &first_batch);
    text = replica_environment("LAYERX_REPLICA_BATCH_LIMIT");
    if (status == LXP_OK && text != NULL)
        status = replica_parse_u64(text, &batch_limit);
    text = replica_environment("LAYERX_REPLICA_POLL_MS");
    if (status == LXP_OK && text != NULL)
        status = replica_parse_u64(text, &poll_ms);
    if (status == LXP_OK && (poll_ms == 0U || poll_ms > 60000U))
        status = LXP_ERR_NON_CANONICAL;
    if (status == LXP_OK) status = replica_authorization(&authorization);
    if (status == LXP_OK) {
        replica_ids = malloc(sizeof(*replica_ids) * LXP_MAX_BATCH_REPLICAS);
        memory = malloc(LXP_REPLICA_ARENA_BYTES);
        if (replica_ids == NULL || memory == NULL) status = LXP_ERR_IO;
    }
    if (status == LXP_OK) {
        (void)memset(replica_ids, 0,
                     sizeof(*replica_ids) * LXP_MAX_BATCH_REPLICAS);
        status = replica_roster(replica_id, replica_ids, &replica_count,
                                &threshold);
    }
    if (status == LXP_OK)
        status = lxp_arena_init(&arena, memory, LXP_REPLICA_ARENA_BYTES);
    if (status == LXP_OK)
        status = gp_runtime_open(&runtime, configuration_path,
                                 state_directory);
    if (status == LXP_OK) {
        engine = gp_runtime_engine(runtime);
        if (engine == NULL || engine->kernel == NULL ||
            engine->kernel->state == NULL)
            status = LXP_ERR_MODULE_DISABLED;
    }
    if (status == LXP_OK) {
        status = lxp_log_open_or_create(&replica_log, replica_log_path,
                                        LXP_REPLICA_LOG_BYTES);
        replica_open = status == LXP_OK;
    }
    if (status == LXP_OK)
        status = lxp_log_recover_complete_records(&replica_log, NULL, NULL);
    if (status == LXP_OK) {
        status = lxp_log_open(&availability_log, availability_path);
        availability_open = status == LXP_OK;
    }
    if (status == LXP_OK) status = lxp_replica_init(&replica, &replica_log);
    if (status == LXP_OK)
        status = lxp_replica_bind_execution(
            &replica, engine, engine->kernel->current_state_root, first_batch,
            engine->kernel->state->next_sequence);
    if (status == LXP_OK)
        status = lxp_replica_bind_eligibility(
            &replica, replica_id, (const uint8_t (*)[32])replica_ids,
            replica_count, threshold);
    if (status == LXP_OK) {
        (void)memset(&action, 0, sizeof(action));
        action.sa_handler = replica_signal;
        if (sigemptyset(&action.sa_mask) != 0 ||
            sigaction(SIGINT, &action, NULL) != 0 ||
            sigaction(SIGTERM, &action, NULL) != 0)
            status = LXP_ERR_IO;
    }
    for (batch = first_batch; status == LXP_OK; ++batch) {
        lxp_batch_body body;
        lxp_byte_span canonical;
        bool acknowledged = false;
        bool eligible = false;
        if (replica_stop_requested != 0) break;
        if (batch_limit != 0U && batch - first_batch >= batch_limit) break;
        status = replica_await_body(&availability_log, availability_path,
                                    &availability_open, batch, poll_ms,
                                    &arena, &body);
        if (status == LXP_ERR_DA_MISSING && replica_stop_requested != 0) {
            status = LXP_OK;
            break;
        }
        if (status == LXP_OK) status = gp_runtime_prepare(runtime, &body);
        if (status == LXP_OK)
            status = lxp_batch_body_encode(&body, &arena, &canonical);
        if (status == LXP_OK)
            status = lxp_replica_ingest_batch(
                &replica, canonical.bytes, canonical.length,
                configuration.network_id, &authorization, &arena,
                &acknowledged);
        if (status != LXP_OK) break;
        if (!acknowledged) {
            status = LXP_FATAL_REPLAY_DIVERGENCE;
            break;
        }
        (void)lxp_replica_batch_eligible(&replica, &eligible);
        (void)fprintf(stdout,
            "layerxd replica executed batch=%llu first_sequence=%llu "
            "last_sequence=%llu acknowledged=%llu eligible=%d\n",
            (unsigned long long)replica.head.batch_number,
            (unsigned long long)replica.head.first_sequence,
            (unsigned long long)replica.head.last_sequence,
            (unsigned long long)replica.acknowledged_batch_count,
            eligible ? 1 : 0);
        (void)fflush(stdout);
    }
    if (status != LXP_OK)
        (void)fprintf(stderr,
            "layerxd replica refused batch=%llu status=%d halted=%d\n",
            (unsigned long long)batch, (int)status,
            replica.halted ? 1 : 0);
    if (availability_open) {
        lxp_result closed = lxp_log_close(&availability_log);
        if (status == LXP_OK) status = closed;
    }
    if (replica_open) {
        lxp_result closed = lxp_log_close(&replica_log);
        if (status == LXP_OK) status = closed;
    }
    gp_runtime_close(runtime);
    free(memory);
    free(replica_ids);
    return status;
}
