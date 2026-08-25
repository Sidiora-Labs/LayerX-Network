#include "layerx/lx_escrow.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <stdbool.h>
#include <string.h>

static const uint32_t activity_types[] = {
    LX_ESCROW_OPEN, LX_ESCROW_CAPTURE, LX_ESCROW_PARTIAL_CAPTURE,
    LX_ESCROW_RELEASE, LX_ESCROW_TIMEOUT, LX_ESCROW_DISPUTE_OPEN,
    LX_ESCROW_DISPUTE_RESOLVE
};

typedef struct escrow_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
} escrow_decoded;

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
    escrow_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        (payload == NULL && payload_length != 0U))
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(escrow_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (escrow_decoded *)memory;
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = payload_length;
    *decoded = value;
    return LXP_OK;
}

static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{
    const escrow_decoded *value = (const escrow_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        value->ordinal == 0U || value->ordinal > 7U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const escrow_decoded *value = (const escrow_decoded *)decoded;
    (void)activity;
    (void)authority;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_emit_event(ctx, value->ordinal, value->payload,
                              value->payload_length);
}

static lxp_result module_epoch_end(lxp_module_ctx *ctx, uint64_t epoch,
                                   uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, 1U);
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_ESCROW, root);
}

const lxp_module_iface *lx_escrow_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_ESCROW, 1U, "escrow", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        lx_escrow_epoch_begin, module_epoch_end, module_state_root, NULL
    };
    return &iface;
}

lxp_result lx_escrow_lookup(lx_escrow_store *store,
                            const uint8_t escrow_id[32],
                            lx_escrow_record **record)
{
    size_t i;
    if (store == NULL || escrow_id == NULL || record == NULL ||
        store->count > LX_ESCROW_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < store->count; ++i) {
        if (memcmp(store->records[i].escrow_id, escrow_id, 32U) == 0) {
            *record = &store->records[i];
            return LXP_OK;
        }
    }
    return LXP_ERR_ESCROW_STATE;
}

lxp_result lx_escrow_state_put(lx_escrow_store *store,
                               const lx_escrow_record *record)
{
    lx_escrow_record *existing;
    if (store == NULL || record == NULL ||
        store->count > LX_ESCROW_STORE_CAPACITY ||
        lxp_ct_is_zero(record->escrow_id, 32U) ||
        record->state < LX_ESCROW_STATE_OPEN ||
        record->state > LX_ESCROW_STATE_TIMED_OUT)
        return LXP_ERR_NON_CANONICAL;
    if (lx_escrow_lookup(store, record->escrow_id, &existing) == LXP_OK)
        return LXP_ERR_ESCROW_STATE;
    if (store->count == LX_ESCROW_STORE_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    store->records[store->count++] = *record;
    return LXP_OK;
}

static lxp_result validate_open(const lx_escrow_open_request *request)
{
    lx_escrow_record *existing;
    if (request == NULL || request->store == NULL ||
        request->store->count > LX_ESCROW_STORE_CAPACITY ||
        request->owner == NULL ||
        request->escrow_account == NULL || request->asset == NULL ||
        request->owner->kind != LX_ACCOUNT_AGENT_MAIN ||
        request->escrow_account->kind != LX_ACCOUNT_AGENT_ESCROW ||
        lxp_u128_is_zero(request->amount) || request->asset->paused ||
        request->record.state != LX_ESCROW_STATE_OPEN ||
        !lxp_u128_is_zero(request->record.captured_amount) ||
        lxp_u128_cmp(request->record.locked_amount, request->amount) != 0 ||
        memcmp(request->record.owner, request->owner->id, 32U) != 0 ||
        memcmp(request->record.escrow_account,
               request->escrow_account->id, 32U) != 0 ||
        memcmp(request->record.asset_id, request->asset->asset_id, 32U) != 0)
        return LXP_ERR_NON_CANONICAL;
    if (!request->owner->has_asset || !request->escrow_account->has_asset ||
        memcmp(request->owner->asset_id, request->asset->asset_id, 32U) != 0 ||
        memcmp(request->escrow_account->asset_id,
               request->asset->asset_id, 32U) != 0)
        return LXP_ERR_ASSET_MISMATCH;
    if (lx_escrow_lookup(request->store, request->record.escrow_id,
                         &existing) == LXP_OK ||
        request->store->count == LX_ESCROW_STORE_CAPACITY)
        return LXP_ERR_ESCROW_STATE;
    return LXP_OK;
}

lxp_result lx_escrow_open_execute(lxp_module_ctx *ctx,
                                  const lx_escrow_open_request *request,
                                  lxp_receipt *receipt)
{
    lxp_transfer_set set;
    lxp_result status = validate_open(request);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = 1U;
    set.legs[0].from = request->owner;
    set.legs[0].to = request->escrow_account;
    (void)memcpy(set.legs[0].asset_id, request->asset->asset_id, 32U);
    set.legs[0].amount = request->amount;
    set.legs[0].reason = LXP_REASON_ESCROW_LOCK;
    set.context = request->context;
    status = lxp_ctx_emit_transfer_set(ctx, &set, receipt);
    if (status != LXP_OK) return status;
    return lx_escrow_state_put(request->store, &request->record);
}
