#ifndef LAYERX_LXP_BRIDGE_CREDIT_H
#define LAYERX_LXP_BRIDGE_CREDIT_H

#include "layerx/lxp_genesis.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_u128.h"

enum {
    LXP_BRIDGE_CREDIT = (8U << 16U) | 1U,
    LXP_BRIDGE_PROFILE_BYTES = 207,
    LXP_BRIDGE_CREDIT_BYTES = 427,
    LXP_BRIDGE_CREDIT_SIGNED_BYTES = 363,
    LXP_BRIDGE_COMET_PROOF_KIND = 1,
    LXP_BRIDGE_COMET_MAX_HISTORY = 8192
};

typedef struct lxp_bridge_profile {
    uint8_t bytes[LXP_BRIDGE_PROFILE_BYTES];
} lxp_bridge_profile;

typedef struct lxp_bridge_credit {
    uint8_t bytes[LXP_BRIDGE_CREDIT_BYTES];
} lxp_bridge_credit;

extern const uint8_t lxp_bridge_profile_key[32];
lxp_result lxp_bridge_profile_validate(const lxp_bridge_profile *profile);
lxp_result lxp_bridge_genesis_profile(const lxp_genesis_manifest *manifest,
                                     lxp_bridge_profile *profile, bool *present);
lxp_result lxp_bridge_genesis_append(lxp_genesis_manifest *manifest,
                                    const lxp_bridge_profile *profile);
lxp_result lxp_bridge_credit_verify(const lxp_bridge_profile *profile,
                                    const lxp_bridge_credit *credit,
                                    uint32_t network_id,
                                    uint16_t protocol_version,
                                    uint8_t nullifier[32]);
const lxp_module_iface *lxp_bridge_module_iface(void);
lxp_result lxp_bridge_credit_bind_receipt(lxp_receipt *receipt,
                                         const lxp_module_ctx *ctx);
lxp_result lxp_ctx_bridge_credit(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const lxp_bridge_credit *credit);

#endif
