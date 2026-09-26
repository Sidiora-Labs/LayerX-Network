#include "layerx/lx_web.h"

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_state.h"

#include <string.h>

_Static_assert((int)LX_WEB_MODULE_ID == (int)LXP_MODULE_WEB,
               "web module id matches the kernel module table");
_Static_assert((LX_WEB_OBSERVATION_ACTIVITY >> 16) == LXP_MODULE_WEB &&
                   (LX_WEB_ATTESTOR_SET_ACTIVITY >> 16) == LXP_MODULE_WEB,
               "web activities belong to the web module");
_Static_assert((int)LX_WEB_ATTESTOR_SET_MAX_BYTES <=
                   (int)LXP_MODULE_MAX_VALUE_BYTES,
               "the attestor set fits one module storage entry");

enum {
    WEB_ORDINAL_OBSERVATION = LX_WEB_OBSERVATION_ACTIVITY & 0xFFFF,
    WEB_ORDINAL_ATTESTOR_SET = LX_WEB_ATTESTOR_SET_ACTIVITY & 0xFFFF
};

static const uint32_t activity_types[] = {
    LX_WEB_OBSERVATION_ACTIVITY, LX_WEB_ATTESTOR_SET_ACTIVITY
};

static const uint8_t attestor_set_key[] = {
    'w', 'e', 'b', '/', 'a', 't', 't', 'e', 's', 't', 'o', 'r', 's'
};

typedef struct web_decoded {
    uint16_t ordinal;
    lxp_byte_span payload;
} web_decoded;

/* Web activities read and write the paid request records, the committed
 * answers and the attestor set in Programs module storage, so they only run
 * in a Programs module context. */
static bool web_context(const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->kernel != NULL &&
           ctx->module_id == LXP_MODULE_PROGRAMS;
}

static lx_web_store *web_store(const lxp_module_ctx *ctx)
{
    return (lx_web_store *)ctx->kernel->module_runtime[LXP_MODULE_WEB];
}

static lxp_result attestor_set_load(lxp_module_ctx *ctx,
                                    lx_web_attestor_set *set)
{
    const uint8_t *bytes;
    size_t length;
    lxp_result status = lxp_ctx_kv_get(ctx, attestor_set_key,
                                       sizeof(attestor_set_key), &bytes,
                                       &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_ATTESTATION_THRESHOLD;
    if (status != LXP_OK) return status;
    return lx_web_attestor_set_decode(bytes, length, set);
}

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t manifest_length)
{
    if (ctx == NULL || (manifest == NULL && manifest_length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, manifest_length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t payload_length,
                                void **decoded)
{
    web_decoded *value;
    void *memory = NULL;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || payload == NULL ||
        payload_length == 0U ||
        (ordinal != WEB_ORDINAL_OBSERVATION &&
         ordinal != WEB_ORDINAL_ATTESTOR_SET))
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (!web_context(ctx)) return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(web_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (web_decoded *)memory;
    value->ordinal = ordinal;
    value->payload.bytes = payload;
    value->payload.length = payload_length;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const web_decoded *value = (const web_decoded *)decoded;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_activity_module_id(activity->activity_type) != LXP_MODULE_WEB ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (!web_context(ctx)) return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_ctx_charge_gas(ctx, value->payload.length + 1U);
    if (status != LXP_OK) return status;
    if (value->ordinal == WEB_ORDINAL_OBSERVATION) {
        lx_web_observation observation;
        lx_web_attestor_set set;
        const lx_web_store *store = web_store(ctx);
        if (store == NULL) return LXP_ERR_MODULE_DISABLED;
        status = lx_web_observation_decode(value->payload.bytes,
                                           value->payload.length,
                                           &observation);
        if (status != LXP_OK) return status;
        if (observation.network_id != store->network_id)
            return LXP_ERR_WRONG_NETWORK;
        return attestor_set_load(ctx, &set);
    }
    {
        lx_web_attestor_set set;
        return lx_web_attestor_set_decode(value->payload.bytes,
                                          value->payload.length, &set);
    }
}

static lxp_result execute_observation(lxp_module_ctx *ctx,
                                      const web_decoded *value)
{
    lx_web_attestor_set set;
    lx_web_committed committed;
    lx_web_intake_request request;
    lx_web_store *store = web_store(ctx);
    lxp_result status;
    if (store == NULL) return LXP_ERR_MODULE_DISABLED;
    status = attestor_set_load(ctx, &set);
    if (status != LXP_OK) return status;
    request.store = store;
    request.attestors = &set;
    request.payload = value->payload.bytes;
    request.payload_length = value->payload.length;
    return lx_web_intake(ctx, &request, &committed);
}

static lxp_result execute_attestor_set(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lxp_authority_resolved *authority)
{
    lx_web_attestor_set set;
    uint8_t bytes[LX_WEB_ATTESTOR_SET_MAX_BYTES];
    size_t length;
    lxp_result status = lx_web_attestor_set_execute(ctx, activity, authority,
                                                    &set);
    if (status == LXP_OK)
        status = lx_web_attestor_set_encode(&set, bytes, sizeof(bytes),
                                            &length);
    if (status != LXP_OK) return status;
    return lxp_ctx_kv_put(ctx, attestor_set_key, sizeof(attestor_set_key),
                          bytes, length);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const web_decoded *value = (const web_decoded *)decoded;
    (void)effects;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (!web_context(ctx)) return LXP_ERR_CONTEXT_MISMATCH;
    if (value->ordinal == WEB_ORDINAL_OBSERVATION)
        return execute_observation(ctx, value);
    return execute_attestor_set(ctx, activity, authority);
}

static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_WEB, root);
}

const lxp_module_iface *lx_web_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_WEB, 1U, "web", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_state_root, NULL
    };
    return &iface;
}
