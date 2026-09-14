#ifndef LAYERX_LXP_MODULE_CTX_H
#define LAYERX_LXP_MODULE_CTX_H
#include "layerx/lxp_kernel.h"
const lxp_module_iface *lxp_governance_module_iface(void);
const lxp_module_iface *lxp_governance_module_iface_for_handover(bool enabled);
bool lxp_governance_activity(uint32_t activity_type);
lxp_result lxp_governance_identity_refresh(const lxp_kernel *kernel,
                                           lxp_identity *identity);
lxp_result lxp_governance_identities_restore(const lxp_kernel *kernel,
                                             lxp_identity_store *identities);
lxp_result lxp_governance_onboard(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority);
lxp_result lxp_governance_onboarding_prepared(const lxp_module_ctx *ctx);
lxp_result lxp_governance_rotation(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const uint8_t previous[223], uint8_t next[223]);
lxp_result lxp_governance_rotation_prepared(const lxp_module_ctx *ctx);
lxp_result lxp_governance_rotation_accounts(const lxp_module_ctx *ctx,
    lx_account_registry *accounts, bool apply);
lxp_result lxp_governance_rotation_refresh(const lxp_kernel *kernel,
    lxp_identity *identity);
#endif
