#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

enum {
    CALL_FIXED_BYTES = 32 + 2 + 2 + 4 + 2 + 4 +
                       LX_PROGRAMS_CALL_BUDGET_FIELDS * 8,
    DEPLOY_FIXED_BYTES = 104,
    UPGRADE_FIXED_BYTES = 106
};

static void write_u16(uint8_t *out, uint16_t value)
{
    out[0] = (uint8_t)(value >> 8U);
    out[1] = (uint8_t)value;
}

static void write_u32(uint8_t *out, uint32_t value)
{
    out[0] = (uint8_t)(value >> 24U);
    out[1] = (uint8_t)(value >> 16U);
    out[2] = (uint8_t)(value >> 8U);
    out[3] = (uint8_t)value;
}

static void write_u64(uint8_t *out, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        out[index] = (uint8_t)(value >> ((7U - index) * 8U));
}

static void append_u32_leb(uint8_t *out, size_t *cursor, uint32_t value)
{
    do {
        uint8_t byte = (uint8_t)(value & 0x7fU);
        value >>= 7U;
        out[(*cursor)++] = value == 0U ? byte : (uint8_t)(byte | 0x80U);
    } while (value != 0U);
}

static void append_bytes(uint8_t *out, size_t *cursor,
                         const uint8_t *bytes, size_t length)
{
    (void)memcpy(out + *cursor, bytes, length);
    *cursor += length;
}

static void append_name(uint8_t *out, size_t *cursor, const char *name)
{
    size_t length = strlen(name);
    append_u32_leb(out, cursor, (uint32_t)length);
    append_bytes(out, cursor, (const uint8_t *)name, length);
}

static void append_section(uint8_t *out, size_t *cursor, uint8_t id,
                           const uint8_t *body, size_t length)
{
    out[(*cursor)++] = id;
    append_u32_leb(out, cursor, (uint32_t)length);
    append_bytes(out, cursor, body, length);
}

static size_t staged_terminal_module(uint8_t *out, bool resource)
{
    static const uint8_t header[] = {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    uint8_t section[768];
    uint8_t body[256];
    size_t cursor = 0U;
    size_t length = 0U;
    size_t body_length = 0U;
    size_t index;
    append_bytes(out, &cursor, header, sizeof(header));
    section[length++] = 5U;
    section[length++] = 0x60U; section[length++] = 4U;
    for (index = 0U; index < 4U; ++index) section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 6U;
    section[length++] = 0x7eU; section[length++] = 0x7eU;
    for (index = 0U; index < 4U; ++index) section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 3U;
    for (index = 0U; index < 3U; ++index) section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 1U;
    section[length++] = 0x7fU; section[length++] = 1U; section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 2U;
    section[length++] = 0x7fU; section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    append_section(out, &cursor, 1U, section, length);
    length = 0U; section[length++] = 4U;
    append_name(section, &length, "layerx_v1");
    append_name(section, &length, "storage_write"); section[length++] = 0U; section[length++] = 0U;
    append_name(section, &length, "layerx_v1");
    append_name(section, &length, "event_emit"); section[length++] = 0U; section[length++] = 0U;
    append_name(section, &length, "layerx_v1");
    append_name(section, &length, "transfer_402"); section[length++] = 0U; section[length++] = 1U;
    append_name(section, &length, "layerx_v2_candidate");
    append_name(section, &length, "refusal_write"); section[length++] = 0U; section[length++] = 2U;
    append_section(out, &cursor, 2U, section, length);
    { static const uint8_t functions[] = {2U, 3U, 4U};
      append_section(out, &cursor, 3U, functions, sizeof(functions)); }
    { static const uint8_t memory[] = {1U, 1U, 1U, 1U};
      append_section(out, &cursor, 5U, memory, sizeof(memory)); }
    length = 0U; section[length++] = 3U;
    append_name(section, &length, "layerx_reserve"); section[length++] = 0U; section[length++] = 4U;
    append_name(section, &length, "layerx_call"); section[length++] = 0U; section[length++] = 5U;
    append_name(section, &length, "memory"); section[length++] = 2U; section[length++] = 0U;
    append_section(out, &cursor, 7U, section, length);
    body[body_length++] = 0U;
    { static const uint8_t staged[] = {
        0x41U,0U,0x41U,1U,0x41U,1U,0x41U,1U,0x10U,0U,0x1aU,
        0x41U,0U,0x41U,1U,0x41U,1U,0x41U,1U,0x10U,1U,0x1aU,
        0x42U,0U,0x42U,1U,0x41U,0x80U,1U,0x41U,32U,
        0x41U,0xa0U,1U,0x41U,32U,0x10U,2U,0x1aU
      }; append_bytes(body, &body_length, staged, sizeof(staged)); }
    if (resource) {
        static const uint8_t loop[] = {0x03U,0x40U,0x0cU,0U,0x0bU,0x41U,0U,0x0bU};
        append_bytes(body, &body_length, loop, sizeof(loop));
    } else {
        static const uint8_t refuse[] = {
            0x41U,1U,0x41U,2U,0x41U,1U,0x10U,3U,0x1aU,
            0x41U,0x40U,0x0bU
        };
        append_bytes(body, &body_length, refuse, sizeof(refuse));
    }
    length = 0U; section[length++] = 2U;
    section[length++] = 4U; section[length++] = 0U;
    section[length++] = 0x41U; section[length++] = 0U; section[length++] = 0x0bU;
    append_u32_leb(section, &length, (uint32_t)body_length);
    append_bytes(section, &length, body, body_length);
    append_section(out, &cursor, 10U, section, length);
    length = 0U; section[length++] = 3U;
    section[length++] = 0U; section[length++] = 0x41U; section[length++] = 0U; section[length++] = 0x0bU;
    section[length++] = 3U; section[length++] = 'k'; section[length++] = 'v'; section[length++] = 'x';
    section[length++] = 0U; section[length++] = 0x41U; section[length++] = 0x80U; section[length++] = 1U; section[length++] = 0x0bU;
    section[length++] = 32U; for (index = 0U; index < 32U; ++index) section[length++] = 9U;
    section[length++] = 0U; section[length++] = 0x41U; section[length++] = 0xa0U; section[length++] = 1U; section[length++] = 0x0bU;
    section[length++] = 32U; for (index = 0U; index < 32U; ++index) section[length++] = 10U;
    append_section(out, &cursor, 11U, section, length);
    return cursor;
}

static size_t candidate_module(uint8_t *out, const uint8_t *entry,
                               size_t entry_length)
{
    static const uint8_t header[] = {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    static const uint8_t types[] = {
        1U, 14U, 2U, 0x60U, 1U, 0x7fU, 1U, 0x7fU,
        0x60U, 2U, 0x7fU, 0x7fU, 1U, 0x7fU
    };
    static const uint8_t functions[] = {3U, 3U, 2U, 0U, 1U};
    static const uint8_t memory[] = {5U, 4U, 1U, 1U, 1U, 1U};
    static const uint8_t exports[] = {
        7U, 41U, 3U,
        14U, 'l','a','y','e','r','x','_','r','e','s','e','r','v','e', 0U, 0U,
        11U, 'l','a','y','e','r','x','_','c','a','l','l', 0U, 1U,
        6U, 'm','e','m','o','r','y', 2U, 0U
    };
    size_t cursor = 0U;
    size_t code_payload = 1U + 5U + 2U + entry_length;
    (void)memcpy(out + cursor, header, sizeof(header)); cursor += sizeof(header);
    (void)memcpy(out + cursor, types, sizeof(types)); cursor += sizeof(types);
    (void)memcpy(out + cursor, functions, sizeof(functions)); cursor += sizeof(functions);
    (void)memcpy(out + cursor, memory, sizeof(memory)); cursor += sizeof(memory);
    (void)memcpy(out + cursor, exports, sizeof(exports)); cursor += sizeof(exports);
    out[cursor++] = 10U;
    out[cursor++] = (uint8_t)code_payload;
    out[cursor++] = 2U;
    out[cursor++] = 4U; out[cursor++] = 0U;
    out[cursor++] = 0x41U; out[cursor++] = 0U; out[cursor++] = 0x0bU;
    out[cursor++] = (uint8_t)(entry_length + 1U);
    out[cursor++] = 0U;
    (void)memcpy(out + cursor, entry, entry_length);
    return cursor + entry_length;
}

static int exact_fee_applied(lxp_u128 actor_before, lxp_u128 treasury_before,
                             const lx_account *actor,
                             const lx_account *treasury, lxp_u128 fee)
{
    lxp_u128 expected_actor;
    lxp_u128 expected_treasury;
    return lxp_u128_sub(actor_before, fee, &expected_actor) == LXP_OK &&
           lxp_u128_add(treasury_before, fee, &expected_treasury) == LXP_OK &&
           lxp_u128_cmp(actor->balance, expected_actor) == 0 &&
           lxp_u128_cmp(treasury->balance, expected_treasury) == 0 ? 0 : 1;
}

static size_t call_payload_with_capabilities(
    uint8_t *out, const uint8_t program_id[32],
    const uint8_t *capabilities, size_t capabilities_length)
{
    static const uint8_t entrypoint[] = "run";
    const uint64_t budget[LX_PROGRAMS_CALL_BUDGET_FIELDS] = {
        1000000U, 1048576U, 1048576U, 1048576U, 64U, 1024U, 64U
    };
    size_t cursor = 0U;
    size_t index;
    (void)memcpy(out + cursor, program_id, 32U);
    cursor += 32U;
    write_u16(out + cursor, LX_PROGRAMS_ABI_VERSION);
    cursor += 2U;
    write_u16(out + cursor, (uint16_t)(sizeof(entrypoint) - 1U));
    cursor += 2U;
    write_u32(out + cursor, 0U);
    cursor += 4U;
    write_u16(out + cursor, (uint16_t)capabilities_length);
    cursor += 2U;
    write_u32(out + cursor, 16U);
    cursor += 4U;
    for (index = 0U; index < LX_PROGRAMS_CALL_BUDGET_FIELDS; ++index) {
        write_u64(out + cursor, budget[index]);
        cursor += 8U;
    }
    (void)memcpy(out + cursor, entrypoint, sizeof(entrypoint) - 1U);
    cursor += sizeof(entrypoint) - 1U;
    (void)memcpy(out + cursor, capabilities, capabilities_length);
    return cursor + capabilities_length;
}

static size_t call_payload(uint8_t *out, const uint8_t program_id[32])
{
    static const uint8_t capabilities[] = {0U, 0U};
    return call_payload_with_capabilities(out, program_id, capabilities,
                                          sizeof(capabilities));
}

static size_t staged_call_payload(uint8_t *out, const uint8_t program_id[32])
{
    uint8_t capabilities[85] = {0U};
    size_t cursor = 0U;
    size_t index;
    capabilities[cursor++] = 0U;
    capabilities[cursor++] = 3U;
    capabilities[cursor++] = 2U;
    capabilities[cursor++] = 3U;
    capabilities[cursor++] = 5U;
    for (index = 0U; index < 32U; ++index) capabilities[cursor++] = 9U;
    for (index = 0U; index < 32U; ++index) capabilities[cursor++] = 10U;
    capabilities[cursor + 15U] = 1U;
    cursor += 16U;
    return call_payload_with_capabilities(out, program_id, capabilities,
                                          cursor);
}

static int malformed_call_payloads(void)
{
    uint8_t arena_bytes[8192];
    uint8_t payload[CALL_FIXED_BYTES + LX_PROGRAMS_MAX_ENTRYPOINT_BYTES + 4U];
    uint8_t program_id[32];
    lxp_arena arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    uint64_t parameters = 1U;
    void *decoded = NULL;
    size_t length;
    (void)memset(program_id, 0x31, sizeof(program_id));
    length = call_payload(payload, program_id);
    if (lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS, 1U, 0U, 1U,
                            1000000U, &arena, false) != LXP_OK)
        return 1;
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) != LXP_OK ||
        decoded == NULL ||
        lxp_programs_call_decode(&ctx, payload, CALL_FIXED_BYTES - 1U,
                                 &decoded) != LXP_ERR_TRUNCATED)
        return 1;
    payload[length] = 0U;
    if (lxp_programs_call_decode(&ctx, payload, length + 1U, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    (void)memset(payload, 0, 32U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    write_u16(payload + 32U, LX_PROGRAMS_ABI_VERSION + 1U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    write_u16(payload + 34U, 0U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    payload[CALL_FIXED_BYTES] = (uint8_t)'/';
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    write_u32(payload + 36U, LX_PROGRAMS_MAX_CALLDATA_BYTES + 1U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    write_u16(payload + 40U, LX_PROGRAMS_MAX_CAPABILITY_BYTES + 1U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    length = call_payload(payload, program_id);
    write_u32(payload + 42U, LX_PROGRAMS_MAX_RESPONSE_BYTES + 1U);
    if (lxp_programs_call_decode(&ctx, payload, length, &decoded) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    return lxp_state_store_destroy(&state) == LXP_OK ? 0 : 1;
}

static size_t deploy_payload(uint8_t *out, const uint8_t program_id[32],
                             const uint8_t authority[32], const uint8_t *wasm,
                             size_t wasm_length, uint8_t code_hash[32])
{
    (void)lxp_hash_sha256(wasm, wasm_length, code_hash);
    (void)memcpy(out, program_id, 32U);
    write_u16(out + 32U, LX_PROGRAMS_ABI_VERSION);
    out[34] = 1U;
    out[35] = 0U;
    (void)memcpy(out + 36U, authority, 32U);
    (void)memcpy(out + 68U, code_hash, 32U);
    write_u32(out + 100U, (uint32_t)wasm_length);
    (void)memcpy(out + DEPLOY_FIXED_BYTES, wasm, wasm_length);
    return DEPLOY_FIXED_BYTES + wasm_length;
}

static size_t upgrade_payload(uint8_t *out, const uint8_t program_id[32],
                              const uint8_t old_hash[32], const uint8_t *wasm,
                              size_t wasm_length, uint8_t new_hash[32])
{
    (void)lxp_hash_sha256(wasm, wasm_length, new_hash);
    (void)memcpy(out, program_id, 32U);
    write_u16(out + 32U, LX_PROGRAMS_ABI_VERSION);
    out[34] = 0U;
    out[35] = 0U;
    (void)memcpy(out + 36U, old_hash, 32U);
    (void)memcpy(out + 68U, new_hash, 32U);
    write_u16(out + 100U, 0U);
    write_u32(out + 102U, (uint32_t)wasm_length);
    (void)memcpy(out + UPGRADE_FIXED_BYTES, wasm, wasm_length);
    return UPGRADE_FIXED_BYTES + wasm_length;
}

static void fill_activity(lxp_activity *activity, uint32_t activity_type,
                          const uint8_t *payload, size_t payload_length,
                          const uint8_t *did, size_t did_length,
                          const uint8_t authority[32])
{
    (void)memset(activity, 0, sizeof(*activity));
    activity->protocol_version = LXP_PROTOCOL_VERSION;
    activity->network_id = 7U;
    activity->activity_type = activity_type;
    activity->actor_did = (lxp_byte_span){did, did_length};
    activity->authority = (lxp_byte_span){authority, 32U};
    activity->timestamp_bound = (lxp_timestamp_bound){1U, 100U};
    activity->idempotency_key[31] = 1U;
    activity->payload = (lxp_byte_span){payload, payload_length};
    (void)lxp_hash_payload(payload, payload_length, activity->payload_hash);
}

static int deploy_and_upgrade_persist_exact_artifacts(void)
{
    static const uint8_t success_entry[] = {0x41U, 0U, 0x0bU};
    static const uint8_t upgraded_entry[] = {0x41U, 7U, 0x0bU};
    static const uint8_t did[] = "did:lxp:program-call";
    static const uint8_t actor_name[] = "agent:program-call:main";
    static const uint8_t treasury_name[] = "system:fees";
    uint8_t program_id[32];
    uint8_t primary_key[32] = {1U};
    uint8_t wasm[128];
    uint8_t upgraded_wasm[128];
    uint8_t failure_wasm[1024];
    uint8_t resource_wasm[1024];
    uint8_t payload[UPGRADE_FIXED_BYTES + sizeof(failure_wasm)];
    uint8_t call[CALL_FIXED_BYTES + 128U];
    uint8_t code_hash[32];
    uint8_t upgraded_hash[32];
    uint8_t failure_hash[32];
    uint8_t resource_hash[32];
    uint8_t first_terminal_root[32];
    uint8_t actor_id[32];
    uint8_t treasury_id[32];
    uint8_t fee_asset[32] = {9U};
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_identity_store identities = {0};
    lxp_identity *identity;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_fee_params fees = {0};
    lx_account_registry accounts;
    lx_account *actor;
    lx_account *treasury;
    lxp_transfer_asset_state fee_asset_state;
    lx_programs_transfer_runtime runtime;
    lxp_activity activity;
    lxp_receipt receipt;
    uint64_t parameters = 1U;
    size_t payload_length;
    size_t wasm_length = candidate_module(wasm, success_entry,
                                          sizeof(success_entry));
    size_t upgraded_wasm_length = candidate_module(
        upgraded_wasm, upgraded_entry, sizeof(upgraded_entry));
    size_t failure_wasm_length = staged_terminal_module(failure_wasm, false);
    size_t resource_wasm_length = staged_terminal_module(resource_wasm, true);
    size_t module_kv_before;
    lxp_u128 actor_before;
    lxp_u128 treasury_before;
    (void)memset(program_id, 0x31, sizeof(program_id));
    (void)memset(&authority, 0, sizeof(authority));
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string(actor_name, sizeof(actor_name) - 1U,
                                  actor_id) != LXP_OK ||
        lx_account_id_from_string(treasury_name, sizeof(treasury_name) - 1U,
                                  treasury_id) != LXP_OK ||
        lx_account_open(&accounts, actor_name, sizeof(actor_name) - 1U,
                        actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &actor) != LXP_OK ||
        lx_account_open(&accounts, treasury_name, sizeof(treasury_name) - 1U,
                        treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS, NULL,
                        &treasury) != LXP_OK ||
        lxp_ledger_bootstrap_balance(actor, fee_asset,
                                     (lxp_u128){0U, UINT64_MAX}, 1U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(treasury, fee_asset,
                                     (lxp_u128){0U, 0U}, 0U) != LXP_OK)
        return 1;
    (void)memcpy(authority.principal, actor_id, sizeof(actor_id));
    (void)memset(authority.authority_hash, 0x55, 32U);
    (void)memset(&fee_asset_state, 0, sizeof(fee_asset_state));
    (void)memcpy(fee_asset_state.asset_id, fee_asset, sizeof(fee_asset));
    fee_asset_state.registered = true;
    runtime = (lx_programs_transfer_runtime){&accounts, &fee_asset_state, 1U};
    payload_length = deploy_payload(payload, program_id, authority.principal,
                                    wasm, wasm_length, code_hash);
    fill_activity(&activity, LX_PROGRAMS_DEPLOY, payload, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    fees.version = 1U;
    fees.multiplier_basis_points = 10000U;
    if (lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_identity_register(&identities, did, sizeof(did) - 1U,
                              primary_key, &identity) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS, &runtime) !=
            LXP_OK ||
        lxp_programs_bind_fee_transaction(&kernel) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
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
    execution.fee_parameters = &fees;
    execution.gas_limit = 1000000U;
    execution.arena = &arena;
    if (lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK ||
        receipt.result_code != LXP_OK ||
        receipt.module_id != LXP_MODULE_PROGRAMS ||
        receipt.module_version != LX_PROGRAMS_ABI_VERSION ||
        receipt.effects.count != 1U ||
        receipt.effects.effects[0].event_type != LX_PROGRAMS_EVENT_DEPLOYED ||
        receipt.effects.effects[0].body_length != 32U ||
        memcmp(receipt.effects.effects[0].body, code_hash, 32U) != 0 ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) == 0 ||
        identity->next_sequence != 1U || state.next_sequence != 2U ||
        kernel.blob_count != 1U || kernel.blobs[0].module_id != LXP_MODULE_PROGRAMS ||
        memcmp(kernel.blobs[0].key, code_hash, 32U) != 0 ||
        kernel.blobs[0].length != wasm_length ||
        memcmp(kernel.blobs[0].bytes, wasm, wasm_length) != 0)
        return 1;
    payload_length = staged_call_payload(call, program_id);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 1U;
    activity.idempotency_key[31] = 2U;
    activity.fee_limit = (lxp_u128){0U, UINT64_MAX};
    execution.fee_balance = (lxp_u128){0U, UINT64_MAX};
    execution.global_sequence = 2U;
    actor_before = actor->balance;
    treasury_before = treasury->balance;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_OK ||
        !receipt.program_outcome.present ||
        receipt.program_outcome.terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS ||
        receipt.program_outcome.runtime_version == 0U ||
        receipt.program_outcome.abi_version != LX_PROGRAMS_ABI_VERSION ||
        receipt.program_outcome.fee_schedule_version != 1U ||
        receipt.effects.count != 1U ||
        receipt.effects.effects[0].event_type != LX_PROGRAMS_EVENT_CALL_OUTCOME ||
        receipt.effects.effects[0].body_length == 0U ||
        lxp_ct_is_zero(receipt.program_outcome.call_graph_root, 32U) ||
        lxp_ct_is_zero(receipt.program_outcome.terminal_payload_root, 32U) ||
        !lxp_ct_is_zero(receipt.program_outcome.transfer_root, 32U) ||
        lxp_u128_is_zero(receipt.fee_charged) ||
        lxp_u128_cmp(receipt.fee_charged,
                     receipt.program_outcome.fee_units) != 0 ||
        identity->next_sequence != 2U || state.next_sequence != 3U ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) == 0 ||
        exact_fee_applied(actor_before, treasury_before, actor, treasury,
                          receipt.fee_charged) != 0)
        return 1;
    (void)memcpy(first_terminal_root,
                 receipt.program_outcome.terminal_payload_root, 32U);
    payload_length = upgrade_payload(payload, program_id, code_hash,
                                     upgraded_wasm, upgraded_wasm_length,
                                     upgraded_hash);
    fill_activity(&activity, LX_PROGRAMS_UPGRADE, payload, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 2U;
    activity.idempotency_key[31] = 3U;
    activity.fee_limit = (lxp_u128){0U, 0U};
    execution.global_sequence = 3U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK ||
        receipt.result_code != LXP_OK ||
        receipt.module_id != LXP_MODULE_PROGRAMS ||
        receipt.module_version != LX_PROGRAMS_ABI_VERSION ||
        receipt.effects.count != 1U ||
        receipt.effects.effects[0].event_type != LX_PROGRAMS_EVENT_UPGRADED ||
        receipt.effects.effects[0].body_length != 64U ||
        memcmp(receipt.effects.effects[0].body, code_hash, 32U) != 0 ||
        memcmp(receipt.effects.effects[0].body + 32U, upgraded_hash, 32U) != 0 ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) == 0 ||
        identity->next_sequence != 3U || state.next_sequence != 4U ||
        kernel.blob_count != 2U ||
        memcmp(kernel.blobs[0].key, code_hash, 32U) != 0 ||
        kernel.blobs[0].length != wasm_length ||
        memcmp(kernel.blobs[0].bytes, wasm, wasm_length) != 0 ||
        kernel.blobs[1].module_id != LXP_MODULE_PROGRAMS ||
        memcmp(kernel.blobs[1].key, upgraded_hash, 32U) != 0 ||
        kernel.blobs[1].length != upgraded_wasm_length ||
        memcmp(kernel.blobs[1].bytes, upgraded_wasm,
               upgraded_wasm_length) != 0)
        return 1;
    payload_length = staged_call_payload(call, program_id);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 3U;
    activity.idempotency_key[31] = 4U;
    activity.fee_limit = (lxp_u128){0U, UINT64_MAX};
    execution.global_sequence = 4U;
    actor_before = actor->balance;
    treasury_before = treasury->balance;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_OK ||
        !receipt.program_outcome.present ||
        receipt.program_outcome.terminal_kind != LXP_PROGRAM_TERMINAL_SUCCESS ||
        receipt.effects.count != 1U ||
        receipt.effects.effects[0].event_type != LX_PROGRAMS_EVENT_CALL_OUTCOME ||
        memcmp(receipt.program_outcome.terminal_payload_root,
               first_terminal_root, 32U) == 0 ||
        lxp_u128_is_zero(receipt.fee_charged) ||
        identity->next_sequence != 4U || state.next_sequence != 5U ||
        exact_fee_applied(actor_before, treasury_before, actor, treasury,
                          receipt.fee_charged) != 0)
        return 1;
    payload_length = upgrade_payload(payload, program_id, upgraded_hash,
                                     failure_wasm, failure_wasm_length,
                                     failure_hash);
    fill_activity(&activity, LX_PROGRAMS_UPGRADE, payload, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 4U;
    activity.idempotency_key[31] = 5U;
    execution.global_sequence = 5U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_OK)
        return 1;
    payload_length = call_payload(call, program_id);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 5U;
    activity.idempotency_key[31] = 6U;
    activity.fee_limit = (lxp_u128){0U, UINT64_MAX};
    execution.global_sequence = 6U;
    module_kv_before = kernel.module_kv_count;
    actor_before = actor->balance;
    treasury_before = treasury->balance;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_ERR_PROGRAM_REFUSED ||
        !receipt.program_outcome.present ||
        receipt.program_outcome.terminal_kind != LXP_PROGRAM_TERMINAL_FAILURE ||
        receipt.program_outcome.result_code != LXP_ERR_PROGRAM_REFUSED ||
        receipt.effects.count != 0U ||
        !lxp_ct_is_zero(receipt.program_outcome.transfer_root, 32U) ||
        lxp_u128_is_zero(receipt.fee_charged) ||
        kernel.module_kv_count != module_kv_before ||
        identity->next_sequence != 6U || state.next_sequence != 7U ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) == 0 ||
        exact_fee_applied(actor_before, treasury_before, actor, treasury,
                          receipt.fee_charged) != 0)
        return 1;
    payload_length = upgrade_payload(payload, program_id, failure_hash,
                                     resource_wasm, resource_wasm_length,
                                     resource_hash);
    fill_activity(&activity, LX_PROGRAMS_UPGRADE, payload, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 6U;
    activity.idempotency_key[31] = 7U;
    activity.fee_limit = (lxp_u128){0U, 0U};
    execution.global_sequence = 7U;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_OK)
        return 1;
    payload_length = call_payload(call, program_id);
    write_u64(call + 46U, 100U);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                  did, sizeof(did) - 1U, primary_key);
    activity.account_sequence = 7U;
    activity.idempotency_key[31] = 8U;
    activity.fee_limit = (lxp_u128){0U, UINT64_MAX};
    execution.global_sequence = 8U;
    module_kv_before = kernel.module_kv_count;
    actor_before = actor->balance;
    treasury_before = treasury->balance;
    if (lxp_arena_reset(&arena, 0U) != LXP_OK ||
        lxp_kernel_execute_activity(&kernel, &activity, &execution, &receipt) !=
            LXP_OK || receipt.result_code != LXP_ERR_GAS_EXHAUSTED ||
        !receipt.program_outcome.present ||
        receipt.program_outcome.terminal_kind != LXP_PROGRAM_TERMINAL_RESOURCE ||
        receipt.program_outcome.result_code != LXP_ERR_GAS_EXHAUSTED ||
        receipt.program_outcome.cpu_fuel == 0U ||
        receipt.effects.count != 0U ||
        !lxp_ct_is_zero(receipt.program_outcome.transfer_root, 32U) ||
        lxp_u128_is_zero(receipt.fee_charged) ||
        kernel.module_kv_count != module_kv_before ||
        identity->next_sequence != 8U || state.next_sequence != 9U ||
        memcmp(receipt.previous_state_root, receipt.resulting_state_root, 32U) == 0 ||
        exact_fee_applied(actor_before, treasury_before, actor, treasury,
                          receipt.fee_charged) != 0)
        return 1;
    return lxp_state_store_destroy(&state) == LXP_OK ? 0 : 1;
}

int main(void)
{
    if (malformed_call_payloads() != 0) return 1;
    return deploy_and_upgrade_persist_exact_artifacts();
}
