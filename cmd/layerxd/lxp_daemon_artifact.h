#ifndef LXP_DAEMON_ARTIFACT_H
#define LXP_DAEMON_ARTIFACT_H

#include "layerx/lxp_result.h"

#include <stddef.h>
#include <stdint.h>

lxp_result lxp_daemon_artifact_read(
    const char *path, size_t maximum_length, size_t exact_length,
    uint8_t **bytes, size_t *length);

#endif
