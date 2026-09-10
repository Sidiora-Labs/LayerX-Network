#include "layerx/lx_budget.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_protocol.h"

#include <stdbool.h>
#include <string.h>

static const uint32_t activity_types[] = {
    LX_BUDGET_CREATE, LX_BUDGET_FUND, LX_BUDGET_AMEND,
    LX_BUDGET_DELEGATE_ADD, LX_BUDGET_DELEGATE_REMOVE,
    LX_BUDGET_SPEND, LX_BUDGET_CLOSE
};

typedef union budget_typed_payload {
    lx_budget_create_payload create;
    lx_budget_amount_payload amount;
    lx_budget_amend_payload amend;
    lx_budget_delegate_payload delegate;
    lx_budget_spend_payload spend;
    lx_budget_close_payload close;
} budget_typed_payload;

typedef struct budget_decoded {
    uint16_t ordinal;
    const uint8_t *payload;
    size_t payload_length;
    budget_typed_payload *typed;
} budget_decoded;

static void budget_event_u64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static lxp_result budget_asset_state(lxp_module_ctx *ctx,
                                     const uint8_t asset_id[32],
                                     lxp_transfer_asset_state *state)
{
    lx_asset_runtime *runtime;
    size_t i;
    if (ctx == NULL || ctx->kernel == NULL || asset_id == NULL ||
        state == NULL || ctx->kernel->module_kv_count >
                             LXP_KERNEL_MAX_MODULE_KV)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        lx_asset_record record;
        lxp_result status;
        if (entry->module_id != LXP_MODULE_ASSET || entry->key_length != 38U ||
            memcmp(entry->key, "asset:", 6U) != 0 ||
            memcmp(entry->key + 6U, asset_id, 32U) != 0)
            continue;
        status = lx_asset_record_decode(entry->value, entry->value_length,
                                        &record);
        return status == LXP_OK ? lx_asset_transfer_state(&record, state) :
                                  status;
    }
    runtime = (lx_asset_runtime *)ctx->kernel->module_runtime[LXP_MODULE_ASSET];
    if (runtime == NULL) return LXP_ERR_ASSET_MISMATCH;
    if (runtime->transfer_assets != NULL &&
        runtime->transfer_asset_count <= LX_ASSET_REGISTRY_CAPACITY)
        for (i = 0U; i < runtime->transfer_asset_count; ++i)
            if (memcmp(runtime->transfer_assets[i].asset_id, asset_id,
                       32U) == 0) {
                *state = runtime->transfer_assets[i];
                return LXP_OK;
            }
    if (runtime->assets != NULL &&
        runtime->asset_count <= LX_ASSET_REGISTRY_CAPACITY)
        for (i = 0U; i < runtime->asset_count; ++i)
            if (memcmp(runtime->assets[i].asset_id, asset_id, 32U) == 0)
                return lx_asset_transfer_state(&runtime->assets[i], state);
    return LXP_ERR_ASSET_MISMATCH;
}

static lxp_result budget_load(lxp_module_ctx *ctx, const uint8_t id[32],
                              lx_budget_record *record)
{
    uint8_t key[LX_BUDGET_STATE_KEY_BYTES];
    const uint8_t *bytes;
    size_t length;
    lx_budget_runtime *runtime;
    lx_budget_record *base;
    lxp_result status = lx_budget_state_key(id, key);
    if (status != LXP_OK) return status;
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status == LXP_OK) return lx_budget_record_decode(bytes, length, record);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    runtime = (lx_budget_runtime *)lxp_ctx_module_runtime(ctx);
    if (runtime == NULL || runtime->store == NULL) return LXP_ERR_UNKNOWN_FIELD;
    status = lx_budget_lookup(runtime->store, id, &base);
    if (status != LXP_OK) return status;
    *record = *base;
    return LXP_OK;
}

static lxp_result budget_save(lxp_module_ctx *ctx,
                              const lx_budget_record *record)
{
    uint8_t key[LX_BUDGET_STATE_KEY_BYTES];
    uint8_t bytes[LX_BUDGET_RECORD_MAX_BYTES];
    size_t length;
    lxp_result status = lx_budget_record_encode(record, bytes, sizeof(bytes),
                                                &length);
    if (status == LXP_OK) status = lx_budget_state_key(record->budget_id, key);
    return status == LXP_OK ?
        lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length) : status;
}

static lxp_result budget_capacity(lxp_module_ctx *ctx)
{
    lx_budget_runtime *runtime =
        (lx_budget_runtime *)lxp_ctx_module_runtime(ctx);
    lx_budget_store *store = runtime == NULL ? NULL : runtime->store;
    size_t count = store == NULL ? 0U : store->count;
    size_t i;
    if (count > (size_t)LX_BUDGET_STORE_CAPACITY) return LXP_FATAL_INVARIANT;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        lx_budget_record *existing;
        if (entry->module_id != LXP_MODULE_BUDGET ||
            entry->key_length != (uint16_t)LX_BUDGET_STATE_KEY_BYTES ||
            memcmp(entry->key, "budget:", 7U) != 0)
            continue;
        if (store != NULL &&
            lx_budget_lookup(store, entry->key + 7U, &existing) == LXP_OK)
            continue;
        ++count;
    }
    return count >= (size_t)LX_BUDGET_STORE_CAPACITY ?
        LXP_ERR_ARENA_EXHAUSTED : LXP_OK;
}

static lxp_result budget_transfer(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const uint8_t asset_id[32],
                                  lx_account *from, lx_account *to,
                                  lx_account *sequence_account,
                                  lxp_u128 amount, uint16_t reason,
                                  lxp_authorization_kind authority_kind)
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_transfer_asset_state asset_state;
    lxp_receipt receipt;
    lxp_result status;
    if (from == NULL || to == NULL || sequence_account == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (sequence_account->next_sequence == UINT64_MAX) return LXP_ERR_OVERFLOW;
    status = budget_asset_state(ctx, asset_id, &asset_state);
    if (status != LXP_OK) return status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    (void)memset(&receipt, 0, sizeof(receipt));
    set.leg_count = 1U;
    set.legs[0].from = from;
    set.legs[0].to = to;
    (void)memcpy(set.legs[0].asset_id, asset_id, 32U);
    set.legs[0].amount = amount;
    set.legs[0].reason = reason;
    set.context.assets = &asset_state;
    set.context.asset_count = 1U;
    set.context.sequence_account = sequence_account;
    set.context.actor_sequence = activity->account_sequence;
    set.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    set.context.debit_authority_kind = authority_kind;
    (void)memcpy(set.context.authorized_from, from->id, 32U);
    lx_budget_bind_source_authority(&set, &source);
    return lxp_ctx_emit_transfer_set(ctx, &set, &receipt);
}

static bool budget_name_binds_actor(const lx_account *account,
                                    const lxp_activity *activity,
                                    const char *suffix, size_t suffix_length,
                                    bool exact)
{
    size_t prefix_length;
    size_t bound;
    if (account == NULL || activity == NULL ||
        activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U ||
        activity->actor_did.length > (size_t)LXP_MAX_DID_LENGTH)
        return false;
    prefix_length = 6U + activity->actor_did.length;
    bound = prefix_length + suffix_length;
    if ((size_t)account->name_length < bound ||
        memcmp(account->name, "agent:", 6U) != 0 ||
        memcmp(account->name + 6U, activity->actor_did.bytes,
               activity->actor_did.length) != 0 ||
        memcmp(account->name + prefix_length, suffix, suffix_length) != 0)
        return false;
    return exact ? (size_t)account->name_length == bound :
                   (size_t)account->name_length > bound;
}

static lxp_result budget_authorized_signer(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const budget_decoded *value,
    lx_account **signer)
{
    uint8_t actor[32];
    lxp_result status;
    if (authority == NULL || activity == NULL || value == NULL ||
        value->typed == NULL || authority->kind != LXP_AUTHORITY_OWNER ||
        activity->authority.length != 32U ||
        activity->authority.bytes == NULL ||
        memcmp(authority->verified_key, activity->authority.bytes, 32U) != 0 ||
        activity->activity_type !=
            (((uint32_t)LXP_MODULE_BUDGET << 16U) | value->ordinal))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_did_id_derive(activity->actor_did.bytes,
                               activity->actor_did.length, actor);
    if (status == LXP_OK && memcmp(actor, authority->actor, 32U) != 0)
        status = LXP_ERR_UNAUTHORIZED_DEBIT;
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, authority->principal, signer);
    if (status != LXP_OK) return status;
    if ((*signer)->kind != LX_ACCOUNT_AGENT_MAIN ||
        !budget_name_binds_actor(*signer, activity, ":main", 5U, true) ||
        !(*signer)->has_authority_key ||
        memcmp((*signer)->authority_key, authority->verified_key, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (activity->account_sequence < (*signer)->next_sequence)
        return LXP_ERR_SEQUENCE_REUSED;
    return activity->account_sequence > (*signer)->next_sequence ?
        LXP_ERR_SEQUENCE_GAP : LXP_OK;
}

static lxp_result budget_execute_create(lxp_module_ctx *ctx,
                                        const lxp_activity *activity,
                                        const lx_budget_create_payload *payload,
                                        lx_account *owner)
{
    lx_budget_record record;
    lx_account *budget_account = NULL;
    uint8_t event[80];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = budget_capacity(ctx);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, payload->budget_account,
                                      &budget_account);
    if (status != LXP_OK) return status;
    if (budget_account->kind != LX_ACCOUNT_AGENT_BUDGET ||
        !budget_name_binds_actor(budget_account, activity, ":budget:", 8U,
                                 false))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    (void)memset(&record, 0, sizeof(record));
    (void)memcpy(record.budget_id, payload->budget_id, 32U);
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.budget_account, budget_account->id, 32U);
    (void)memcpy(record.asset_id, payload->asset_id, 32U);
    (void)memcpy(record.purpose_hash, payload->purpose_hash, 32U);
    record.per_period_limit = payload->per_period_limit;
    record.configured_period_limit = payload->per_period_limit;
    record.carry_cap = payload->carry_cap;
    record.period_length = payload->period_length;
    record.period_start = payload->period_start;
    record.expiry = payload->expiry;
    record.revocation_sequence = payload->revocation_sequence;
    record.rollover_policy =
        (lx_budget_rollover_policy)payload->rollover_policy;
    status = lx_budget_record_validate(&record);
    if (status == LXP_OK)
        status = budget_transfer(ctx, activity, record.asset_id, owner,
                                 budget_account, owner, payload->amount,
                                 LXP_REASON_BUDGET_FUND, LXP_AUTH_OWNER);
    if (status == LXP_OK) status = budget_save(ctx, &record);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    (void)memcpy(event + 32U, record.asset_id, 32U);
    status = lxp_u128_to_be(payload->amount, event + 64U);
    return status == LXP_OK ?
        lxp_ctx_emit_event(ctx, 1U, event, sizeof(event)) : status;
}

static lxp_result budget_execute_fund(lxp_module_ctx *ctx,
                                      const lxp_activity *activity,
                                      const lx_budget_amount_payload *payload,
                                      lx_account *owner)
{
    lx_budget_record record;
    lx_account *budget_account;
    uint8_t event[48];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_UNKNOWN_FIELD;
    if (record.revoked) return LXP_ERR_BUDGET_REVOKED;
    if (memcmp(record.owner, owner->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_ctx_account_find(ctx, record.budget_account, &budget_account);
    if (status != LXP_OK) return status;
    status = budget_transfer(ctx, activity, record.asset_id, owner,
                             budget_account, owner, payload->amount,
                             LXP_REASON_BUDGET_FUND, LXP_AUTH_OWNER);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    status = lxp_u128_to_be(payload->amount, event + 32U);
    return status == LXP_OK ?
        lxp_ctx_emit_event(ctx, 2U, event, sizeof(event)) : status;
}

static lxp_result budget_execute_amend(lxp_module_ctx *ctx,
                                       const lx_budget_amend_payload *payload,
                                       lx_account *owner)
{
    lx_budget_record record;
    lxp_u128 limit;
    uint8_t event[56];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_UNKNOWN_FIELD;
    if (record.revoked) return LXP_ERR_BUDGET_REVOKED;
    if (memcmp(record.owner, owner->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_u128_add(payload->per_period_limit, record.carried, &limit);
    if (status != LXP_OK) return status;
    if (lxp_u128_cmp(limit, record.spent_this_period) < 0)
        return LXP_ERR_BUDGET_PERIOD_CAP;
    record.configured_period_limit = payload->per_period_limit;
    record.per_period_limit = limit;
    record.carry_cap = payload->carry_cap;
    record.expiry = payload->expiry;
    record.rollover_policy =
        (lx_budget_rollover_policy)payload->rollover_policy;
    status = lx_budget_record_validate(&record);
    if (status == LXP_OK) status = budget_save(ctx, &record);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    status = lxp_u128_to_be(record.per_period_limit, event + 32U);
    budget_event_u64(event + 48U, record.expiry);
    return status == LXP_OK ?
        lxp_ctx_emit_event(ctx, 3U, event, sizeof(event)) : status;
}

static lxp_result budget_execute_delegate(
    lxp_module_ctx *ctx, uint16_t ordinal,
    const lx_budget_delegate_payload *payload, lx_account *owner)
{
    lx_budget_record record;
    uint8_t event[64];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_UNKNOWN_FIELD;
    if (record.revoked) return LXP_ERR_BUDGET_REVOKED;
    if (memcmp(record.owner, owner->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = ordinal == 4U ?
        lx_budget_delegate_add_execute(&record, payload->delegate) :
        lx_budget_delegate_remove_execute(&record, payload->delegate);
    if (status == LXP_OK) status = budget_save(ctx, &record);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    (void)memcpy(event + 32U, payload->delegate, 32U);
    return lxp_ctx_emit_event(ctx, ordinal, event, sizeof(event));
}

static lxp_result budget_execute_spend(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lxp_authority_resolved *authority,
                                       const lx_budget_spend_payload *payload,
                                       lx_account *signer)
{
    lx_budget_record record;
    lx_account *budget_account = NULL;
    lx_account *recipient = NULL;
    lxp_u128 balance;
    uint8_t event[80];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status != LXP_OK) return status;
    if (memcmp(record.owner, signer->id, 32U) != 0 &&
        !lx_budget_delegate_present(&record, authority->actor))
        return LXP_ERR_UNAUTHORIZED_DELEGATE;
    status = lxp_ctx_account_find(ctx, record.budget_account, &budget_account);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, payload->recipient, &recipient);
    if (status != LXP_OK) return status;
    if (budget_account->kind != LX_ACCOUNT_AGENT_BUDGET ||
        recipient->kind != LX_ACCOUNT_AGENT_MAIN)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_state_balance_get(budget_account, record.asset_id, &balance);
    if (status != LXP_OK) return status;
    status = lx_budget_spend_prepare(&record, lxp_ctx_batch_timestamp_ms(ctx),
                                     balance, payload->amount);
    if (status == LXP_OK)
        status = budget_transfer(ctx, activity, record.asset_id,
                                 budget_account, recipient, signer,
                                 payload->amount, LXP_REASON_BUDGET_SPEND,
                                 LXP_AUTH_BUDGET_ALLOWANCE);
    if (status == LXP_OK) status = budget_save(ctx, &record);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    (void)memcpy(event + 32U, recipient->id, 32U);
    status = lxp_u128_to_be(payload->amount, event + 64U);
    return status == LXP_OK ?
        lxp_ctx_emit_event(ctx, 6U, event, sizeof(event)) : status;
}

static lxp_result budget_execute_close(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lx_budget_close_payload *payload,
                                       lx_account *owner)
{
    lx_budget_record record;
    lx_account *budget_account;
    lxp_u128 balance;
    uint8_t event[56];
    lxp_result status = budget_load(ctx, payload->budget_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_UNKNOWN_FIELD;
    if (memcmp(record.owner, owner->id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    if (payload->revocation_sequence <= record.revocation_sequence)
        return LXP_ERR_STALE_REVOCATION;
    status = lxp_ctx_account_find(ctx, record.budget_account, &budget_account);
    if (status != LXP_OK) return status;
    status = lxp_state_balance_get(budget_account, record.asset_id, &balance);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(balance))
        status = budget_transfer(ctx, activity, record.asset_id,
                                 budget_account, owner, owner, balance,
                                 LXP_REASON_BUDGET_DEFUND,
                                 LXP_AUTH_BUDGET_ALLOWANCE);
    if (status != LXP_OK) return status;
    record.revocation_sequence = payload->revocation_sequence;
    record.closed = true;
    status = budget_save(ctx, &record);
    if (status != LXP_OK) return status;
    (void)memcpy(event, record.budget_id, 32U);
    status = lxp_u128_to_be(balance, event + 32U);
    budget_event_u64(event + 48U, record.revocation_sequence);
    return status == LXP_OK ?
        lxp_ctx_emit_event(ctx, 7U, event, sizeof(event)) : status;
}

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *manifest,
                                 size_t length)
{
    if (ctx == NULL || (manifest == NULL && length != 0U))
        return LXP_ERR_NON_CANONICAL;
    return lxp_ctx_charge_gas(ctx, length);
}

static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *payload, size_t length,
                                void **decoded)
{
    budget_decoded *value;
    void *memory;
    lxp_result status;
    if (ctx == NULL || decoded == NULL || ordinal == 0U || ordinal > 7U ||
        payload == NULL || length == 0U) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value), _Alignof(budget_decoded),
                                 &memory);
    if (status != LXP_OK) return status;
    value = (budget_decoded *)memory;
    (void)memset(value, 0, sizeof(*value));
    value->ordinal = ordinal;
    value->payload = payload;
    value->payload_length = length;
    status = lxp_ctx_arena_alloc(ctx, sizeof(*value->typed),
                                 _Alignof(budget_typed_payload), &memory);
    if (status != LXP_OK) return status;
    value->typed = (budget_typed_payload *)memory;
    (void)memset(value->typed, 0, sizeof(*value->typed));
    switch (ordinal) {
    case 1U:
        status = lx_budget_create_decode(payload, length,
                                         &value->typed->create);
        break;
    case 2U:
        status = lx_budget_amount_decode(payload, length,
                                         &value->typed->amount);
        break;
    case 3U:
        status = lx_budget_amend_decode(payload, length, &value->typed->amend);
        break;
    case 4U:
    case 5U:
        status = lx_budget_delegate_decode(payload, length,
                                           &value->typed->delegate);
        break;
    case 6U:
        status = lx_budget_spend_decode(payload, length,
                                        &value->typed->spend);
        break;
    default:
        status = lx_budget_close_decode(payload, length,
                                        &value->typed->close);
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
    const budget_decoded *value = (const budget_decoded *)decoded;
    if (ctx == NULL || activity == NULL || authority == NULL || value == NULL ||
        value->typed == NULL || value->ordinal == 0U || value->ordinal > 7U)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (activity->activity_type !=
        (((uint32_t)LXP_MODULE_BUDGET << 16U) | value->ordinal))
        return LXP_ERR_UNKNOWN_ACTIVITY;
    return lxp_ctx_charge_gas(ctx, value->payload_length + 1U);
}

static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{
    const budget_decoded *value = (const budget_decoded *)decoded;
    lx_account *signer = NULL;
    lxp_result status;
    (void)effects;
    if (ctx == NULL || value == NULL) return LXP_ERR_UNKNOWN_ACTIVITY;
    status = budget_authorized_signer(ctx, activity, authority, value, &signer);
    if (status != LXP_OK) return status;
    switch (value->ordinal) {
    case 1U:
        return budget_execute_create(ctx, activity, &value->typed->create,
                                     signer);
    case 2U:
        return budget_execute_fund(ctx, activity, &value->typed->amount,
                                   signer);
    case 3U:
        return budget_execute_amend(ctx, &value->typed->amend, signer);
    case 4U:
    case 5U:
        return budget_execute_delegate(ctx, value->ordinal,
                                       &value->typed->delegate, signer);
    case 6U:
        return budget_execute_spend(ctx, activity, authority,
                                    &value->typed->spend, signer);
    case 7U:
        return budget_execute_close(ctx, activity, &value->typed->close,
                                    signer);
    default:
        break;
    }
    return LXP_ERR_UNKNOWN_ACTIVITY;
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
    return lxp_state_subtree_root(ctx->kernel, LXP_MODULE_BUDGET, root);
}

const lxp_module_iface *lx_budget_module_iface(void)
{
    static const lxp_module_iface iface = {
        LXP_MODULE_BUDGET, 1U, "budget", activity_types,
        sizeof(activity_types) / sizeof(activity_types[0]),
        module_genesis, module_decode, module_validate, module_execute,
        lx_budget_epoch_begin, module_epoch_end, module_state_root, NULL
    };
    return &iface;
}
