#ifndef LAYERX_LXP_MODULE_CTX_H
#define LAYERX_LXP_MODULE_CTX_H
#include "layerx/lxp_kernel.h"
const lxp_module_iface *lxp_governance_module_iface(void);
bool lxp_governance_activity(uint32_t activity_type);
lxp_result lxp_governance_identity_refresh(const lxp_kernel *kernel,
                                           lxp_identity *identity);
#endif
