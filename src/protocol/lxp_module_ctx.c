#include "layerx/lxp_kernel.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_transfer.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_bridge_credit.h"
#include "layerx/programs.h"

#include <limits.h>
#include <stdlib.h>
#include <string.h>

lxp_result lxp_ctx_emit_programs_maintenance_transfer_set(
    lxp_module_ctx *ctx, const lxp_transfer_set *set, lxp_receipt *receipt);

typedef struct kv_view {
    const uint8_t *key;
    size_t key_length;
    const uint8_t *value;
    size_t value_length;
} kv_view;

typedef struct lxp_prepared_account_change {
    lx_account before;
    lx_account after;
} lxp_prepared_account_change;

struct lxp_prepared_module_transition {
    uint16_t module_id;
    uint16_t protocol_version;
    uint64_t epoch;
    uint64_t global_sequence;
    uint64_t batch_number;
    lxp_exec_clock clock;
    uint64_t gas_limit;
    uint8_t activity_id[32];
    uint8_t level_snapshot_token[32];
    lxp_call_admission_facts call_admission;
    lxp_ledger_admission_facts ledger_admission;
    uint64_t gas_used;
    lxp_effect_buffer effects;
    lxp_program_outcome program_outcome;
    lxp_ledger_receipt_input ledger_receipt;
    bool ledger_receipt_present;
    lxp_module_kv_change staged[LXP_MODULE_MAX_STAGED_WRITES];
    bool kv_existed[LXP_MODULE_MAX_STAGED_WRITES];
    uint32_t kv_before_length[LXP_MODULE_MAX_STAGED_WRITES];
    uint8_t kv_before[LXP_MODULE_MAX_STAGED_WRITES]
                     [LXP_MODULE_MAX_VALUE_BYTES];
    size_t staged_count;
    lx_account_registration staged_accounts[
        LXP_MODULE_MAX_STAGED_ACCOUNTS];
    size_t staged_account_count;
    lxp_prepared_account_change accounts[
        LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U];
    size_t account_count;
    lxp_module_blob blobs[LXP_KERNEL_MAX_STAGED_BLOBS];
    size_t blob_count;
};

static lxp_result asset_staged_record(const lxp_module_ctx *ctx,
    const uint8_t id[32], lx_asset_record *record)
{
    uint8_t key[38] = "asset:";
    (void)memcpy(key + 6U, id, 32U);
    for (size_t i = 0U; i < ctx->staged_count; ++i)
        if (ctx->staged[i].key_length == sizeof(key) &&
            memcmp(ctx->staged[i].key, key, sizeof(key)) == 0 &&
            !ctx->staged[i].deleted)
            return lx_asset_record_decode(ctx->staged[i].value,
                ctx->staged[i].value_length, record);
    for (size_t i = 0U; i < ctx->kernel->module_kv_count; ++i)
        if (ctx->kernel->module_kv[i].module_id == LXP_MODULE_ASSET &&
            ctx->kernel->module_kv[i].key_length == sizeof(key) &&
            memcmp(ctx->kernel->module_kv[i].key, key, sizeof(key)) == 0)
            return lx_asset_record_decode(ctx->kernel->module_kv[i].value,
                ctx->kernel->module_kv[i].value_length, record);
    const lx_asset_runtime *runtime = ctx->kernel->module_runtime[LXP_MODULE_ASSET];
    if (runtime != NULL && runtime->asset_count <= LX_ASSET_REGISTRY_CAPACITY)
        for (size_t i = 0U; i < runtime->asset_count; ++i)
            if (memcmp(runtime->assets[i].asset_id, id, 32U) == 0) {
                *record = runtime->assets[i];
                return LXP_OK;
            }
    return LXP_ERR_ASSET_MISMATCH;
}

lxp_result lxp_ctx_bind_asset_supply(lxp_module_ctx *ctx,
    const uint8_t asset_id[32], lxp_u128 before, lxp_u128 after)
{
    lx_asset_record record;
    lxp_receipt check;
    lxp_result status;
    if (ctx == NULL || asset_id == NULL || !ctx->mutable ||
        ctx->module_id != LXP_MODULE_ASSET || !ctx->ledger_admission.bound ||
        memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
        ctx->ledger_receipt.supply_binding_version != 0U)
        return LXP_ERR_NON_CANONICAL;
    status = asset_staged_record(ctx, asset_id, &record);
    if (status != LXP_OK || lxp_u128_cmp(record.total_units, after) != 0)
        return LXP_FATAL_SUPPLY_MISMATCH;
    (void)memset(&check, 0, sizeof(check));
    check.module_id = LXP_MODULE_ASSET;
    check.operation = (uint8_t)lxp_activity_type_ordinal(ctx->ledger_admission.activity_type);
    check.amount = ctx->ledger_receipt.amount;
    check.supply_binding_version = 1U;
    check.total_units_before = before;
    check.total_units_after = after;
    (void)memcpy(check.asset, asset_id, 32U);
    status = lxp_receipt_validate_supply(&check);
    if (status != LXP_OK) return status;
    if (ctx->ledger_receipt_present &&
        memcmp(ctx->ledger_receipt.asset, asset_id, 32U) != 0)
        return LXP_FATAL_SUPPLY_MISMATCH;
    ctx->ledger_receipt.supply_binding_version = 1U;
    ctx->ledger_receipt.total_units_before = before;
    ctx->ledger_receipt.total_units_after = after;
    ctx->ledger_receipt.operation = check.operation;
    (void)memcpy(ctx->ledger_receipt.asset, asset_id, 32U);
    return LXP_OK;
}

static lxp_result commit_account(const lxp_module_ctx *ctx,
                                 lx_account_registry *registry,
                                 const lx_account_registration *registration,
                                 lx_account **account)
{
    if (ctx->module_id == LXP_MODULE_BRIDGE &&
        registration->account.kind == LX_ACCOUNT_AGENT_MAIN)
        return lx_account_credit_registration_commit(registry, registration, account);
    if (ctx->module_id == LXP_MODULE_ASSET) {
        lx_asset_record record;
        if (!ctx->ledger_admission.bound ||
            memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
            asset_staged_record(ctx, registration->account.asset_id, &record) != LXP_OK ||
            record.paused || lx_account_validate_canonical(&registration->account) != LXP_OK)
            return LXP_FATAL_INVARIANT;
        if (registration->account.kind == LX_ACCOUNT_MODULE_VALUE) {
            lxp_u128 initial = lxp_u128_is_zero(record.supply_cap) ?
                (lxp_u128){UINT64_MAX, UINT64_MAX} : record.supply_cap;
            uint8_t name[LX_ASSET_ISSUANCE_NAME_BYTES];
            uint8_t id[32];
            if (lx_asset_issuance_name(record.asset_id, name, id) != LXP_OK ||
                ctx->ledger_admission.activity_type != LX_ASSET_REGISTER ||
                registration->account.name_length != sizeof(name) ||
                memcmp(registration->account.name, name, sizeof(name)) != 0 ||
                memcmp(registration->account.id, id, sizeof(id)) != 0 ||
                registration->account.has_authority_key ||
                registration->account.next_sequence != 0U ||
                !lxp_u128_is_zero(record.total_units) ||
                lxp_u128_cmp(registration->account.balance, initial) != 0)
                return LXP_FATAL_INVARIANT;
        }
    }
    if (ctx->module_id == LXP_MODULE_ASSET &&
        ctx->ledger_admission.bound &&
        ctx->ledger_admission.activity_type == LX_ASSET_ACCOUNT_OPEN &&
        registration->account.kind == LX_ACCOUNT_AGENT_MAIN &&
        lx_account_validate_canonical(&registration->account) == LXP_OK &&
        lxp_u128_is_zero(registration->account.balance) &&
        registration->account.next_sequence == 0U)
        return lx_account_credit_registration_commit(registry, registration, account);
    return lx_account_registration_commit(registry, registration, account);
}

static bool account_equal(const lx_account *left, const lx_account *right)
{
    return memcmp(left->id, right->id, sizeof(left->id)) == 0 &&
           left->name_length == right->name_length &&
           left->name_length <= sizeof(left->name) &&
           memcmp(left->name, right->name, left->name_length) == 0 &&
           left->kind == right->kind &&
           lxp_u128_cmp(left->balance, right->balance) == 0 &&
           left->has_asset == right->has_asset &&
           (!left->has_asset ||
            memcmp(left->asset_id, right->asset_id,
                   sizeof(left->asset_id)) == 0) &&
           left->next_sequence == right->next_sequence &&
           left->created_at_sequence == right->created_at_sequence &&
           left->frozen == right->frozen &&
           left->has_open_reference == right->has_open_reference &&
           left->has_authority_key == right->has_authority_key &&
           (!left->has_authority_key ||
            memcmp(left->authority_key, right->authority_key,
                   sizeof(left->authority_key)) == 0);
}

static bool ledger_admission_equal(const lxp_ledger_admission_facts *left,
                                    const lxp_ledger_admission_facts *right)
{
    return left->bound == right->bound &&
           left->activity_type == right->activity_type &&
           left->account_present == right->account_present &&
           left->next_sequence == right->next_sequence &&
           memcmp(left->account_id, right->account_id, 32U) == 0 &&
           memcmp(left->activity_binding, right->activity_binding, 32U) == 0;
}

static bool call_admission_equal(const lxp_call_admission_facts *left,
                                 const lxp_call_admission_facts *right)
{
    return left->present == right->present &&
           memcmp(left->activity_binding, right->activity_binding, 32U) == 0 &&
           memcmp(left->payer, right->payer, 32U) == 0 &&
           lxp_u128_cmp(left->available_fee_units,
                        right->available_fee_units) == 0 &&
           lxp_u128_cmp(left->signed_fee_limit,
                        right->signed_fee_limit) == 0 &&
           left->fee_schedule_version == right->fee_schedule_version &&
           left->metering_schedule_version ==
               right->metering_schedule_version &&
           memcmp(left->metering_schedule_coefficients,
                  right->metering_schedule_coefficients,
                  sizeof(left->metering_schedule_coefficients)) == 0 &&
           memcmp(left->fee_schedule_prices, right->fee_schedule_prices,
                  sizeof(left->fee_schedule_prices)) == 0 &&
           left->parameter_version == right->parameter_version;
}

static bool effects_are_canonical(uint16_t module_id,
                                  const lxp_effect_buffer *effects)
{
    size_t i;
    if (effects == NULL || effects->count > LXP_MAX_EFFECTS)
        return false;
    for (i = 0U; i < effects->count; ++i)
        if (effects->effects[i].module_id != module_id ||
            effects->effects[i].ordinal != i)
            return false;
    return true;
}

static bool key_equal(const uint8_t *left, size_t left_length,
                      const uint8_t *right, size_t right_length)
{
    return left_length == right_length &&
           memcmp(left, right, left_length) == 0;
}

static int key_compare(const uint8_t *left, size_t left_length,
                       const uint8_t *right, size_t right_length)
{
    size_t common = left_length < right_length ? left_length : right_length;
    int comparison = memcmp(left, right, common);
    if (comparison != 0) return comparison;
    return left_length < right_length ? -1 : left_length != right_length;
}

static size_t committed_find(const lxp_module_ctx *ctx, const uint8_t *key,
                             size_t key_length)
{
    size_t i;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        if (entry->module_id == ctx->module_id &&
            key_equal(entry->key, entry->key_length, key, key_length))
            return i;
    }
    return ctx->kernel->module_kv_count;
}

static size_t staged_find(const lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length)
{
    size_t i;
    for (i = 0U; i < ctx->staged_count; ++i)
        if (key_equal(ctx->staged[i].key, ctx->staged[i].key_length,
                      key, key_length)) return i;
    return ctx->staged_count;
}

static size_t committed_blob_find(const lxp_module_ctx *ctx,
                                  const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < ctx->kernel->blob_count; ++i)
        if (ctx->kernel->blobs[i].module_id == ctx->module_id &&
            memcmp(ctx->kernel->blobs[i].key, key, 32U) == 0) return i;
    return ctx->kernel->blob_count;
}

static size_t staged_blob_find(const lxp_module_ctx *ctx,
                               const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        if (memcmp(ctx->staged_blobs[i].key, key, 32U) == 0) return i;
    return ctx->staged_blob_count;
}

static lxp_result key_check(const uint8_t *key, size_t key_length)
{
    if (key == NULL || key_length == 0U) return LXP_ERR_NON_CANONICAL;
    if (key_length > LXP_MODULE_MAX_KEY_BYTES) return LXP_ERR_LENGTH_LIMIT;
    return LXP_OK;
}

lxp_result lxp_module_ctx_init(lxp_module_ctx *ctx, lxp_kernel *kernel,
                               uint16_t module_id,
                               uint64_t batch_timestamp_ms, uint64_t epoch,
                               uint64_t global_sequence, uint64_t gas_limit,
                               lxp_arena *arena, bool mutable)
{
    const lxp_module_registration *registration;
    if (ctx == NULL || kernel == NULL || arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (lxp_kernel_module_by_id(kernel, module_id, epoch, &registration) !=
        LXP_OK) return LXP_ERR_MODULE_DISABLED;
    (void)registration;
    (void)memset(ctx, 0, sizeof(*ctx));
    ctx->kernel = kernel;
    ctx->module_id = module_id;
    ctx->clock.sealed_timestamp_ms = batch_timestamp_ms;
    ctx->clock.bound = 1U;
    ctx->epoch = epoch;
    ctx->global_sequence = global_sequence;
    ctx->gas_limit = gas_limit;
    ctx->arena = arena;
    ctx->mutable = mutable;
    return LXP_OK;
}

lxp_result lxp_module_ctx_set_mutable(lxp_module_ctx *ctx, bool mutable)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    if (!mutable && (ctx->staged_count != 0U ||
                     ctx->staged_account_count != 0U))
        return LXP_FATAL_INVARIANT;
    ctx->mutable = mutable;
    return LXP_OK;
}

lxp_result lxp_module_ctx_bind_effects(lxp_module_ctx *ctx,
                                       lxp_effect_buffer *effects)
{
    if (ctx == NULL || effects == NULL) return LXP_ERR_NON_CANONICAL;
    ctx->effects = effects;
    ctx->next_effect_ordinal = 0U;
    return LXP_OK;
}

lxp_result lxp_ctx_verified_receipt_facts(
    const lxp_module_ctx *ctx, const uint8_t receipt_digest[32],
    lxp_verified_receipt_facts *facts)
{
    if (ctx == NULL || ctx->verified_receipts == NULL)
        return LXP_ERR_UNKNOWN_FIELD;
    return lxp_verified_receipt_index_lookup(ctx->verified_receipts,
                                              receipt_digest, facts);
}

lxp_result lxp_ctx_kv_get(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t **value,
                          size_t *value_length)
{
    size_t location;
    lxp_result status = key_check(key, key_length);
    if (status != LXP_OK || ctx == NULL || value == NULL ||
        value_length == NULL) return status != LXP_OK ? status :
                                      LXP_ERR_NON_CANONICAL;
    location = staged_find(ctx, key, key_length);
    if (location != ctx->staged_count) {
        if (ctx->staged[location].deleted) return LXP_ERR_UNKNOWN_FIELD;
        *value = ctx->staged[location].value;
        *value_length = ctx->staged[location].value_length;
        return LXP_OK;
    }
    location = committed_find(ctx, key, key_length);
    if (location == ctx->kernel->module_kv_count)
        return LXP_ERR_UNKNOWN_FIELD;
    *value = ctx->kernel->module_kv[location].value;
    *value_length = ctx->kernel->module_kv[location].value_length;
    return LXP_OK;
}

static lxp_result stage_change(lxp_module_ctx *ctx, const uint8_t *key,
                               size_t key_length, const uint8_t *value,
                               size_t value_length, bool deleted)
{
    size_t location;
    lxp_result status = key_check(key, key_length);
    if (status != LXP_OK || ctx == NULL) return status != LXP_OK ? status :
                                               LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable) return LXP_FATAL_INVARIANT;
    if (!deleted && (value == NULL || value_length >
                     LXP_MODULE_MAX_VALUE_BYTES))
        return value == NULL ? LXP_ERR_NON_CANONICAL : LXP_ERR_LENGTH_LIMIT;
    location = staged_find(ctx, key, key_length);
    if (location == ctx->staged_count) {
        if (ctx->staged_reserve > LXP_MODULE_MAX_STAGED_WRITES ||
            ctx->staged_count >= LXP_MODULE_MAX_STAGED_WRITES - ctx->staged_reserve)
            return LXP_ERR_ARENA_EXHAUSTED;
        ++ctx->staged_count;
        (void)memset(&ctx->staged[location], 0, sizeof(ctx->staged[location]));
        ctx->staged[location].key_length = (uint16_t)key_length;
        (void)memcpy(ctx->staged[location].key, key, key_length);
    }
    ctx->staged[location].deleted = deleted;
    ctx->staged[location].value_length = (uint32_t)value_length;
    if (!deleted && value_length != 0U)
        (void)memcpy(ctx->staged[location].value, value, value_length);
    return LXP_OK;
}

lxp_result lxp_ctx_kv_put(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length, const uint8_t *value,
                          size_t value_length)
{
    return stage_change(ctx, key, key_length, value, value_length, false);
}

lxp_result lxp_ctx_kv_del(lxp_module_ctx *ctx, const uint8_t *key,
                          size_t key_length)
{
    return stage_change(ctx, key, key_length, NULL, 0U, true);
}

static lxp_result account_registry_preview(
    const lxp_module_ctx *ctx, lx_account_registry *preview)
{
    lx_account_registry *live;
    size_t i;
    lxp_result status;
    if (ctx == NULL || ctx->kernel == NULL || ctx->kernel->state == NULL ||
        preview == NULL || ctx->staged_account_count >
            LXP_MODULE_MAX_STAGED_ACCOUNTS)
        return LXP_ERR_NON_CANONICAL;
    live = ctx->kernel->state->accounts;
    if (live == NULL || live->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_registry_init(preview);
    if (status != LXP_OK) return status;
    preview->count = live->count;
    (void)memcpy(preview->accounts, live->accounts,
                 live->count * sizeof(live->accounts[0]));
    for (i = 0U; i < ctx->staged_account_count; ++i) {
        lx_account *committed;
        status = commit_account(ctx,
            preview, &ctx->staged_accounts[i], &committed);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

lxp_result lxp_ctx_asset_issuance_stage(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, lx_account **account)
{
    lx_asset_register_payload payload;
    lx_account_registration registration;
    lx_account *existing;
    lxp_u128 initial;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || account == NULL ||
        !ctx->mutable || ctx->module_id != LXP_MODULE_ASSET ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        ctx->kernel->state->accounts == NULL || ctx->kernel->journal == NULL ||
        !ctx->kernel->journal->open || ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence ||
        !ctx->ledger_admission.bound ||
        ctx->ledger_admission.activity_type != LX_ASSET_REGISTER ||
        memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
        activity->activity_type != LX_ASSET_REGISTER ||
        authority->kind != LXP_AUTHORITY_OWNER || activity->authority.length != 32U ||
        memcmp(activity->authority.bytes, authority->verified_key, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK) status = lx_asset_register_decode(
        activity->payload.bytes, activity->payload.length, &payload);
    if (status != LXP_OK) return status;
    if (ctx->staged_account_count >= LXP_MODULE_MAX_STAGED_ACCOUNTS ||
        ctx->kernel->state->accounts->count + ctx->staged_account_count >=
            LX_ACCOUNT_REGISTRY_CAPACITY) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memset(&registration, 0, sizeof(registration));
    status = lx_asset_issuance_name(payload.asset_id, registration.account.name,
                                     registration.account.id);
    registration.account.name_length = LX_ASSET_ISSUANCE_NAME_BYTES;
    registration.account.kind = LX_ACCOUNT_MODULE_VALUE;
    registration.account.created_at_sequence = ctx->global_sequence;
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, registration.account.id, &existing);
    if (status == LXP_OK) return LXP_ERR_ASSET_ALREADY_REGISTERED;
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status;
    initial = lxp_u128_is_zero(payload.supply_cap) ?
        (lxp_u128){ UINT64_MAX, UINT64_MAX } : payload.supply_cap;
    status = lxp_ledger_bootstrap_balance(&registration.account, payload.asset_id,
                                          initial, 0U);
    if (status == LXP_OK) status = lx_account_validate_canonical(&registration.account);
    if (status == LXP_OK) status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    if (status != LXP_OK) return status;
    registration.expected_count = ctx->kernel->state->accounts->count + ctx->staged_account_count;
    ctx->staged_accounts[ctx->staged_account_count] = registration;
    *account = &ctx->staged_accounts[ctx->staged_account_count++].account;
    return LXP_OK;
}

lxp_result lxp_ctx_asset_account_stage(
    lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, lx_account **account)
{
    static const uint8_t hex[] = "0123456789abcdef";
    lx_asset_account_open_payload payload;
    lx_account_registration registration;
    lx_account *existing;
    lx_account *candidate = &registration.account;
    size_t cursor;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || account == NULL ||
        !ctx->mutable || ctx->module_id != LXP_MODULE_ASSET ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        ctx->kernel->state->accounts == NULL || ctx->kernel->journal == NULL ||
        !ctx->kernel->journal->open ||
        ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence ||
        !ctx->ledger_admission.bound ||
        ctx->ledger_admission.activity_type != LX_ASSET_ACCOUNT_OPEN ||
        memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
        activity->activity_type != LX_ASSET_ACCOUNT_OPEN ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        activity->authority.length != 32U ||
        memcmp(activity->authority.bytes, authority->verified_key, 32U) != 0 ||
        activity->actor_did.bytes == NULL || activity->actor_did.length == 0U ||
        activity->actor_did.length > LX_ACCOUNT_NAME_MAX - 77U)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK)
        status = lx_asset_account_open_decode(activity->payload.bytes,
                                               activity->payload.length, &payload);
    if (status != LXP_OK) return status;
    if (ctx->staged_account_count >= LXP_MODULE_MAX_STAGED_ACCOUNTS ||
        ctx->kernel->state->accounts->count + ctx->staged_account_count >=
            LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_ARENA_EXHAUSTED;
    (void)memset(&registration, 0, sizeof(registration));
    (void)memcpy(candidate->name, "agent:", 6U);
    (void)memcpy(candidate->name + 6U, activity->actor_did.bytes,
                 activity->actor_did.length);
    cursor = 6U + activity->actor_did.length;
    (void)memcpy(candidate->name + cursor, ":asset:", 7U);
    cursor += 7U;
    for (size_t i = 0U; i < 32U; ++i) {
        candidate->name[cursor++] = hex[payload.asset_id[i] >> 4U];
        candidate->name[cursor++] = hex[payload.asset_id[i] & 15U];
    }
    candidate->name_length = (uint16_t)cursor;
    candidate->kind = LX_ACCOUNT_AGENT_MAIN;
    candidate->has_asset = true;
    (void)memcpy(candidate->asset_id, payload.asset_id, 32U);
    candidate->has_authority_key = true;
    (void)memcpy(candidate->authority_key, authority->verified_key, 32U);
    candidate->created_at_sequence = ctx->global_sequence;
    status = lx_account_id_from_string(candidate->name, cursor, candidate->id);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, candidate->id, &existing);
    if (status == LXP_OK) return LXP_ERR_CONTEXT_MISMATCH;
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status;
    status = lx_account_validate_canonical(candidate);
    if (status == LXP_OK)
        status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    if (status != LXP_OK) return status;
    registration.expected_count = ctx->kernel->state->accounts->count +
                                  ctx->staged_account_count;
    ctx->staged_accounts[ctx->staged_account_count] = registration;
    *account = &ctx->staged_accounts[ctx->staged_account_count++].account;
    return LXP_OK;
}

lxp_result lxp_ctx_account_stage_module_value(
    lxp_module_ctx *ctx, const uint8_t account_id[32],
    const uint8_t asset_id[32], lx_account **account, bool *created)
{
    const lxp_module_registration *module;
    lx_account_registry *preview;
    lx_account_registration registration;
    lx_account *prepared;
    size_t i;
    lxp_result status;
    if (ctx == NULL || account_id == NULL || asset_id == NULL ||
        account == NULL || created == NULL || !ctx->mutable ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        ctx->kernel->journal == NULL || !ctx->kernel->journal->open ||
        ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->staged_account_count; ++i) {
        lx_account *staged = &ctx->staged_accounts[i].account;
        if (memcmp(staged->id, account_id, 32U) != 0) continue;
        if (!staged->has_asset || memcmp(staged->asset_id, asset_id, 32U) != 0)
            return LXP_ERR_ASSET_MISMATCH;
        *account = staged;
        *created = false;
        return LXP_OK;
    }
    if (ctx->staged_account_count == LXP_MODULE_MAX_STAGED_ACCOUNTS)
        return LXP_ERR_ARENA_EXHAUSTED;
    status = lxp_kernel_module_by_id(ctx->kernel, ctx->module_id, ctx->epoch,
                                     &module);
    if (status != LXP_OK) return status;
    preview = (lx_account_registry *)malloc(sizeof(*preview));
    if (preview == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    status = account_registry_preview(ctx, preview);
    if (status == LXP_OK)
        status = lx_account_module_value_prepare(
            preview, (const uint8_t *)module->name, strlen(module->name),
            account_id, asset_id, ctx->global_sequence, &registration,
            &prepared, created);
    if (status == LXP_OK && !*created) {
        size_t location = (size_t)(prepared - preview->accounts);
        if (location >= preview->count) status = LXP_FATAL_INVARIANT;
        else *account = &ctx->kernel->state->accounts->accounts[location];
    }
    if (status == LXP_OK && *created) {
        size_t location = ctx->staged_account_count++;
        ctx->staged_accounts[location] = registration;
        *account = &ctx->staged_accounts[location].account;
    }
    free(preview);
    if (status != LXP_OK) return status;
    status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    if (status != LXP_OK && *created) --ctx->staged_account_count;
    return status;
}

lxp_result lxp_ctx_account_find(lxp_module_ctx *ctx,
                                const uint8_t account_id[32],
                                lx_account **account)
{
    lx_account_registry *registry;
    size_t i;
    if (ctx == NULL || account_id == NULL || account == NULL ||
        ctx->kernel == NULL || ctx->kernel->state == NULL)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->staged_account_count; ++i)
        if (memcmp(ctx->staged_accounts[i].account.id, account_id, 32U) == 0) {
            *account = &ctx->staged_accounts[i].account;
            return LXP_OK;
        }
    registry = ctx->kernel->state->accounts;
    if (registry == NULL || registry->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < registry->count; ++i)
        if (memcmp(registry->accounts[i].id, account_id, 32U) == 0) {
            *account = &registry->accounts[i];
            return LXP_OK;
        }
    return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
}

lxp_result lxp_module_ctx_commit(lxp_module_ctx *ctx)
{
    size_t i;
    lxp_result status;
    if (ctx == NULL || !ctx->mutable)
        return LXP_FATAL_INVARIANT;
    if (!ctx->commit_prepared) {
        status = lxp_module_ctx_prepare_commit(ctx);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < ctx->staged_account_count; ++i) {
        lx_account *committed;
        status = commit_account(ctx,
            ctx->kernel->state->accounts, &ctx->staged_accounts[i],
            &committed);
        if (status != LXP_OK) return LXP_FATAL_INVARIANT;
    }
    for (i = 0U; i < ctx->staged_count; ++i) {
        lxp_module_kv_change *change = &ctx->staged[i];
        size_t location = committed_find(ctx, change->key,
                                         change->key_length);
        if (change->deleted) {
            if (location != ctx->kernel->module_kv_count) {
                size_t tail = ctx->kernel->module_kv_count - location - 1U;
                if (tail != 0U)
                    (void)memmove(&ctx->kernel->module_kv[location],
                                  &ctx->kernel->module_kv[location + 1U],
                                  tail * sizeof(ctx->kernel->module_kv[0]));
                --ctx->kernel->module_kv_count;
            }
            continue;
        }
        if (location == ctx->kernel->module_kv_count)
            ++ctx->kernel->module_kv_count;
        ctx->kernel->module_kv[location].module_id = ctx->module_id;
        ctx->kernel->module_kv[location].key_length = change->key_length;
        ctx->kernel->module_kv[location].value_length = change->value_length;
        (void)memcpy(ctx->kernel->module_kv[location].key, change->key,
                     change->key_length);
        (void)memcpy(ctx->kernel->module_kv[location].value, change->value,
                     change->value_length);
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i) {
        lxp_module_blob *staged = &ctx->staged_blobs[i];
        size_t location = committed_blob_find(ctx, staged->key);
        if (staged->deleted) {
            if (location != ctx->kernel->blob_count) {
                size_t tail = ctx->kernel->blob_count - location - 1U;
                ctx->kernel->blob_total_bytes -= ctx->kernel->blobs[location].length;
                free(ctx->kernel->blobs[location].bytes);
                if (tail != 0U) (void)memmove(&ctx->kernel->blobs[location],
                    &ctx->kernel->blobs[location + 1U], tail * sizeof(ctx->kernel->blobs[0]));
                --ctx->kernel->blob_count;
            }
            continue;
        }
        if (location == ctx->kernel->blob_count) {
            location = ctx->kernel->blob_count++;
            ctx->kernel->blobs[location] = *staged;
            ctx->kernel->blob_total_bytes += staged->length;
            staged->bytes = NULL;
        }
    }
    ctx->staged_blob_count = 0U;
    (void)memset(&ctx->ledger_receipt, 0, sizeof(ctx->ledger_receipt));
    ctx->ledger_receipt_present = false;
    ctx->staged_count = 0U;
    ctx->staged_account_count = 0U;
    ctx->transfer_snapshot_count = 0U;
    ctx->transfer_applied = false;
    ctx->commit_prepared = false;
    if (ctx->activity_state_release != NULL)
        ctx->activity_state_release(ctx->activity_state);
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
    return LXP_OK;
}

lxp_result lxp_module_ctx_prepare_commit(lxp_module_ctx *ctx)
{
    size_t additions = 0U;
    size_t blob_additions = 0U;
    size_t blob_bytes = 0U;
    size_t i;
    lx_account_registry *account_preview = NULL;
    uint8_t account_root[32];
    lxp_result status;
    if (ctx == NULL || !ctx->mutable || ctx->commit_prepared)
        return LXP_FATAL_INVARIANT;
    for (i = 0U; i < ctx->staged_count; ++i)
        if (!ctx->staged[i].deleted &&
            committed_find(ctx, ctx->staged[i].key,
                           ctx->staged[i].key_length) ==
                ctx->kernel->module_kv_count) ++additions;
    if (additions > LXP_KERNEL_MAX_MODULE_KV - ctx->kernel->module_kv_count)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (ctx->staged_account_count != 0U) {
        account_preview = (lx_account_registry *)malloc(
            sizeof(*account_preview));
        if (account_preview == NULL) return LXP_ERR_ARENA_EXHAUSTED;
        status = account_registry_preview(ctx, account_preview);
        if (status == LXP_OK)
            status = lx_account_registry_root(account_preview, account_root);
#ifdef LXP_TESTING
        if (status == LXP_OK && ctx->module_id == LXP_MODULE_BRIDGE &&
            ctx->bridge_credit_fail_stage == 4U) status = LXP_ERR_IO;
#endif
        free(account_preview);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        if (!ctx->staged_blobs[i].deleted && committed_blob_find(ctx, ctx->staged_blobs[i].key) ==
            ctx->kernel->blob_count) {
            ++blob_additions;
            if (SIZE_MAX - blob_bytes < ctx->staged_blobs[i].length)
                return LXP_ERR_OVERFLOW;
            blob_bytes += ctx->staged_blobs[i].length;
        }
    if (blob_additions > LXP_KERNEL_MAX_BLOBS - ctx->kernel->blob_count ||
        blob_bytes > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES -
                         ctx->kernel->blob_total_bytes)
        return LXP_ERR_ARENA_EXHAUSTED;
    ctx->commit_prepared = true;
    return LXP_OK;
}

static size_t preview_kv_find(const lxp_kernel *preview, uint16_t module_id,
                              const uint8_t *key, size_t key_length)
{
    size_t i;
    for (i = 0U; i < preview->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &preview->module_kv[i];
        if (entry->module_id == module_id &&
            key_equal(entry->key, entry->key_length, key, key_length))
            return i;
    }
    return preview->module_kv_count;
}

static size_t preview_blob_find(const lxp_kernel *preview,
                                uint16_t module_id, const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < preview->blob_count; ++i)
        if (preview->blobs[i].module_id == module_id &&
            memcmp(preview->blobs[i].key, key, 32U) == 0) return i;
    return preview->blob_count;
}

static lxp_result preview_apply_module(const lxp_module_ctx *ctx,
                                       lxp_kernel *preview)
{
    size_t i;
    if (ctx->staged_count > LXP_MODULE_MAX_STAGED_WRITES ||
        ctx->staged_blob_count > LXP_KERNEL_MAX_STAGED_BLOBS ||
        preview->module_kv_count > LXP_KERNEL_MAX_MODULE_KV ||
        preview->blob_count > LXP_KERNEL_MAX_BLOBS ||
        preview->blob_total_bytes > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES)
        return LXP_FATAL_INVARIANT;
    for (i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *change = &ctx->staged[i];
        size_t location = preview_kv_find(
            preview, ctx->module_id, change->key, change->key_length);
        if (change->deleted) {
            if (location != preview->module_kv_count) {
                size_t tail = preview->module_kv_count - location - 1U;
                if (tail != 0U)
                    (void)memmove(&preview->module_kv[location],
                                  &preview->module_kv[location + 1U],
                                  tail * sizeof(preview->module_kv[0]));
                --preview->module_kv_count;
            }
            continue;
        }
        if (location == preview->module_kv_count) {
            if (preview->module_kv_count == LXP_KERNEL_MAX_MODULE_KV)
                return LXP_FATAL_INVARIANT;
            ++preview->module_kv_count;
        }
        preview->module_kv[location].module_id = ctx->module_id;
        preview->module_kv[location].key_length = change->key_length;
        preview->module_kv[location].value_length = change->value_length;
        (void)memcpy(preview->module_kv[location].key, change->key,
                     change->key_length);
        (void)memcpy(preview->module_kv[location].value, change->value,
                     change->value_length);
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i) {
        size_t length = ctx->staged_blobs[i].length;
        size_t location = preview_blob_find(preview, ctx->module_id,
                                            ctx->staged_blobs[i].key);
        if (ctx->staged_blobs[i].deleted) {
            if (location != preview->blob_count) {
                size_t tail = preview->blob_count - location - 1U;
                preview->blob_total_bytes -= preview->blobs[location].length;
                if (tail != 0U) (void)memmove(&preview->blobs[location],
                    &preview->blobs[location + 1U], tail * sizeof(preview->blobs[0]));
                --preview->blob_count;
            }
            continue;
        }
        if (location != preview->blob_count) continue;
        if (preview->blob_count == LXP_KERNEL_MAX_BLOBS ||
            length > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES -
                         preview->blob_total_bytes)
            return LXP_FATAL_INVARIANT;
        preview->blobs[preview->blob_count++] = ctx->staged_blobs[i];
        preview->blob_total_bytes += length;
    }
    return LXP_OK;
}

lxp_result lxp_module_ctx_preview_root(const lxp_module_ctx *ctx,
                                       uint8_t root[32])
{
    lxp_kernel *preview;
    lxp_result status;
    if (ctx == NULL || root == NULL || !ctx->commit_prepared)
        return LXP_FATAL_INVARIANT;
    preview = (lxp_kernel *)malloc(sizeof(*preview));
    if (preview == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *preview = *ctx->kernel;
    status = preview_apply_module(ctx, preview);
    if (status == LXP_OK)
        status = lxp_state_subtree_root(preview, ctx->module_id, root);
    free(preview);
    return status;
}

static size_t preview_state_cell_find(const lxp_state_store *store,
                                      const uint8_t key[32])
{
    size_t i;
    for (i = 0U; i < store->count; ++i)
        if (memcmp(store->cells[i].key, key, 32U) == 0) return i;
    return store->count;
}

static lxp_result preview_apply_journal(const lxp_state_journal *journal,
                                        lxp_state_store *preview)
{
    size_t i;
    lxp_result status;
    if (journal->count > LXP_MAX_TRANSFER_SET_LEGS ||
        journal->store->count > LXP_STATE_MAX_CELLS ||
        journal->store->idempotency_count > LXP_STATE_MAX_IDEMPOTENCY)
        return LXP_FATAL_INVARIANT;
    status = lxp_idempotency_can_commit(journal);
    if (status != LXP_OK) return status;
    for (i = 0U; i < journal->count; ++i) {
        size_t location = preview_state_cell_find(
            preview, journal->staged[i].key);
        if (location == preview->count) {
            if (preview->count == LXP_STATE_MAX_CELLS)
                return LXP_ERR_ARENA_EXHAUSTED;
            ++preview->count;
            (void)memcpy(preview->cells[location].key,
                         journal->staged[i].key, 32U);
        }
        preview->cells[location].value = journal->staged[i].value;
    }
    if (journal->has_idempotency) {
        if (journal->staged_idempotency.receipt_length >
            LXP_STATE_MAX_RECEIPT_BYTES)
            return LXP_FATAL_INVARIANT;
        preview->idempotency[preview->idempotency_count++] =
            journal->staged_idempotency;
    }
    preview->next_sequence = journal->global_sequence + 1U;
    return LXP_OK;
}

lxp_result lxp_module_ctx_preview_state_root(
    const lxp_module_ctx *ctx, const lxp_state_journal *journal,
    uint8_t root[32])
{
    lxp_kernel *preview_kernel;
    lxp_state_store *preview_state;
    lx_account_registry *preview_accounts = NULL;
    lxp_result status;
    lxp_result destroy_status;
    if (ctx == NULL || journal == NULL || root == NULL ||
        !ctx->commit_prepared || !journal->open || journal->store == NULL ||
        ctx->kernel == NULL || ctx->kernel->state != journal->store)
        return LXP_FATAL_INVARIANT;
    status = lxp_state_writer_assert_owner(journal->store);
    if (status != LXP_OK) return status;
    if (ctx->global_sequence != journal->global_sequence)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (journal->global_sequence != journal->store->next_sequence)
        return LXP_ERR_SEQUENCE_GAP;
    if (journal->global_sequence == UINT64_MAX) return LXP_ERR_OVERFLOW;
    preview_kernel = (lxp_kernel *)malloc(sizeof(*preview_kernel));
    preview_state = (lxp_state_store *)malloc(sizeof(*preview_state));
    if (preview_kernel == NULL || preview_state == NULL) {
        free(preview_state);
        free(preview_kernel);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    status = lxp_state_store_init(preview_state,
                                  journal->store->next_sequence);
    if (status != LXP_OK) {
        free(preview_state);
        free(preview_kernel);
        return status;
    }
    preview_state->count = journal->store->count;
    (void)memcpy(preview_state->cells, journal->store->cells,
                 sizeof(preview_state->cells));
    preview_state->idempotency_count = journal->store->idempotency_count;
    (void)memcpy(preview_state->idempotency, journal->store->idempotency,
                 sizeof(preview_state->idempotency));
    if (journal->store->accounts != NULL) {
        preview_accounts = (lx_account_registry *)malloc(
            sizeof(*preview_accounts));
        if (preview_accounts == NULL)
            status = LXP_ERR_ARENA_EXHAUSTED;
        else status = account_registry_preview(ctx, preview_accounts);
    } else if (ctx->staged_account_count != 0U) {
        status = LXP_FATAL_INVARIANT;
    }
    preview_state->accounts = preview_accounts;
    preview_state->account_root_required =
        journal->store->account_root_required;
    *preview_kernel = *ctx->kernel;
    preview_kernel->state = preview_state;
    if (status == LXP_OK)
        status = preview_apply_journal(journal, preview_state);
    if (status == LXP_OK) status = preview_apply_module(ctx, preview_kernel);
    if (status == LXP_OK) status = lxp_state_root(preview_kernel, root);
    destroy_status = lxp_state_store_destroy(preview_state);
    free(preview_accounts);
    free(preview_state);
    free(preview_kernel);
    if (status == LXP_OK && destroy_status != LXP_OK)
        return destroy_status;
    return status;
}

static void restore_transfer_snapshots(lxp_module_ctx *ctx)
{
    size_t i;
    for (i = 0U; i < ctx->transfer_snapshot_count; ++i) {
        lxp_module_account_snapshot *snapshot = &ctx->transfer_snapshots[i];
        (void)lxp_ledger_restore_account_snapshot(
            snapshot->account, snapshot->balance, snapshot->asset_id,
            snapshot->has_asset, snapshot->next_sequence);
    }
    ctx->transfer_snapshot_count = 0U;
    ctx->transfer_applied = false;
}

void lxp_module_ctx_rollback(lxp_module_ctx *ctx)
{
    size_t i;
    if (ctx == NULL) return;
    restore_transfer_snapshots(ctx);
    ctx->commit_prepared = false;
    ctx->staged_count = 0U;
    ctx->staged_account_count = 0U;
    for (i = 0U; i < ctx->staged_blob_count; ++i)
        free(ctx->staged_blobs[i].bytes);
    ctx->staged_blob_count = 0U;
    (void)memset(&ctx->ledger_receipt, 0, sizeof(ctx->ledger_receipt));
    ctx->ledger_receipt_present = false;
    if (ctx->activity_state_release != NULL)
        ctx->activity_state_release(ctx->activity_state);
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
}

lxp_result lxp_ctx_blob_get(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t **bytes, size_t *length)
{
    size_t location;
    if (ctx == NULL || key == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    location = staged_blob_find(ctx, key);
    if (location != ctx->staged_blob_count) {
        if (ctx->staged_blobs[location].deleted) return LXP_ERR_UNKNOWN_FIELD;
        *bytes = ctx->staged_blobs[location].bytes;
        *length = ctx->staged_blobs[location].length;
        return LXP_OK;
    }
    location = committed_blob_find(ctx, key);
    if (location == ctx->kernel->blob_count) return LXP_ERR_UNKNOWN_FIELD;
    *bytes = ctx->kernel->blobs[location].bytes;
    *length = ctx->kernel->blobs[location].length;
    return LXP_OK;
}

lxp_result lxp_ctx_blob_put(lxp_module_ctx *ctx, const uint8_t key[32],
                            const uint8_t *bytes, size_t length)
{
    uint8_t digest[32];
    uint8_t *copy;
    size_t location;
    lxp_result status;
    if (ctx == NULL || key == NULL || bytes == NULL || length == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable) return LXP_FATAL_INVARIANT;
    if (length > LXP_KERNEL_MAX_BLOB_BYTES) return LXP_ERR_LENGTH_LIMIT;
    status = lxp_hash_sha256(bytes, length, digest);
    if (status != LXP_OK) return status;
    if (memcmp(digest, key, 32U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    location = staged_blob_find(ctx, key);
    if (location != ctx->staged_blob_count)
        return !ctx->staged_blobs[location].deleted &&
                       ctx->staged_blobs[location].length == length &&
                       memcmp(ctx->staged_blobs[location].bytes, bytes,
                              length) == 0 ? LXP_OK : LXP_FATAL_INVARIANT;
    location = committed_blob_find(ctx, key);
    if (location != ctx->kernel->blob_count)
        return ctx->kernel->blobs[location].length == length &&
                       memcmp(ctx->kernel->blobs[location].bytes, bytes,
                              length) == 0 ? LXP_OK : LXP_FATAL_INVARIANT;
    if (ctx->staged_blob_count == LXP_KERNEL_MAX_STAGED_BLOBS)
        return LXP_ERR_ARENA_EXHAUSTED;
    copy = (uint8_t *)malloc(length);
    if (copy == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memcpy(copy, bytes, length);
    location = ctx->staged_blob_count++;
    ctx->staged_blobs[location].module_id = ctx->module_id;
    (void)memcpy(ctx->staged_blobs[location].key, key, 32U);
    ctx->staged_blobs[location].length = length;
    ctx->staged_blobs[location].bytes = copy;
    ctx->staged_blobs[location].deleted = false;
    return LXP_OK;
}

lxp_result lxp_ctx_blob_del(lxp_module_ctx *ctx, const uint8_t key[32])
{
    size_t location;
    size_t committed;
    if (ctx == NULL || key == NULL || !ctx->mutable)
        return LXP_ERR_NON_CANONICAL;
    location = staged_blob_find(ctx, key);
    committed = committed_blob_find(ctx, key);
    if (location != ctx->staged_blob_count) {
        if (ctx->staged_blobs[location].deleted) return LXP_OK;
        if (committed == ctx->kernel->blob_count) {
            size_t tail = ctx->staged_blob_count - location - 1U;
            free(ctx->staged_blobs[location].bytes);
            if (tail != 0U)
                (void)memmove(&ctx->staged_blobs[location],
                              &ctx->staged_blobs[location + 1U],
                              tail * sizeof(ctx->staged_blobs[0]));
            --ctx->staged_blob_count;
            (void)memset(&ctx->staged_blobs[ctx->staged_blob_count], 0,
                         sizeof(ctx->staged_blobs[0]));
            return LXP_OK;
        }
    } else {
        if (committed == ctx->kernel->blob_count)
            return LXP_ERR_UNKNOWN_FIELD;
        if (ctx->staged_blob_count == LXP_KERNEL_MAX_STAGED_BLOBS)
            return LXP_ERR_ARENA_EXHAUSTED;
        location = ctx->staged_blob_count++;
        (void)memset(&ctx->staged_blobs[location], 0,
                     sizeof(ctx->staged_blobs[location]));
        ctx->staged_blobs[location].module_id = ctx->module_id;
        (void)memcpy(ctx->staged_blobs[location].key, key, 32U);
    }
    free(ctx->staged_blobs[location].bytes);
    ctx->staged_blobs[location].bytes = NULL;
    ctx->staged_blobs[location].length = 0U;
    ctx->staged_blobs[location].deleted = true;
    return LXP_OK;
}

lxp_result lxp_ctx_bind_activity_state(lxp_module_ctx *ctx, void *state,
                                       lxp_activity_state_release_fn release)
{
    if (ctx == NULL || state == NULL || release == NULL ||
        ctx->activity_state != NULL) return LXP_ERR_NON_CANONICAL;
    ctx->activity_state = state;
    ctx->activity_state_release = release;
    return LXP_OK;
}

void *lxp_ctx_activity_state(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? NULL : ctx->activity_state;
}

void *lxp_ctx_take_activity_state(lxp_module_ctx *ctx)
{
    void *state;
    if (ctx == NULL) return NULL;
    state = ctx->activity_state;
    ctx->activity_state = NULL;
    ctx->activity_state_release = NULL;
    return state;
}

const uint8_t *lxp_ctx_activity_id(const lxp_module_ctx *ctx)
{
    return ctx == NULL || lxp_ct_is_zero(ctx->activity_id, 32U) ? NULL :
           ctx->activity_id;
}

lxp_result lxp_ctx_ledger_execution_sequence(
    lxp_module_ctx *ctx, const uint8_t principal[32],
    uint64_t legacy_sequence, uint64_t *sequence)
{
    const lxp_module_registration *registration;
    const lxp_ledger_admission_facts *facts;
    lx_account *account = NULL;
    lxp_result status;
    if (ctx == NULL || ctx->kernel == NULL || principal == NULL ||
        sequence == NULL)
        return LXP_ERR_NON_CANONICAL;
    *sequence = legacy_sequence;
    if (ctx->module_id != LXP_MODULE_PROGRAMS ||
        ctx->protocol_version != LXP_PROTOCOL_VERSION_STATE_COMMITMENT)
        return LXP_OK;
    status = lxp_kernel_module_by_id(ctx->kernel, ctx->module_id,
                                     ctx->epoch, &registration);
    if (status != LXP_OK) return status;
    if (registration->abi_version != LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION)
        return LXP_OK;
    facts = &ctx->ledger_admission;
    if (facts->activity_type == LX_PROGRAMS_SANDBOX)
        return LXP_OK;
    if (facts->activity_type != LX_PROGRAMS_CALL &&
        facts->activity_type != LX_PROGRAMS_WIND_DOWN)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (!facts->bound ||
        memcmp(facts->activity_binding, ctx->activity_id, 32U) != 0 ||
        memcmp(facts->account_id, principal, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_ctx_account_find(ctx, principal, &account);
    if (status != LXP_OK && status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE)
        return status;
    if (facts->account_present != (account != NULL))
        return LXP_ERR_CONTEXT_MISMATCH;
    if (account == NULL) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    if (account->next_sequence != facts->next_sequence)
        return LXP_ERR_CONTEXT_MISMATCH;
    *sequence = facts->next_sequence;
    return LXP_OK;
}

const lxp_call_admission_facts *lxp_ctx_call_admission(
    const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->call_admission.present ?
           &ctx->call_admission : NULL;
}

static lxp_result outcome_copy_artifacts(lxp_program_outcome *target,
                                          const lxp_program_outcome *source,
                                          lxp_arena *arena)
{
    lxp_program_outcome copy = *source;
    lxp_byte_span *destinations[2] = {
        &copy.terminal_payload, &copy.call_graph_payload};
    const lxp_byte_span sources[2] = {
        source->terminal_payload, source->call_graph_payload};
    size_t index;
    for (index = 0U; index < 2U; ++index) {
        void *bytes = NULL;
        lxp_result status;
        destinations[index]->bytes = NULL;
        if (sources[index].length == 0U) continue;
        if (sources[index].bytes == NULL ||
            sources[index].length > LXP_MAX_ACTIVITY_BYTES)
            return LXP_ERR_LENGTH_LIMIT;
        status = lxp_arena_alloc(arena, sources[index].length, 1U, &bytes);
        if (status != LXP_OK) return status;
        (void)memcpy(bytes, sources[index].bytes, sources[index].length);
        destinations[index]->bytes = bytes;
    }
    *target = copy;
    return LXP_OK;
}

lxp_result lxp_ctx_bind_program_outcome(
    lxp_module_ctx *ctx, const lxp_program_outcome *outcome)
{
    lxp_result status;
    if (ctx == NULL || outcome == NULL || !outcome->present ||
        ctx->module_id != LXP_MODULE_PROGRAMS ||
        ctx->program_outcome.present)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_program_outcome_validate_for_protocol(
        outcome, ctx->protocol_version);
    if (status != LXP_OK) return status;
    if (!ctx->call_admission.present ||
        outcome->fee_schedule_version !=
            ctx->call_admission.fee_schedule_version ||
        outcome->metering_schedule_version !=
            ctx->call_admission.metering_schedule_version)
        return LXP_FATAL_INVARIANT;
    return outcome_copy_artifacts(&ctx->program_outcome, outcome, ctx->arena);
}

const lxp_program_outcome *lxp_ctx_program_outcome(
    const lxp_module_ctx *ctx)
{
    return ctx != NULL && ctx->program_outcome.present ?
           &ctx->program_outcome : NULL;
}

static bool has_prefix(const kv_view *view, const uint8_t *prefix,
                       size_t prefix_length)
{
    return prefix_length <= view->key_length &&
           (prefix_length == 0U ||
            memcmp(view->key, prefix, prefix_length) == 0);
}

lxp_result lxp_ctx_kv_iter(lxp_module_ctx *ctx, const uint8_t *prefix,
                           size_t prefix_length, lxp_kv_visit_fn visit,
                           void *user)
{
    kv_view views[LXP_KERNEL_MAX_MODULE_KV + LXP_MODULE_MAX_STAGED_WRITES];
    size_t count = 0U;
    size_t i;
    if (ctx == NULL || visit == NULL ||
        (prefix == NULL && prefix_length != 0U) ||
        prefix_length > LXP_MODULE_MAX_KEY_BYTES)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        size_t staged;
        if (entry->module_id != ctx->module_id) continue;
        staged = staged_find(ctx, entry->key, entry->key_length);
        if (staged != ctx->staged_count) {
            const lxp_module_kv_change *change = &ctx->staged[staged];
            if (change->deleted) continue;
            views[count++] = (kv_view){ change->key, change->key_length,
                                        change->value,
                                        change->value_length };
        } else {
            views[count++] = (kv_view){ entry->key, entry->key_length,
                                        entry->value,
                                        entry->value_length };
        }
    }
    for (i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *change = &ctx->staged[i];
        if (!change->deleted && committed_find(ctx, change->key,
                                               change->key_length) ==
            ctx->kernel->module_kv_count)
            views[count++] = (kv_view){ change->key, change->key_length,
                                        change->value,
                                        change->value_length };
    }
    for (i = 1U; i < count; ++i) {
        kv_view value = views[i];
        size_t position = i;
        while (position != 0U &&
               key_compare(value.key, value.key_length,
                           views[position - 1U].key,
                           views[position - 1U].key_length) < 0) {
            views[position] = views[position - 1U];
            --position;
        }
        views[position] = value;
    }
    for (i = 0U; i < count; ++i) {
        lxp_result status;
        if (!has_prefix(&views[i], prefix, prefix_length)) continue;
        status = visit(views[i].key, views[i].key_length, views[i].value,
                       views[i].value_length, user);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result emit_transfer_set(lxp_module_ctx *ctx,
                                    const lxp_transfer_set *set,
                                    lxp_receipt *receipt,
                                    bool programs_maintenance)
{
    lxp_transfer_set emitted;
    size_t i;
    if (ctx == NULL || set == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (!ctx->mutable || ctx->kernel->apply_transfer_set == NULL)
        return LXP_ERR_BALANCE_BYPASS;
    if (ctx->transfer_applied && !programs_maintenance)
        return LXP_ERR_BALANCE_BYPASS;
    if (programs_maintenance &&
        (ctx->module_id != LXP_MODULE_PROGRAMS ||
         !set->context.protocol_system_capability))
        return LXP_ERR_BALANCE_BYPASS;
    for (i = 0U; i < set->leg_count; ++i) {
        lx_account *accounts[2] = { set->legs[i].from, set->legs[i].to };
        size_t side;
        for (side = 0U; side < 2U; ++side) {
            size_t prior;
            if (accounts[side] == NULL) return LXP_ERR_NON_CANONICAL;
            for (prior = 0U; prior < ctx->transfer_snapshot_count; ++prior)
                if (ctx->transfer_snapshots[prior].account == accounts[side])
                    break;
            if (prior != ctx->transfer_snapshot_count) continue;
            if (prior == LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U)
                return LXP_ERR_ARENA_EXHAUSTED;
            ctx->transfer_snapshots[prior].account = accounts[side];
            ctx->transfer_snapshots[prior].balance = accounts[side]->balance;
            (void)memcpy(ctx->transfer_snapshots[prior].asset_id,
                         accounts[side]->asset_id, 32U);
            ctx->transfer_snapshots[prior].has_asset = accounts[side]->has_asset;
            ctx->transfer_snapshots[prior].next_sequence =
                accounts[side]->next_sequence;
            ++ctx->transfer_snapshot_count;
        }
    }
    if (set->context.sequence_account != NULL) {
        size_t prior;
        lx_account *account = set->context.sequence_account;
        for (prior = 0U; prior < ctx->transfer_snapshot_count; ++prior)
            if (ctx->transfer_snapshots[prior].account == account) break;
        if (prior == ctx->transfer_snapshot_count) {
            if (prior == LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U)
                return LXP_ERR_ARENA_EXHAUSTED;
            ctx->transfer_snapshots[prior].account = account;
            ctx->transfer_snapshots[prior].balance = account->balance;
            (void)memcpy(ctx->transfer_snapshots[prior].asset_id,
                         account->asset_id, 32U);
            ctx->transfer_snapshots[prior].has_asset = account->has_asset;
            ctx->transfer_snapshots[prior].next_sequence =
                account->next_sequence;
            ++ctx->transfer_snapshot_count;
        }
    }
    emitted = *set;
    emitted.context.origin_module_id = ctx->module_id;
    for (i = 0U; i < emitted.context.source_authority_count; ++i)
        if (emitted.context.source_authorities[i].debit_authority_kind ==
                LXP_AUTH_PROGRAM_SPEND &&
            ctx->module_id != LXP_MODULE_PROGRAMS)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
    {
        lxp_result status = lxp_kernel_apply_transfer_set(ctx->kernel,
                                                          &emitted, receipt);
        if (status != LXP_OK) {
            restore_transfer_snapshots(ctx);
            return status;
        }
    }
    ctx->transfer_applied = true;
    return LXP_OK;
}

lxp_result lxp_ctx_emit_transfer_set(lxp_module_ctx *ctx,
                                     const lxp_transfer_set *set,
                                     lxp_receipt *receipt)
{
    return emit_transfer_set(ctx, set, receipt, false);
}

lxp_result lxp_ctx_emit_monetary_transfer_set(lxp_module_ctx *ctx,
                                              const lxp_transfer_set *set,
                                              lxp_receipt *receipt)
{
    lxp_effect effect;
    lxp_result status;
    if (ctx == NULL || set == NULL || receipt == NULL || ctx->effects == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = emit_transfer_set(ctx, set, receipt, false);
    if (status != LXP_OK) return status;
    if (ctx->next_effect_ordinal == UINT16_MAX) {
        restore_transfer_snapshots(ctx);
        return LXP_ERR_OVERFLOW;
    }
    (void)memset(&effect, 0, sizeof(effect));
    effect.module_id = ctx->module_id;
    effect.ordinal = ctx->next_effect_ordinal;
    effect.kind = LXP_EFFECT_TRANSFER;
    effect.monetary = true;
    (void)memcpy(effect.transfer_set_root, receipt->transfer_set_root, 32U);
    status = lxp_effect_buffer_add(ctx->effects, &effect);
    if (status != LXP_OK) {
        restore_transfer_snapshots(ctx);
        return status;
    }
    ++ctx->next_effect_ordinal;
    return LXP_OK;
}

lxp_result lxp_ctx_bridge_credit(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const lxp_bridge_credit *credit)
{
    lxp_bridge_profile profile;
    const uint8_t *stored;
    size_t stored_length;
    uint8_t nullifier[32];
    uint8_t replay_key[50] = "deposit-nullifier:";
    uint8_t supply_key[47] = "custody-issued:";
    uint8_t supply_bytes[16];
    uint8_t event[208];
    uint8_t balances[112];
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t beneficiary[32];
    size_t name_length;
    lxp_u128 amount;
    lxp_u128 issued = {0U, 0U};
    lxp_u128 total = {0U, 0U};
    lxp_u128 next_issued;
    lxp_u128 next_reserve;
    lxp_u128 recipient_before;
    lx_account *reserve;
    lx_account *recipient = NULL;
    lxp_transfer_asset_state asset;
    lxp_transfer_source_authority source;
    const lx_asset_runtime *runtime;
    const lxp_transfer_asset_state *registered_asset = NULL;
    lxp_transfer_set transfer;
    lxp_receipt receipt;
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || credit == NULL ||
        !ctx->mutable || ctx->module_id != LXP_MODULE_BRIDGE ||
        ctx->protocol_version != 3U || ctx->kernel == NULL ||
        ctx->kernel->state == NULL || ctx->kernel->state->accounts == NULL ||
        ctx->kernel->state->accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY ||
        !ctx->kernel->state->account_root_required ||
        ctx->kernel->journal == NULL || !ctx->kernel->journal->open ||
        ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence ||
        ctx->staged_account_count != 0U || ctx->staged_count != 0U ||
        ctx->transfer_snapshot_count != 0U || ctx->ledger_receipt_present ||
        ctx->transfer_applied || ctx->effects == NULL || ctx->effects->count != 0U ||
        ctx->next_effect_ordinal != 0U ||
        activity->activity_type != LXP_BRIDGE_CREDIT ||
        activity->payload.bytes == NULL || activity->payload.length != sizeof(credit->bytes) ||
        memcmp(activity->payload.bytes, credit->bytes, sizeof(credit->bytes)) != 0 ||
        activity->authority.length != 32U || activity->actor_did.bytes == NULL ||
        activity->actor_did.length == 0U ||
        activity->actor_did.length > sizeof(name) - 11U ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        lxp_ct_memcmp(authority->verified_key, activity->authority.bytes, 32U) != 0 ||
        lxp_ct_memcmp(authority->verified_key, credit->bytes + 139U, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status != LXP_OK) return status;
    runtime = (const lx_asset_runtime *)ctx->kernel->module_runtime[LXP_MODULE_ASSET];
    if (runtime == NULL || runtime->accounts != ctx->kernel->state->accounts ||
        runtime->transfer_assets == NULL || runtime->transfer_asset_count > LX_ASSET_REGISTRY_CAPACITY ||
        runtime->network_id != activity->network_id || runtime->protocol_version != 3U)
        return LXP_ERR_ASSET_MISMATCH;
    for (size_t index = 0U; index < runtime->transfer_asset_count; ++index)
        if (memcmp(runtime->transfer_assets[index].asset_id, credit->bytes + 75U, 32U) == 0) {
            if (registered_asset != NULL) return LXP_ERR_ASSET_MISMATCH;
            registered_asset = &runtime->transfer_assets[index];
        }
    if (registered_asset == NULL || !registered_asset->registered) return LXP_ERR_ASSET_MISMATCH;
    if (registered_asset->paused) return LXP_ERR_ASSET_PAUSED;
    status = lxp_ctx_kv_get(ctx, lxp_bridge_profile_key, 32U, &stored, &stored_length);
    if (status != LXP_OK || stored_length != sizeof(profile.bytes))
        return LXP_ERR_DEPOSIT_PROOF_NOT_FINAL;
    (void)memcpy(profile.bytes, stored, sizeof(profile.bytes));
    status = lxp_bridge_credit_verify(&profile, credit, activity->network_id,
                                      activity->protocol_version, nullifier);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(nullifier, activity->idempotency_key, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    (void)memcpy(replay_key + 18U, nullifier, 32U);
    status = lxp_ctx_kv_get(ctx, replay_key, sizeof(replay_key), &stored, &stored_length);
    if (status == LXP_OK) return LXP_ERR_DEPOSIT_ALREADY_CREDITED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, activity->actor_did.bytes, activity->actor_did.length);
    name_length = 6U + activity->actor_did.length;
    (void)memcpy(name + name_length, ":main", 5U);
    name_length += 5U;
    status = lx_account_id_from_string(name, name_length, beneficiary);
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(beneficiary, credit->bytes + 107U, 32U) != 0 ||
        lxp_ct_memcmp(beneficiary, authority->principal, 32U) != 0)
        return LXP_ERR_ACCOUNT_ID_MISMATCH;
    status = lxp_u128_from_be(credit->bytes + 191U, &amount);
    if (status != LXP_OK) return status;
    (void)memcpy(supply_key + 15U, profile.bytes + 97U, 32U);
    status = lxp_ctx_kv_get(ctx, supply_key, sizeof(supply_key), &stored, &stored_length);
    if (status == LXP_OK) {
        if (stored_length != 16U) return LXP_FATAL_SUPPLY_MISMATCH;
        status = lxp_u128_from_be(stored, &issued);
    }
    if (status != LXP_OK) return status;
    for (size_t index = 0U; index < ctx->kernel->state->accounts->count; ++index) {
        const lx_account *account = &ctx->kernel->state->accounts->accounts[index];
        if (account->has_asset && memcmp(account->asset_id, profile.bytes + 97U, 32U) == 0 &&
            lxp_u128_add(total, account->balance, &total) != LXP_OK)
            return LXP_FATAL_SUPPLY_MISMATCH;
    }
    if (lxp_u128_cmp(total, issued) != 0) return LXP_FATAL_SUPPLY_MISMATCH;
    status = lxp_u128_add(issued, amount, &next_issued);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, profile.bytes + 129U, &reserve);
    if (status != LXP_OK) return status;
    if (reserve->kind != LX_ACCOUNT_SYSTEM_PAXEER_RESERVE || reserve->frozen ||
        !reserve->has_asset || memcmp(reserve->asset_id, profile.bytes + 97U, 32U) != 0 ||
        reserve->next_sequence == UINT64_MAX)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_u128_add(reserve->balance, amount, &next_reserve);
    if (status != LXP_OK) return status;
    status = lxp_ctx_account_find(ctx, beneficiary, &recipient);
    if (status == LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) {
        lx_account_registration *registration = &ctx->staged_accounts[0];
        if (ctx->kernel->state->accounts->count >= LX_ACCOUNT_REGISTRY_CAPACITY)
            return LXP_ERR_ARENA_EXHAUSTED;
        (void)memset(registration, 0, sizeof(*registration));
        registration->expected_count = ctx->kernel->state->accounts->count;
        recipient = &registration->account;
        (void)memcpy(recipient->id, beneficiary, 32U);
        (void)memcpy(recipient->name, name, name_length);
        recipient->name_length = (uint16_t)name_length;
        recipient->kind = LX_ACCOUNT_AGENT_MAIN;
        recipient->has_asset = true;
        (void)memcpy(recipient->asset_id, profile.bytes + 97U, 32U);
        recipient->has_authority_key = true;
        (void)memcpy(recipient->authority_key, authority->verified_key, 32U);
        recipient->created_at_sequence = ctx->global_sequence;
        ctx->staged_account_count = 1U;
        status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    }
    if (status == LXP_OK &&
        (recipient->kind != LX_ACCOUNT_AGENT_MAIN || recipient->frozen ||
         !recipient->has_asset || memcmp(recipient->asset_id, profile.bytes + 97U, 32U) != 0 ||
         !recipient->has_authority_key ||
         memcmp(recipient->authority_key, authority->verified_key, 32U) != 0))
        status = LXP_ERR_UNAUTHORIZED_DEBIT;
    if (status == LXP_OK) status = lxp_u128_to_be(next_issued, supply_bytes);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, supply_key, sizeof(supply_key), supply_bytes, 16U);
    if (status == LXP_OK)
        status = lxp_ctx_kv_put(ctx, replay_key, sizeof(replay_key), credit->bytes,
                                sizeof(credit->bytes));
#ifdef LXP_TESTING
    if (status == LXP_OK && ctx->bridge_credit_fail_stage == 1U) status = LXP_ERR_IO;
#endif
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(ctx);
        return status;
    }
    ctx->transfer_snapshots[0].account = reserve;
    ctx->transfer_snapshots[0].balance = reserve->balance;
    ctx->transfer_snapshots[0].has_asset = reserve->has_asset;
    ctx->transfer_snapshots[0].next_sequence = reserve->next_sequence;
    (void)memcpy(ctx->transfer_snapshots[0].asset_id, reserve->asset_id, 32U);
    ctx->transfer_snapshot_count = 1U;
    status = lxp_ledger_apply_bridge_credit(reserve, profile.bytes + 97U, amount);
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(ctx);
        return status;
    }
#ifdef LXP_TESTING
    if (ctx->bridge_credit_fail_stage == 2U) {
        lxp_module_ctx_rollback(ctx);
        return LXP_ERR_IO;
    }
#endif
    asset = *registered_asset;
    (void)memset(&transfer, 0, sizeof(transfer));
    transfer.leg_count = 1U;
    transfer.legs[0].from = reserve;
    transfer.legs[0].to = recipient;
    transfer.legs[0].amount = amount;
    transfer.legs[0].reason = LXP_REASON_DEPOSIT;
    (void)memcpy(transfer.legs[0].asset_id, asset.asset_id, 32U);
    transfer.context.assets = &asset;
    transfer.context.asset_count = 1U;
    transfer.context.protocol_system_capability = true;
    transfer.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    (void)memset(&source, 0, sizeof(source));
    (void)memcpy(source.authorized_from, reserve->id, 32U);
    source.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    source.protocol_system_capability = true;
    transfer.context.source_authorities = &source;
    transfer.context.source_authority_count = 1U;
    recipient_before = recipient->balance;
    (void)memset(&receipt, 0, sizeof(receipt));
    status = lxp_ctx_emit_monetary_transfer_set(ctx, &transfer, &receipt);
    if (status == LXP_OK) {
        (void)memcpy(event, credit->bytes + 43U, 96U);
        (void)memcpy(event + 96U, credit->bytes + 191U, 16U);
        (void)memcpy(event + 112U, credit->bytes + 5U, 32U);
        status = lxp_hash_sha256(credit->bytes, sizeof(credit->bytes), event + 144U);
        if (status == LXP_OK) status = lxp_u128_to_be(issued, event + 176U);
        if (status == LXP_OK) status = lxp_u128_to_be(next_issued, event + 192U);
    }
    if (status == LXP_OK)
        status = lxp_ctx_emit_event(ctx, 1U, event, sizeof(event));
    if (status == LXP_OK) {
        (void)memcpy(balances, reserve->id, 32U);
        status = lxp_u128_to_be(ctx->transfer_snapshots[0].balance, balances + 32U);
        if (status == LXP_OK) status = lxp_u128_to_be(reserve->balance, balances + 48U);
        if (status == LXP_OK) status = lxp_u128_to_be(recipient_before, balances + 64U);
        if (status == LXP_OK) status = lxp_u128_to_be(recipient->balance, balances + 80U);
        for (size_t index = 0U; index < 8U; ++index) {
            balances[96U + index] = (uint8_t)(ctx->transfer_snapshots[0].next_sequence >> (56U - index * 8U));
            balances[104U + index] = (uint8_t)(reserve->next_sequence >> (56U - index * 8U));
        }
        if (status == LXP_OK)
            status = lxp_ctx_emit_event(ctx, 2U, balances, sizeof(balances));
    }
    if (status == LXP_OK) {
        total = (lxp_u128){0U, 0U};
        for (size_t index = 0U; index < ctx->kernel->state->accounts->count; ++index) {
            const lx_account *account = &ctx->kernel->state->accounts->accounts[index];
            if (account->has_asset && memcmp(account->asset_id, asset.asset_id, 32U) == 0 &&
                lxp_u128_add(total, account->balance, &total) != LXP_OK) {
                status = LXP_FATAL_SUPPLY_MISMATCH;
                break;
            }
        }
        if (status == LXP_OK && ctx->staged_account_count != 0U)
            status = lxp_u128_add(total, recipient->balance, &total);
        if (status == LXP_OK && lxp_u128_cmp(total, next_issued) != 0)
            status = LXP_FATAL_SUPPLY_MISMATCH;
    }
#ifdef LXP_TESTING
    if (status == LXP_OK && ctx->bridge_credit_fail_stage == 3U) status = LXP_ERR_IO;
#endif
    if (status != LXP_OK) lxp_module_ctx_rollback(ctx);
    return status;
}

lxp_result lxp_ctx_bind_ledger_receipt(
    lxp_module_ctx *ctx, const lxp_ledger_receipt_input *input)
{
    lxp_u128 expected_from;
    lxp_u128 expected_to;
    size_t index;
    size_t matching_effects = 0U;
    if (ctx != NULL && input != NULL && input->operation == 4U) {
        const lx_account *account;
        lx_account *owner_account = NULL;
        if (!ctx->mutable || ctx->module_id != LXP_MODULE_ASSET ||
            !ctx->ledger_admission.bound ||
            ctx->ledger_admission.activity_type != LX_ASSET_ACCOUNT_OPEN ||
            memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
            ctx->transfer_applied || ctx->ledger_receipt_present ||
            ctx->staged_account_count != 1U || ctx->effects == NULL ||
            ctx->effects->count != 1U ||
            ctx->effects->effects[0].monetary ||
            ctx->effects->effects[0].module_id != LXP_MODULE_ASSET ||
            input->leg_count != 1U || !lxp_u128_is_zero(input->amount) ||
            lxp_u128_cmp(input->from_balance_before, input->from_balance_after) != 0 ||
            !lxp_u128_is_zero(input->to_balance_before) ||
            !lxp_u128_is_zero(input->to_balance_after) ||
            input->global_sequence != ctx->global_sequence ||
            input->timestamp != lxp_ctx_batch_timestamp_ms(ctx) ||
            memcmp(input->transaction_id, ctx->activity_id, 32U) != 0 ||
            memcmp(input->from, ctx->ledger_admission.account_id, 32U) != 0 ||
            lxp_ct_is_zero(input->authorization_hash, 32U) ||
            lxp_ct_is_zero(input->context_hash, 32U) ||
            memcmp(input->transfer_set_root, input->context_hash, 32U) != 0 ||
            !lxp_ct_is_zero(input->previous_state_root, 32U) ||
            !lxp_ct_is_zero(input->resulting_state_root, 32U) ||
            !lxp_ct_is_zero(input->batch_id, 32U)) return LXP_ERR_NON_CANONICAL;
        if (ctx->ledger_admission.account_present) {
            if (lxp_ctx_account_find(ctx, input->from, &owner_account) != LXP_OK ||
                lxp_u128_cmp(owner_account->balance,
                             input->from_balance_before) != 0 ||
                owner_account->next_sequence != input->from_sequence ||
                input->from_sequence != ctx->ledger_admission.next_sequence)
                return LXP_ERR_NON_CANONICAL;
        } else if (!lxp_u128_is_zero(input->from_balance_before) ||
                   input->from_sequence != 0U ||
                   ctx->ledger_admission.next_sequence != 0U ||
                   lxp_ctx_account_find(ctx, input->from, &owner_account) !=
                       LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE)
            return LXP_ERR_NON_CANONICAL;
        account = &ctx->staged_accounts[0].account;
        if (account->kind != LX_ACCOUNT_AGENT_MAIN ||
            lx_account_validate_canonical(account) != LXP_OK ||
            !lxp_u128_is_zero(account->balance) ||
            memcmp(account->id, input->to, 32U) != 0 ||
            memcmp(account->asset_id, input->asset, 32U) != 0)
            return LXP_ERR_NON_CANONICAL;
        ctx->ledger_receipt = *input;
        ctx->ledger_receipt_present = true;
        return LXP_OK;
    }
    if (ctx == NULL || input == NULL || !ctx->mutable ||
        ctx->module_id != LXP_MODULE_ASSET || !ctx->transfer_applied ||
        ctx->ledger_receipt_present ||
        !ctx->ledger_admission.bound ||
        memcmp(ctx->ledger_admission.activity_binding, ctx->activity_id, 32U) != 0 ||
        ctx->ledger_admission.activity_type !=
            ((uint32_t)LXP_MODULE_ASSET << 16U | input->operation) ||
        (input->operation != 5U && input->operation != 6U &&
         input->operation != 10U && input->operation != 11U) ||
        lxp_u128_is_zero(input->amount) ||
        input->leg_count != 1U || input->global_sequence != ctx->global_sequence ||
        input->timestamp != lxp_ctx_batch_timestamp_ms(ctx) ||
        memcmp(input->transaction_id, ctx->activity_id, 32U) != 0 ||
        lxp_ct_is_zero(input->asset, 32U) ||
        lxp_ct_is_zero(input->from, 32U) || lxp_ct_is_zero(input->to, 32U) ||
        lxp_ct_memcmp(input->from, input->to, 32U) == 0 ||
        lxp_ct_is_zero(input->transfer_set_root, 32U) ||
        lxp_ct_is_zero(input->authorization_hash, 32U) ||
        lxp_ct_is_zero(input->context_hash, 32U) ||
        !lxp_ct_is_zero(input->previous_state_root, 32U) ||
        !lxp_ct_is_zero(input->resulting_state_root, 32U) ||
        !lxp_ct_is_zero(input->batch_id, 32U) ||
        lxp_u128_sub(input->from_balance_before, input->amount,
                     &expected_from) != LXP_OK ||
        lxp_u128_add(input->to_balance_before, input->amount,
                     &expected_to) != LXP_OK ||
        lxp_u128_cmp(expected_from, input->from_balance_after) != 0 ||
        lxp_u128_cmp(expected_to, input->to_balance_after) != 0)
        return LXP_ERR_NON_CANONICAL;
    {
        size_t from_matches = 0U;
        size_t to_matches = 0U;
        for (index = 0U; index < ctx->transfer_snapshot_count; ++index) {
            const lxp_module_account_snapshot *snapshot = &ctx->transfer_snapshots[index];
            const lx_account *account = snapshot->account;
            if (account == NULL || !snapshot->has_asset || !account->has_asset ||
                memcmp(snapshot->asset_id, input->asset, 32U) != 0 ||
                memcmp(account->asset_id, input->asset, 32U) != 0) continue;
            if (memcmp(account->id, input->from, 32U) == 0 &&
                lxp_u128_cmp(snapshot->balance, input->from_balance_before) == 0 &&
                lxp_u128_cmp(account->balance, input->from_balance_after) == 0)
                ++from_matches;
            if (memcmp(account->id, input->to, 32U) == 0 &&
                lxp_u128_cmp(snapshot->balance, input->to_balance_before) == 0 &&
                lxp_u128_cmp(account->balance, input->to_balance_after) == 0)
                ++to_matches;
        }
        if (from_matches != 1U || to_matches != 1U) return LXP_FATAL_INVARIANT;
    }
    if (ctx->effects == NULL || ctx->effects->count != 1U)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < ctx->effects->count; ++index) {
        const lxp_effect *effect = &ctx->effects->effects[index];
        if (effect->module_id == ctx->module_id && effect->monetary &&
            effect->kind == LXP_EFFECT_TRANSFER &&
            lxp_ct_memcmp(effect->transfer_set_root,
                          input->transfer_set_root, 32U) == 0)
            ++matching_effects;
    }
    if (matching_effects != 1U) return LXP_FATAL_INVARIANT;
    ctx->ledger_receipt = *input;
    ctx->ledger_receipt_present = true;
    return LXP_OK;
}

void lxp_prepared_module_transition_destroy(
    lxp_prepared_module_transition *prepared)
{
    size_t i;
    if (prepared == NULL) return;
    free((void *)prepared->program_outcome.terminal_payload.bytes);
    free((void *)prepared->program_outcome.call_graph_payload.bytes);
    for (i = 0U; i < prepared->blob_count; ++i)
        free(prepared->blobs[i].bytes);
    (void)memset(prepared, 0, sizeof(*prepared));
    free(prepared);
}

lxp_result lxp_module_ctx_export_prepared(
    lxp_module_ctx *ctx, const lxp_effect_buffer *effects,
    const uint8_t level_snapshot_token[32],
    lxp_prepared_module_transition **prepared)
{
    lxp_prepared_module_transition *result;
    size_t i;
    lxp_result status;
    bool prepared_here = false;
    if (ctx == NULL || effects == NULL || level_snapshot_token == NULL ||
        lxp_ct_is_zero(level_snapshot_token, 32U) || prepared == NULL ||
        *prepared != NULL || !ctx->mutable || ctx->kernel == NULL ||
        ctx->effects != effects || ctx->next_effect_ordinal != effects->count ||
        !effects_are_canonical(ctx->module_id, effects) ||
        ctx->staged_count > LXP_MODULE_MAX_STAGED_WRITES ||
        ctx->staged_reserve != 0U ||
        ctx->staged_account_count > LXP_MODULE_MAX_STAGED_ACCOUNTS ||
        ctx->transfer_snapshot_count >
            LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U ||
        ctx->staged_blob_count > LXP_KERNEL_MAX_STAGED_BLOBS)
        return LXP_ERR_NON_CANONICAL;
    if (!ctx->commit_prepared) {
        status = lxp_module_ctx_prepare_commit(ctx);
        if (status != LXP_OK) return status;
        prepared_here = true;
    }
    result = (lxp_prepared_module_transition *)calloc(1U, sizeof(*result));
    if (result == NULL) {
        if (prepared_here) ctx->commit_prepared = false;
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    result->module_id = ctx->module_id;
    result->protocol_version = ctx->protocol_version;
    result->epoch = ctx->epoch;
    result->global_sequence = ctx->global_sequence;
    result->batch_number = ctx->batch_number;
    result->clock = ctx->clock;
    result->gas_limit = ctx->gas_limit;
    (void)memcpy(result->activity_id, ctx->activity_id, 32U);
    (void)memcpy(result->level_snapshot_token, level_snapshot_token, 32U);
    result->call_admission = ctx->call_admission;
    result->ledger_admission = ctx->ledger_admission;
    result->gas_used = ctx->gas_used;
    result->effects = *effects;
    result->program_outcome = ctx->program_outcome;
    result->program_outcome.terminal_payload.bytes = NULL;
    result->program_outcome.call_graph_payload.bytes = NULL;
    {
        lxp_byte_span *destinations[2] = {
            &result->program_outcome.terminal_payload,
            &result->program_outcome.call_graph_payload};
        const lxp_byte_span sources[2] = {
            ctx->program_outcome.terminal_payload,
            ctx->program_outcome.call_graph_payload};
        for (i = 0U; i < 2U; ++i) {
            uint8_t *bytes;
            if (sources[i].length == 0U) continue;
            bytes = (uint8_t *)malloc(sources[i].length);
            if (bytes == NULL) {
                lxp_prepared_module_transition_destroy(result);
                return LXP_ERR_ARENA_EXHAUSTED;
            }
            (void)memcpy(bytes, sources[i].bytes, sources[i].length);
            destinations[i]->bytes = bytes;
        }
    }
    result->ledger_receipt = ctx->ledger_receipt;
    result->ledger_receipt_present = ctx->ledger_receipt_present;
    result->staged_count = ctx->staged_count;
    (void)memcpy(result->staged, ctx->staged,
                 ctx->staged_count * sizeof(ctx->staged[0]));
    for (i = 0U; i < ctx->staged_count; ++i) {
        size_t location = committed_find(ctx, ctx->staged[i].key,
                                         ctx->staged[i].key_length);
        if (location == ctx->kernel->module_kv_count) continue;
        result->kv_existed[i] = true;
        result->kv_before_length[i] =
            ctx->kernel->module_kv[location].value_length;
        (void)memcpy(result->kv_before[i],
                     ctx->kernel->module_kv[location].value,
                     result->kv_before_length[i]);
    }
    result->staged_account_count = ctx->staged_account_count;
    (void)memcpy(result->staged_accounts, ctx->staged_accounts,
                 ctx->staged_account_count * sizeof(ctx->staged_accounts[0]));
    for (i = 0U; i < ctx->transfer_snapshot_count; ++i) {
        const lxp_module_account_snapshot *snapshot =
            &ctx->transfer_snapshots[i];
        size_t staged;
        for (staged = 0U; staged < ctx->staged_account_count; ++staged)
            if (snapshot->account == &ctx->staged_accounts[staged].account)
                break;
        if (staged != ctx->staged_account_count) continue;
        if (snapshot->account == NULL) {
            lxp_prepared_module_transition_destroy(result);
            if (prepared_here) ctx->commit_prepared = false;
            return LXP_FATAL_INVARIANT;
        }
        result->accounts[result->account_count].before = *snapshot->account;
        result->accounts[result->account_count].before.balance =
            snapshot->balance;
        (void)memcpy(result->accounts[result->account_count].before.asset_id,
                     snapshot->asset_id, 32U);
        result->accounts[result->account_count].before.has_asset =
            snapshot->has_asset;
        result->accounts[result->account_count].before.next_sequence =
            snapshot->next_sequence;
        result->accounts[result->account_count].after = *snapshot->account;
        ++result->account_count;
    }
    for (i = 0U; i < ctx->staged_blob_count; ++i) {
        uint8_t *copy = NULL;
        if (!ctx->staged_blobs[i].deleted)
            copy = (uint8_t *)malloc(ctx->staged_blobs[i].length);
        if (!ctx->staged_blobs[i].deleted && copy == NULL) {
            lxp_prepared_module_transition_destroy(result);
            if (prepared_here) ctx->commit_prepared = false;
            return LXP_ERR_ARENA_EXHAUSTED;
        }
        if (!ctx->staged_blobs[i].deleted)
            (void)memcpy(copy, ctx->staged_blobs[i].bytes,
                         ctx->staged_blobs[i].length);
        result->blobs[result->blob_count] = ctx->staged_blobs[i];
        result->blobs[result->blob_count].bytes = copy;
        ++result->blob_count;
    }
    *prepared = result;
    return LXP_OK;
}

lxp_result lxp_module_ctx_import_prepared(
    lxp_module_ctx *ctx, const lxp_prepared_module_transition *prepared,
    const uint8_t level_snapshot_token[32], lxp_effect_buffer *effects)
{
    uint8_t *blob_copies[LXP_KERNEL_MAX_STAGED_BLOBS] = { NULL };
    lx_account *accounts[LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U];
    size_t i;
    if (ctx == NULL || prepared == NULL || level_snapshot_token == NULL ||
        lxp_ct_is_zero(level_snapshot_token, 32U) || effects == NULL ||
        !ctx->mutable || ctx->kernel == NULL || ctx->kernel->state == NULL ||
        ctx->effects != effects || effects->count != 0U ||
        ctx->next_effect_ordinal != 0U ||
        ctx->kernel->state->accounts == NULL || ctx->staged_count != 0U ||
        ctx->staged_account_count != 0U || ctx->staged_blob_count != 0U ||
        ctx->transfer_snapshot_count != 0U || ctx->commit_prepared ||
        prepared->module_id != ctx->module_id ||
        prepared->protocol_version != ctx->protocol_version ||
        prepared->epoch != ctx->epoch ||
        prepared->global_sequence != ctx->global_sequence ||
        prepared->batch_number != ctx->batch_number ||
        prepared->clock.sealed_timestamp_ms != ctx->clock.sealed_timestamp_ms ||
        prepared->clock.bound != ctx->clock.bound ||
        prepared->gas_limit != ctx->gas_limit ||
        memcmp(prepared->activity_id, ctx->activity_id, 32U) != 0 ||
        memcmp(prepared->level_snapshot_token,
               level_snapshot_token, 32U) != 0 ||
        !effects_are_canonical(ctx->module_id, &prepared->effects) ||
        prepared->staged_count > LXP_MODULE_MAX_STAGED_WRITES ||
        prepared->staged_account_count > LXP_MODULE_MAX_STAGED_ACCOUNTS ||
        prepared->account_count > LXP_MAX_TRANSFER_SET_LEGS * 2U + 1U ||
        prepared->blob_count > LXP_KERNEL_MAX_STAGED_BLOBS ||
        !ledger_admission_equal(&prepared->ledger_admission,
                                &ctx->ledger_admission) ||
        !call_admission_equal(&prepared->call_admission,
                              &ctx->call_admission))
        return LXP_ERR_CONTEXT_MISMATCH;
    for (i = 0U; i < prepared->staged_count; ++i) {
        size_t location = committed_find(ctx, prepared->staged[i].key,
                                         prepared->staged[i].key_length);
        bool existed = location != ctx->kernel->module_kv_count;
        if (existed != prepared->kv_existed[i] ||
            (existed &&
             (ctx->kernel->module_kv[location].value_length !=
                  prepared->kv_before_length[i] ||
              memcmp(ctx->kernel->module_kv[location].value,
                     prepared->kv_before[i],
                     prepared->kv_before_length[i]) != 0)))
            return LXP_ERR_CONTEXT_MISMATCH;
    }
    for (i = 0U; i < prepared->staged_account_count; ++i) {
        size_t account_index;
        for (account_index = 0U;
             account_index < ctx->kernel->state->accounts->count;
             ++account_index)
            if (memcmp(ctx->kernel->state->accounts->accounts[account_index].id,
                       prepared->staged_accounts[i].account.id, 32U) == 0)
                return LXP_ERR_CONTEXT_MISMATCH;
    }
    for (i = 0U; i < prepared->account_count; ++i) {
        if (lxp_ctx_account_find(ctx, prepared->accounts[i].before.id,
                                 &accounts[i]) != LXP_OK ||
            !account_equal(accounts[i], &prepared->accounts[i].before))
            return LXP_ERR_CONTEXT_MISMATCH;
    }
    {
        lxp_result status = outcome_copy_artifacts(
            &ctx->program_outcome, &prepared->program_outcome, ctx->arena);
        if (status != LXP_OK) return status;
    }
    for (i = 0U; i < prepared->blob_count; ++i) {
        size_t location = committed_blob_find(ctx, prepared->blobs[i].key);
        if ((!prepared->blobs[i].deleted &&
             location != ctx->kernel->blob_count) ||
            (prepared->blobs[i].deleted &&
             location == ctx->kernel->blob_count))
            return LXP_ERR_CONTEXT_MISMATCH;
        if (!prepared->blobs[i].deleted)
            blob_copies[i] = (uint8_t *)malloc(prepared->blobs[i].length);
        if (!prepared->blobs[i].deleted && blob_copies[i] == NULL) {
            while (i != 0U) free(blob_copies[--i]);
            return LXP_ERR_ARENA_EXHAUSTED;
        }
        if (!prepared->blobs[i].deleted)
            (void)memcpy(blob_copies[i], prepared->blobs[i].bytes,
                         prepared->blobs[i].length);
    }
    ctx->staged_count = prepared->staged_count;
    (void)memcpy(ctx->staged, prepared->staged,
                 prepared->staged_count * sizeof(prepared->staged[0]));
    ctx->staged_account_count = prepared->staged_account_count;
    (void)memcpy(ctx->staged_accounts, prepared->staged_accounts,
                 prepared->staged_account_count *
                     sizeof(prepared->staged_accounts[0]));
    for (i = 0U; i < ctx->staged_account_count; ++i)
        ctx->staged_accounts[i].expected_count =
            ctx->kernel->state->accounts->count + i;
    for (i = 0U; i < prepared->account_count; ++i) {
        lxp_module_account_snapshot *snapshot =
            &ctx->transfer_snapshots[ctx->transfer_snapshot_count++];
        snapshot->account = accounts[i];
        snapshot->balance.hi = accounts[i]->balance.hi;
        snapshot->balance.lo = accounts[i]->balance.lo;
        (void)memcpy(snapshot->asset_id, accounts[i]->asset_id, 32U);
        snapshot->has_asset = accounts[i]->has_asset;
        snapshot->next_sequence = accounts[i]->next_sequence;
        *accounts[i] = prepared->accounts[i].after;
    }
    ctx->transfer_applied = prepared->account_count != 0U;
    for (i = 0U; i < prepared->blob_count; ++i) {
        ctx->staged_blobs[i] = prepared->blobs[i];
        ctx->staged_blobs[i].bytes = blob_copies[i];
    }
    ctx->staged_blob_count = prepared->blob_count;
    ctx->gas_used = prepared->gas_used;
    ctx->ledger_receipt = prepared->ledger_receipt;
    ctx->ledger_receipt_present = prepared->ledger_receipt_present;
    {
        lxp_result status = lxp_module_ctx_prepare_commit(ctx);
        if (status != LXP_OK) {
            lxp_module_ctx_rollback(ctx);
            return status;
        }
    }
    *effects = prepared->effects;
    ctx->next_effect_ordinal = (uint16_t)effects->count;
    return LXP_OK;
}

lxp_result lxp_ctx_emit_programs_maintenance_transfer_set(
    lxp_module_ctx *ctx, const lxp_transfer_set *set, lxp_receipt *receipt)
{
    return emit_transfer_set(ctx, set, receipt, true);
}

lxp_result lxp_ctx_emit_event(lxp_module_ctx *ctx, uint16_t event_type,
                              const uint8_t *body, size_t body_length)
{
    lxp_effect effect;
    if (ctx == NULL || ctx->effects == NULL ||
        (body == NULL && body_length != 0U) || body_length > 256U)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(&effect, 0, sizeof(effect));
    effect.module_id = ctx->module_id;
    effect.ordinal = ctx->next_effect_ordinal;
    effect.event_type = event_type;
    effect.kind = LXP_EFFECT_EVENT;
    effect.body_length = (uint16_t)body_length;
    if (body_length != 0U) (void)memcpy(effect.body, body, body_length);
    if (ctx->next_effect_ordinal == UINT16_MAX) return LXP_ERR_OVERFLOW;
    ++ctx->next_effect_ordinal;
    return lxp_effect_buffer_add(ctx->effects, &effect);
}

uint64_t lxp_ctx_batch_timestamp_ms(const lxp_module_ctx *ctx)
{
    uint64_t timestamp_ms = 0U;
    return ctx != NULL && lxp_exec_clock_read(&ctx->clock, &timestamp_ms) ==
           LXP_OK ? timestamp_ms : 0U;
}

uint64_t lxp_ctx_batch_number(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? 0U : ctx->batch_number;
}

uint64_t lxp_ctx_epoch(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? 0U : ctx->epoch;
}

uint64_t lxp_ctx_global_sequence(const lxp_module_ctx *ctx)
{
    return ctx == NULL ? 0U : ctx->global_sequence;
}

lxp_result lxp_ctx_read_param(const lxp_module_ctx *ctx, uint32_t parameter_id,
                              uint64_t *value)
{
    if (ctx == NULL || value == NULL) return LXP_ERR_NON_CANONICAL;
    if (ctx->kernel->read_parameter == NULL) return LXP_ERR_UNKNOWN_FIELD;
    return ctx->kernel->read_parameter(ctx->kernel->parameter_set,
                                       parameter_id, value);
}

lxp_result lxp_ctx_charge_gas(lxp_module_ctx *ctx, uint64_t units)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    if (UINT64_MAX - ctx->gas_used < units ||
        ctx->gas_used + units > ctx->gas_limit) return LXP_ERR_GAS_EXHAUSTED;
    ctx->gas_used += units;
    return LXP_OK;
}

lxp_result lxp_ctx_arena_alloc(lxp_module_ctx *ctx, size_t size,
                               size_t alignment, void **allocation)
{
    if (ctx == NULL) return LXP_ERR_NON_CANONICAL;
    return lxp_arena_alloc(ctx->arena, size, alignment, allocation);
}

lxp_result lxp_module_savepoint_begin(lxp_module_ctx *ctx,
                                      lxp_module_savepoint *savepoint)
{
    if (ctx == NULL || savepoint == NULL || savepoint->active ||
        ctx->effects == NULL || ctx->commit_prepared)
        return LXP_ERR_NON_CANONICAL;
    *savepoint = (lxp_module_savepoint){
        lxp_arena_mark(ctx->arena), ctx->staged_count,
        ctx->staged_account_count, ctx->transfer_snapshot_count,
        ctx->staged_blob_count, ctx->effects->count,
        ctx->next_effect_ordinal, ctx->transfer_applied, true
    };
    return LXP_OK;
}

lxp_result lxp_module_savepoint_discard(lxp_module_ctx *ctx,
                                        lxp_module_savepoint *savepoint)
{
    size_t index;
    lxp_result status;
    if (ctx == NULL || savepoint == NULL || !savepoint->active ||
        ctx->effects == NULL ||
        ctx->staged_account_count != savepoint->staged_account_count ||
        ctx->transfer_snapshot_count != savepoint->transfer_snapshot_count ||
        ctx->transfer_applied != savepoint->transfer_applied)
        return LXP_FATAL_INVARIANT;
    for (index = savepoint->staged_blob_count;
         index < ctx->staged_blob_count; ++index) {
        free(ctx->staged_blobs[index].bytes);
        (void)memset(&ctx->staged_blobs[index], 0,
                     sizeof(ctx->staged_blobs[index]));
    }
    ctx->staged_blob_count = savepoint->staged_blob_count;
    ctx->staged_count = savepoint->staged_count;
    ctx->effects->count = savepoint->effect_count;
    ctx->next_effect_ordinal = savepoint->next_effect_ordinal;
    status = lxp_arena_reset(ctx->arena, savepoint->arena_mark);
    if (status == LXP_OK) savepoint->active = false;
    return status;
}

lxp_result lxp_module_savepoint_accept(lxp_module_ctx *ctx,
                                       lxp_module_savepoint *savepoint)
{
    if (ctx == NULL || savepoint == NULL || !savepoint->active)
        return LXP_ERR_NON_CANONICAL;
    savepoint->active = false;
    return LXP_OK;
}

lxp_result lxp_module_staged_reserve(lxp_module_ctx *ctx, size_t count)
{
    if (ctx == NULL || count == 0U || ctx->commit_prepared ||
        ctx->staged_reserve != 0U || count > LXP_MODULE_MAX_STAGED_WRITES ||
        ctx->staged_count > LXP_MODULE_MAX_STAGED_WRITES - count)
        return LXP_ERR_ARENA_EXHAUSTED;
    ctx->staged_reserve = count;
    return LXP_OK;
}

lxp_result lxp_module_staged_release(lxp_module_ctx *ctx, size_t count)
{
    if (ctx == NULL || count == 0U || ctx->staged_reserve != count)
        return LXP_FATAL_INVARIANT;
    ctx->staged_reserve = 0U;
    return LXP_OK;
}

void *lxp_ctx_module_runtime(const lxp_module_ctx *ctx)
{
    if (ctx == NULL || ctx->kernel == NULL || ctx->module_id == 0U ||
        ctx->module_id > LXP_MODULE_RESERVED_COUNT)
        return NULL;
    return ctx->kernel->module_runtime[ctx->module_id];
}
