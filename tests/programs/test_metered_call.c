#define main metered_fixture_reference_main
int metered_fixture_reference_main(int argc, char **argv);
#include "test_call_activity.c"
#undef main
#include "layerx/lxp_batch_identity.h"

#define METERED_CHECK(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "metered call check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

typedef struct metered_fixture {
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lxp_transfer_asset_state asset;
    lx_programs_transfer_runtime runtime;
    lxp_fee_params fees;
    lxp_arena arena;
    uint8_t arena_bytes[4U * LXP_MAX_ACTIVITY_BYTES];
    lxp_identity *identity;
    lx_account *actor;
    lx_account *payee;
    lx_account *source;
    uint64_t parameters;
    uint8_t owner_key[32];
    uint8_t delegate_key[32];
    uint8_t program[32];
    uint8_t source_id[32];
    uint8_t grant_id[32];
    uint8_t call[1024];
    size_t call_length;
    lxp_authority_grant grant;
    lxp_authority_resolved authority;
    lxp_transfer_allowance allowance;
    lxp_kernel_execution execution;
    lxp_activity activity;
    lxp_receipt receipt;
    uint8_t signature[64];
} metered_fixture;

static const uint8_t metered_owner_seed[32] = {0x33U};
static const uint8_t metered_delegate_seed[32] = {0x34U};
static const uint8_t metered_did[] = "did:lxp:metered-call";
static const uint8_t metered_program_seed[] = "metered-vault";

static void metered_i32(uint8_t *body, size_t *length, uint32_t value)
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

static size_t metered_mixed_module(uint8_t *out, const metered_fixture *f)
{
    static const uint8_t header[] = {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    static const uint8_t types[] = {
        4U,
        0x60U, 6U, 0x7eU, 0x7eU, 0x7fU, 0x7fU, 0x7fU, 0x7fU, 1U, 0x7fU,
        0x60U, 10U, 0x7eU, 0x7eU, 0x7fU, 0x7fU, 0x7fU, 0x7fU, 0x7fU, 0x7fU,
        0x7fU, 0x7fU, 1U, 0x7fU,
        0x60U, 1U, 0x7fU, 1U, 0x7fU,
        0x60U, 2U, 0x7fU, 0x7fU, 1U, 0x7fU
    };
    static const uint8_t functions[] = {2U, 2U, 3U};
    static const uint8_t memory[] = {1U, 1U, 1U, 1U};
    uint8_t section[512];
    uint8_t body[192];
    size_t cursor = 0U, length = 0U, body_length = 0U, index;
    append_bytes(out, &cursor, header, sizeof(header));
    append_section(out, &cursor, 1U, types, sizeof(types));
    section[length++] = 2U;
    append_name(section, &length, "layerx_v1");
    append_name(section, &length, "transfer_402");
    section[length++] = 0U; section[length++] = 0U;
    append_name(section, &length, "layerx_v2");
    append_name(section, &length, "transfer_program_402");
    section[length++] = 0U; section[length++] = 1U;
    append_section(out, &cursor, 2U, section, length);
    append_section(out, &cursor, 3U, functions, sizeof(functions));
    append_section(out, &cursor, 5U, memory, sizeof(memory));
    length = 0U;
    section[length++] = 3U;
    append_name(section, &length, "layerx_reserve");
    section[length++] = 0U; section[length++] = 2U;
    append_name(section, &length, "layerx_call");
    section[length++] = 0U; section[length++] = 3U;
    append_name(section, &length, "memory");
    section[length++] = 2U; section[length++] = 0U;
    append_section(out, &cursor, 7U, section, length);
    body[body_length++] = 1U;
    body[body_length++] = 1U;
    body[body_length++] = 0x7fU;
    for (index = 0U; index < 2U; ++index) {
        body[body_length++] = 0x42U; body[body_length++] = 0U;
        body[body_length++] = 0x42U; body[body_length++] = 2U;
        metered_i32(body, &body_length, 160U);
        metered_i32(body, &body_length, 32U);
        metered_i32(body, &body_length, 192U);
        metered_i32(body, &body_length, 32U);
        body[body_length++] = 0x10U; body[body_length++] = 0U;
        body[body_length++] = 0x22U; body[body_length++] = 2U;
        body[body_length++] = 0x04U; body[body_length++] = 0x40U;
        body[body_length++] = 0x20U; body[body_length++] = 2U;
        body[body_length++] = 0x0fU; body[body_length++] = 0x0bU;
    }
    body[body_length++] = 0x42U; body[body_length++] = 0U;
    body[body_length++] = 0x42U; body[body_length++] = 5U;
    metered_i32(body, &body_length, 0U);
    metered_i32(body, &body_length, sizeof(metered_program_seed) - 1U);
    metered_i32(body, &body_length, 128U);
    metered_i32(body, &body_length, 32U);
    metered_i32(body, &body_length, 160U);
    metered_i32(body, &body_length, 32U);
    metered_i32(body, &body_length, 192U);
    metered_i32(body, &body_length, 32U);
    body[body_length++] = 0x10U; body[body_length++] = 1U;
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
    section[length++] = 2U;
    section[length++] = 0U;
    metered_i32(section, &length, 0U);
    section[length++] = 0x0bU;
    append_u32_leb(section, &length, sizeof(metered_program_seed) - 1U);
    append_bytes(section, &length, metered_program_seed,
                  sizeof(metered_program_seed) - 1U);
    section[length++] = 0U;
    metered_i32(section, &length, 128U);
    section[length++] = 0x0bU;
    append_u32_leb(section, &length, 96U);
    append_bytes(section, &length, f->source_id, 32U);
    append_bytes(section, &length, f->asset.asset_id, 32U);
    append_bytes(section, &length, f->payee->id, 32U);
    append_section(out, &cursor, 11U, section, length);
    return cursor;
}

static int metered_account(metered_fixture *f, const char *name,
                            uint64_t balance, lx_account **account)
{
    uint8_t id[32];
    METERED_CHECK(lx_account_id_from_string((const uint8_t *)name,
                                            strlen(name), id) == LXP_OK);
    METERED_CHECK(lx_account_open(&f->accounts, (const uint8_t *)name,
                                   strlen(name), id, 1U,
                                   LX_ACCOUNT_OPEN_GENESIS, NULL, account) ==
                  LXP_OK);
    METERED_CHECK(lxp_ledger_bootstrap_balance(*account, f->asset.asset_id,
                                              (lxp_u128){0U, balance}, 0U) ==
                  LXP_OK);
    return 0;
}

static int metered_activity(metered_fixture *f, uint32_t type,
                             const uint8_t *payload, size_t length,
                             uint8_t marker, bool delegated)
{
    const uint8_t *key = delegated ? f->delegate_key : f->owner_key;
    const uint8_t *seed = delegated ? metered_delegate_seed : metered_owner_seed;
    uint8_t digest[32];
    lxp_byte_span encoded;
    EVP_PKEY *signer_key;
    EVP_MD_CTX *signer_context;
    size_t signature_length = sizeof(f->signature);
    METERED_CHECK(lxp_arena_reset(&f->arena, 0U) == LXP_OK);
    fill_activity(&f->activity, type, payload, length, metered_did,
                   sizeof(metered_did) - 1U, key);
    f->activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    f->activity.account_sequence = f->identity->next_sequence;
    f->activity.idempotency_key[31] = marker;
    f->activity.fee_limit = (lxp_u128){0U, 67108864U};
    f->activity.signature = (lxp_byte_span){f->signature, sizeof(f->signature)};
    METERED_CHECK(lxp_hash_payload(payload, length, f->activity.payload_hash) ==
                  LXP_OK);
    METERED_CHECK(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    signer_key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    signer_context = EVP_MD_CTX_new();
    METERED_CHECK(signer_key != NULL && signer_context != NULL);
    METERED_CHECK(EVP_DigestSignInit(signer_context, NULL, NULL, NULL,
                                     signer_key) == 1);
    METERED_CHECK(EVP_DigestSign(signer_context, f->signature, &signature_length,
                                digest, sizeof(digest)) == 1);
    EVP_MD_CTX_free(signer_context);
    EVP_PKEY_free(signer_key);
    METERED_CHECK(signature_length == sizeof(f->signature));
    METERED_CHECK(lxp_activity_verify_signature(&f->activity) == LXP_OK);
    METERED_CHECK(lxp_authority_resolve_activity(&f->kernel, f->identity,
        &f->activity, !delegated, true, 10U, 100U, f->state.next_sequence,
        &f->grant, &f->authority) == LXP_OK);
    lxp_authority_allowance_bind(&f->grant, &f->authority, &f->allowance);
    (void)memset(&f->execution, 0, sizeof(f->execution));
    f->execution.network_id = 7U;
    f->execution.batch_number = 1U;
    f->execution.batch_timestamp_ms = 10U;
    f->execution.maximum_timestamp_window = 100U;
    f->execution.global_sequence = f->state.next_sequence;
    f->execution.recorded_module_version = LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION;
    f->execution.recorded_metering_schedule_version = 1U;
    f->execution.recorded_fee_schedule_version = 1U;
    f->execution.parameter_version = 1U;
    f->execution.signature_valid = true;
    f->execution.sequencer_private_key = executed_sequencer_seed;
    f->execution.identities = &f->identities;
    f->execution.authority = &f->authority;
    f->execution.allowance = &f->allowance;
    f->execution.fee_parameters = &f->fees;
    f->execution.fee_balance = f->actor->balance;
    f->execution.gas_limit = 1000000U;
    f->execution.arena = &f->arena;
    METERED_CHECK(lxp_activity_encode(&f->activity, &f->arena, &encoded) == LXP_OK);
    METERED_CHECK(lxp_activity_id(encoded.bytes, encoded.length, digest) == LXP_OK);
    METERED_CHECK(lxp_batch_identity_activity(f->kernel.current_state_root,
        digest, f->state.next_sequence, 1U, f->execution.batch_id) == LXP_OK);
    return 0;
}

static int metered_fixture_init(metered_fixture *f, uint64_t activity_limit,
                                bool enforce_allowances)
{
    lx_account *treasury;
    lxp_module_ctx registration;
    lxp_effect_buffer effects;
    lxp_genesis_manifest manifest = {0};
    lx_programs_fee_genesis_parameters fees = {0};
    uint8_t wasm[1024], payload[1128], code_hash[32], capabilities[512];
    uint8_t program_capabilities[PROGRAM_SPEND_CAPABILITY_BYTES];
    lxp_byte_span encoded;
    lxp_module_kv_entry *record;
    size_t wasm_length, length, program_length;
    bool created = false;
    METERED_CHECK(executed_public_key(metered_owner_seed, f->owner_key) == 0);
    METERED_CHECK(executed_public_key(metered_delegate_seed, f->delegate_key) == 0);
    f->asset.asset_id[0] = 9U;
    f->asset.registered = true;
    f->program[0] = 0x47U;
    METERED_CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    METERED_CHECK(metered_account(f, "agent:did:lxp:metered-call:main",
                                   UINT64_C(1000000000), &f->actor) == 0);
    METERED_CHECK(metered_account(f, "agent:did:lxp:metered-payee:main", 0U,
                                   &f->payee) == 0);
    METERED_CHECK(metered_account(f, "system:fees", 0U, &treasury) == 0);
    f->actor->has_authority_key = true;
    (void)memcpy(f->actor->authority_key, f->owner_key, 32U);
    METERED_CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    METERED_CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    f->parameters = 1U;
    METERED_CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                                     &f->parameters, 0U) == LXP_OK);
    METERED_CHECK(install_metering_v1(&f->kernel) == LXP_OK);
    METERED_CHECK(lxp_kernel_register_module(&f->kernel,
                    programs_module_registration_v4()) == LXP_OK);
    f->runtime.accounts = &f->accounts;
    f->runtime.assets = &f->asset;
    f->runtime.asset_count = 1U;
    f->runtime.fee_schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U
    };
    (void)memcpy(f->runtime.occupancy_asset_id, f->asset.asset_id, 32U);
    f->runtime.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
    f->runtime.metering_schedule_context = &f->kernel;
    f->runtime.resolve_occupancy_parameters = lxp_programs_fee_governance_resolve_runtime;
    f->runtime.occupancy_parameter_context = &f->kernel;
    (void)memcpy(manifest.signer_public_key, f->owner_key, 32U);
    fees.schedule = f->runtime.fee_schedule;
    (void)memcpy(fees.occupancy_asset_id, f->asset.asset_id, 32U);
    fees.target_occupancy_byte_batches = 3U;
    fees.response_denominator = 1U;
    fees.maximum_change_numerator = 1U;
    fees.maximum_change_denominator = 1U;
    fees.minimum_fee_units_per_occupancy_byte_batch = 1U;
    fees.maximum_fee_units_per_occupancy_byte_batch = 10U;
    METERED_CHECK(lxp_programs_fee_genesis_append(&manifest, &fees) == LXP_OK);
    METERED_CHECK(lxp_programs_fee_genesis_materialize(&manifest, &f->kernel) == LXP_OK);
    if (enforce_allowances) {
        lxp_module_kv_entry *policy = &f->kernel.module_kv[f->kernel.module_kv_count++];
        (void)memset(policy, 0, sizeof(*policy));
        policy->module_id = LXP_MODULE_GOVERNANCE;
        policy->key_length = 32U;
        (void)memcpy(policy->key, LXP_NATIVE_FEE_AUTHORITY_PARAMETER,
                     sizeof(LXP_NATIVE_FEE_AUTHORITY_PARAMETER) - 1U);
        policy->value_length = 32U;
        policy->value[31] = 2U;
    }
    METERED_CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_PROGRAMS,
                                                  &f->runtime) == LXP_OK);
    METERED_CHECK(lxp_programs_bind_fee_transaction(&f->kernel) == LXP_OK);
    METERED_CHECK(lxp_kernel_set_capabilities(&f->kernel, NULL,
                    lxp_kernel_canonical_ledger_apply) == LXP_OK);
    METERED_CHECK(lxp_identity_register(&f->identities, metered_did,
        sizeof(metered_did) - 1U, f->owner_key, &f->identity) == LXP_OK);
    f->identity->revocation_sequence = 1U;
    f->fees.version = 1U;
    f->fees.multiplier_basis_points = 10000U;
    METERED_CHECK(lxp_arena_init(&f->arena, f->arena_bytes,
                                 sizeof(f->arena_bytes)) == LXP_OK);
    METERED_CHECK(lxp_programs_account_derive(f->program, metered_program_seed,
        sizeof(metered_program_seed) - 1U, f->source_id) == LXP_OK);
    wasm_length = metered_mixed_module(wasm, f);
    length = program_spend_deploy_payload(payload, f->program,
        f->identity->did_id, wasm, wasm_length, code_hash);
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_DEPLOY, payload, length, 1U,
                                   false) == 0);
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_OK);
    METERED_CHECK(f->receipt.result_code == LXP_OK);
    METERED_CHECK(lxp_arena_reset(&f->arena, 0U) == LXP_OK);
    METERED_CHECK(lxp_state_journal_open(&f->state, f->state.next_sequence,
                                         &f->journal) == LXP_OK);
    METERED_CHECK(lxp_module_ctx_init(&registration, &f->kernel,
        LXP_MODULE_PROGRAMS, 10U, 0U, f->state.next_sequence, 100000U,
        &f->arena, true) == LXP_OK);
    registration.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    METERED_CHECK(lxp_effect_buffer_init(&effects) == LXP_OK);
    METERED_CHECK(lxp_module_ctx_bind_effects(&registration, &effects) == LXP_OK);
    METERED_CHECK(lxp_programs_account_register(&registration, f->program,
        metered_program_seed, sizeof(metered_program_seed) - 1U,
        f->asset.asset_id, &f->source, &created) == LXP_OK);
    METERED_CHECK(created && f->source != NULL);
    METERED_CHECK(lxp_module_ctx_prepare_commit(&registration) == LXP_OK);
    METERED_CHECK(lxp_state_journal_commit(&f->journal) == LXP_OK);
    METERED_CHECK(lxp_module_ctx_commit(&registration) == LXP_OK);
    f->source = program_spend_account(&f->accounts, f->source_id);
    METERED_CHECK(f->source != NULL);
    METERED_CHECK(lxp_ledger_bootstrap_balance(f->source, f->asset.asset_id,
                                               (lxp_u128){0U, 40U}, 0U) == LXP_OK);
    (void)memset(&f->grant, 0, sizeof(f->grant));
    (void)memcpy(f->grant.grantor, f->identity->did_id, 32U);
    (void)memcpy(f->grant.grantee, f->identity->did_id, 32U);
    (void)memcpy(f->grant.key, f->delegate_key, 32U);
    f->grant.kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    f->grant.scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    f->grant.scope.activity_ordinal_min = 3U;
    f->grant.scope.activity_ordinal_max = 3U;
    (void)memcpy(f->grant.scope.asset_id, f->asset.asset_id, 32U);
    f->grant.scope.maximum_per_activity.lo = activity_limit;
    f->grant.scope.maximum_total.lo = 7U;
    f->grant.scope.purpose_hash[0] = 1U;
    f->grant.not_before = 1U;
    f->grant.not_after = 1000U;
    f->grant.grantor_revocation_sequence = 1U;
    f->grant.fee_budget.present = enforce_allowances;
    (void)memcpy(f->grant.fee_budget.asset_id, f->asset.asset_id, 32U);
    f->grant.fee_budget.maximum_per_activity = (lxp_u128){0U, 67108864U};
    f->grant.fee_budget.maximum_total = (lxp_u128){0U, 268435456U};
    METERED_CHECK(lxp_grant_id_compute(&f->grant, f->grant.grant_id) == LXP_OK);
    (void)memcpy(f->grant_id, f->grant.grant_id, 32U);
    METERED_CHECK(lxp_grant_encode(&f->grant, &f->arena, &encoded) == LXP_OK);
    METERED_CHECK(encoded.length <= LXP_MODULE_MAX_VALUE_BYTES);
    record = &f->kernel.module_kv[f->kernel.module_kv_count++];
    (void)memset(record, 0, sizeof(*record));
    record->module_id = LXP_MODULE_GOVERNANCE;
    record->key_length = 33U;
    record->key[0] = 5U;
    (void)memcpy(record->key + 1U, f->grant_id, 32U);
    record->value_length = (uint32_t)encoded.length;
    (void)memcpy(record->value, encoded.bytes, encoded.length);
    program_length = program_spend_capabilities(program_capabilities,
        f->program, metered_program_seed, sizeof(metered_program_seed) - 1U,
        f->source_id, f->asset.asset_id, f->payee->id, 20U);
    write_u16(capabilities, 2U);
    capabilities[2] = 5U;
    (void)memcpy(capabilities + 3U, f->asset.asset_id, 32U);
    (void)memcpy(capabilities + 35U, f->payee->id, 32U);
    (void)memset(capabilities + 67U, 0, 16U);
    write_u64(capabilities + 75U, 20U);
    (void)memcpy(capabilities + 83U, program_capabilities + 2U,
                  program_length - 2U);
    f->call_length = call_payload_with_capabilities(f->call, f->program,
        capabilities, 83U + program_length - 2U);
    write_u16(f->call + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    return 0;
}

static int metered_spent(const metered_fixture *f, uint64_t expected)
{
    lxp_authority_grant loaded;
    METERED_CHECK(lxp_authority_grant_load(&f->kernel, f->grant_id,
                                           &loaded) == LXP_OK);
    METERED_CHECK(loaded.scope.spent_total.hi == 0U &&
                  loaded.scope.spent_total.lo == expected);
    return 0;
}

static int metered_fees(const metered_fixture *f, lxp_u128 *total)
{
    lxp_authority_grant loaded;
    METERED_CHECK(!lxp_u128_is_zero(f->receipt.fee_charged));
    METERED_CHECK(lxp_u128_add(*total, f->receipt.fee_charged, total) == LXP_OK);
    METERED_CHECK(lxp_authority_grant_load(&f->kernel, f->grant_id, &loaded) == LXP_OK);
    METERED_CHECK(loaded.fee_budget.present &&
        lxp_u128_cmp(loaded.fee_budget.spent_total, *total) == 0 &&
        lxp_u128_cmp(loaded.fee_budget.spent_this_period, *total) == 0);
    return 0;
}

static int metered_legacy_replay(void)
{
    metered_fixture *live = calloc(1U, sizeof(*live));
    metered_fixture *replay = calloc(1U, sizeof(*replay));
    uint8_t live_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t replay_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t expected_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t expected_root[32];
    lxp_arena live_arena, replay_arena;
    lxp_byte_span live_receipt, replay_receipt;
    FILE *historical = fopen("tests/fixtures/authority/legacy-paid-grant-replay.bin", "rb");
    METERED_CHECK(historical != NULL);
    METERED_CHECK(live != NULL && replay != NULL);
    METERED_CHECK(metered_fixture_init(live, 3U, false) == 0);
    METERED_CHECK(metered_fixture_init(replay, 3U, false) == 0);
    METERED_CHECK(!live->grant.fee_budget.present && !replay->grant.fee_budget.present);
    METERED_CHECK(memcmp(live->grant_id, replay->grant_id, 32U) == 0);
    METERED_CHECK(fread(expected_root, 1U, 32U, historical) == 32U);
    METERED_CHECK(memcmp(live->grant_id, expected_root, 32U) == 0);
    METERED_CHECK(fread(expected_root, 1U, 32U, historical) == 32U);
    METERED_CHECK(memcmp(live->kernel.current_state_root, expected_root, 32U) == 0);
    for (uint8_t marker = 7U; marker <= 9U; ++marker) {
        uint8_t encoded_length[4];
        size_t expected_length;
        live->source->frozen = marker == 7U;
        replay->source->frozen = marker == 7U;
        METERED_CHECK(lxp_state_root(&live->kernel, live->kernel.current_state_root) == LXP_OK);
        METERED_CHECK(lxp_state_root(&replay->kernel, replay->kernel.current_state_root) == LXP_OK);
        METERED_CHECK(metered_activity(live, LX_PROGRAMS_CALL, live->call,
                                       live->call_length, marker, true) == 0);
        METERED_CHECK(metered_activity(replay, LX_PROGRAMS_CALL, replay->call,
                                       replay->call_length, marker, true) == 0);
        replay->execution.allowance = NULL;
        METERED_CHECK(lxp_kernel_execute_activity(&live->kernel, &live->activity,
                        &live->execution, &live->receipt) == LXP_OK);
        METERED_CHECK(lxp_kernel_execute_activity(&replay->kernel, &replay->activity,
                        &replay->execution, &replay->receipt) == LXP_OK);
        METERED_CHECK(live->receipt.result_code ==
            (marker == 7U ? LXP_ERR_PROGRAM_REFUSED : LXP_OK));
        METERED_CHECK(!lxp_u128_is_zero(live->receipt.fee_charged));
        METERED_CHECK(metered_spent(live, 0U) == 0 && metered_spent(replay, 0U) == 0);
        METERED_CHECK(memcmp(live->kernel.current_state_root,
                             replay->kernel.current_state_root, 32U) == 0);
        METERED_CHECK(lxp_arena_init(&live_arena, live_bytes, sizeof(live_bytes)) == LXP_OK);
        METERED_CHECK(lxp_arena_init(&replay_arena, replay_bytes, sizeof(replay_bytes)) == LXP_OK);
        METERED_CHECK(lxp_receipt_encode(&live->receipt, true, &live_arena, &live_receipt) == LXP_OK);
        METERED_CHECK(lxp_receipt_encode(&replay->receipt, true, &replay_arena, &replay_receipt) == LXP_OK);
        METERED_CHECK(live_receipt.length == replay_receipt.length);
        METERED_CHECK(memcmp(live_receipt.bytes, replay_receipt.bytes, live_receipt.length) == 0);
        METERED_CHECK(fread(encoded_length, 1U, 4U, historical) == 4U);
        expected_length = ((size_t)encoded_length[0] << 24U) |
            ((size_t)encoded_length[1] << 16U) | ((size_t)encoded_length[2] << 8U) |
            (size_t)encoded_length[3];
        METERED_CHECK(expected_length == live_receipt.length && expected_length <= sizeof(expected_bytes));
        METERED_CHECK(fread(expected_bytes, 1U, expected_length, historical) == expected_length);
        METERED_CHECK(memcmp(live_receipt.bytes, expected_bytes, expected_length) == 0);
        METERED_CHECK(fread(expected_root, 1U, 32U, historical) == 32U);
        METERED_CHECK(memcmp(live->kernel.current_state_root, expected_root, 32U) == 0);
    }
    METERED_CHECK(fgetc(historical) == EOF && !ferror(historical));
    METERED_CHECK(fclose(historical) == 0);
    METERED_CHECK(live->payee->balance.lo == 18U && replay->payee->balance.lo == 18U);
    while (live->kernel.blob_count != 0U)
        free(live->kernel.blobs[--live->kernel.blob_count].bytes);
    while (replay->kernel.blob_count != 0U)
        free(replay->kernel.blobs[--replay->kernel.blob_count].bytes);
    METERED_CHECK(lxp_state_store_destroy(&live->state) == LXP_OK);
    METERED_CHECK(lxp_state_store_destroy(&replay->state) == LXP_OK);
    free(live);
    free(replay);
    return 0;
}

static int metered_fee_capacity(void)
{
    metered_fixture *f = calloc(1U, sizeof(*f));
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_authority_grant loaded;
    uint8_t root[32];
    lxp_u128 actor_balance;
    size_t retries = 0U;
    uint64_t sequence;
    METERED_CHECK(f != NULL && metered_fixture_init(f, 4U, true) == 0);
    while (f->kernel.module_kv_count < LXP_KERNEL_MAX_MODULE_KV - 4U) {
        size_t index = f->kernel.module_kv_count++;
        lxp_module_kv_entry *entry = &f->kernel.module_kv[index];
        (void)memset(entry, 0, sizeof(*entry));
        entry->module_id = LXP_MODULE_GOVERNANCE;
        entry->key_length = 9U;
        entry->key[0] = 0xf0U;
        write_u64(entry->key + 1U, (uint64_t)index);
        entry->value_length = 1U;
        entry->value[0] = 1U;
    }
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    (void)memcpy(root, f->kernel.current_state_root, 32U);
    actor_balance = f->actor->balance;
    sequence = f->state.next_sequence;
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length, 0x50U, true) == 0);
    METERED_CHECK(lxp_kernel_prepare_activity_batch(&f->kernel, &f->activity,
        &f->execution, 1U, 1U, &prepared, &retries) == LXP_ERR_ARENA_EXHAUSTED);
    METERED_CHECK(prepared == NULL && f->state.next_sequence == sequence);
    METERED_CHECK(f->kernel.module_kv_count == LXP_KERNEL_MAX_MODULE_KV - 4U);
    METERED_CHECK(memcmp(root, f->kernel.current_state_root, 32U) == 0);
    METERED_CHECK(lxp_u128_cmp(f->actor->balance, actor_balance) == 0);
    METERED_CHECK(f->payee->balance.lo == 0U && f->source->balance.lo == 40U);
    METERED_CHECK(lxp_authority_grant_load(&f->kernel, f->grant_id, &loaded) == LXP_OK);
    METERED_CHECK(lxp_u128_is_zero(loaded.scope.spent_total) &&
        lxp_u128_is_zero(loaded.fee_budget.spent_total));
    --f->kernel.module_kv_count;
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length, 0x50U, true) == 0);
    METERED_CHECK(lxp_kernel_prepare_activity_batch(&f->kernel, &f->activity,
        &f->execution, 1U, 1U, &prepared, &retries) == LXP_OK);
    const lxp_kernel *settled = lxp_kernel_prepared_batch_settled_kernel(prepared);
    const lxp_receipt *receipts = lxp_kernel_prepared_batch_receipts(prepared);
    METERED_CHECK(settled != NULL && settled->module_kv_count == LXP_KERNEL_MAX_MODULE_KV);
    METERED_CHECK(receipts != NULL && receipts[0].result_code == LXP_OK);
    METERED_CHECK(lxp_authority_grant_load(settled, f->grant_id, &loaded) == LXP_OK);
    METERED_CHECK(loaded.scope.spent_total.lo == 4U && loaded.scope.spent_total.hi == 0U);
    METERED_CHECK(!lxp_u128_is_zero(loaded.fee_budget.spent_total) &&
        lxp_u128_cmp(loaded.fee_budget.spent_total, receipts[0].fee_charged) == 0);
    lxp_kernel_prepared_batch_destroy(prepared);
    METERED_CHECK(f->state.next_sequence == sequence &&
        lxp_u128_cmp(f->actor->balance, actor_balance) == 0);
    while (f->kernel.blob_count != 0U)
        free(f->kernel.blobs[--f->kernel.blob_count].bytes);
    METERED_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}

int main(void)
{
    metered_fixture *f = calloc(1U, sizeof(*f));
    lxp_u128 actor_before;
    lxp_u128 paid_fees = {0U, 0U};
    uint8_t root[32];
    uint64_t sequence;
    METERED_CHECK(metered_legacy_replay() == 0);
    METERED_CHECK(metered_fee_capacity() == 0);
    METERED_CHECK(f != NULL && metered_fixture_init(f, 4U, true) == 0);
    f->source->frozen = true;
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                                   2U, true) == 0);
    {
        lxp_authority_grant loaded;
        (void)memset(f->runtime.occupancy_asset_id, 0, 32U);
        METERED_CHECK(lxp_authority_fee_resolve(&f->kernel, &f->authority, &f->activity,
            f->execution.batch_timestamp_ms, 0U, f->activity.fee_limit, &loaded) == LXP_OK);
        METERED_CHECK(lxp_authority_fee_resolve(&f->kernel, &f->authority, &f->activity,
            f->execution.batch_timestamp_ms, 1U, f->activity.fee_limit, &loaded) == LXP_OK);
        METERED_CHECK(lxp_authority_fee_resolve(&f->kernel, &f->authority, &f->activity,
            f->execution.batch_timestamp_ms, UINT32_MAX, f->activity.fee_limit,
            &loaded) != LXP_OK);
        (void)memcpy(f->runtime.occupancy_asset_id, f->asset.asset_id, 32U);
    }
    {
        lxp_kernel_prepared_batch *prepared = NULL;
        lxp_authority_grant loaded;
        size_t retries = 0U;
        uint8_t live_root[32];
        lxp_u128 live_balance = f->actor->balance;
        (void)memcpy(live_root, f->kernel.current_state_root, 32U);
        lxp_result prepared_result = lxp_kernel_prepare_activity_batch(&f->kernel,
            &f->activity, &f->execution, 1U, 1U, &prepared, &retries);
        if (prepared_result != LXP_OK)
            (void)fprintf(stderr, "metered private batch result %d\n", prepared_result);
        METERED_CHECK(prepared_result == LXP_OK);
        METERED_CHECK(prepared != NULL && lxp_kernel_prepared_batch_count(prepared) == 1U);
        const lxp_receipt *receipts = lxp_kernel_prepared_batch_receipts(prepared);
        METERED_CHECK(receipts != NULL && receipts[0].result_code == LXP_ERR_PROGRAM_REFUSED &&
            !lxp_u128_is_zero(receipts[0].fee_charged));
        METERED_CHECK(lxp_authority_grant_load(lxp_kernel_prepared_batch_settled_kernel(prepared),
            f->grant_id, &loaded) == LXP_OK);
        METERED_CHECK(lxp_u128_cmp(loaded.fee_budget.spent_total, receipts[0].fee_charged) == 0);
        METERED_CHECK(lxp_u128_is_zero(loaded.scope.spent_total));
        lxp_kernel_prepared_batch_destroy(prepared);
        METERED_CHECK(lxp_authority_grant_load(&f->kernel, f->grant_id, &loaded) == LXP_OK &&
            lxp_u128_is_zero(loaded.fee_budget.spent_total));
        METERED_CHECK(memcmp(live_root, f->kernel.current_state_root, 32U) == 0 &&
            lxp_u128_cmp(live_balance, f->actor->balance) == 0);
        {
            uint8_t legacy_call[sizeof(f->call)];
            (void)memcpy(legacy_call, f->call, f->call_length);
            write_u16(legacy_call + 32U, 1U);
            METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, legacy_call,
                f->call_length, 0x40U, true) == 0);
            prepared = NULL;
            METERED_CHECK(lxp_kernel_prepare_activity_batch(&f->kernel, &f->activity,
                &f->execution, 1U, 1U, &prepared, &retries) == LXP_OK);
            receipts = lxp_kernel_prepared_batch_receipts(prepared);
            METERED_CHECK(receipts != NULL && receipts[0].result_code == LXP_ERR_VERSION_UNSUPPORTED &&
                receipts[0].program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_FAILURE);
            METERED_CHECK(lxp_ct_is_zero(receipts[0].transfer_set_root, 32U));
            METERED_CHECK(lxp_authority_grant_load(lxp_kernel_prepared_batch_settled_kernel(prepared),
                f->grant_id, &loaded) == LXP_OK && lxp_u128_is_zero(loaded.scope.spent_total));
            lxp_kernel_prepared_batch_destroy(prepared);
            METERED_CHECK(memcmp(live_root, f->kernel.current_state_root, 32U) == 0 &&
                lxp_u128_cmp(live_balance, f->actor->balance) == 0);
            METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                2U, true) == 0);
        }
    }
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_OK);
    METERED_CHECK(f->receipt.result_code == LXP_ERR_PROGRAM_REFUSED);
    METERED_CHECK(f->receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_FAILURE);
    METERED_CHECK(f->payee->balance.lo == 0U && f->source->balance.lo == 40U);
    METERED_CHECK(metered_spent(f, 0U) == 0);
    METERED_CHECK(metered_fees(f, &paid_fees) == 0);
    f->source->frozen = false;
    METERED_CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                                   3U, true) == 0);
    actor_before = f->actor->balance;
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_OK);
    METERED_CHECK(f->receipt.result_code == LXP_OK);
    METERED_CHECK(f->payee->balance.lo == 9U && f->source->balance.lo == 35U);
    METERED_CHECK(actor_before.lo - f->actor->balance.lo ==
                  4U + f->receipt.fee_charged.lo);
    METERED_CHECK(metered_spent(f, 4U) == 0);
    METERED_CHECK(metered_fees(f, &paid_fees) == 0);
    METERED_CHECK(lxp_u128_is_zero(f->grant.scope.spent_total));
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                                   4U, true) == 0);
    METERED_CHECK(f->grant.scope.spent_total.lo == 4U);
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_OK);
    METERED_CHECK(f->receipt.result_code == LXP_ERR_PROGRAM_REFUSED);
    METERED_CHECK(f->receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_FAILURE);
    METERED_CHECK(f->payee->balance.lo == 9U && f->source->balance.lo == 35U);
    METERED_CHECK(metered_spent(f, 4U) == 0);
    METERED_CHECK(metered_fees(f, &paid_fees) == 0);
    METERED_CHECK(f->grant.scope.spent_total.lo == 4U);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                                   5U, true) == 0);
    (void)memcpy(root, f->kernel.current_state_root, 32U);
    sequence = f->state.next_sequence;
    (void)memcpy(f->authority.principal, f->payee->id, 32U);
    (void)memcpy(f->allowance.grantor, f->payee->id, 32U);
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_ERR_AUTH_SCOPE);
    METERED_CHECK(memcmp(root, f->kernel.current_state_root, 32U) == 0);
    METERED_CHECK(f->state.next_sequence == sequence);
    {
        lxp_authority_grant loaded;
        METERED_CHECK(lxp_authority_grant_load(&f->kernel, f->grant_id, &loaded) == LXP_OK);
        METERED_CHECK(lxp_u128_cmp(loaded.fee_budget.spent_total, paid_fees) == 0);
    }
    METERED_CHECK(metered_spent(f, 4U) == 0);
    while (f->kernel.blob_count != 0U)
        free(f->kernel.blobs[--f->kernel.blob_count].bytes);
    METERED_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    paid_fees = (lxp_u128){0U, 0U};
    f = calloc(1U, sizeof(*f));
    METERED_CHECK(f != NULL && metered_fixture_init(f, 3U, true) == 0);
    METERED_CHECK(metered_activity(f, LX_PROGRAMS_CALL, f->call, f->call_length,
                                   6U, true) == 0);
    METERED_CHECK(lxp_kernel_execute_activity(&f->kernel, &f->activity,
                    &f->execution, &f->receipt) == LXP_OK);
    METERED_CHECK(f->receipt.result_code == LXP_ERR_PROGRAM_REFUSED);
    METERED_CHECK(f->receipt.program_outcome.terminal_kind == LXP_PROGRAM_TERMINAL_FAILURE);
    METERED_CHECK(f->payee->balance.lo == 0U && f->source->balance.lo == 40U);
    METERED_CHECK(metered_spent(f, 0U) == 0);
    METERED_CHECK(metered_fees(f, &paid_fees) == 0);
    while (f->kernel.blob_count != 0U)
        free(f->kernel.blobs[--f->kernel.blob_count].bytes);
    METERED_CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f);
    return 0;
}
