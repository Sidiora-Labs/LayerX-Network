#include "layerx/lxp_kernel.h"

#include <stdint.h>
#include <string.h>

static lxp_result module_genesis(lxp_module_ctx *ctx, const uint8_t *bytes,
                                 size_t length)
{ (void)ctx; (void)bytes; (void)length; return LXP_OK; }
static lxp_result module_decode(lxp_module_ctx *ctx, uint16_t ordinal,
                                const uint8_t *bytes, size_t length,
                                void **decoded)
{ (void)ctx; (void)ordinal; (void)bytes; (void)length; *decoded = NULL;
  return LXP_OK; }
static lxp_result module_validate(lxp_module_ctx *ctx,
                                  const lxp_activity *activity,
                                  const lxp_authority_resolved *authority,
                                  const void *decoded)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; return LXP_OK; }
static lxp_result module_execute(lxp_module_ctx *ctx,
                                 const lxp_activity *activity,
                                 const lxp_authority_resolved *authority,
                                 const void *decoded,
                                 lxp_effect_buffer *effects)
{ (void)ctx; (void)activity; (void)authority; (void)decoded; (void)effects;
  return LXP_OK; }
static lxp_result module_epoch(lxp_module_ctx *ctx, uint64_t epoch,
                               uint64_t timestamp)
{ (void)ctx; (void)epoch; (void)timestamp; return LXP_OK; }
static lxp_result module_root(lxp_module_ctx *ctx, uint8_t root[32])
{ (void)ctx; (void)memset(root, 0, 32U); return LXP_OK; }

static lxp_result read_parameter(const void *parameters, uint32_t id,
                                 uint64_t *value)
{
    const uint64_t *base = parameters;
    *value = *base + id;
    return LXP_OK;
}

static lxp_transfer_set applied_set;
static size_t applied_count;

static lxp_result apply_transfer(lxp_kernel *kernel,
                                 const lxp_transfer_set *set,
                                 lxp_receipt *receipt)
{
    (void)kernel;
    if (set->leg_count == 0U || set->legs[0].from == NULL ||
        set->legs[0].to == NULL)
        return LXP_ERR_NON_CANONICAL;
    applied_set = *set;
    ++applied_count;
    receipt->module_id = set->context.origin_module_id;
    receipt->global_sequence = (uint64_t)set->leg_count;
    receipt->operation = (uint8_t)set->legs[0].reason;
    receipt->amount = set->legs[0].amount;
    (void)memcpy(receipt->asset, set->legs[0].asset_id, 32U);
    (void)memcpy(receipt->from, set->legs[0].from->id, 32U);
    (void)memcpy(receipt->to, set->legs[0].to->id, 32U);
    return LXP_OK;
}

static lxp_result visit(const uint8_t *key, size_t key_length,
                        const uint8_t *value, size_t value_length, void *user)
{
    uint32_t *order = user;
    if (key_length != 1U || value_length != 1U || key[0] != (uint8_t)*order ||
        value[0] != (uint8_t)(key[0] + 10U)) return LXP_FATAL_INVARIANT;
    ++*order;
    return LXP_OK;
}

int main(void)
{
    static const uint32_t types[] = { UINT32_C(0x00010001) };
    lxp_module_iface iface = { LXP_MODULE_ASSET, 1U, "asset", types, 1U,
        module_genesis, module_decode, module_validate, module_execute,
        module_epoch, module_epoch, module_root, NULL };
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx asset;
    lxp_module_ctx preview_ctx;
    uint64_t parameters = 40U;
    uint8_t arena_bytes[128];
    lxp_arena arena;
    uint8_t key_a = 1U;
    uint8_t key_b = 2U;
    uint8_t value_a = 11U;
    uint8_t value_b = 12U;
    const uint8_t *found;
    size_t found_length;
    uint64_t parameter;
    uint32_t order = 1U;
    lx_account from_account;
    lx_account to_account;
    lxp_transfer_asset_state asset_state;
    lxp_transfer_set set;
    lxp_receipt receipt;
    uint8_t state_key[32] = { 0x44U };
    uint8_t preview_value = 13U;
    uint8_t before_root[32];
    uint8_t preview_root[32];
    uint8_t committed_root[32];
    void *allocation;

    (void)memset(&from_account, 0, sizeof(from_account));
    (void)memset(&to_account, 0, sizeof(to_account));
    (void)memset(&asset_state, 0, sizeof(asset_state));
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&receipt, 0, sizeof(receipt));
    (void)memset(&from_account.id, 0xA1, sizeof(from_account.id));
    (void)memset(&to_account.id, 0xB2, sizeof(to_account.id));
    (void)memset(&asset_state.asset_id, 0xC3, sizeof(asset_state.asset_id));
    from_account.balance = (lxp_u128){ 0U, 500U };
    from_account.has_asset = true;
    (void)memcpy(from_account.asset_id, asset_state.asset_id, 32U);
    to_account.has_asset = true;
    (void)memcpy(to_account.asset_id, asset_state.asset_id, 32U);
    asset_state.registered = true;

    set.leg_count = 1U;
    set.legs[0].from = &from_account;
    set.legs[0].to = &to_account;
    (void)memcpy(set.legs[0].asset_id, asset_state.asset_id, 32U);
    set.legs[0].amount = (lxp_u128){ 0U, 77U };
    set.legs[0].reason = LXP_REASON_PAYMENT;
    set.legs[0].supply_mode = LXP_TRANSFER_CONSERVED;
    set.context.assets = &asset_state;
    set.context.asset_count = 1U;
    set.context.sequence_account = &from_account;
    set.context.debit_authority_kind = LXP_AUTH_PROTOCOL_MODULE;
    set.context.protocol_system_capability = true;
    (void)memcpy(set.context.authorized_from, from_account.id, 32U);

    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 3U) !=
            LXP_OK || lxp_kernel_register_module(&kernel, &iface) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, read_parameter,
                                    apply_transfer) != LXP_OK ||
        lxp_module_ctx_init(&asset, &kernel, LXP_MODULE_ASSET, 900U, 3U,
                            8U, 20U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&asset, &key_b, 1U, &value_b, 1U) != LXP_OK ||
        lxp_ctx_kv_put(&asset, &key_a, 1U, &value_a, 1U) != LXP_OK ||
        lxp_ctx_kv_get(&asset, &key_a, 1U, &found, &found_length) != LXP_OK ||
        found_length != 1U || found[0] != value_a ||
        lxp_ctx_kv_iter(&asset, NULL, 0U, visit, &order) != LXP_OK ||
        order != 3U || lxp_module_ctx_commit(&asset) != LXP_OK ||
        lxp_ctx_batch_timestamp_ms(&asset) != 900U ||
        lxp_ctx_epoch(&asset) != 3U || lxp_ctx_global_sequence(&asset) != 8U ||
        lxp_ctx_read_param(&asset, 2U, &parameter) != LXP_OK ||
        parameter != 42U || lxp_ctx_charge_gas(&asset, 15U) != LXP_OK ||
        lxp_ctx_charge_gas(&asset, 6U) != LXP_ERR_GAS_EXHAUSTED ||
        lxp_ctx_arena_alloc(&asset, 8U, 8U, &allocation) != LXP_OK ||
        allocation == NULL ||
        lxp_ctx_emit_transfer_set(&asset, &set, &receipt) != LXP_OK)
        return 1;

    if (applied_count != 1U ||
        applied_set.context.origin_module_id != LXP_MODULE_ASSET ||
        set.context.origin_module_id != 0U || applied_set.leg_count != 1U ||
        applied_set.legs[0].from != &from_account ||
        applied_set.legs[0].to != &to_account ||
        applied_set.legs[0].reason != LXP_REASON_PAYMENT ||
        applied_set.legs[0].supply_mode != LXP_TRANSFER_CONSERVED ||
        memcmp(applied_set.legs[0].asset_id, asset_state.asset_id, 32U) != 0 ||
        applied_set.legs[0].amount.hi != 0U ||
        applied_set.legs[0].amount.lo != 77U ||
        applied_set.context.asset_count != 1U ||
        applied_set.context.assets != &asset_state ||
        applied_set.context.debit_authority_kind != LXP_AUTH_PROTOCOL_MODULE ||
        memcmp(applied_set.context.authorized_from, from_account.id, 32U) != 0)
        return 1;

    if (receipt.module_id != LXP_MODULE_ASSET || receipt.global_sequence != 1U ||
        receipt.operation != (uint8_t)LXP_REASON_PAYMENT ||
        receipt.amount.hi != 0U || receipt.amount.lo != 77U ||
        memcmp(receipt.asset, asset_state.asset_id, 32U) != 0 ||
        memcmp(receipt.from, from_account.id, 32U) != 0 ||
        memcmp(receipt.to, to_account.id, 32U) != 0)
        return 1;

    if (lxp_module_ctx_set_mutable(&asset, false) != LXP_OK ||
        lxp_ctx_kv_put(&asset, &key_a, 1U, &value_a, 1U) !=
            LXP_FATAL_INVARIANT ||
        lxp_ctx_emit_transfer_set(&asset, &set, &receipt) !=
            LXP_ERR_BALANCE_BYPASS ||
        applied_count != 1U ||
        lxp_state_root(&kernel, before_root) != LXP_OK ||
        lxp_state_journal_open(&store, 0U, &journal) != LXP_OK ||
        lxp_state_journal_set(&journal, state_key,
                              (lxp_u128){ 0U, 19U }) != LXP_OK ||
        lxp_module_ctx_init(&preview_ctx, &kernel, LXP_MODULE_ASSET,
                            901U, 3U, 0U, 20U, &arena, true) != LXP_OK ||
        lxp_ctx_kv_put(&preview_ctx, &key_a, 1U, &preview_value, 1U) !=
            LXP_OK ||
        lxp_module_ctx_prepare_commit(&preview_ctx) != LXP_OK ||
        lxp_module_ctx_preview_state_root(&preview_ctx, &journal,
                                          preview_root) != LXP_OK ||
        !journal.open || store.next_sequence != 0U || store.count != 0U ||
        kernel.module_kv[1].value[0] != value_a ||
        memcmp(before_root, preview_root, 32U) == 0 ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&preview_ctx) != LXP_OK ||
        lxp_state_root(&kernel, committed_root) != LXP_OK ||
        memcmp(preview_root, committed_root, 32U) != 0 ||
        lxp_state_store_destroy(&store) != LXP_OK) return 1;
    return 0;
}
