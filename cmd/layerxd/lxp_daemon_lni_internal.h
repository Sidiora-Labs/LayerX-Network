#ifndef LAYERX_LXP_DAEMON_LNI_INTERNAL_H
#define LAYERX_LXP_DAEMON_LNI_INTERNAL_H

#include "layerx/lxp_daemon.h"

lxp_result lxp_daemon_lni_serve_connected(
    lxp_daemon_lni_server *server, int descriptor);
lxp_result lxp_daemon_lni_simulate(
    lxp_daemon_protocol_owner *owner,
    const uint8_t sequencer_private_key[32],
    const uint8_t *request, size_t request_length,
    uint8_t *response, size_t response_capacity, size_t *response_length,
    uint8_t *evidence, size_t evidence_capacity, size_t *evidence_length);

#endif
