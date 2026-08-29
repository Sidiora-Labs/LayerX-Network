#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_receipt.h"

#include <string.h>

enum { PROGRAM_TRANSFER_LEG_BYTES = 112 };

typedef struct programs_transfer_activity {
    uint8_t program_id[32];
    uint16_t leg_count;
    const uint8_t *legs;
} programs_transfer_activity;

static uint16_t read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | (uint16_t)bytes[1]);
}

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t i;
    for (i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static lx_account *account_by_id(lx_account_registry *accounts,
                                 const uint8_t id[32])
{
    size_t i;
    if (accounts == NULL) return NULL;
    for (i = 0U; i < accounts->count; ++i)
        if (lxp_ct_memcmp(accounts->accounts[i].id, id, 32U) == 0)
            return &accounts->accounts[i];
    return NULL;
}

static lxp_result source_authority_add(
    lxp_transfer_source_authority
        authorities[LXP_MAX_TRANSFER_SET_LEGS],
    size_t *authority_count, const lxp_transfer_leg *leg,
    const lxp_authority_resolved *resolved)
{
    lxp_transfer_source_authority *authority;
    size_t index;
    if (authorities == NULL || authority_count == NULL || leg == NULL ||
        leg->from == NULL || resolved == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_ct_memcmp(leg->from->id, resolved->principal, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    for (index = 0U; index < *authority_count; ++index)
        if (lxp_ct_memcmp(authorities[index].authorized_from,
                          leg->from->id, 32U) == 0)
            return LXP_OK;
    if (*authority_count >= LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_LENGTH_LIMIT;
    authority = &authorities[*authority_count];
    (void)memset(authority, 0, sizeof(*authority));
    (void)memcpy(authority->authorized_from, leg->from->id, 32U);
    authority->debit_authority_kind = LXP_AUTH_OWNER;
    authority->protocol_system_capability = false;
    ++*authority_count;
    return LXP_OK;
}

lxp_result lxp_programs_transfer_decode(lxp_module_ctx *ctx,
                                        const uint8_t *payload,
                                        size_t payload_length,
                                        void **decoded)
{
    programs_transfer_activity *value;
    void *allocation;
    uint16_t leg_count;
    size_t expected;
    lxp_result status;
    if (ctx == NULL || payload == NULL || decoded == NULL || payload_length < 34U)
        return LXP_ERR_TRUNCATED;
    leg_count = read_u16(payload + 32U);
    if (leg_count == 0U || leg_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_LENGTH_LIMIT;
    expected = 34U + (size_t)leg_count * PROGRAM_TRANSFER_LEG_BYTES;
    if (payload_length != expected) return LXP_ERR_NON_CANONICAL;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value),
                                 _Alignof(programs_transfer_activity),
                                 &allocation);
    if (status != LXP_OK) return status;
    value = (programs_transfer_activity *)allocation;
    (void)memcpy(value->program_id, payload, 32U);
    value->leg_count = leg_count;
    value->legs = payload + 34U;
    *decoded = value;
    return LXP_OK;
}

static lxp_result authorize_leg(const programs_transfer_activity *value,
                                const lxp_authority_resolved *authority,
                                const uint8_t *leg)
{
    uint64_t program[4];
    uint64_t principal[4];
    uint64_t authority_hash[4];
    uint64_t asset[4];
    uint64_t to[4];
    size_t i;
    if (lxp_ct_memcmp(leg, authority->principal, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    for (i = 0U; i < 4U; ++i) {
        program[i] = read_u64(value->program_id + i * 8U);
        principal[i] = read_u64(authority->principal + i * 8U);
        authority_hash[i] = read_u64(authority->authority_hash + i * 8U);
        asset[i] = read_u64(leg + 32U + i * 8U);
        to[i] = read_u64(leg + 64U + i * 8U);
    }
    return layerx_programs_authorize_402lxp_leg(
        program[0], program[1], program[2], program[3],
        principal[0], principal[1], principal[2], principal[3],
        authority_hash[0], authority_hash[1], authority_hash[2], authority_hash[3],
        asset[0], asset[1], asset[2], asset[3],
        to[0], to[1], to[2], to[3], read_u64(leg + 96U),
        read_u64(leg + 104U));
}

lxp_result lxp_programs_transfer_validate(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded)
{
    const programs_transfer_activity *value =
        (const programs_transfer_activity *)decoded;
    size_t i;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        lxp_ct_is_zero(value->program_id, 32U) ||
        lxp_ct_is_zero(authority->authority_hash, 32U))
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < value->leg_count; ++i) {
        status = authorize_leg(value, authority,
                               value->legs + i * PROGRAM_TRANSFER_LEG_BYTES);
        if (status != LXP_OK) return status;
    }
    return lxp_ctx_charge_gas(ctx, (uint64_t)value->leg_count * 112U);
}

lxp_result lxp_programs_transfer_execute_root(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects, uint8_t transfer_root[32])
{
    const programs_transfer_activity *value =
        (const programs_transfer_activity *)decoded;
    lx_programs_transfer_runtime *runtime;
    lxp_transfer_source_authority
        source_authorities[LXP_MAX_TRANSFER_SET_LEGS];
    lxp_transfer_set set;
    lxp_receipt receipt;
    lx_account *sequence_account;
    size_t source_authority_count = 0U;
    size_t i;
    lxp_result status;
    (void)effects;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        transfer_root == NULL)
        return LXP_ERR_NON_CANONICAL;
    runtime = (lx_programs_transfer_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->accounts == NULL || runtime->assets == NULL)
        return LXP_ERR_MODULE_DISABLED;
    sequence_account = account_by_id(runtime->accounts, authority->principal);
    if (sequence_account == NULL)
        return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    (void)memset(source_authorities, 0, sizeof(source_authorities));
    (void)memset(&set, 0, sizeof(set));
    set.leg_count = value->leg_count;
    for (i = 0U; i < value->leg_count; ++i) {
        const uint8_t *leg = value->legs + i * PROGRAM_TRANSFER_LEG_BYTES;
        set.legs[i].from = account_by_id(runtime->accounts, leg);
        set.legs[i].to = account_by_id(runtime->accounts, leg + 64U);
        if (set.legs[i].from == NULL || set.legs[i].to == NULL)
            return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
        (void)memcpy(set.legs[i].asset_id, leg + 32U, 32U);
        set.legs[i].amount = (lxp_u128){read_u64(leg + 96U),
                                        read_u64(leg + 104U)};
        set.legs[i].reason = LXP_REASON_PAYMENT;
        set.legs[i].supply_mode = LXP_TRANSFER_CONSERVED;
        status = source_authority_add(source_authorities,
                                      &source_authority_count,
                                      &set.legs[i], authority);
        if (status != LXP_OK) return status;
    }
    set.context.assets = runtime->assets;
    set.context.asset_count = runtime->asset_count;
    (void)memcpy(set.context.authorized_from, authority->principal, 32U);
    set.context.actor_sequence = activity->account_sequence;
    set.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    set.context.sequence_account = sequence_account;
    set.context.debit_authority_kind = LXP_AUTH_OWNER;
    set.context.source_authorities = source_authorities;
    set.context.source_authority_count = source_authority_count;
    (void)memset(&receipt, 0, sizeof(receipt));
    status = lxp_ctx_emit_transfer_set(ctx, &set, &receipt);
    if (status != LXP_OK) return status;
    (void)memcpy(transfer_root, receipt.transfer_set_root, 32U);
    return lxp_ctx_emit_event(ctx, LX_PROGRAMS_EVENT_TRANSFERRED,
                              receipt.transfer_set_root, 32U);
}

lxp_result lxp_programs_transfer_execute(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const void *decoded,
    lxp_effect_buffer *effects)
{
    uint8_t transfer_root[32];
    return lxp_programs_transfer_execute_root(ctx, activity, authority,
                                               decoded, effects,
                                               transfer_root);
}
