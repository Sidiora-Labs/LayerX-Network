#ifndef LAYERX_LXP_STATE_INTERNAL_H
#define LAYERX_LXP_STATE_INTERNAL_H

#include "layerx/lxp_result.h"

#include <stddef.h>

struct lxp_kernel;

lxp_result lxp_state_module_root_count(const struct lxp_kernel *kernel,
                                       size_t *count);

#endif
