#include "layerx/lxp_kernel.h"

#include "layerx/lx_batch.h"
#include "layerx/lx_web.h"
#include "layerx/lxp_hash.h"
#include "layerx/programs.h"

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

static const uint8_t web_attestor_key[] = "web/attestors";

/* One web activity through dispatch inside its own journal: committed when
 * asked and the module accepted it, rolled back otherwise. */
static lxp_result web_dispatch(lxp_kernel *kernel, uint16_t context_module,
                               const lxp_activity *activity,
                               const lxp_authority_resolved *authority,
                               bool commit, lxp_result *module_result)
{
    static uint8_t arena_bytes[65536];
    const lxp_module_registration *registration;
    lxp_arena arena;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_result status;
    *module_result = LXP_FATAL_INVARIANT;
    status = lxp_kernel_module_for_activity(kernel, activity->activity_type,
                                            kernel->epoch, &registration);
    if (status != LXP_OK) return status;
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_state_journal_open(kernel->state, kernel->state->next_sequence,
                               kernel->journal) != LXP_OK ||
        lxp_module_ctx_init(&ctx, kernel, context_module, 10U, kernel->epoch,
                            kernel->state->next_sequence, 100000U, &arena,
                            true) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    if (lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    status = lxp_kernel_dispatch(registration, &ctx, activity, authority,
                                 &effects, module_result);
    if (commit && status == LXP_OK && *module_result == LXP_OK) {
        if (lxp_module_ctx_prepare_commit(&ctx) != LXP_OK ||
            lxp_state_journal_commit(kernel->journal) != LXP_OK ||
            lxp_module_ctx_commit(&ctx) != LXP_OK)
            return LXP_FATAL_INVARIANT;
        return status;
    }
    lxp_module_ctx_rollback(&ctx);
    if (lxp_state_journal_rollback(kernel->journal) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return status;
}

static void web_activity(lxp_activity *activity, uint32_t activity_type,
                         const uint8_t *payload, size_t length)
{
    static const uint8_t did[] = "did:lxp:web-dispatch";
    (void)memset(activity, 0, sizeof(*activity));
    activity->protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity->network_id = 7U;
    activity->activity_type = activity_type;
    activity->actor_did = (lxp_byte_span){ did, sizeof(did) - 1U };
    activity->payload = (lxp_byte_span){ payload, length };
    (void)lxp_hash_payload(payload, length, activity->payload_hash);
}

static const lxp_module_kv_entry *web_attestor_entry(const lxp_kernel *kernel)
{
    size_t index;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (entry->module_id == LXP_MODULE_PROGRAMS &&
            entry->key_length == sizeof(web_attestor_key) - 1U &&
            memcmp(entry->key, web_attestor_key, entry->key_length) == 0)
            return entry;
    }
    return NULL;
}

/* The web module registers beside the programs module: both web activities
 * resolve to it, the attestor set and the observation run through dispatch
 * in the Programs module context, and module 12 stays unknown. */
static int web_dispatch_checks(void)
{
    static const uint32_t twelve_types[] = { UINT32_C(0x000C0001) };
    static lxp_kernel kernel;
    static lxp_state_store state;
    static lxp_state_journal journal;
    static lx_web_store web;
    static lx_web_attestor_set set;
    static lx_web_observation observation;
    static uint8_t observation_bytes[LX_WEB_OBSERVATION_MAX_BYTES];
    static uint8_t root_arena_bytes[65536];
    static lx_batch_header header;
    uint8_t set_bytes[LX_WEB_ATTESTOR_SET_MAX_BYTES];
    uint8_t web_root[32];
    size_t set_length;
    size_t observation_length;
    size_t index;
    uint64_t parameters = 1U;
    lxp_module_iface twelve = { LXP_MODULE_RESERVED_COUNT + 1U, 1U, "twelve",
        twelve_types, 1U, genesis, decode, validate, execute, epoch, epoch,
        root, NULL };
    lxp_authority_resolved authority = { { 0 }, { 0 }, LXP_AUTHORITY_OWNER,
                                         { 0 }, NULL, { 0 }, { 0 } };
    const lxp_module_registration *observation_registration;
    const lxp_module_registration *attestor_registration;
    const lxp_module_registration *registration;
    const lxp_module_kv_entry *entry;
    lxp_activity activity;
    lxp_arena root_arena;
    lxp_result module_result;
    if (lxp_state_store_init(&state, 1U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   programs_module_registration_v4()) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_web_module_iface()) != LXP_OK ||
        lxp_kernel_register_module(&kernel, &twelve) != LXP_ERR_UNKNOWN_MODULE)
        return 1;
    if (lxp_kernel_module_for_activity(&kernel, LX_WEB_OBSERVATION_ACTIVITY, 0U,
                                       &observation_registration) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_WEB_ATTESTOR_SET_ACTIVITY,
                                       0U, &attestor_registration) != LXP_OK ||
        observation_registration != attestor_registration ||
        observation_registration->module_id != LXP_MODULE_WEB ||
        lxp_kernel_module_for_activity(&kernel, UINT32_C(0x000B0003), 0U,
                                       &registration) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        lxp_kernel_module_for_activity(&kernel, twelve_types[0], 0U,
                                       &registration) !=
            LXP_ERR_UNKNOWN_MODULE ||
        lxp_kernel_module_by_id(&kernel, LXP_MODULE_RESERVED_COUNT + 1U, 0U,
                                &registration) != LXP_ERR_UNKNOWN_MODULE)
        return 1;
    (void)memset(&web, 0, sizeof(web));
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_WEB, &web) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    web.network_id = 7U;
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_WEB, &web) != LXP_OK)
        return 1;

    (void)memset(&observation, 0, sizeof(observation));
    observation.origin = LX_WEB_ORIGIN_PROGRAM;
    observation.network_id = 7U;
    (void)memset(observation.program_id, 0x71, 32U);
    observation.request_id = 9U;
    observation.kind = LX_WEB_KIND_FETCH;
    observation.full_length = 1U;
    observation.response_length = 1U;
    observation.response[0] = (uint8_t)'x';
    observation.signature_count = 1U;
    if (lx_web_observation_encode(&observation, observation_bytes,
                                  sizeof(observation_bytes),
                                  &observation_length) != LXP_OK)
        return 1;
    web_activity(&activity, LX_WEB_OBSERVATION_ACTIVITY, observation_bytes,
                 observation_length);
    /* No attestor set is registered yet, so validation refuses it. */
    if (web_dispatch(&kernel, LXP_MODULE_PROGRAMS, &activity, &authority,
                     false, &module_result) != LXP_OK ||
        module_result != LXP_ERR_ATTESTATION_THRESHOLD)
        return 1;

    (void)memset(&set, 0, sizeof(set));
    for (index = 0U; index < 3U; ++index) {
        set.attestors[index].signer[0] = (uint8_t)(index + 1U);
        set.attestors[index].payout_account[0] = (uint8_t)(0x10U + index);
    }
    set.count = 3U;
    set.threshold = 2U;
    if (lx_web_attestor_set_encode(&set, set_bytes, sizeof(set_bytes),
                                   &set_length) != LXP_OK)
        return 1;
    web_activity(&activity, LX_WEB_ATTESTOR_SET_ACTIVITY, set_bytes,
                 set_length);
    /* A web activity outside the Programs module context is refused before
     * the module sees it. */
    if (web_dispatch(&kernel, LXP_MODULE_WEB, &activity, &authority, false,
                     &module_result) != LXP_ERR_CONTEXT_MISMATCH ||
        web_attestor_entry(&kernel) != NULL)
        return 1;
    /* Without the governance key the attestor set is refused. */
    if (web_dispatch(&kernel, LXP_MODULE_PROGRAMS, &activity, &authority,
                     true, &module_result) != LXP_OK ||
        module_result != LXP_ERR_AUTH_SCOPE ||
        web_attestor_entry(&kernel) != NULL)
        return 1;
    kernel.handover.enabled = true;
    (void)memset(kernel.handover.governance_public_key, 0x42, 32U);
    (void)memcpy(authority.verified_key, kernel.handover.governance_public_key,
                 32U);
    if (web_dispatch(&kernel, LXP_MODULE_PROGRAMS, &activity, &authority,
                     true, &module_result) != LXP_OK ||
        module_result != LXP_OK)
        return 1;
    entry = web_attestor_entry(&kernel);
    if (entry == NULL || entry->value_length != set_length ||
        memcmp(entry->value, set_bytes, set_length) != 0)
        return 1;

    /* With the set registered the observation reaches intake, which finds no
     * request recorded for it; one for another network never gets there. */
    web_activity(&activity, LX_WEB_OBSERVATION_ACTIVITY, observation_bytes,
                 observation_length);
    if (web_dispatch(&kernel, LXP_MODULE_PROGRAMS, &activity, &authority,
                     true, &module_result) != LXP_OK ||
        module_result != LXP_ERR_UNKNOWN_FIELD ||
        web.pending_count != 0U || web.committed_count != 0U)
        return 1;
    observation.network_id = 8U;
    if (lx_web_observation_encode(&observation, observation_bytes,
                                  sizeof(observation_bytes),
                                  &observation_length) != LXP_OK)
        return 1;
    web_activity(&activity, LX_WEB_OBSERVATION_ACTIVITY, observation_bytes,
                 observation_length);
    if (web_dispatch(&kernel, LXP_MODULE_PROGRAMS, &activity, &authority,
                     true, &module_result) != LXP_OK ||
        module_result != LXP_ERR_WRONG_NETWORK)
        return 1;

    /* The batch header carries the root of the bound web store. */
    if (lxp_arena_init(&root_arena, root_arena_bytes,
                       sizeof(root_arena_bytes)) != LXP_OK ||
        lx_web_root(&web, &root_arena, web_root) != LXP_OK ||
        lxp_arena_reset(&root_arena, 0U) != LXP_OK ||
        lx_batch_header_set_web_root(&header,
            (const lx_web_store *)kernel.module_runtime[LXP_MODULE_WEB],
            &root_arena) != LXP_OK ||
        memcmp(header.web_root, web_root, 32U) != 0)
        return 1;
    if (lxp_state_store_destroy(&state) != LXP_OK) return 1;
    return 0;
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
                                         { 0 }, NULL, { 0 }, { 0 } };
    lxp_fee_params fee_parameters = { 1U, { 0U, 1U }, { 0U, 0U },
        { 0U, 0U }, { 0U, 0U }, { 0U, 0U }, 10000U, 0U, {{0U, 0U}} , 0U, {{0U, 0U}} };
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
    if (web_dispatch_checks() != 0) return 1;
    return 0;
}
