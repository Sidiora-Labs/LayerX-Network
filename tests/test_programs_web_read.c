#define main web_read_reference_main
int web_read_reference_main(int argc, char **argv);
#include "programs/test_call_activity.c"
#undef main

#include "layerx/lx_web.h"

#define WEB_READ_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "programs web read check failed at line %d\n", \
                  __LINE__); \
    return 1; } } while (0)

enum {
    WEB_READ_WASM_BYTES = 1024,
    WEB_READ_REQUEST_OFFSET = 0,
    WEB_READ_EXPECTED_OFFSET = 128,
    WEB_READ_OUTPUT_OFFSET = 256,
    WEB_READ_OUTPUT_CAPACITY = LX_WEB_ANSWER_HEADER_BYTES +
        LX_WEB_MAX_RESPONSE_BYTES,
    WEB_READ_FULL_LENGTH = 5000,
    WEB_READ_LONG_RESPONSE = 1500,
    WEB_READ_STATUS_BOUNDS = -3,
    WEB_READ_STATUS_ABSENT = -7
};

static const uint8_t web_read_text[] = "Paxeer X Network";
static const uint64_t web_read_owned_request = UINT64_C(0x0102030405060708);
static const uint64_t web_read_absent_request = UINT64_C(0x0102030405060709);
static const uint64_t web_read_long_request = UINT64_C(0x010203040506070a);

enum {
    WEB_READ_TEXT_BYTES = sizeof(web_read_text) - 1U,
    WEB_READ_RECORD_BYTES = LX_WEB_ANSWER_HEADER_BYTES + WEB_READ_TEXT_BYTES
};

static void web_read_i32(uint8_t *body, size_t *length, int32_t value)
{
    int64_t remaining = value;
    bool more = true;
    body[(*length)++] = 0x41U;
    while (more) {
        uint8_t byte = (uint8_t)((uint64_t)remaining & 0x7fU);
        remaining = remaining < 0 ? -((-remaining + 127) / 128) :
                                    remaining / 128;
        more = !((remaining == 0 && (byte & 0x40U) == 0U) ||
                 (remaining == -1 && (byte & 0x40U) != 0U));
        if (more) byte |= 0x80U;
        body[(*length)++] = byte;
    }
}

/* ABI-v4 guest that reads the committed web answer for one request id. It
 * traps unless web_read returns the expected status and, when a record is
 * expected, unless every byte the host wrote equals that record. */
static size_t web_read_module(uint8_t *out, uint64_t request_id,
                              int32_t capacity, int32_t expected_status,
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
    uint8_t request[8];
    uint8_t section[512];
    uint8_t body[512];
    size_t cursor = 0U;
    size_t length = 0U;
    size_t body_length = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        request[index] = (uint8_t)(request_id >> (8U * index));
    append_bytes(out, &cursor, header, sizeof(header));
    append_section(out, &cursor, 1U, types, sizeof(types));
    section[length++] = 1U;
    append_name(section, &length, "layerx_v4");
    append_name(section, &length, "web_read");
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
    web_read_i32(body, &body_length, WEB_READ_REQUEST_OFFSET);
    web_read_i32(body, &body_length, 8);
    web_read_i32(body, &body_length, WEB_READ_OUTPUT_OFFSET);
    web_read_i32(body, &body_length, capacity);
    body[body_length++] = 0x10U;
    body[body_length++] = 0U;
    web_read_i32(body, &body_length, expected_status);
    body[body_length++] = 0x47U;
    body[body_length++] = 0x04U; body[body_length++] = 0x40U;
    body[body_length++] = 0x00U;
    body[body_length++] = 0x0bU;
    if (expected != NULL) {
        for (index = 0U; index < WEB_READ_RECORD_BYTES / 8U; ++index) {
            web_read_i32(body, &body_length, 0);
            body[body_length++] = 0x29U; body[body_length++] = 3U;
            append_u32_leb(body, &body_length,
                           (uint32_t)(WEB_READ_OUTPUT_OFFSET + index * 8U));
            web_read_i32(body, &body_length, 0);
            body[body_length++] = 0x29U; body[body_length++] = 3U;
            append_u32_leb(body, &body_length,
                           (uint32_t)(WEB_READ_EXPECTED_OFFSET + index * 8U));
            body[body_length++] = 0x52U;
            body[body_length++] = 0x04U; body[body_length++] = 0x40U;
            body[body_length++] = 0x00U;
            body[body_length++] = 0x0bU;
        }
    }
    web_read_i32(body, &body_length, 0);
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
    web_read_i32(section, &length, WEB_READ_REQUEST_OFFSET);
    section[length++] = 0x0bU;
    append_u32_leb(section, &length, 8U);
    append_bytes(section, &length, request, 8U);
    if (expected != NULL) {
        section[length++] = 0U;
        web_read_i32(section, &length, WEB_READ_EXPECTED_OFFSET);
        section[length++] = 0x0bU;
        append_u32_leb(section, &length, (uint32_t)WEB_READ_RECORD_BYTES);
        append_bytes(section, &length, expected, WEB_READ_RECORD_BYTES);
    }
    append_section(out, &cursor, 11U, section, length);
    return cursor;
}

static void web_read_observation(lx_web_observation *observation,
                                 const uint8_t program_id[32],
                                 uint64_t request_id, uint8_t digest,
                                 uint32_t full_length,
                                 const uint8_t *response,
                                 uint32_t response_length)
{
    (void)memset(observation, 0, sizeof(*observation));
    observation->origin = LX_WEB_ORIGIN_PROGRAM;
    observation->network_id = 7U;
    (void)memcpy(observation->program_id, program_id, 32U);
    observation->request_id = request_id;
    observation->kind = LX_WEB_KIND_FETCH;
    (void)memset(observation->content_digest, digest, 32U);
    observation->full_length = full_length;
    observation->response_length = response_length;
    (void)memcpy(observation->response, response, response_length);
}

static void web_read_expected_record(uint8_t record[WEB_READ_RECORD_BYTES])
{
    size_t index;
    (void)memset(record, 0x5a, 32U);
    for (index = 0U; index < 4U; ++index) {
        record[32U + index] =
            (uint8_t)((uint32_t)WEB_READ_FULL_LENGTH >> (8U * index));
        record[36U + index] =
            (uint8_t)((uint32_t)WEB_READ_TEXT_BYTES >> (8U * index));
    }
    (void)memcpy(record + LX_WEB_ANSWER_HEADER_BYTES, web_read_text,
                 WEB_READ_TEXT_BYTES);
}

static int web_read_case(void)
{
    static const uint8_t did[] = "did:lxp:programs-web-read";
    static const uint8_t actor_name[] = "agent:did:lxp:programs-web-read:main";
    static const uint8_t treasury_name[] = "system:fees";
    static const uint8_t actor_seed[32] = {0x33U};
    static const uint8_t grant_id[32] = {0};
    static const uint8_t no_capabilities[] = {0U, 0U};
    static uint8_t payload[104U + WEB_READ_WASM_BYTES];
    static uint8_t call[CALL_FIXED_BYTES + 256U];
    static uint8_t arena_bytes[2U * LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t web_arena_bytes[65536];
    static uint8_t long_response[WEB_READ_LONG_RESPONSE];
    static lx_web_observation observation;
    static lx_web_answer answer;
    uint8_t wasm[WEB_READ_WASM_BYTES];
    uint8_t code_hash[32];
    uint8_t owner_program[32], absent_program[32], stranger_program[32];
    uint8_t bounds_program[32];
    uint8_t expected[WEB_READ_RECORD_BYTES];
    uint8_t primary_key[32] = {0};
    uint8_t actor_id[32], treasury_id[32];
    uint8_t fee_asset[32] = {9U};
    lxp_authority_scope scope = {0};
    lxp_arena arena;
    lxp_arena web_arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx web_ctx;
    lxp_module_ctx view_ctx;
    lxp_effect_buffer web_effects;
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
    uint64_t identity_sequence = 0U;
    size_t payload_length;
    size_t wasm_length;
    size_t index;
    (void)memset(owner_program, 0x71, sizeof(owner_program));
    (void)memset(absent_program, 0x72, sizeof(absent_program));
    (void)memset(stranger_program, 0x73, sizeof(stranger_program));
    (void)memset(bounds_program, 0x74, sizeof(bounds_program));
    for (index = 0U; index < sizeof(long_response); ++index)
        long_response[index] = (uint8_t)(index % 251U);
    (void)memset(&authority, 0, sizeof(authority));
    web_read_expected_record(expected);
    WEB_READ_CHECK(executed_public_key(actor_seed, primary_key) == 0);
    WEB_READ_CHECK(lx_account_registry_init(&accounts) == LXP_OK);
    WEB_READ_CHECK(lx_account_id_from_string(actor_name,
        sizeof(actor_name) - 1U, actor_id) == LXP_OK);
    WEB_READ_CHECK(lx_account_id_from_string(treasury_name,
        sizeof(treasury_name) - 1U, treasury_id) == LXP_OK);
    WEB_READ_CHECK(lx_account_open(&accounts, actor_name,
        sizeof(actor_name) - 1U, actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL,
        &actor) == LXP_OK);
    WEB_READ_CHECK(lx_account_open(&accounts, treasury_name,
        sizeof(treasury_name) - 1U, treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS,
        NULL, &treasury) == LXP_OK);
    WEB_READ_CHECK(lxp_ledger_bootstrap_balance(actor, fee_asset,
        (lxp_u128){0U, UINT64_MAX}, 1U) == LXP_OK);
    WEB_READ_CHECK(lxp_ledger_bootstrap_balance(treasury, fee_asset,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    WEB_READ_CHECK(lxp_did_id_derive(did, sizeof(did) - 1U,
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
    WEB_READ_CHECK(lxp_authority_hash(authority.kind, grant_id, primary_key,
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
    WEB_READ_CHECK(lxp_state_store_init(&state, 1U) == LXP_OK);
    WEB_READ_CHECK(lxp_identity_register(&identities, did, sizeof(did) - 1U,
                                         primary_key, &identity) == LXP_OK);
    WEB_READ_CHECK(lxp_kernel_create(&kernel, &state, &journal, &parameters,
                                     0U) == LXP_OK);
    WEB_READ_CHECK(install_metering_v1(&kernel) == LXP_OK);
    WEB_READ_CHECK(lxp_kernel_register_module(&kernel,
        programs_module_registration_v4()) == LXP_OK);
    WEB_READ_CHECK(lxp_kernel_bind_module_runtime(&kernel,
        LXP_MODULE_PROGRAMS, &runtime) == LXP_OK);
    WEB_READ_CHECK(lxp_programs_bind_fee_transaction(&kernel) == LXP_OK);
    WEB_READ_CHECK(lxp_kernel_set_capabilities(&kernel, NULL,
        lxp_kernel_canonical_ledger_apply) == LXP_OK);
    WEB_READ_CHECK(lxp_state_root(&kernel, kernel.current_state_root) == LXP_OK);
    WEB_READ_CHECK(lxp_arena_init(&arena, arena_bytes,
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
        const uint8_t *programs[4];
        const uint8_t *records[4];
        uint64_t requests[4];
        int32_t capacities[4];
        int32_t statuses[4];
        size_t deploy;
        programs[0] = owner_program;
        programs[1] = absent_program;
        programs[2] = stranger_program;
        programs[3] = bounds_program;
        records[0] = expected; records[1] = NULL;
        records[2] = NULL; records[3] = NULL;
        requests[0] = web_read_owned_request;
        requests[1] = web_read_absent_request;
        requests[2] = web_read_owned_request;
        requests[3] = web_read_long_request;
        capacities[0] = WEB_READ_OUTPUT_CAPACITY;
        capacities[1] = WEB_READ_OUTPUT_CAPACITY;
        capacities[2] = WEB_READ_OUTPUT_CAPACITY;
        capacities[3] = LX_WEB_ANSWER_HEADER_BYTES + WEB_READ_LONG_RESPONSE - 1;
        statuses[0] = WEB_READ_RECORD_BYTES;
        statuses[1] = WEB_READ_STATUS_ABSENT;
        statuses[2] = WEB_READ_STATUS_ABSENT;
        statuses[3] = WEB_READ_STATUS_BOUNDS;
        for (deploy = 0U; deploy < 4U; ++deploy) {
            wasm_length = web_read_module(wasm, requests[deploy],
                                          capacities[deploy],
                                          statuses[deploy], records[deploy]);
            WEB_READ_CHECK(wasm_length <= sizeof(wasm));
            payload_length = program_spend_deploy_payload(payload,
                programs[deploy], authority.principal, wasm, wasm_length,
                code_hash);
            write_u16(payload + 32U, LX_PROGRAMS_GUEST_ABI_V4_VERSION);
            fill_activity(&activity, LX_PROGRAMS_DEPLOY, payload,
                          payload_length, did, sizeof(did) - 1U, primary_key);
            activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
            activity.account_sequence = identity_sequence;
            activity.idempotency_key[31] = (uint8_t)(0x41U + deploy);
            execution.global_sequence = state.next_sequence;
            WEB_READ_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
            WEB_READ_CHECK(execute_artifact_fixture_activity(&kernel,
                &activity, &execution, &receipt) == LXP_OK);
            WEB_READ_CHECK(receipt.result_code == LXP_OK);
            WEB_READ_CHECK(identity->next_sequence == ++identity_sequence);
        }
    }
    /* Answers are committed in the programs module storage through the web
     * module's own writer; nothing else seeds what the host binding reads. */
    WEB_READ_CHECK(lxp_state_journal_open(&state, state.next_sequence,
                                          &journal) == LXP_OK);
    WEB_READ_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                                  sizeof(web_arena_bytes)) == LXP_OK);
    WEB_READ_CHECK(lxp_module_ctx_init(&web_ctx, &kernel, LXP_MODULE_PROGRAMS,
        10U, 0U, state.next_sequence, 100000U, &web_arena, true) == LXP_OK);
    web_ctx.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    WEB_READ_CHECK(lxp_effect_buffer_init(&web_effects) == LXP_OK);
    WEB_READ_CHECK(lxp_module_ctx_bind_effects(&web_ctx,
                                               &web_effects) == LXP_OK);
    web_read_observation(&observation, owner_program, web_read_owned_request,
                         0x5aU, WEB_READ_FULL_LENGTH, web_read_text,
                         WEB_READ_TEXT_BYTES);
    WEB_READ_CHECK(lx_web_committed_put(&web_ctx, &observation) == LXP_OK);
    web_read_observation(&observation, bounds_program, web_read_long_request,
                         0x6bU, WEB_READ_LONG_RESPONSE, long_response,
                         WEB_READ_LONG_RESPONSE);
    WEB_READ_CHECK(lx_web_committed_put(&web_ctx, &observation) == LXP_OK);
    /* Staged answers are not committed and never visible to a read. */
    WEB_READ_CHECK(lx_web_committed_read(&web_ctx, owner_program,
                                         web_read_owned_request, &answer) ==
                   LXP_ERR_UNKNOWN_FIELD);
    WEB_READ_CHECK(lxp_module_ctx_prepare_commit(&web_ctx) == LXP_OK);
    WEB_READ_CHECK(lxp_state_journal_commit(&journal) == LXP_OK);
    WEB_READ_CHECK(lxp_module_ctx_commit(&web_ctx) == LXP_OK);
    WEB_READ_CHECK(lxp_state_root(&kernel, kernel.current_state_root) == LXP_OK);
    /* Each guest traps unless the host returns exactly the expected status and
     * record, so a successful receipt proves the read it compiled in. */
    {
        const uint8_t *callers[4];
        size_t caller;
        callers[0] = owner_program;
        callers[1] = absent_program;
        callers[2] = stranger_program;
        callers[3] = bounds_program;
        for (caller = 0U; caller < 4U; ++caller) {
            payload_length = call_payload_with_capabilities(call,
                callers[caller], no_capabilities, sizeof(no_capabilities));
            write_u16(call + 32U, LX_PROGRAMS_GUEST_ABI_V4_VERSION);
            fill_activity(&activity, LX_PROGRAMS_CALL, call, payload_length,
                          did, sizeof(did) - 1U, primary_key);
            activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
            activity.account_sequence = identity_sequence;
            activity.idempotency_key[31] = (uint8_t)(0x51U + caller);
            activity.fee_limit = (lxp_u128){0U, 67108864U};
            execution.fee_balance = actor->balance;
            execution.global_sequence = state.next_sequence;
            WEB_READ_CHECK(lxp_arena_reset(&arena, 0U) == LXP_OK);
            WEB_READ_CHECK(execute_artifact_fixture_activity(&kernel,
                &activity, &execution, &receipt) == LXP_OK);
            WEB_READ_CHECK(receipt.result_code == LXP_OK);
            WEB_READ_CHECK(identity->next_sequence == ++identity_sequence);
        }
    }
    /* The committed read itself: present, absent and another program's. */
    WEB_READ_CHECK(lxp_arena_init(&web_arena, web_arena_bytes,
                                  sizeof(web_arena_bytes)) == LXP_OK);
    WEB_READ_CHECK(lxp_module_ctx_init(&view_ctx, &kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, state.next_sequence, 100000U,
        &web_arena, false) == LXP_OK);
    WEB_READ_CHECK(lx_web_committed_read(&view_ctx, owner_program,
                                         web_read_owned_request, &answer) ==
                   LXP_OK);
    WEB_READ_CHECK(memcmp(answer.program_id, owner_program, 32U) == 0 &&
                   answer.request_id == web_read_owned_request &&
                   answer.full_length == WEB_READ_FULL_LENGTH &&
                   answer.response_length == WEB_READ_TEXT_BYTES &&
                   memcmp(answer.content_digest, expected, 32U) == 0 &&
                   memcmp(answer.response, web_read_text,
                          WEB_READ_TEXT_BYTES) == 0);
    WEB_READ_CHECK(lx_web_committed_read(&view_ctx, bounds_program,
                                         web_read_long_request, &answer) ==
                   LXP_OK);
    WEB_READ_CHECK(answer.response_length == WEB_READ_LONG_RESPONSE &&
                   memcmp(answer.response, long_response,
                          WEB_READ_LONG_RESPONSE) == 0);
    WEB_READ_CHECK(lx_web_committed_read(&view_ctx, owner_program,
                                         web_read_absent_request, &answer) ==
                   LXP_ERR_UNKNOWN_FIELD);
    WEB_READ_CHECK(lx_web_committed_read(&view_ctx, stranger_program,
                                         web_read_owned_request, &answer) ==
                   LXP_ERR_UNKNOWN_FIELD);
    WEB_READ_CHECK(lxp_state_store_destroy(&state) == LXP_OK);
    return 0;
}

int main(void)
{
    if (web_read_case() != 0) return 1;
    return 0;
}
