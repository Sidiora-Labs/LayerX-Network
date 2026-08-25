#include "layerx/lx_asset.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static const uint32_t activity_types[] = {
    LX_ASSET_REGISTER, LX_ASSET_PAUSE, LX_ASSET_UNPAUSE,
    LX_ASSET_ACCOUNT_OPEN, LX_ASSET_SEND, LX_ASSET_RECEIVE,
    LX_ASSET_GRANT_ISSUE, LX_ASSET_GRANT_REVOKE
};

typedef struct asset_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
} asset_decoded;

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
    asset_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 8U ||
        (payload == NULL && payload_length != 0U)) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(asset_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (asset_decoded *)memory;
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
    const asset_decoded *value = (const asset_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        value->ordinal == 0U || value->ordinal > 8U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const asset_decoded *value = (const asset_decoded *)decoded;
    (void)activity;
    (void)authority;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_emit_event(ctx, value->ordinal, value->payload,
                              value->payload_length);
}

static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{
    (void)epoch;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_ctx_charge_gas(ctx, 1U);
}

static lxp_result module_state_root(lxp_module_ctx *ctx, uint8_t root[32])
{
    if (ctx == NULL || root == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_ASSET, root);
}

const lxp_module_iface *lx_asset_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_ASSET, 1U, "asset", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_state_root, NULL
    };
    return &iface;
}

lxp_result lx_asset_registry_init(lx_asset_registry *registry,
                                  uint64_t next_sequence)
{
    if (registry == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(registry, 0, sizeof(*registry));
    registry->next_sequence = next_sequence;
    return LXP_OK;
}

static bool symbol_valid(const lx_asset_record *record)
{
    size_t i;
    if (record->symbol_length == 0U ||
        record->symbol_length > LX_ASSET_SYMBOL_MAX ||
        record->symbol[record->symbol_length] != '\0') return false;
    for (i = 0U; i < record->symbol_length; ++i)
        if (!((record->symbol[i] >= 'A' && record->symbol[i] <= 'Z') ||
              (record->symbol[i] >= '0' && record->symbol[i] <= '9')))
            return false;
    return true;
}

lxp_result lx_asset_register(lx_asset_registry *registry,
                             const lx_asset_record *record,
                             uint64_t sequence, lxp_u128 fee)
{
    size_t i;
    lxp_u128 charged;
    lxp_result status;
    if (registry == NULL || record == NULL || sequence != registry->next_sequence ||
        lxp_ct_is_zero(record->asset_id, 32U) || !symbol_valid(record) ||
        record->decimals > 38U ||
        record->custody_kind != LX_ASSET_CUSTODY_PAXEER ||
        record->custody_reference_length == 0U ||
        record->custody_reference_length > LX_ASSET_CUSTODY_REFERENCE_MAX ||
        registry->count > LX_ASSET_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_u128_add(registry->fees_charged, fee, &charged);
    if (status != LXP_OK) return status;
    registry->fees_charged = charged;
    ++registry->next_sequence;
    for (i = 0U; i < registry->count; ++i)
        if (memcmp(registry->assets[i].asset_id, record->asset_id, 32U) == 0)
            return LXP_ERR_ASSET_ALREADY_REGISTERED;
    if (registry->count == LX_ASSET_REGISTRY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    registry->assets[registry->count++] = *record;
    return LXP_OK;
}

lxp_result lx_asset_lookup(lx_asset_registry *registry,
                           const uint8_t asset_id[32],
                           lx_asset_record **record)
{
    size_t i;
    if (registry == NULL || asset_id == NULL || record == NULL ||
        registry->count > LX_ASSET_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < registry->count; ++i) {
        if (memcmp(registry->assets[i].asset_id, asset_id, 32U) == 0) {
            *record = &registry->assets[i];
            return LXP_OK;
        }
    }
    return LXP_ERR_ASSET_MISMATCH;
}

lxp_result lx_asset_pause(lx_asset_registry *registry,
                          const uint8_t asset_id[32])
{
    lx_asset_record *record;
    lxp_result status = lx_asset_lookup(registry, asset_id, &record);
    if (status == LXP_OK) record->paused = true;
    return status;
}

lxp_result lx_asset_unpause(lx_asset_registry *registry,
                            const uint8_t asset_id[32])
{
    lx_asset_record *record;
    lxp_result status = lx_asset_lookup(registry, asset_id, &record);
    if (status == LXP_OK) record->paused = false;
    return status;
}

lxp_result lx_asset_amount_decode(const uint8_t *bytes, size_t length,
                                  lxp_u128 *amount)
{
    lxp_u128 value = { 0U, 0U };
    size_t i;
    if (bytes == NULL || amount == NULL || length == 0U ||
        (length > 1U && bytes[0] == (uint8_t)'0')) return LXP_ERR_INVALID_AMOUNT;
    for (i = 0U; i < length; ++i) {
        lxp_u256 product;
        lxp_u128 next;
        lxp_u128 digit;
        if (bytes[i] < (uint8_t)'0' || bytes[i] > (uint8_t)'9')
            return LXP_ERR_INVALID_AMOUNT;
        digit = (lxp_u128){ 0U, bytes[i] - (uint8_t)'0' };
        if (lxp_u128_mul(value, (lxp_u128){ 0U, 10U }, &product) != LXP_OK ||
            product.words[2] != 0U || product.words[3] != 0U)
            return LXP_ERR_INVALID_AMOUNT;
        next = (lxp_u128){ product.words[1], product.words[0] };
        if (lxp_u128_add(next, digit, &value) != LXP_OK)
            return LXP_ERR_INVALID_AMOUNT;
    }
    *amount = value;
    return LXP_OK;
}

lxp_result lx_asset_record_encode(const lx_asset_record *record,
                                  uint8_t *bytes, size_t capacity,
                                  size_t *length)
{
    size_t required;
    size_t cursor = 0U;
    if (record == NULL || bytes == NULL || length == NULL || !symbol_valid(record))
        return LXP_ERR_NON_CANONICAL;
    required = 32U + 1U + record->symbol_length + 1U + 1U + 2U +
               record->custody_reference_length + 1U;
    if (required > capacity) return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(bytes + cursor, record->asset_id, 32U); cursor += 32U;
    bytes[cursor++] = record->symbol_length;
    (void)memcpy(bytes + cursor, record->symbol, record->symbol_length);
    cursor += record->symbol_length;
    bytes[cursor++] = record->decimals;
    bytes[cursor++] = (uint8_t)record->custody_kind;
    bytes[cursor++] = (uint8_t)(record->custody_reference_length >> 8U);
    bytes[cursor++] = (uint8_t)record->custody_reference_length;
    (void)memcpy(bytes + cursor, record->custody_reference,
                 record->custody_reference_length);
    cursor += record->custody_reference_length;
    bytes[cursor++] = record->paused ? 1U : 0U;
    *length = cursor;
    return LXP_OK;
}

lxp_result lx_asset_record_decode(const uint8_t *bytes, size_t length,
                                  lx_asset_record *record)
{
    size_t cursor = 0U;
    uint16_t reference_length;
    if (bytes == NULL || record == NULL || length < 38U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->asset_id, bytes, 32U); cursor += 32U;
    record->symbol_length = bytes[cursor++];
    if (record->symbol_length == 0U || record->symbol_length > LX_ASSET_SYMBOL_MAX ||
        record->symbol_length > length - cursor) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(record->symbol, bytes + cursor, record->symbol_length);
    cursor += record->symbol_length;
    record->decimals = bytes[cursor++];
    record->custody_kind = (lx_asset_custody_kind)bytes[cursor++];
    reference_length = (uint16_t)((uint16_t)bytes[cursor] << 8U) |
                       bytes[cursor + 1U];
    cursor += 2U;
    if (reference_length == 0U || reference_length >
        LX_ASSET_CUSTODY_REFERENCE_MAX || reference_length > length - cursor - 1U)
        return LXP_ERR_NON_CANONICAL;
    record->custody_reference_length = reference_length;
    (void)memcpy(record->custody_reference, bytes + cursor, reference_length);
    cursor += reference_length;
    if (cursor + 1U != length || bytes[cursor] > 1U)
        return LXP_ERR_NON_CANONICAL;
    record->paused = bytes[cursor] != 0U;
    return symbol_valid(record) ? LXP_OK : LXP_ERR_NON_CANONICAL;
}

lxp_result lx_asset_transfer_state(const lx_asset_record *record,
                                   lxp_transfer_asset_state *state)
{
    if (record == NULL || state == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(state, 0, sizeof(*state));
    (void)memcpy(state->asset_id, record->asset_id, 32U);
    state->registered = true;
    state->paused = record->paused;
    return LXP_OK;
}
