#ifndef LAYERX_DAEMON_DEPLOYMENT_H
#define LAYERX_DAEMON_DEPLOYMENT_H
#include "layerx/lxp_daemon.h"

lxp_result lxp_daemon_deployment_encode(
    const lxp_kernel *kernel, const lxp_daemon_activity_evidence *evidence,
    uint32_t network_id, lxp_arena *arena, lxp_byte_span *encoded);
#endif
