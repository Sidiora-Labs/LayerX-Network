#include "layerx/lxp_kernel.h"

#include "layerx/lxp_hash.h"

#include <stdint.h>
#include <string.h>

static bool validate_mutates;
static bool execute_fails;
static uint32_t fee_calls;

static lxp_result genesis(lxp_module_ctx *ctx, const uint8_t *bytes, size_t n)
{ (void)ctx; (void)bytes; (void)n; return LXP_OK; }
static lxp_result decode(lxp_module_ctx *ctx, uint16_t ordinal,
                         const uint8_t *bytes, size_t n, void **decoded)
{ (void)ctx; (void)ordinal; (void)n; *decoded = (void *)bytes; return LXP_OK; }
static lxp_result validate(lxp_module_ctx *ctx, const lxp_activity *activity,
                           const lxp_authority_resolved *authority,
                           const void *decoded)
{
    static const uint8_t key[] = "bad";
    static const uint8_t value[] = "write";
    (void)activity; (void)authority; (void)decoded;
    return validate_mutates ? lxp_ctx_kv_put(ctx, key, sizeof(key), value,
                                             sizeof(value)) : LXP_OK;
}
static lxp_result execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                          const lxp_authority_resolved *authority,
                          const void *decoded, lxp_effect_buffer *effects)
{
    static const uint8_t key[] = "state";
    static const uint8_t value[] = "changed";
    (void)activity; (void)authority; (void)decoded; (void)effects;
    if (lxp_ctx_kv_put(ctx, key, sizeof(key), value, sizeof(value)) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return execute_fails ? LXP_ERR_AGREEMENT_STATE : LXP_OK;
}
static lxp_result epoch(lxp_module_ctx *ctx, uint64_t number, uint64_t ts)
{ (void)ctx; (void)number; (void)ts; return LXP_OK; }
static lxp_result root(lxp_module_ctx *ctx, uint8_t out[32])
{ (void)ctx; (void)memset(out, 0, 32U); return LXP_OK; }
static lxp_result prepare_fee(lxp_kernel *kernel,
                              const lxp_activity *activity,
                              const lxp_authority_resolved *authority,
                              lxp_u128 fee,
                              void **transaction)
{
    (void)kernel; (void)activity; (void)authority; (void)fee;
    ++fee_calls;
    *transaction = &fee_calls;
    return LXP_OK;
}
static void commit_fee(lxp_kernel *kernel, void *transaction)
{ (void)kernel; (void)transaction; }
static void rollback_fee(lxp_kernel *kernel, void *transaction)
{ (void)kernel; (void)transaction; --fee_calls; }

static void fill_activity(lxp_activity *activity, const uint8_t *did,
                          size_t did_length, uint64_t sequence)
{
    static const uint8_t payload[] = { 9U };
    static const uint8_t authority[] = { 1U };
    static const uint8_t signature[] = { 1U };
    (void)memset(activity, 0, sizeof(*activity));
    activity->protocol_version = LXP_PROTOCOL_VERSION;
    activity->network_id = 7U;
    activity->activity_type = UINT32_C(0x00010001);
    activity->actor_did = (lxp_byte_span){ did, did_length };
    activity->authority = (lxp_byte_span){ authority, sizeof(authority) };
    activity->account_sequence = sequence;
    activity->timestamp_bound = (lxp_timestamp_bound){ 1U, 100U };
    activity->idempotency_key[31] = (uint8_t)(sequence + 1U);
    activity->fee_limit = (lxp_u128){ 0U, 100U };
    (void)lxp_hash_payload(payload, sizeof(payload), activity->payload_hash);
    activity->payload = (lxp_byte_span){ payload, sizeof(payload) };
    activity->signature = (lxp_byte_span){ signature, sizeof(signature) };
}

int main(void)
{
    static const uint8_t did[] = "did:lxp:dispatch";
    static const uint32_t types[] = { UINT32_C(0x00010001) };
    uint8_t primary_key[32] = { 1U };
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities = { 0 };
    lxp_identity *identity;
    lxp_kernel kernel;
    uint64_t parameters = 1U;
    lxp_module_iface iface = { LXP_MODULE_ASSET, 1U, "asset", types, 1U,
        genesis, decode, validate, execute, epoch, epoch, root, NULL };
    lxp_authority_resolved authority = { { 0 }, { 0 }, LXP_AUTHORITY_OWNER,
                                         { 0 }, NULL, { 0 } };
    lxp_fee_params fee_parameters = { 1U, { 0U, 1U }, { 0U, 0U },
        { 0U, 0U }, { 0U, 0U }, { 0U, 0U }, 10000U };
    lxp_kernel_execution execution;
    lxp_activity activity;
    lxp_receipt receipt;
    size_t module_kv_before;
    if (lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_identity_register(&identities, did, sizeof(did) - 1U,
                              primary_key, &identity) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) !=
            LXP_OK || lxp_kernel_register_module(&kernel, &iface) != LXP_OK ||
        lxp_kernel_set_fee_transaction(
            &kernel, &(lxp_kernel_fee_transaction){ prepare_fee, commit_fee,
                                                    rollback_fee }) != LXP_OK)
        return 1;
    (void)memset(&execution, 0, sizeof(execution));
    execution.network_id = 7U;
    execution.batch_number = 1U;
    execution.batch_timestamp_ms = 10U;
    execution.maximum_timestamp_window = 100U;
    execution.epoch = 0U;
    execution.recorded_module_version = 1U;
    execution.parameter_version = 1U;
    execution.signature_valid = true;
    execution.identities = &identities;
    execution.authority = &authority;
    execution.fee_parameters = &fee_parameters;
    execution.fee_balance = (lxp_u128){ 0U, 1000U };
    execution.gas_limit = 100U;
    execution.arena = &arena;
    fill_activity(&activity, did, sizeof(did) - 1U, 0U);
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    execute_fails = true;
    execution.global_sequence = 0U;
    module_kv_before = kernel.module_kv_count;
    if (lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_ERR_AGREEMENT_STATE ||
        receipt.effects.count != 0U || receipt.module_version != 1U ||
        identity->next_sequence != 1U || state.next_sequence != 1U ||
        kernel.module_kv_count != module_kv_before || fee_calls != 1U ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) ==
            0) return 1;
    fill_activity(&activity, did, sizeof(did) - 1U, 1U);
    if (lxp_arena_reset(&arena, 0U) != LXP_OK) return 1;
    validate_mutates = true;
    execute_fails = false;
    execution.global_sequence = 1U;
    if (lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_FATAL_INVARIANT || identity->next_sequence != 1U ||
        state.next_sequence != 1U || fee_calls != 2U) return 1;
    if (lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
}
