#include "layerx/lxp_daemon.h"

#include <string.h>

static void *executor_run(void *argument)
{
    lxp_daemon *daemon = (lxp_daemon *)argument;
    for (;;) {
        lxp_daemon_activity activities[LXP_DAEMON_MAX_BATCH_ACTIVITIES];
        size_t activity_count;
        size_t consumed_count = 0U;
        size_t i;
        uint64_t sequence;
        lxp_result status;
        (void)pthread_mutex_lock(&daemon->mutex);
        while (daemon->queue_count == 0U && !daemon->stop_requested)
            (void)pthread_cond_wait(
                &daemon->queue_changed, &daemon->mutex);
        if (daemon->queue_count == 0U && daemon->stop_requested) {
            (void)pthread_mutex_unlock(&daemon->mutex);
            break;
        }
        activity_count = daemon->apply_batch == NULL ? 1U :
            (daemon->queue_count < LXP_DAEMON_MAX_BATCH_ACTIVITIES ?
                 daemon->queue_count : LXP_DAEMON_MAX_BATCH_ACTIVITIES);
        if (UINT64_MAX - daemon->next_sequence == 0U) {
            daemon->failure = LXP_ERR_SEQUENCE_GAP;
            daemon->accepting = false;
            daemon->stop_requested = true;
            daemon->queue_count = 0U;
            (void)pthread_cond_broadcast(&daemon->queue_changed);
            (void)pthread_mutex_unlock(&daemon->mutex);
            break;
        }
        if (UINT64_MAX - daemon->next_sequence < activity_count)
            activity_count = (size_t)(UINT64_MAX - daemon->next_sequence);
        for (i = 0U; i < activity_count; ++i) {
            size_t at = (daemon->queue_head + i) %
                LXP_DAEMON_QUEUE_CAPACITY;
            activities[i] = daemon->queue[at];
        }
        sequence = daemon->next_sequence;
        (void)pthread_mutex_unlock(&daemon->mutex);
        status = daemon->apply_batch != NULL ?
            daemon->apply_batch(daemon->apply_context, sequence,
                                activities, activity_count,
                                &consumed_count) :
            daemon->apply(daemon->apply_context, sequence,
                          activities[0].bytes, activities[0].length);
        if (daemon->apply_batch == NULL) consumed_count = 1U;
        (void)pthread_mutex_lock(&daemon->mutex);
        if (status == LXP_OK &&
            (consumed_count == 0U || consumed_count > activity_count))
            status = LXP_FATAL_INVARIANT;
        if (status != LXP_OK) {
            daemon->failure = status;
            daemon->accepting = false;
            daemon->stop_requested = true;
            daemon->queue_count = 0U;
        } else {
            daemon->queue_head = (daemon->queue_head + consumed_count) %
                LXP_DAEMON_QUEUE_CAPACITY;
            daemon->queue_count -= consumed_count;
            daemon->next_sequence += consumed_count;
            if (!daemon->accepting) {
                daemon->stop_requested = true;
                daemon->queue_count = 0U;
            }
        }
        (void)pthread_cond_broadcast(&daemon->queue_changed);
        (void)pthread_mutex_unlock(&daemon->mutex);
    }
    return NULL;
}

static void *worker_run(void *argument)
{
    lxp_daemon *daemon = (lxp_daemon *)argument;
    (void)pthread_mutex_lock(&daemon->mutex);
    while (!daemon->stop_requested)
        (void)pthread_cond_wait(&daemon->queue_changed, &daemon->mutex);
    (void)pthread_mutex_unlock(&daemon->mutex);
    return NULL;
}

static lxp_result daemon_start(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_fn apply, lxp_daemon_apply_batch_fn apply_batch,
    void *apply_context)
{
    size_t requested_workers;
    size_t i;
    if (daemon == NULL || config == NULL ||
        (apply == NULL) == (apply_batch == NULL) ||
        config->role < LXP_DAEMON_SEQUENCER ||
        config->role > LXP_DAEMON_GUARANTOR || config->network_id == 0U)
        return LXP_ERR_NON_CANONICAL;
    requested_workers = config->verify_workers + config->network_workers +
        config->projection_workers + config->checkpoint_workers;
    if (requested_workers > LXP_DAEMON_MAX_WORKERS * 4U ||
        (config->serial_execution && requested_workers != 0U))
        return LXP_ERR_LENGTH_LIMIT;
    (void)memset(daemon, 0, sizeof(*daemon));
    daemon->config = *config;
    daemon->apply = apply;
    daemon->apply_batch = apply_batch;
    daemon->apply_context = apply_context;
    daemon->next_sequence = config->start_sequence;
    daemon->failure = LXP_OK;
    if (pthread_mutex_init(&daemon->mutex, NULL) != 0 ||
        pthread_cond_init(&daemon->queue_changed, NULL) != 0)
        return LXP_ERR_IO;
    daemon->primitives_initialized = true;
    daemon->accepting = true;
    if (pthread_create(
            &daemon->executor_thread, NULL, executor_run, daemon) != 0) {
        (void)lxp_daemon_shutdown(daemon);
        return LXP_ERR_IO;
    }
    daemon->executor_started = true;
    for (i = 0U; i < requested_workers; ++i) {
        if (pthread_create(
                &daemon->workers[i], NULL, worker_run, daemon) != 0) {
            (void)lxp_daemon_shutdown(daemon);
            return LXP_ERR_IO;
        }
        ++daemon->worker_count;
    }
    return LXP_OK;
}

lxp_result lxp_daemon_start(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_fn apply, void *apply_context)
{
    return daemon_start(daemon, config, apply, NULL, apply_context);
}

lxp_result lxp_daemon_start_batch(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_batch_fn apply_batch, void *apply_context)
{
    return daemon_start(daemon, config, NULL, apply_batch, apply_context);
}

lxp_result lxp_daemon_start_protocol(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_fn apply, void *apply_context,
    lxp_daemon_protocol_owner *protocol_owner,
    const char *loopback_address, uint16_t port)
{
    lxp_result status;
    if (protocol_owner == NULL || !protocol_owner->attached ||
        loopback_address == NULL || port == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_start(daemon, config, apply, apply_context);
    if (status == LXP_OK)
        status = lxp_daemon_protocol_listener_start(
            protocol_owner, loopback_address, port);
    if (status != LXP_OK) {
        if (daemon != NULL && daemon->primitives_initialized)
            (void)lxp_daemon_shutdown(daemon);
        return status;
    }
    daemon->protocol_owner = protocol_owner;
    return LXP_OK;
}

lxp_result lxp_daemon_start_protocol_batch(
    lxp_daemon *daemon, const lxp_daemon_configuration *config,
    lxp_daemon_apply_batch_fn apply_batch, void *apply_context,
    lxp_daemon_protocol_owner *protocol_owner,
    const char *loopback_address, uint16_t port)
{
    lxp_result status;
    if (apply_batch == NULL || protocol_owner == NULL ||
        !protocol_owner->attached || loopback_address == NULL || port == 0U)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_daemon_start_batch(
        daemon, config, apply_batch, apply_context);
    if (status == LXP_OK)
        status = lxp_daemon_protocol_listener_start(
            protocol_owner, loopback_address, port);
    if (status != LXP_OK) {
        if (daemon != NULL && daemon->primitives_initialized)
            (void)lxp_daemon_shutdown(daemon);
        return status;
    }
    daemon->protocol_owner = protocol_owner;
    return status;
}

lxp_result lxp_daemon_submit(
    lxp_daemon *daemon, const uint8_t *activity, size_t activity_length)
{
    size_t tail;
    if (daemon == NULL || activity == NULL || activity_length == 0U ||
        activity_length > LXP_DAEMON_ACTIVITY_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)pthread_mutex_lock(&daemon->mutex);
    while (daemon->queue_count == LXP_DAEMON_QUEUE_CAPACITY &&
           daemon->accepting)
        (void)pthread_cond_wait(&daemon->queue_changed, &daemon->mutex);
    if (!daemon->accepting) {
        lxp_result failure = daemon->failure == LXP_OK ?
            LXP_ERR_MODULE_DISABLED : daemon->failure;
        (void)pthread_mutex_unlock(&daemon->mutex);
        return failure;
    }
    tail = (daemon->queue_head + daemon->queue_count) %
        LXP_DAEMON_QUEUE_CAPACITY;
    (void)memcpy(daemon->queue[tail].bytes, activity, activity_length);
    daemon->queue[tail].length = activity_length;
    ++daemon->queue_count;
    (void)pthread_cond_signal(&daemon->queue_changed);
    (void)pthread_mutex_unlock(&daemon->mutex);
    return LXP_OK;
}
