#include "layerx/lx_asset.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_protocol.h"

#include <string.h>
#include <stdlib.h>

static const uint32_t activity_types[] = {
    LX_ASSET_REGISTER, LX_ASSET_PAUSE, LX_ASSET_UNPAUSE,
    LX_ASSET_ACCOUNT_OPEN, LX_ASSET_SEND, LX_ASSET_RECEIVE,
    LX_ASSET_GRANT_ISSUE, LX_ASSET_GRANT_REVOKE, LX_ASSET_MINT, LX_ASSET_BURN
};

typedef union asset_typed_payload {
    lx_asset_register_payload registration;
    lx_asset_account_open_payload account_open;
    lx_asset_supply_payload supply;
    lx_asset_grant_revoke_payload revocation;
    lxp_receive receive;
    lxp_payer_grant grant;
} asset_typed_payload;

typedef struct asset_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
    lxp_send send;
    bool send_present;
    asset_typed_payload *typed;
} asset_decoded;

static const lx_asset_record *runtime_asset(
    const lx_asset_runtime *runtime, const uint8_t asset_id[32])
{
    size_t index;
    if (runtime == NULL || runtime->assets == NULL || asset_id == NULL ||
        runtime->asset_count == 0U ||
        runtime->asset_count > LX_ASSET_REGISTRY_CAPACITY)
        return NULL;
    for (index = 0U; index < runtime->asset_count; ++index)
        if (lxp_ct_memcmp(runtime->assets[index].asset_id,
                          asset_id, 32U) == 0)
            return &runtime->assets[index];
    return NULL;
}

static bool source_matches_actor(const lx_account *source,
                                 const lxp_activity *activity)
{
    static const uint8_t prefix[] = "agent:";
    static const uint8_t suffix[] = ":main";
    size_t expected_length;
    if (source == NULL || activity == NULL ||
        activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U ||
        activity->actor_did.length > LXP_MAX_DID_LENGTH)
        return false;
    expected_length = sizeof(prefix) - 1U + activity->actor_did.length;
    if (source->name_length < expected_length ||
        memcmp(source->name, prefix, sizeof(prefix) - 1U) != 0 ||
        memcmp(source->name + sizeof(prefix) - 1U,
               activity->actor_did.bytes, activity->actor_did.length) != 0)
        return false;
    if (source->name_length == expected_length + sizeof(suffix) - 1U)
        return memcmp(source->name + expected_length,
                       suffix, sizeof(suffix) - 1U) == 0;
    if (source->name_length == expected_length + 71U && source->has_asset &&
        memcmp(source->name + expected_length, ":asset:", 7U) == 0) {
        static const uint8_t hex[] = "0123456789abcdef";
        size_t i;
        for (i = 0U; i < 32U; ++i)
            if (source->name[expected_length + 7U + i * 2U] !=
                    hex[source->asset_id[i] >> 4U] ||
                source->name[expected_length + 8U + i * 2U] !=
                    hex[source->asset_id[i] & 15U])
                return false;
        return true;
    }
    return false;
}

static lxp_result send_context(const lxp_send *send, uint8_t context[32])
{
    uint8_t material[32U + 32U + 32U + 16U + 32U];
    lxp_result status;
    if (send == NULL || context == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(material, send->from, 32U);
    (void)memcpy(material + 32U, send->to, 32U);
    (void)memcpy(material + 64U, send->asset, 32U);
    status = lxp_u128_to_be(send->amount, material + 96U);
    if (status == LXP_OK)
        (void)memcpy(material + 112U, send->idempotency_key, 32U);
    return status == LXP_OK ?
        lxp_hash_context_value(material, sizeof(material), context) : status;
}

static lxp_result asset_load(lxp_module_ctx *ctx, const uint8_t id[32],
                              lx_asset_record *record);

static lxp_result validate_send(lxp_module_ctx *ctx,
                                const lxp_activity *activity,
                                const lxp_authority_resolved *authority,
                                const lxp_send *send)
{
    lx_asset_runtime *runtime;
    lxp_send_environment environment;
    lx_asset_record record;
    lxp_transfer_asset_state transfer_asset;
    lx_account *source;
    uint8_t expected_context[32];
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || send == NULL)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_asset_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL || runtime->asset_count == 0U ||
        runtime->transfer_assets == NULL || runtime->transfer_asset_count == 0U ||
        runtime->network_id == 0U ||
        !lxp_protocol_version_uses_occupancy(runtime->protocol_version) ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        lxp_ctx_activity_id(ctx) == NULL ||
        lxp_ct_is_zero(lxp_ctx_activity_id(ctx), 32U) ||
        ctx->kernel->state->accounts != runtime->accounts ||
        !ctx->kernel->state->account_root_required ||
        activity->activity_type != LX_ASSET_SEND ||
        activity->network_id != runtime->network_id ||
        activity->protocol_version != runtime->protocol_version ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        send->authorization.kind != LXP_AUTH_OWNER ||
        lxp_ct_memcmp(activity->idempotency_key,
                      send->idempotency_key, 32U) != 0 ||
        lxp_ct_memcmp(authority->verified_key,
                      send->authorization.public_key, 32U) != 0 ||
        lxp_ct_memcmp(send->from, send->to, 32U) == 0 ||
        asset_load(ctx, send->asset, &record) != LXP_OK)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    (void)memset(&environment, 0, sizeof(environment));
    environment.accounts = runtime->accounts;
    (void)lx_asset_transfer_state(&record, &transfer_asset);
    environment.assets = &transfer_asset;
    environment.asset_count = 1U;
    environment.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    environment.network_id = runtime->network_id;
    environment.protocol_version = runtime->protocol_version;
    status = lxp_send_validate(send, &environment);
    if (status == LXP_OK && runtime_asset(runtime, send->asset) != NULL) {
        environment.assets = runtime->transfer_assets;
        environment.asset_count = runtime->transfer_asset_count;
        status = lxp_send_validate(send, &environment);
        for (size_t i = 0U; status == LXP_OK && i < environment.asset_count; ++i)
            if (memcmp(environment.assets[i].asset_id, send->asset, 32U) == 0 &&
                environment.assets[i].paused) status = LXP_ERR_ASSET_PAUSED;
    }
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, send->from, &source);
    if (status == LXP_OK && !source_matches_actor(source, activity))
        status = LXP_ERR_UNAUTHORIZED_DEBIT;
    if (status == LXP_OK) status = send_context(send, expected_context);
    if (status == LXP_OK && lxp_ct_memcmp(
            expected_context, send->context_hash, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    return status;
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
    asset_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U ||
        ordinal > 11U || ordinal == 9U ||
        (payload == NULL && payload_length != 0U)) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(asset_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (asset_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = payload_length;
    if (ordinal == lxp_activity_type_ordinal(LX_ASSET_SEND)) {
        status = lxp_send_decode(payload, payload_length, &value->send);
        if (status != LXP_OK) return status;
        value->send_present = true;
    }
    if (ordinal != 2U && ordinal != 3U && ordinal != 5U) {
        status = lxp_ctx_arena_alloc(ctx, sizeof(*value->typed),
                                     _Alignof(asset_typed_payload), &memory);
        if (status != LXP_OK) return status;
        value->typed = (asset_typed_payload *)memory;
    }
    switch (ordinal) {
    case 1U:
        status = lx_asset_register_decode(payload, payload_length,
                                          &value->typed->registration);
        break;
    case 4U:
        status = lx_asset_account_open_decode(payload, payload_length,
                                              &value->typed->account_open);
        break;
    case 6U:
        status = lxp_receive_decode(payload, payload_length, &value->typed->receive);
        break;
    case 7U:
        status = lxp_payer_grant_decode(payload, payload_length, &value->typed->grant);
        break;
    case 8U:
        status = lx_asset_grant_revoke_decode(payload, payload_length,
                                              &value->typed->revocation);
        break;
    case 10U:
    case 11U:
        status = lx_asset_supply_decode(payload, payload_length, &value->typed->supply);
        break;
    default:
        break;
    }
    if (status != LXP_OK) return status;
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
        value->ordinal == 0U || value->ordinal > 11U || value->ordinal == 9U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (value->send_present) {
        lxp_result status = validate_send(ctx, activity, authority,
                                          &value->send);
        if (status != LXP_OK) return status;
    }
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

#include "lx_asset_execution.h"

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const asset_decoded *value = (const asset_decoded *)decoded;
    lx_asset_runtime *runtime;
    const lx_asset_record *asset;
    lx_asset_record record;
    lxp_transfer_asset_state transfer_asset;
    lx_asset_transfer_request request;
    lxp_transfer_source_authority source_authority;
    lxp_ledger_receipt_input input;
    lxp_receipt transfer_receipt;
    lx_account *from;
    lx_account *to;
    uint8_t authorization[512];
    size_t authorization_length = 0U;
    lxp_result status;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    if (!value->send_present && value->ordinal != 2U && value->ordinal != 3U)
        return asset_execute_typed(ctx, activity, authority, value);
    if (!value->send_present)
        return lxp_ctx_emit_event(ctx, value->ordinal, value->payload,
                                  value->payload_length);
    status = validate_send(ctx, activity, authority, &value->send);
    runtime = (lx_asset_runtime *)lxp_ctx_module_runtime(ctx);
    if (status == LXP_OK) status = asset_load(ctx, value->send.asset, &record);
    asset = &record;
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, value->send.from, &from);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, value->send.to, &to);
    if (status != LXP_OK || runtime == NULL || asset == NULL)
        return status != LXP_OK ? status : LXP_ERR_ASSET_MISMATCH;
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&source_authority, 0, sizeof(source_authority));
    request.from = from;
    request.to = to;
    request.asset = asset;
    request.amount = value->send.amount;
    (void)lx_asset_transfer_state(asset, &transfer_asset);
    request.context.assets = &transfer_asset;
    request.context.asset_count = 1U;
    (void)memcpy(request.context.authorized_from, value->send.from, 32U);
    request.context.actor_sequence = value->send.sequence;
    request.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    request.context.expires_at = value->send.expires_at;
    request.context.sequence_account = from;
    request.context.debit_authority_kind = LXP_AUTH_OWNER;
    source_authority.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(source_authority.authorized_from, value->send.from, 32U);
    request.context.source_authorities = &source_authority;
    request.context.source_authority_count = 1U;
    (void)memset(&transfer_receipt, 0, sizeof(transfer_receipt));
    (void)memset(&input, 0, sizeof(input));
    input.from_balance_before = from->balance;
    input.to_balance_before = to->balance;
    status = lx_asset_send_execute(ctx, &request, &transfer_receipt);
    if (status != LXP_OK) return status;
    status = lxp_send_authorization_message(
        &value->send, authorization, sizeof(authorization),
        &authorization_length);
    if (status == LXP_OK)
        status = lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE,
                                 authorization, authorization_length,
                                 input.authorization_hash);
    if (status != LXP_OK) return status;
    (void)memcpy(input.transaction_id, lxp_ctx_activity_id(ctx), 32U);
    input.operation = (uint8_t)lxp_activity_type_ordinal(LX_ASSET_SEND);
    input.global_sequence = lxp_ctx_global_sequence(ctx);
    (void)memcpy(input.asset, value->send.asset, 32U);
    input.amount = value->send.amount;
    (void)memcpy(input.from, value->send.from, 32U);
    input.from_balance_after = from->balance;
    input.from_sequence = value->send.sequence;
    (void)memcpy(input.to, value->send.to, 32U);
    input.to_balance_after = to->balance;
    (void)memcpy(input.transfer_set_root,
                 transfer_receipt.transfer_set_root, 32U);
    (void)memcpy(input.context_hash, value->send.context_hash, 32U);
    input.timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    input.leg_count = 1U;
    return lxp_ctx_bind_ledger_receipt(ctx, &input);
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
    if (record->issuer_kind != 0U) {
        for (i = 0U; i < record->symbol_length; ++i)
            if ((unsigned char)record->symbol[i] > 0x7fU) return false;
        return true;
    }
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
    if (record->name_length > LX_ASSET_NAME_MAX || record->issuer_kind > 2U ||
        record->custody_reference_length > LX_ASSET_CUSTODY_REFERENCE_MAX)
        return LXP_ERR_NON_CANONICAL;
    required = 2U + 32U + 1U + record->symbol_length + 1U + 1U + 2U +
               record->custody_reference_length + 1U + 1U + record->name_length + 97U;
    if (required > capacity) return LXP_ERR_LENGTH_LIMIT;
    bytes[cursor++] = 0U;
    bytes[cursor++] = 3U;
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
    bytes[cursor++] = record->name_length;
    (void)memcpy(bytes + cursor, record->name, record->name_length);
    cursor += record->name_length;
    (void)lxp_u128_to_be(record->supply_cap, bytes + cursor); cursor += 16U;
    (void)memcpy(bytes + cursor, record->issuer_did32, 32U); cursor += 32U;
    bytes[cursor++] = record->issuer_kind;
    (void)lxp_u128_to_be(record->total_units, bytes + cursor); cursor += 16U;
    (void)memcpy(bytes + cursor, record->salt, 32U); cursor += 32U;
    *length = cursor;
    return LXP_OK;
}

lxp_result lx_asset_record_decode(const uint8_t *bytes, size_t length,
                                  lx_asset_record *record)
{
    size_t cursor = 2U;
    uint16_t reference_length;
    if (bytes == NULL || record == NULL || length < 140U ||
        bytes[0] != 0U || bytes[1] != 3U) return LXP_ERR_NON_CANONICAL;
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->asset_id, bytes + cursor, 32U); cursor += 32U;
    record->symbol_length = bytes[cursor++];
    if (record->symbol_length == 0U || record->symbol_length > LX_ASSET_SYMBOL_MAX ||
        length - cursor < record->symbol_length + 103U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(record->symbol, bytes + cursor, record->symbol_length);
    cursor += record->symbol_length;
    record->decimals = bytes[cursor++];
    record->custody_kind = (lx_asset_custody_kind)bytes[cursor++];
    reference_length = (uint16_t)((uint16_t)bytes[cursor] << 8U) | bytes[cursor + 1U];
    cursor += 2U;
    if (reference_length > LX_ASSET_CUSTODY_REFERENCE_MAX ||
        length - cursor < reference_length + 99U) return LXP_ERR_NON_CANONICAL;
    record->custody_reference_length = reference_length;
    (void)memcpy(record->custody_reference, bytes + cursor, reference_length);
    cursor += reference_length;
    if (bytes[cursor] > 1U) return LXP_ERR_NON_CANONICAL;
    record->paused = bytes[cursor++] != 0U;
    record->name_length = bytes[cursor++];
    if (record->name_length > LX_ASSET_NAME_MAX ||
        length - cursor != record->name_length + 97U) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(record->name, bytes + cursor, record->name_length);
    cursor += record->name_length;
    (void)lxp_u128_from_be(bytes + cursor, &record->supply_cap); cursor += 16U;
    (void)memcpy(record->issuer_did32, bytes + cursor, 32U); cursor += 32U;
    record->issuer_kind = bytes[cursor++];
    (void)lxp_u128_from_be(bytes + cursor, &record->total_units); cursor += 16U;
    (void)memcpy(record->salt, bytes + cursor, 32U);
    if ((record->issuer_kind == 0U && record->custody_reference_length == 0U) ||
        (record->issuer_kind == 1U && record->custody_reference_length != 0U) ||
        (record->issuer_kind != 0U && (record->name_length == 0U ||
         lxp_ct_is_zero(record->issuer_did32, 32U)))) return LXP_ERR_NON_CANONICAL;
    return symbol_valid(record) && record->decimals <= 38U && record->issuer_kind <= 2U ?
        LXP_OK : LXP_ERR_NON_CANONICAL;
}

lxp_result lx_asset_record_migrate_v2(const uint8_t *bytes, size_t length,
    const uint8_t salt[32], uint8_t *output, size_t capacity, size_t *output_length)
{
    uint8_t candidate[384];
    lx_asset_record record;
    lxp_hash_context hash;
    uint8_t expected[32];
    lxp_result status;
    if (bytes == NULL || salt == NULL || output == NULL || output_length == NULL ||
        length < 2U || length > sizeof(candidate) - 32U ||
        bytes[0] != 0U || bytes[1] != 2U)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(candidate, bytes, length);
    candidate[1] = 3U;
    (void)memcpy(candidate + length, salt, 32U);
    status = lx_asset_record_decode(candidate, length + 32U, &record);
    if (status != LXP_OK) return status;
    if (record.issuer_kind == 1U) {
        lxp_hash_init(&hash);
        status = lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1", 11U);
        if (status == LXP_OK) status = lxp_hash_update(&hash, record.issuer_did32, 32U);
        if (status == LXP_OK) status = lxp_hash_update(&hash, salt, 32U);
        if (status == LXP_OK) status = lxp_hash_final(&hash, expected);
        if (status != LXP_OK) return status;
        if (memcmp(expected, record.asset_id, 32U) != 0) return LXP_ERR_ASSET_MISMATCH;
    }
    return lx_asset_record_encode(&record, output, capacity, output_length);
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
