#include "layerx/lxp_daemon.h"

lxp_result lxp_daemon_shutdown(lxp_daemon *daemon)
{
    size_t i;
    lxp_result status;
    if (daemon == NULL || !daemon->primitives_initialized)
        return LXP_ERR_NON_CANONICAL;
    (void)pthread_mutex_lock(&daemon->mutex);
    daemon->accepting = false;
    daemon->stop_requested = true;
    (void)pthread_cond_broadcast(&daemon->queue_changed);
    (void)pthread_mutex_unlock(&daemon->mutex);
    if (daemon->executor_started)
        (void)pthread_join(daemon->executor_thread, NULL);
    for (i = 0U; i < daemon->worker_count; ++i)
        (void)pthread_join(daemon->workers[i], NULL);
    status = daemon->failure;
    if (daemon->protocol_owner != NULL) {
        lxp_result owner_status = lxp_daemon_protocol_listener_stop(
            daemon->protocol_owner);
        if (owner_status == LXP_OK)
            owner_status = lxp_daemon_protocol_owner_detach(
                daemon->protocol_owner);
        if (status == LXP_OK) status = owner_status;
        daemon->protocol_owner = NULL;
    }
    (void)pthread_cond_destroy(&daemon->queue_changed);
    (void)pthread_mutex_destroy(&daemon->mutex);
    daemon->primitives_initialized = false;
    return status;
}
