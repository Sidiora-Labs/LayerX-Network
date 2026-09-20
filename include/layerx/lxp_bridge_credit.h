#ifndef LAYERX_LXP_BRIDGE_CREDIT_H
#define LAYERX_LXP_BRIDGE_CREDIT_H

#include "layerx/lxp_bridge_light.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lxp_module.h"
#include "layerx/lxp_u128.h"

enum {
    LXP_BRIDGE_CREDIT = (8U << 16U) | 1U,
    LXP_BRIDGE_PROFILE_BYTES = 207,
    LXP_BRIDGE_CREDIT_BYTES = 363,
    LXP_BRIDGE_CREDIT_MIN_PAYLOAD_BYTES = LXP_BRIDGE_CREDIT_BYTES + 5,
    LXP_BRIDGE_LIGHT_PROOF_KIND = 2
};

typedef struct lxp_bridge_profile {
    uint8_t bytes[LXP_BRIDGE_PROFILE_BYTES];
} lxp_bridge_profile;

typedef struct lxp_bridge_credit {
    uint8_t bytes[LXP_BRIDGE_CREDIT_BYTES];
    const uint8_t *proof;
    size_t proof_length;
} lxp_bridge_credit;

extern const uint8_t lxp_bridge_profile_key[32];
extern const uint8_t lxp_bridge_light_trust_key[32];
lxp_result lxp_bridge_credit_parse(const uint8_t *payload, size_t length,
                                   lxp_bridge_credit *credit);
bool lxp_bridge_credit_matches(const lxp_bridge_credit *credit,
                               const uint8_t *payload, size_t length);
bool lxp_bridge_credit_owner_bound(const uint8_t *did, size_t did_length,
                                   const uint8_t owner_key[32]);
void lxp_bridge_profile_trust(const lxp_bridge_profile *profile,
                              lxp_bridge_light_trust *trust);
lxp_result lxp_bridge_light_trust_load(lxp_module_ctx *ctx,
                                       const lxp_bridge_profile *profile,
                                       lxp_bridge_light_trust *trust);
lxp_result lxp_bridge_profile_validate(const lxp_bridge_profile *profile);
lxp_result lxp_bridge_genesis_profile(const lxp_genesis_manifest *manifest,
                                     lxp_bridge_profile *profile, bool *present);
lxp_result lxp_bridge_genesis_append(lxp_genesis_manifest *manifest,
                                    const lxp_bridge_profile *profile);
lxp_result lxp_bridge_credit_verify(const lxp_bridge_profile *profile,
                                    const lxp_bridge_credit *credit,
                                    uint32_t network_id,
                                    uint16_t protocol_version,
                                    const lxp_bridge_light_trust *trusted,
                                    uint8_t nullifier[32],
                                    lxp_bridge_light_trust *advanced);
const lxp_module_iface *lxp_bridge_module_iface(void);
lxp_result lxp_bridge_credit_bind_receipt(lxp_receipt *receipt,
                                         const lxp_module_ctx *ctx);
lxp_result lxp_ctx_bridge_credit(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const lxp_bridge_credit *credit);

#endif
