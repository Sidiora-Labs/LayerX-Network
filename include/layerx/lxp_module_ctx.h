#ifndef LAYERX_LXP_MODULE_CTX_H
#define LAYERX_LXP_MODULE_CTX_H
#include "layerx/lxp_kernel.h"
const lxp_module_iface *lxp_governance_module_iface(void);
bool lxp_governance_activity(uint32_t activity_type);
lxp_result lxp_governance_identity_refresh(const lxp_kernel *kernel,
                                           lxp_identity *identity);
lxp_result lxp_governance_identities_restore(const lxp_kernel *kernel,
                                             lxp_identity_store *identities);
lxp_result lxp_governance_onboard(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority);
lxp_result lxp_governance_onboarding_prepared(const lxp_module_ctx *ctx);
#endif
