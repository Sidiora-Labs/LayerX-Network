#define main oracle_read_reference_main
int oracle_read_reference_main(int argc, char **argv);
#include "programs/test_call_activity.c"
#undef main

#include "layerx/lx_oracle.h"
#include "layerx/lx_perps.h"

#define ORACLE_READ_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "programs oracle read check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

enum {
    ORACLE_READ_WASM_BYTES = 1024,
    ORACLE_READ_SEQUENCE = 9,
    ORACLE_READ_PRICE = 123456,
    ORACLE_READ_OBSERVED_AT = 500,
    ORACLE_READ_SOURCE = 7,
    ORACLE_READ_MARKET_OFFSET = 0,
    ORACLE_READ_EXPECTED_OFFSET = 128,
    ORACLE_READ_OUTPUT_OFFSET = 256
};

static void oracle_read_i32(uint8_t *body, size_t *length, uint32_t value)
{
    body[(*length)++] = 0x41U;
    do {
        uint8_t byte = (uint8_t)(value & 0x7fU);
        value >>= 7U;
        if (value != 0U || (byte & 0x40U) != 0U) byte |= 0x80U;
        body[(*length)++] = byte;
        if (value == 0U && (byte & 0x80U) != 0U) {
            body[(*length)++] = 0U;
            break;
        }
    } while (value != 0U);
}

static void oracle_read_call_import(uint8_t *body, size_t *length)
{
    oracle_read_i32(body, length, (uint32_t)ORACLE_READ_MARKET_OFFSET);
    oracle_read_i32(body, length, 32U);
    oracle_read_i32(body, length, (uint32_t)ORACLE_READ_OUTPUT_OFFSET);
    oracle_read_i32(body, length, (uint32_t)LX_ORACLE_COMMITTED_BYTES);
    body[(*length)++] = 0x10U;
    body[(*length)++] = 0U;
}

/* ABI-v2 guest that reads the committed perps observation for one market. The
 * comparing form traps unless every byte the host returns equals the record the
 * test derived from the perps engine state; the refusing form drops the status
 * so the typed host refusal, not a guest trap, decides the receipt. */
static size_t oracle_read_module(uint8_t *out, const uint8_t market_id[32],
                                 const uint8_t *expected)
{
    static const uint8_t header[] = {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    static const uint8_t types[] = {
        3U,
        0x60U, 4U, 0x7fU, 0x7fU, 0x7fU, 0x7fU, 1U, 0x7fU,
        0x60U, 1U, 0x7fU, 1U, 0x7fU,
        0x60U, 2U, 0x7fU, 0x7fU, 1U, 0x7fU
    };
    static const uint8_t functions[] = {2U, 1U, 2U};
    static const uint8_t memory[] = {1U, 1U, 1U, 1U};
    uint8_t section[256];
    uint8_t body[256];
    size_t cursor = 0U;
    size_t length = 0U;
    size_t body_length = 0U;
    size_t index;
    append_bytes(out, &cursor, header, sizeof(header));
    append_section(out, &cursor, 1U, types, sizeof(types));
    section[length++] = 1U;
    append_name(section, &length, "layerx_v2");
    append_name(section, &length, "oracle_read");
    section[length++] = 0U;
    section[length++] = 0U;
    append_section(out, &cursor, 2U, section, length);
    append_section(out, &cursor, 3U, functions, sizeof(functions));
    append_section(out, &cursor, 5U, memory, sizeof(memory));
    length = 0U;
    section[length++] = 3U;
    append_name(section, &length, "layerx_reserve");
    section[length++] = 0U; section[length++] = 1U;
    append_name(section, &length, "layerx_call");
    section[length++] = 0U; section[length++] = 2U;
    append_name(section, &length, "memory");
    section[length++] = 2U; section[length++] = 0U;
    append_section(out, &cursor, 7U, section, length);
    body[body_length++] = 0U;
    oracle_read_call_import(body, &body_length);
    if (expected != NULL) {
        oracle_read_i32(body, &body_length,
                        (uint32_t)LX_ORACLE_COMMITTED_BYTES);
        body[body_length++] = 0x47U;
        body[body_length++] = 0x04U; body[body_length++] = 0x40U;
        body[body_length++] = 0x00U;
        body[body_length++] = 0x0bU;
        for (index = 0U; index < LX_ORACLE_COMMITTED_BYTES / 8U; ++index) {
            oracle_read_i32(body, &body_length, 0U);
            body[body_length++] = 0x29U; body[body_length++] = 3U;
            append_u32_leb(body, &body_length,
                           (uint32_t)(ORACLE_READ_OUTPUT_OFFSET + index * 8U));
            oracle_read_i32(body, &body_length, 0U);
            body[body_length++] = 0x29U; body[body_length++] = 3U;
            append_u32_leb(body, &body_length,
                           (uint32_t)(ORACLE_READ_EXPECTED_OFFSET + index * 8U));
            body[body_length++] = 0x52U;
            body[body_length++] = 0x04U; body[body_length++] = 0x40U;
            body[body_length++] = 0x00U;
            body[body_length++] = 0x0bU;
        }
    } else {
        body[body_length++] = 0x1aU;
    }
    oracle_read_i32(body, &body_length, 0U);
    body[body_length++] = 0x0bU;
    length = 0U;
    section[length++] = 2U;
    section[length++] = 4U;
    section[length++] = 0U; section[length++] = 0x41U;
    section[length++] = 0U; section[length++] = 0x0bU;
    append_u32_leb(section, &length, (uint32_t)body_length);
    append_bytes(section, &length, body, body_length);
    append_section(out, &cursor, 10U, section, length);
    length = 0U;
    section[length++] = expected != NULL ? 2U : 1U;
    section[length++] = 0U;
    oracle_read_i32(section, &length, (uint32_t)ORACLE_READ_MARKET_OFFSET);
    section[length++] = 0x0bU;
    append_u32_leb(section, &length, 32U);
    append_bytes(section, &length, market_id, 32U);
    if (expected != NULL) {
        section[length++] = 0U;
        oracle_read_i32(section, &length, (uint32_t)ORACLE_READ_EXPECTED_OFFSET);
        section[length++] = 0x0bU;
        append_u32_leb(section, &length, (uint32_t)LX_ORACLE_COMMITTED_BYTES);
        append_bytes(section, &length, expected, LX_ORACLE_COMMITTED_BYTES);
    }
    append_section(out, &cursor, 11U, section, length);
    return cursor;
}

static void oracle_read_market(lx_perps_market *market,
                               const uint8_t market_id[32], bool halted)
{
    (void)memset(market, 0, sizeof(*market));
    (void)memcpy(market->market_id, market_id, 32U);
    (void)memset(market->quote_asset, 0x44, 32U);
    (void)memset(market->administrator, 0x55, 32U);
    (void)memset(market->liquidity_account_id, 0x56, 32U);
    (void)memset(market->long_funding_account_id, 0x57, 32U);
    (void)memset(market->short_funding_account_id, 0x58, 32U);
    (void)memset(market->insurance_account_id, 0x59, 32U);
    market->contract_size = (lxp_u128){0U, 1U};
    market->tick_size = (lxp_u128){0U, 1U};
    market->lot_size = (lxp_u128){0U, 1U};
    market->price_scale = (lxp_u128){0U, 1U};
    market->initial_margin_ratio_bps = 200U;
    market->maintenance_margin_ratio_bps = 100U;
    market->liquidation_fee_bps = 10U;
    market->liquidator_share_bps = 10U;
    market->maximum_funding_rate_bps = 100U;
    market->maximum_deviation_basis_points = 1000U;
    market->funding_interval_ms = 1000U;
    market->maximum_oracle_staleness_ms = 1000U;
    market->minimum_price = (lxp_u128){0U, 1U};
    market->maximum_price = (lxp_u128){0U, 1000000U};
    market->permitted_oracle_key_count = 2U;
    (void)memset(market->permitted_oracle_keys[0], 0x61, 32U);
    (void)memset(market->permitted_oracle_keys[1], 0x62, 32U);
    market->parameter_version = 1U;
    market->halted = halted;
}

static void oracle_read_observation(lx_perps_oracle_state *state,
                                    const uint8_t market_id[32])
{
    (void)memset(state, 0, sizeof(*state));
    (void)memcpy(state->market_id, market_id, 32U);
    state->observation_sequence = ORACLE_READ_SEQUENCE;
    state->price = (lxp_u128){0U, ORACLE_READ_PRICE};
    state->observed_at = ORACLE_READ_OBSERVED_AT;
    state->source_identifier = ORACLE_READ_SOURCE;
    (void)memset(state->oracle_public_key, 0x61, 32U);
}

static int oracle_read_expected_record(const lx_perps_market *market,
                                       const lx_perps_oracle_state *state,
                                       uint8_t record[LX_ORACLE_COMMITTED_BYTES])
{
    static const uint8_t domain[] = "LXP:ORACLE:SOURCE-SET:v1";
    uint8_t preimage[sizeof(domain) - 1U + 1U + LX_ORACLE_MAX_KEYS * 32U];
    uint8_t price[16];
    size_t length = 0U;
    size_t index;
    (void)memcpy(preimage, domain, sizeof(domain) - 1U);
    length += sizeof(domain) - 1U;
    preimage[length++] = market->permitted_oracle_key_count;
    (void)memcpy(preimage + length, market->permitted_oracle_keys[0],
                 (size_t)market->permitted_oracle_key_count * 32U);
    length += (size_t)market->permitted_oracle_key_count * 32U;
    ORACLE_READ_CHECK(lxp_u128_to_be(state->price, price) == LXP_OK);
    for (index = 0U; index < 16U; ++index) record[index] = price[15U - index];
    for (index = 0U; index < 8U; ++index) {
        record[16U + index] = (uint8_t)(state->observed_at >> (8U * index));
        record[24U + index] =
            (uint8_t)(state->observation_sequence >> (8U * index));
    }
    ORACLE_READ_CHECK(lxp_hash_sha256(preimage, length, record + 32U) == LXP_OK);
    return 0;
}

static int oracle_read_case(void)
{
    static const uint8_t did[] = "did:lxp:programs-oracle-read";
    static const uint8_t actor_name[] = "agent:did:lxp:programs-oracle-read:main";
    static const uint8_t treasury_name[] = "system:fees";
    static const uint8_t actor_seed[32] = {0x33U};
    static const uint8_t grant_id[32] = {0};
    static const uint8_t no_capabilities[] = {0U, 0U};
    static uint8_t payload[104U + ORACLE_READ_WASM_BYTES];
    static uint8_t call[CALL_FIXED_BYTES + 256U];
    static uint8_t arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t perps_arena_bytes[65536];
    uint8_t wasm[ORACLE_READ_WASM_BYTES];
    uint8_t code_hash[32];
    uint8_t observed_id[32], unknown_id[32], halted_id[32];
    uint8_t observed_program[32], unknown_program[32], halted_program[32];
    uint8_t expected[LX_ORACLE_COMMITTED_BYTES];
    uint8_t encoded[LX_ORACLE_COMMITTED_BYTES];
    uint8_t primary_key[32] = {0};
    uint8_t actor_id[32], treasury_id[32];
    uint8_t fee_asset[32] = {9U};
    lxp_authority_scope scope = {0};
    lxp_arena arena;
    lxp_arena perps_arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx perps_ctx;
    lxp_module_ctx view_ctx;
    lxp_effect_buffer perps_effects;
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
    lx_perps_market observed_market, halted_market;
    lx_perps_oracle_state observation, loaded;
    lx_oracle_committed committed;
    uint64_t parameters = 1U;
    uint64_t identity_sequence = 0U;
    size_t payload_length;
    size_t wasm_length;
    (void)memset(observed_id, 0x11, sizeof(observed_id));
    (void)memset(unknown_id, 0x22, sizeof(unknown_id));
    (void)memset(halted_id, 0x33, sizeof(halted_id));
    (void)memset(observed_program, 0x71, sizeof(observed_program));
    (void)memset(unknown_program, 0x72, sizeof(unknown_program));
    (void)memset(halted_program, 0x73, sizeof(halted_program));
    (void)memset(&authority, 0, sizeof(authority));
    oracle_read_market(&observed_market, observed_id, false);
    oracle_read_market(&halted_market, halted_id, true);
    oracle_read_observation(&observation, observed_id);
    ORACLE_READ_CHECK(oracle_read_expected_record(&observed_market, &observation,
                                                  expected) == 0);
    ORACLE_READ_CHECK(executed_public_key(actor_seed, primary_key) == 0);
    ORACLE_READ_CHECK(lx_account_registry_init(&accounts) == LXP_OK);
    ORACLE_READ_CHECK(lx_account_id_from_string(actor_name,
        sizeof(actor_name) - 1U, actor_id) == LXP_OK);
    ORACLE_READ_CHECK(lx_account_id_from_string(treasury_name,
        sizeof(treasury_name) - 1U, treasury_id) == LXP_OK);
    ORACLE_READ_CHECK(lx_account_open(&accounts, actor_name,
        sizeof(actor_name) - 1U, actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL,
        &actor) == LXP_OK);
    ORACLE_READ_CHECK(lx_account_open(&accounts, treasury_name,
        sizeof(treasury_name) - 1U, treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS,
        NULL, &treasury) == LXP_OK);
    ORACLE_READ_CHECK(lxp_ledger_bootstrap_balance(actor, fee_asset,
        (lxp_u128){0U, UINT64_MAX}, 1U) == LXP_OK);
    ORACLE_READ_CHECK(lxp_ledger_bootstrap_balance(treasury, fee_asset,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    ORACLE_READ_CHECK(lxp_did_id_derive(did, sizeof(did) - 1U,
                                        authority.principal) == LXP_OK);
    (void)memcpy(authority.actor, authority.principal, 32U);
    (void)memcpy(authority.verified_key, primary_key, 32U);
    authority.kind = LXP_AUTHORITY_OWNER;
    scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    scope.activity_ordinal_min = 1U;
    scope.activity_ordinal_max = 7U;
    scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    scope.maximum_total = scope.maximum_per_activity;
    scope.maximum_per_period = scope.maximum_per_activity;
    authority.scope = &scope;
    ORACLE_READ_CHECK(lxp_authority_hash(authority.kind, grant_id, primary_key,
                                         authority.authority_hash) == LXP_OK);
    (void)memset(&fee_asset_state, 0, sizeof(fee_asset_state));
    (void)memcpy(fee_asset_state.asset_id, fee_asset, sizeof(fee_asset));
    fee_asset_state.registered = true;
    (void)memset(&runtime, 0, sizeof(runtime));
    runtime.accounts = &accounts;
    runtime.assets = &fee_asset_state;
    runtime.asset_count = 1U;
    runtime.fee_schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U
    };
    runtime.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
    runtime.metering_schedule_context = &kernel;
    (void)memcpy(runtime.occupancy_asset_id, fee_asset, 32U);
    runtime.resolve_occupancy_parameters = occupancy_parameters;
    runtime.occupancy_parameter_context = &runtime;
    fees.version = 1U;
    fees.multiplier_basis_points = 10000U;
    ORACLE_READ_CHECK(lxp_state_store_init(&state, 1U) == LXP_OK);
    ORACLE_READ_CHECK(lxp_identity_register(&identities, did, sizeof(did) - 1U,
                                            primary_key, &identity) == LXP_OK);
    ORACLE_READ_CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters,
                                        0U) == LXP_OK);
    ORACLE_READ_CHECK(install_metering_v1(&kernel) == LXP_OK);
    ORACLE_READ_CHECK(lxp_kernel_register_module(&kernel,
        programs_module_registration_v4()) == LXP_OK);
    ORACLE_READ_CHECK(lxp_kernel_register_module(&kernel,
        lx_perps_module_iface()) == LXP_OK);
    ORACLE_READ_CHECK(lxp_kernel_bind_module_runtime(&kernel,
        LXP_MODULE_PROGRAMS, &runtime) == LXP_OK);
    ORACLE_READ_CHECK(lxp_programs_bind_fee_transaction(&kernel) == LXP_OK);
    ORACLE_READ_CHECK(lxp_kernel_set_capabilities(&kernel, NULL,
        lxp_kernel_canonical_ledger_apply) == LXP_OK);
    ORACLE_READ_CHECK(lxp_state_root(&kernel, kernel.current_state_root) == LXP_OK);
    ORACLE_READ_CHECK(lxp_arena_init(&arena, arena_bytes,
                                     sizeof(arena_bytes)) == LXP_OK);
    (void)memset(&execution, 0, sizeof(execution));
    execution.network_id = 7U;
    execution.batch_number = 1U;
    execution.batch_timestamp_ms = 10U;
    execution.maximum_timestamp_window = 100U;
    execution.recorded_module_version = LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION;
    execution.parameter_version = 1U;
    execution.signature_valid = true;
    execution.identities = &identities;
    execution.authority = &authority;
    execution.fee_parameters = &fees;
    execution.gas_limit = 1000000U;
    execution.arena = &arena;
    {
        const uint8_t *ids[3];
        const uint8_t *programs[3];
        const uint8_t *records[3];
        size_t deploy;
        ids[0] = observed_id; ids[1] = unknown_id; ids[2] = halted_id;
        programs[0] = observed_program;
        programs[1] = unknown_program;
        programs[2] = halted_program;
        records[0] = expected; records[1] = NULL; records[2] = NULL;
        for (deploy = 0U; deploy < 3U; ++deploy) {
            wasm_length = oracle_read_module(wasm, ids[deploy], records[deploy]);
            ORACLE_READ_CHECK(wasm_length <= sizeof(wasm));
            payload_length = program_spend_deploy_payload(payload,
                programs[deploy], authority.principal, wasm, wasm_length,
                code_hash);
            fill_activity(&activity, LX_PROGRAMS_DEPLOY, payload,
                          payload_length, did, sizeof(did) - 1U, primary_key);
            activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
            activity.account_sequence = identity_sequence;
            activity.idempotency_key[31] = (uint8_t)(0x41U + deploy);
            execution.global_sequence = state.next_sequence;
            ORACLE_READ_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
            ORACLE_READ_CHECK(execute_artifact_fixture_activity(&kernel,
                &activity, &execution, &receipt) == LXP_OK);
            ORACLE_READ_CHECK(receipt.result_code == LXP_OK);
            ORACLE_READ_CHECK(identity->next_sequence == ++identity_sequence);
        }
    }
    /* The perps engine commits the market parameters and the accepted
     * observation through its own state writers; nothing else seeds the
     * oracle store the programs host binding later reads. */
    ORACLE_READ_CHECK(lxp_state_journal_open(&state, state.next_sequence,
                                             &journal) == LXP_OK);
    ORACLE_READ_CHECK(lxp_arena_init(&perps_arena, perps_arena_bytes,
                                     sizeof(perps_arena_bytes)) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_init(&perps_ctx, &kernel, LXP_MODULE_PERPS,
        10U, 0U, state.next_sequence, 100000U, &perps_arena, true) == LXP_OK);
    perps_ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    ORACLE_READ_CHECK(lxp_effect_buffer_init(&perps_effects) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_bind_effects(&perps_ctx,
                                                  &perps_effects) == LXP_OK);
    ORACLE_READ_CHECK(lx_perps_market_put(&perps_ctx, &observed_market) == LXP_OK);
    ORACLE_READ_CHECK(lx_perps_market_put(&perps_ctx, &halted_market) == LXP_OK);
    ORACLE_READ_CHECK(lx_perps_oracle_state_put(&perps_ctx,
                                                &observation) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_prepare_commit(&perps_ctx) == LXP_OK);
    ORACLE_READ_CHECK(lxp_state_journal_commit(&journal) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_commit(&perps_ctx) == LXP_OK);
    ORACLE_READ_CHECK(lxp_state_root(&kernel, kernel.current_state_root) == LXP_OK);
    ORACLE_READ_CHECK(lxp_arena_init(&perps_arena, perps_arena_bytes,
                                     sizeof(perps_arena_bytes)) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_init(&perps_ctx, &kernel, LXP_MODULE_PERPS,
        10U, 0U, state.next_sequence, 100000U, &perps_arena, false) == LXP_OK);
    ORACLE_READ_CHECK(lx_perps_oracle_state_lookup(&perps_ctx, observed_id,
                                                   &loaded) == LXP_OK);
    ORACLE_READ_CHECK(loaded.observation_sequence ==
                      observation.observation_sequence);
    ORACLE_READ_CHECK(lxp_u128_cmp(loaded.price, observation.price) == 0);
    ORACLE_READ_CHECK(loaded.observed_at == observation.observed_at);
    /* A guest reads exactly those bytes: the call only completes because every
     * i64 of the host record matched the expected image compiled into it. */
    payload_length = call_payload_with_capabilities(call, observed_program,
        no_capabilities, sizeof(no_capabilities));
    write_u16(call + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length, did,
                  sizeof(did) - 1U, primary_key);
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.account_sequence = identity_sequence;
    activity.idempotency_key[31] = 0x51U;
    activity.fee_limit = (lxp_u128){0U, 67108864U};
    execution.fee_balance = actor->balance;
    execution.global_sequence = state.next_sequence;
    ORACLE_READ_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
    ORACLE_READ_CHECK(execute_artifact_fixture_activity(&kernel, &activity,
        &execution, &receipt) == LXP_OK);
    ORACLE_READ_CHECK(receipt.result_code == LXP_OK);
    ORACLE_READ_CHECK(identity->next_sequence == ++identity_sequence);
    /* Unknown and halted markets refuse the call instead of answering it. */
    {
        const uint8_t *refused[2];
        size_t index;
        refused[0] = unknown_program;
        refused[1] = halted_program;
        for (index = 0U; index < 2U; ++index) {
            payload_length = call_payload_with_capabilities(call,
                refused[index], no_capabilities, sizeof(no_capabilities));
            write_u16(call + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
            fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                          did, sizeof(did) - 1U, primary_key);
            activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
            activity.account_sequence = identity_sequence;
            activity.idempotency_key[31] = (uint8_t)(0x61U + index);
            activity.fee_limit = (lxp_u128){0U, 67108864U};
            execution.fee_balance = actor->balance;
            execution.global_sequence = state.next_sequence;
            ORACLE_READ_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
            ORACLE_READ_CHECK(execute_artifact_fixture_activity(&kernel,
                &activity, &execution, &receipt) == LXP_OK);
            ORACLE_READ_CHECK(receipt.result_code == LXP_ERR_NON_CANONICAL);
            ORACLE_READ_CHECK(receipt.program_outcome.present);
            ORACLE_READ_CHECK(receipt.program_outcome.terminal_kind ==
                              LXP_PROGRAM_TERMINAL_FAILURE);
            ORACLE_READ_CHECK(identity->next_sequence == ++identity_sequence);
        }
    }
    /* The host binding itself distinguishes the two refusals by type. */
    ORACLE_READ_CHECK(lxp_arena_init(&perps_arena, perps_arena_bytes,
                                     sizeof(perps_arena_bytes)) == LXP_OK);
    ORACLE_READ_CHECK(lxp_module_ctx_init(&view_ctx, &kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, state.next_sequence, 100000U,
        &perps_arena, false) == LXP_OK);
    ORACLE_READ_CHECK(lx_oracle_committed_read(&view_ctx, observed_id,
                                               &committed) == LXP_OK);
    ORACLE_READ_CHECK(lx_oracle_committed_encode(&committed, encoded) == LXP_OK);
    ORACLE_READ_CHECK(memcmp(encoded, expected,
                             LX_ORACLE_COMMITTED_BYTES) == 0);
    ORACLE_READ_CHECK(lx_oracle_committed_read(&view_ctx, unknown_id,
                                               &committed) ==
                      LXP_ERR_UNKNOWN_FIELD);
    ORACLE_READ_CHECK(lx_oracle_committed_read(&view_ctx, halted_id,
                                               &committed) ==
                      LXP_ERR_MARKET_HALTED);
    ORACLE_READ_CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}

int main(void)
{
    if (oracle_read_case() != 0) return 1;
    return 0;
}
