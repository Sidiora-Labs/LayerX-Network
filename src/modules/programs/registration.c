#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

typedef struct programs_activity {
    uint16_t ordinal;
    uint8_t program_id[32];
    const uint8_t *body;
    size_t body_length;
} programs_activity;

static const uint32_t activity_types[] = {
    LX_PROGRAMS_DEPLOY,
    LX_PROGRAMS_UPGRADE,
    LX_PROGRAMS_CALL,
    LX_PROGRAMS_REGISTRY,
    LX_PROGRAMS_TRANSFER
};

static lxp_result programs_genesis(lxp_module_ctx *ctx,
                                   const uint8_t *manifest, size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result programs_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                  const uint8_t *payload, size_t length,
                                  void **decoded)
{
    programs_activity *value;
    void *allocation;
    lxp_result status;
    if (ordinal == 1U || ordinal == 2U)
        return lxp_programs_lifecycle_decode(ctx, ordinal, payload, length,
                                             decoded);
    if (ordinal == lxp_activity_type_ordinal(LX_PROGRAMS_CALL))
        return lxp_programs_call_decode(ctx, payload, length, decoded);
    if (ordinal == lxp_activity_type_ordinal(LX_PROGRAMS_TRANSFER))
        return lxp_programs_transfer_decode(ctx, payload, length, decoded);
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 4U ||
        payload == NULL || length < 32U)
        return ordinal == 0U || ordinal > 4U ? LXP_ERR_UNKNOWN_ACTIVITY :
                                               LXP_ERR_TRUNCATED;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_activity), &allocation);
    if (status != LXP_OK) return status;
    value = (programs_activity *)allocation;
    value->ordinal = ordinal;
    (void)memcpy(value->program_id, payload, sizeof(value->program_id));
    value->body = payload + sizeof(value->program_id);
    value->body_length = length - sizeof(value->program_id);
    *decoded = value;
    return LXP_OK;
}

static lxp_result programs_validate(lxp_module_ctx *ctx,
                                    const lxp_activity *activity,
                                    const lxp_authority_resolved *authority,
                                    const void *decoded)
{
    const programs_activity *value = (const programs_activity *)decoded;
    if (activity != NULL &&
        (lxp_activity_type_ordinal(activity->activity_type) == 1U ||
         lxp_activity_type_ordinal(activity->activity_type) == 2U))
        return lxp_programs_lifecycle_validate(ctx, activity, authority,
                                               decoded);
    if (activity != NULL && activity->activity_type == LX_PROGRAMS_TRANSFER)
        return lxp_programs_transfer_validate(ctx, activity, authority, decoded);
    if (activity != NULL && activity->activity_type == LX_PROGRAMS_CALL)
        return lxp_programs_call_validate(ctx, activity, authority, decoded);
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_is_zero(authority->principal, sizeof(authority->principal)))
        return LXP_ERR_AUTH_SCOPE;
    if (lxp_ct_is_zero(value->program_id, sizeof(value->program_id)))
        return LXP_ERR_UNKNOWN_FIELD;
    if (value->ordinal != 4U && value->body_length == 0U)
        return LXP_ERR_TRUNCATED;
    return lxp_ctx_charge_gas(ctx, value->body_length + 33U);
}

static lxp_result programs_execute(lxp_module_ctx *ctx,
                                   const lxp_activity *activity,
                                   const lxp_authority_resolved *authority,
                                   const void *decoded,
                                   lxp_effect_buffer *effects)
{
    const programs_activity *value = (const programs_activity *)decoded;
    uint8_t key[34];
    uint8_t record[32U + 8U + 2U];
    size_t i;
    lxp_result status;
    if (activity != NULL &&
        (lxp_activity_type_ordinal(activity->activity_type) == 1U ||
         lxp_activity_type_ordinal(activity->activity_type) == 2U))
        return lxp_programs_lifecycle_execute(ctx, activity, authority,
                                              decoded, effects);
    if (activity != NULL && activity->activity_type == LX_PROGRAMS_TRANSFER)
        return lxp_programs_transfer_execute(ctx, activity, authority, decoded,
                                             effects);
    if (activity != NULL && activity->activity_type == LX_PROGRAMS_CALL)
        return lxp_programs_call_execute(ctx, activity, authority, decoded,
                                         effects);
    (void)effects;
    if (ctx == NULL || authority == NULL || value == NULL)
        return LXP_ERR_NON_CANONICAL;
    key[0] = (uint8_t)'p';
    key[1] = (uint8_t)value->ordinal;
    (void)memcpy(key + 2U, value->program_id, 32U);
    (void)memcpy(record, authority->principal, 32U);
    for (i = 0U; i < 8U; ++i)
        record[32U + i] = (uint8_t)(lxp_ctx_global_sequence(ctx) >>
                                    (56U - 8U * i));
    record[40] = (uint8_t)(value->body_length >> 8U);
    record[41] = (uint8_t)value->body_length;
    if (value->ordinal != 4U) {
        status = lxp_ctx_kv_put(ctx, key, sizeof(key), record, sizeof(record));
        if (status != LXP_OK) return status;
    }
    status = lxp_ctx_emit_event(ctx, value->ordinal, value->program_id,
                                sizeof(value->program_id));
    if (status != LXP_OK) return status;
    return lxp_ctx_charge_gas(ctx, sizeof(record));
}

static lxp_result programs_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                                 uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result programs_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_PROGRAMS, root);
}

const lxp_module_iface *programs_module_registration(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_PROGRAMS,
        LX_PROGRAMS_ABI_VERSION,
        "programs",
        activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        programs_genesis,
        programs_decode,
        programs_validate,
        programs_execute,
        programs_epoch,
        programs_epoch,
        programs_state_root,
        NULL
    };
    return &iface;
}

const lxp_module_iface *lx_programs_module_iface(void)
{
    return programs_module_registration();
}
