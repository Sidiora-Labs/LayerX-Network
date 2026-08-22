#include "layerx/programs.h"

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_hash.h"

#include <string.h>

static size_t charged_fees;
static lxp_u128 last_fee;

static lxp_result apply_transfer_set(lxp_kernel *kernel,
                                     const lxp_transfer_set *set,
                                     lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

static lxp_result charge_fee(lxp_kernel *kernel, const lxp_activity *activity,
                             lxp_u128 fee)
{
    (void)kernel;
    (void)activity;
    ++charged_fees;
    last_fee = fee;
    return LXP_OK;
}

static void write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        bytes[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static void leg(uint8_t *out, const uint8_t from[32], const uint8_t asset[32],
                const uint8_t to[32], uint64_t amount)
{
    (void)memcpy(out, from, 32U);
    (void)memcpy(out + 32U, asset, 32U);
    (void)memcpy(out + 64U, to, 32U);
    write_u64(out + 104U, amount);
}

static int dispatch(lxp_kernel *kernel, lxp_authority_resolved *authority,
                    uint8_t *payload, size_t payload_length,
                    lxp_result expected)
{
    uint8_t arena_bytes[8192];
    lxp_arena arena;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    const lxp_module_registration *registration;
    lxp_result result = LXP_OK;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = LX_PROGRAMS_TRANSFER;
    activity.payload = (lxp_byte_span){payload, payload_length};
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_kernel_module_for_activity(kernel, LX_PROGRAMS_TRANSFER, 0U,
                                       &registration) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_PROGRAMS, 10U, 0U, 1U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK ||
        lxp_kernel_dispatch(registration, &ctx, &activity, authority,
                            &effects, &result) != LXP_OK ||
        result != expected)
        return 1;
    if (result == LXP_OK &&
        (effects.count != 1U || effects.effects[0].module_id != LXP_MODULE_PROGRAMS ||
         effects.effects[0].event_type != LX_PROGRAMS_EVENT_TRANSFERRED ||
         effects.effects[0].body_length != 32U))
        return 1;
    return 0;
}

int main(void)
{
    static const uint8_t actor_did[] = "did:lxp:program-transfer";
    const char *names[3] = {"agent:did:key:a:main", "agent:did:key:b:main",
                            "agent:did:key:c:main"};
    uint8_t ids[3][32];
    lx_account_registry accounts;
    lx_account *opened[3];
    lxp_transfer_asset_state asset = {{1U}, true, false};
    lx_programs_transfer_runtime runtime;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_authority_resolved authority;
    lxp_identity_store identities = {0};
    lxp_identity *identity;
    uint8_t primary_key[32] = {1U};
    static uint8_t execution_arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena execution_arena;
    lxp_kernel_execution execution;
    lxp_fee_params fee_parameters = {1U, {0U, 1U}, {0U, 0U}, {0U, 0U},
                                     {0U, 0U}, {0U, 0U}, 10000U};
    lxp_activity activity;
    lxp_receipt receipt;
    uint8_t zero_root[32] = {0};
    uint8_t payload[258] = {0};
    uint8_t oversized_payload[259];
    uint64_t parameters = 1U;
    size_t i;
    if (lx_account_registry_init(&accounts) != LXP_OK) return 1;
    for (i = 0U; i < 3U; ++i)
        if (lx_account_id_from_string((const uint8_t *)names[i], strlen(names[i]),
                                      ids[i]) != LXP_OK ||
            lx_account_open(&accounts, (const uint8_t *)names[i], strlen(names[i]),
                            ids[i], 1U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                            &opened[i]) != LXP_OK)
            return 1;
    if (lxp_ledger_bootstrap_balance(opened[0], asset.asset_id,
                                     (lxp_u128){0U, 100U}, 1U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened[1], asset.asset_id,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened[2], asset.asset_id,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK)
        return 1;
    runtime = (lx_programs_transfer_runtime){&accounts, &asset, 1U};
    (void)memset(&authority, 0, sizeof(authority));
    (void)memcpy(authority.principal, ids[0], 32U);
    (void)memset(authority.authority_hash, 0x55, 32U);
    (void)memset(payload, 0x77, 32U);
    write_u16(payload + 32U, 2U);
    leg(payload + 34U, ids[0], asset.asset_id, ids[1], 30U);
    leg(payload + 146U, ids[0], asset.asset_id, ids[2], 80U);
    if (lxp_state_store_init(&state, 1U) != LXP_OK ||
        lxp_identity_register(&identities, actor_did, sizeof(actor_did) - 1U,
                              primary_key, &identity) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) != LXP_OK ||
        lxp_kernel_set_fee_charger(&kernel, charge_fee) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_transfer_set) != LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS, &runtime) != LXP_OK ||
        dispatch(&kernel, &authority, payload, sizeof(payload),
                 LXP_ERR_INSUFFICIENT_BALANCE) != 0 ||
        opened[0]->balance.lo != 100U || opened[1]->balance.lo != 0U ||
        opened[2]->balance.lo != 0U)
        return 1;
    write_u64(payload + 250U, 20U);
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION;
    activity.network_id = 7U;
    activity.activity_type = LX_PROGRAMS_TRANSFER;
    activity.actor_did = (lxp_byte_span){actor_did, sizeof(actor_did) - 1U};
    activity.authority = (lxp_byte_span){primary_key, sizeof(primary_key)};
    activity.timestamp_bound = (lxp_timestamp_bound){1U, 100U};
    activity.idempotency_key[31] = 1U;
    activity.fee_limit = (lxp_u128){0U, 1U};
    activity.payload = (lxp_byte_span){payload, sizeof(payload)};
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK ||
        lxp_arena_init(&execution_arena, execution_arena_bytes,
                       sizeof(execution_arena_bytes)) != LXP_OK)
        return 1;
    (void)memset(&execution, 0, sizeof(execution));
    execution.network_id = 7U;
    execution.batch_timestamp_ms = 10U;
    execution.maximum_timestamp_window = 100U;
    execution.global_sequence = 1U;
    execution.recorded_module_version = LX_PROGRAMS_ABI_VERSION;
    execution.parameter_version = 1U;
    execution.signature_valid = true;
    execution.identities = &identities;
    execution.authority = &authority;
    execution.fee_parameters = &fee_parameters;
    execution.fee_balance = (lxp_u128){0U, 1U};
    execution.gas_limit = 100000U;
    execution.arena = &execution_arena;
    if (lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) != LXP_OK ||
        receipt.result_code != LXP_OK || receipt.module_id != LXP_MODULE_PROGRAMS ||
        receipt.module_version != LX_PROGRAMS_ABI_VERSION ||
        receipt.effects.count != 1U ||
        receipt.effects.effects[0].event_type != LX_PROGRAMS_EVENT_TRANSFERRED ||
        receipt.effects.effects[0].body_length != 32U ||
        memcmp(receipt.effects.effects[0].body, zero_root, 32U) == 0 ||
        charged_fees != 1U || last_fee.hi != 0U || last_fee.lo != 1U ||
        identity->next_sequence != 1U ||
        opened[0]->balance.lo != 50U || opened[1]->balance.lo != 30U ||
        opened[2]->balance.lo != 20U)
        return 1;
    write_u64(payload + 250U, 80U);
    activity.account_sequence = 1U;
    activity.idempotency_key[31] = 2U;
    activity.payload_hash[0] = 0U;
    if (lxp_hash_payload(payload, sizeof(payload), activity.payload_hash) != LXP_OK)
        return 1;
    execution.global_sequence = 2U;
    if (lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK ||
        receipt.result_code != LXP_ERR_INSUFFICIENT_BALANCE ||
        receipt.module_id != LXP_MODULE_PROGRAMS ||
        receipt.module_version != LX_PROGRAMS_ABI_VERSION ||
        receipt.effects.count != 0U ||
        receipt.fee_charged.hi != 0U || receipt.fee_charged.lo != 1U ||
        charged_fees != 2U || last_fee.hi != 0U || last_fee.lo != 1U ||
        identity->next_sequence != 2U || state.next_sequence != 3U ||
        opened[0]->balance.lo != 50U || opened[1]->balance.lo != 30U ||
        opened[2]->balance.lo != 20U)
        return 1;
    write_u64(payload + 250U, 20U);
    (void)memcpy(oversized_payload, payload, sizeof(payload));
    oversized_payload[sizeof(payload)] = 0U;
    if (dispatch(&kernel, &authority, oversized_payload,
                 sizeof(oversized_payload), LXP_ERR_NON_CANONICAL) != 0 ||
        opened[0]->balance.lo != 50U || opened[1]->balance.lo != 30U ||
        opened[2]->balance.lo != 20U)
        return 1;
    payload[34] ^= 1U;
    if (dispatch(&kernel, &authority, payload, sizeof(payload),
                 LXP_ERR_AUTH_SCOPE) != 0 || opened[0]->balance.lo != 50U)
        return 1;
    return lxp_state_store_destroy(&state) == LXP_OK ? 0 : 1;
}
