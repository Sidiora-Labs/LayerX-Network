#include "layerx/lx_web.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

lxp_result lx_web_attestor_set_validate(const lx_web_attestor_set *set)
{
    size_t i;
    if (set == NULL || set->count == 0U || set->count > LX_WEB_MAX_ATTESTORS)
        return LXP_ERR_NON_CANONICAL;
    if (set->threshold > set->count ||
        (uint64_t)set->threshold * 2U <= (uint64_t)set->count)
        return LXP_ERR_PARAMETER_BOUNDS;
    for (i = 0U; i < set->count; ++i) {
        if (lxp_ct_is_zero(set->attestors[i].signer, LX_WEB_SIGNER_BYTES) ||
            lxp_ct_is_zero(set->attestors[i].payout_account, 32U))
            return LXP_ERR_NON_CANONICAL;
        if (i != 0U && memcmp(set->attestors[i - 1U].signer,
                              set->attestors[i].signer,
                              LX_WEB_SIGNER_BYTES) >= 0)
            return LXP_ERR_UNSORTED_SEQUENCE;
    }
    return LXP_OK;
}

lxp_result lx_web_attestor_set_encode(const lx_web_attestor_set *set,
                                      uint8_t *bytes, size_t capacity,
                                      size_t *length)
{
    size_t offset = 2U;
    size_t i;
    lxp_result status;
    if (bytes == NULL || length == NULL) return LXP_ERR_NON_CANONICAL;
    status = lx_web_attestor_set_validate(set);
    if (status != LXP_OK) return status;
    if (capacity < 2U + set->count * LX_WEB_ATTESTOR_ENTRY_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    bytes[0] = (uint8_t)set->count;
    bytes[1] = (uint8_t)set->threshold;
    for (i = 0U; i < set->count; ++i) {
        (void)memcpy(bytes + offset, set->attestors[i].signer,
                     LX_WEB_SIGNER_BYTES);
        (void)memcpy(bytes + offset + LX_WEB_SIGNER_BYTES,
                     set->attestors[i].payout_account, 32U);
        offset += LX_WEB_ATTESTOR_ENTRY_BYTES;
    }
    *length = offset;
    return LXP_OK;
}

lxp_result lx_web_attestor_set_decode(const uint8_t *bytes, size_t length,
                                      lx_web_attestor_set *set)
{
    size_t offset = 2U;
    size_t i;
    if (bytes == NULL || set == NULL || length < 2U ||
        bytes[0] == 0U || bytes[0] > LX_WEB_MAX_ATTESTORS ||
        length != 2U + (size_t)bytes[0] * LX_WEB_ATTESTOR_ENTRY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(set, 0, sizeof(*set));
    set->count = bytes[0];
    set->threshold = bytes[1];
    for (i = 0U; i < set->count; ++i) {
        (void)memcpy(set->attestors[i].signer, bytes + offset,
                     LX_WEB_SIGNER_BYTES);
        (void)memcpy(set->attestors[i].payout_account,
                     bytes + offset + LX_WEB_SIGNER_BYTES, 32U);
        offset += LX_WEB_ATTESTOR_ENTRY_BYTES;
    }
    return lx_web_attestor_set_validate(set);
}

lxp_result lx_web_attestor_lookup(const lx_web_attestor_set *set,
                                  const uint8_t signer[LX_WEB_SIGNER_BYTES],
                                  const lx_web_attestor **attestor)
{
    size_t i;
    if (set == NULL || signer == NULL || attestor == NULL ||
        set->count > LX_WEB_MAX_ATTESTORS)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < set->count; ++i)
        if (memcmp(set->attestors[i].signer, signer,
                   LX_WEB_SIGNER_BYTES) == 0) {
            *attestor = &set->attestors[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_FIELD;
}

lxp_result lx_web_attestor_set_execute(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lxp_authority_resolved *authority,
                                       lx_web_attestor_set *set)
{
    lx_web_attestor_set replacement;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || set == NULL ||
        activity->activity_type != LX_WEB_ATTESTOR_SET_ACTIVITY ||
        activity->payload.bytes == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (ctx->kernel == NULL || !ctx->mutable ||
        !ctx->kernel->handover.enabled ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        lxp_ct_is_zero(ctx->kernel->handover.governance_public_key, 32U) ||
        lxp_ct_memcmp(authority->verified_key,
                      ctx->kernel->handover.governance_public_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_activity_verify_payload_hash(activity);
    if (status != LXP_OK) return status;
    status = lx_web_attestor_set_decode(activity->payload.bytes,
                                        activity->payload.length,
                                        &replacement);
    if (status != LXP_OK) return status;
    replacement.updated_sequence = lxp_ctx_global_sequence(ctx);
    *set = replacement;
    return LXP_OK;
}
