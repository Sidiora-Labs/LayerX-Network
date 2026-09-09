#include "layerx/lxp_history.h"
#include "../../../src/modules/programs/storage.h"

typedef struct pay5_native {
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_identity_store identities;
    lxp_authority_resolved authority;
    lxp_authority_scope scope;
    lxp_kernel_execution execution;
    lxp_fee_params fees;
    lx_account_registry accounts;
    lx_programs_transfer_runtime runtime;
    lxp_transfer_asset_state assets[2];
    lx_programs_state_feed_store feed;
    lxp_history history;
    lxp_log feed_log;
    lxp_log canonical_log;
    pthread_mutex_t mutex;
    lxp_arena arena;
    lxp_arena scratch;
    uint64_t parameters;
    uint8_t program[32];
    uint8_t key[32];
    uint8_t asset_id[32];
    uint8_t pda[3][32];
    lx_account *payer[3];
    lx_account *payment_account[3];
    lx_account *treasury;
    uint8_t arena_bytes[8U * LXP_MAX_ACTIVITY_BYTES];
    uint8_t scratch_bytes[8U * LXP_MAX_ACTIVITY_BYTES];
    uint8_t payload[2U * LXP_MAX_ACTIVITY_BYTES];
    lxp_receipt receipt;
} pay5_native;

static const char *const pay5_dids[3] = {
    "did:lxp:token-issuer", "did:lxp:token-recipient", "did:lxp:token-spender"
};

static int pay5_fail(int line, lxp_result status)
{
    (void)fprintf(stderr, "PAY5 native line=%d status=%d\n", line, status);
    return 1;
}
#define PAY5_OK(expression) do { lxp_result pay5_result = (expression); if (pay5_result != LXP_OK) return pay5_fail(__LINE__, pay5_result); } while (0)
#define PAY5_ASSERT(expression) do { if (!(expression)) return pay5_fail(__LINE__, LXP_FATAL_INVARIANT); } while (0)

static int pay5_file(const char *path, uint8_t *bytes, size_t capacity, size_t *length)
{
    FILE *file = fopen(path, "rb");
    if (file == NULL) return 1;
    *length = fread(bytes, 1U, capacity, file);
    int failed = ferror(file) || !feof(file);
    return fclose(file) != 0 || failed;
}

static int pay5_save(const char *label, uint64_t sequence, const char *suffix,
                     lxp_byte_span bytes)
{
    const char *directory = getenv("PAY5_FIXTURE_OUT");
    char path[1024];
    FILE *file;
    if (directory == NULL) return 0;
    int length = snprintf(path, sizeof(path), "%s/%02llu-%s.%s", directory,
        (unsigned long long)sequence, label, suffix);
    if (length < 0 || (size_t)length >= sizeof(path)) return 1;
    file = fopen(path, "wb");
    if (file == NULL) return 1;
    int failed = fwrite(bytes.bytes, 1U, bytes.length, file) != bytes.length;
    return fclose(file) != 0 || failed;
}

static int pay5_execute(pay5_native *env, unsigned caller, uint32_t type,
                        size_t length, lxp_result expected, uint64_t debit,
                        bool payment, const char *label)
{
    lxp_activity activity;
    uint8_t signature[64], key[32], sequencer[32];
    lxp_byte_span encoded;
    lxp_receipt decoded;
    lxp_identity *identity = &env->identities.identities[caller];
    uint64_t identity_before = identity->next_sequence;
    uint64_t ledger_before = env->payer[caller]->next_sequence;
    uint64_t payment_sequence_before = env->payment_account[caller]->next_sequence;
    lxp_u128 payment_before = env->payment_account[caller]->balance;
    lxp_u128 payer_before = env->payer[caller]->balance;
    lxp_u128 treasury_before = env->treasury->balance;
    lxp_u128 delta, expected_balance;
    PAY5_OK(lxp_arena_reset(&env->arena, 0U));
    fill_activity(&activity, type, env->payload, length,
        (const uint8_t *)pay5_dids[caller], strlen(pay5_dids[caller]), env->key);
    activity.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity.account_sequence = identity_before;
    activity.fee_limit = (lxp_u128){0U, 1000000000U};
    write_u64(activity.idempotency_key + 24U, env->state.next_sequence);
    activity.signature = (lxp_byte_span){signature, sizeof(signature)};
    PAY5_ASSERT(lifecycle_vector_signature(&activity, key, signature) == 0);
    PAY5_OK(lxp_activity_verify_signature(&activity));
    (void)memcpy(env->authority.principal, identity->did_id, 32U);
    (void)memcpy(env->authority.actor, identity->did_id, 32U);
    env->execution.global_sequence = env->state.next_sequence;
    env->execution.fee_balance = env->payer[caller]->balance;
    {
        lxp_result result = execute_artifact_fixture_activity(&env->kernel, &activity,
            &env->execution, &env->receipt);
        if (result != LXP_OK) {
            (void)fprintf(stderr, "PAY5 %s state=%llu identity=%llu effects=%zu blobs=%zu\n", label,
                (unsigned long long)env->state.next_sequence,
                (unsigned long long)identity->next_sequence, env->receipt.effects.count, env->kernel.blob_count);
            return pay5_fail(__LINE__, result);
        }
    }
    if (env->receipt.result_code != expected) {
        (void)fprintf(stderr, "PAY5 %s expected=%d actual=%d\n", label, expected, env->receipt.result_code);
        return 1;
    }
    PAY5_ASSERT(identity->next_sequence == identity_before + 1U);
    PAY5_ASSERT(env->payer[caller]->next_sequence == ledger_before + (payment && debit == 0U ? 1U : 0U));
    PAY5_ASSERT(env->payment_account[caller]->next_sequence == payment_sequence_before + (debit != 0U ? 1U : 0U));
    PAY5_OK(lxp_u128_sub(payment_before, (lxp_u128){0U, debit}, &expected_balance));
    PAY5_ASSERT(lxp_u128_cmp(expected_balance, env->payment_account[caller]->balance) == 0);
    delta = env->receipt.fee_charged;
    PAY5_OK(lxp_u128_sub(payer_before, delta, &expected_balance));
    PAY5_ASSERT(lxp_u128_cmp(expected_balance, env->payer[caller]->balance) == 0);
    PAY5_OK(lxp_u128_add(treasury_before, env->receipt.fee_charged, &expected_balance));
    PAY5_ASSERT(lxp_u128_cmp(expected_balance, env->treasury->balance) == 0);
    PAY5_ASSERT(executed_public_key(executed_sequencer_seed, sequencer) == 0);
    PAY5_OK(lxp_receipt_verify(&env->receipt, sequencer, &env->arena));
    PAY5_OK(lxp_receipt_encode(&env->receipt, true, &env->arena, &encoded));
    PAY5_OK(lxp_receipt_decode(encoded.bytes, encoded.length, true, &decoded));
    PAY5_OK(lxp_receipt_verify(&decoded, sequencer, &env->arena));
    PAY5_ASSERT(pay5_save(label, env->execution.global_sequence, "receipt", encoded) == 0);
    if (env->receipt.program_outcome.present) {
        PAY5_ASSERT(pay5_save(label, env->execution.global_sequence, "terminal", env->receipt.program_outcome.terminal_payload) == 0);
        PAY5_ASSERT(pay5_save(label, env->execution.global_sequence, "callgraph", env->receipt.program_outcome.call_graph_payload) == 0);
    }
    PAY5_OK(lxp_activity_encode(&activity, &env->arena, &encoded));
    PAY5_ASSERT(pay5_save(label, env->execution.global_sequence, "activity", encoded) == 0);
    return 0;
}

static int pay5_init(pay5_native *env)
{
    char feed_directory[] = "qual-logs/pay5/feed-XXXXXX";
    char canonical_directory[] = "qual-logs/pay5/canonical-XXXXXX";
    const uint8_t seed[32] = {0x33U};
    const uint8_t native_asset[32] = {9U};
    (void)memset(env, 0, sizeof(*env));
    env->parameters = 1U;
    (void)memset(env->program, 0x55, 32U);
    (void)memset(env->asset_id, 0x44, 32U);
    PAY5_ASSERT(executed_public_key(seed, env->key) == 0);
    PAY5_OK(lx_account_registry_init(&env->accounts));
    for (size_t i = 0U; i < 3U; ++i) {
        char name[160];
        uint8_t id[32];
        lxp_identity *identity;
        int length = snprintf(name, sizeof(name), "agent:%s:asset:%s", pay5_dids[i],
            "4444444444444444444444444444444444444444444444444444444444444444");
        PAY5_ASSERT(length > 0 && (size_t)length < sizeof(name));
        PAY5_OK(lxp_identity_register(&env->identities, (const uint8_t *)pay5_dids[i],
            strlen(pay5_dids[i]), env->key, &identity));
        PAY5_OK(lx_account_id_from_string((const uint8_t *)name, (size_t)length, id));
        PAY5_OK(lx_account_open(&env->accounts, (const uint8_t *)name, (size_t)length,
            id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &env->payment_account[i]));
        PAY5_OK(lxp_ledger_bootstrap_balance(env->payment_account[i], env->asset_id,
            (lxp_u128){0U, 1000000000000U}, 1U));
        length = snprintf(name, sizeof(name), "agent:%s:main", pay5_dids[i]);
        PAY5_ASSERT(length > 0 && (size_t)length < sizeof(name));
        PAY5_OK(lx_account_id_from_string((const uint8_t *)name, (size_t)length, id));
        PAY5_OK(lx_account_open(&env->accounts, (const uint8_t *)name, (size_t)length,
            id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &env->payer[i]));
        PAY5_OK(lxp_ledger_bootstrap_balance(env->payer[i], native_asset,
            (lxp_u128){0U, 1000000000000U}, 7U));
        PAY5_OK(lxp_programs_account_derive(env->program, identity->did_id, 32U, env->pda[i]));
    }
    {
        const uint8_t name[] = "system:fees";
        uint8_t id[32];
        PAY5_OK(lx_account_id_from_string(name, sizeof(name) - 1U, id));
        PAY5_OK(lx_account_open(&env->accounts, name, sizeof(name) - 1U, id,
            1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &env->treasury));
        PAY5_OK(lxp_ledger_bootstrap_balance(env->treasury, native_asset, (lxp_u128){0U, 0U}, 0U));
    }
    (void)memcpy(env->assets[0].asset_id, native_asset, 32U);
    (void)memcpy(env->assets[1].asset_id, env->asset_id, 32U);
    env->assets[0].registered = true;
    env->assets[1].registered = true;
    env->runtime.accounts = &env->accounts;
    env->runtime.assets = env->assets;
    env->runtime.asset_count = 2U;
    env->runtime.fee_schedule = (lx_programs_fee_schedule){1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U};
    env->runtime.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
    env->runtime.metering_schedule_context = &env->kernel;
    env->runtime.resolve_occupancy_parameters = occupancy_parameters;
    env->runtime.occupancy_parameter_context = &env->runtime;
    (void)memcpy(env->runtime.occupancy_asset_id, native_asset, 32U);
    PAY5_OK(lxp_state_store_init(&env->state, 1U));
    PAY5_OK(lxp_kernel_create(&env->kernel, &env->state, &env->journal, &env->parameters, 0U));
    PAY5_OK(install_metering_v1(&env->kernel));
    PAY5_OK(lxp_kernel_register_module(&env->kernel, programs_module_registration_v4()));
    PAY5_OK(lxp_kernel_bind_module_runtime(&env->kernel, LXP_MODULE_PROGRAMS, &env->runtime));
    PAY5_OK(lxp_programs_bind_fee_transaction(&env->kernel));
    PAY5_OK(lxp_kernel_set_capabilities(&env->kernel, NULL, lxp_kernel_canonical_ledger_apply));
    PAY5_OK(lxp_state_root(&env->kernel, env->kernel.current_state_root));
    PAY5_OK(lxp_arena_init(&env->arena, env->arena_bytes, sizeof(env->arena_bytes)));
    PAY5_OK(lxp_arena_init(&env->scratch, env->scratch_bytes, sizeof(env->scratch_bytes)));
    PAY5_ASSERT(mkdtemp(feed_directory) != NULL && mkdtemp(canonical_directory) != NULL);
    PAY5_OK(lxp_log_segment_create(&env->feed_log, feed_directory, 0U, 4U * 1024U * 1024U));
    PAY5_OK(lxp_log_segment_create(&env->canonical_log, canonical_directory, 0U, 4U * 1024U * 1024U));
    {
        char database[256];
        int length = snprintf(database, sizeof(database), "%s/history.sqlite", canonical_directory);
        PAY5_ASSERT(length > 0 && (size_t)length < sizeof(database));
        PAY5_OK(lxp_history_open(&env->history, &env->canonical_log, database,
            "migrations/0007_history_index.sql"));
    }
    PAY5_ASSERT(pthread_mutex_init(&env->mutex, NULL) == 0);
    PAY5_OK(lxp_programs_state_feed_store_open(&env->feed, &env->feed_log,
        &env->canonical_log, &env->history, &env->scratch, &env->mutex));
    PAY5_OK(lxp_programs_state_feed_store_anchor(&env->feed, 1U, env->kernel.current_state_root));
    env->runtime.state_feed = &env->feed.feed;
    PAY5_OK(lxp_programs_bind_state_feed(&env->kernel, &env->feed.feed));
    env->authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(env->authority.verified_key, env->key, 32U);
    env->scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    env->scope.activity_ordinal_min = 1U;
    env->scope.activity_ordinal_max = 7U;
    env->scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    env->scope.maximum_total = env->scope.maximum_per_activity;
    env->scope.maximum_per_period = env->scope.maximum_per_activity;
    env->authority.scope = &env->scope;
    PAY5_OK(lxp_authority_hash(env->authority.kind, (const uint8_t[32]){0}, env->key, env->authority.authority_hash));
    env->fees.version = 1U;
    env->fees.multiplier_basis_points = 10000U;
    env->execution = (lxp_kernel_execution){0};
    env->execution.network_id = 7U;
    env->execution.batch_number = 1U;
    env->execution.batch_timestamp_ms = 10U;
    env->execution.maximum_timestamp_window = 100U;
    env->execution.recorded_module_version = LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION;
    env->execution.parameter_version = 1U;
    env->execution.signature_valid = true;
    env->execution.sequencer_private_key = executed_sequencer_seed;
    env->execution.identities = &env->identities;
    env->execution.authority = &env->authority;
    env->execution.fee_parameters = &env->fees;
    env->execution.gas_limit = 100000000U;
    env->execution.arena = &env->arena;
    return 0;
}

static int pay5_deploy(pay5_native *env, const char *wasm_path, const char *interface_path, const char *label)
{
    static uint8_t wasm[LXP_MAX_ACTIVITY_BYTES], interface_bytes[65536];
    size_t wasm_length, interface_length = 0U;
    uint8_t hash[32];
    PAY5_ASSERT(pay5_file(wasm_path, wasm, sizeof(wasm), &wasm_length) == 0);
    if (interface_path != NULL)
        PAY5_ASSERT(pay5_file(interface_path, interface_bytes, sizeof(interface_bytes), &interface_length) == 0);
    PAY5_OK(lxp_hash_sha256(wasm, wasm_length, hash));
    (void)memcpy(env->payload, env->program, 32U);
    write_u16(env->payload + 32U, 2U);
    env->payload[34] = 1U; env->payload[35] = 0U;
    (void)memcpy(env->payload + 36U, env->identities.identities[0].did_id, 32U);
    (void)memcpy(env->payload + 68U, hash, 32U);
    write_u32(env->payload + 100U, (uint32_t)wasm_length);
    size_t offset = interface_path == NULL ? 104U : 108U;
    if (interface_path != NULL) write_u32(env->payload + 104U, (uint32_t)interface_length);
    (void)memcpy(env->payload + offset, interface_bytes, interface_length);
    (void)memcpy(env->payload + offset + interface_length, wasm, wasm_length);
    PAY5_ASSERT(pay5_execute(env, 0U, LX_PROGRAMS_DEPLOY, offset + interface_length + wasm_length,
        LXP_OK, 0U, false, label) == 0);
    if (interface_path != NULL) {
        lxp_module_ctx ctx;
        uint8_t key[42], expected[65536];
        const uint8_t *value;
        size_t value_length, expected_length;
        (void)memcpy(key, "interface", 10U);
        (void)memcpy(key + 10U, env->program, 32U);
        PAY5_OK(lxp_module_ctx_init(&ctx, &env->kernel, LXP_MODULE_PROGRAMS,
            10U, 0U, env->state.next_sequence, 1000000U, &env->arena, false));
        PAY5_OK(lxp_ctx_kv_get(&ctx, key, sizeof(key), &value, &value_length));
        PAY5_ASSERT(pay5_file("programs/fixtures/pay5/token-lxt20.registry-value",
            expected, sizeof(expected), &expected_length) == 0);
        PAY5_ASSERT(value_length == expected_length && memcmp(value, expected, value_length) == 0);
        PAY5_ASSERT(pay5_save(label, env->execution.global_sequence, "registry-value",
            (lxp_byte_span){value, value_length}) == 0);
    }
    return 0;
}

static int pay5_register(pay5_native *env, const uint8_t *seed, size_t length)
{
    (void)memcpy(env->payload, env->program, 32U);
    (void)memcpy(env->payload + 32U, "LXPA1", 5U);
    (void)memcpy(env->payload + 37U, env->asset_id, 32U);
    write_u32(env->payload + 69U, (uint32_t)length);
    (void)memcpy(env->payload + 73U, seed, length);
    return pay5_execute(env, 0U, LX_PROGRAMS_ACCOUNT, 73U + length, LXP_OK, 0U, false, "account-register");
}

static size_t pay5_grants(pay5_native *env, uint8_t *out, unsigned owner,
                          const uint8_t *to, uint64_t amount, bool funding,
                          const uint8_t *seed, size_t seed_length)
{
    size_t cursor = 2U;
    write_u16(out, to == NULL ? 4U : 5U);
    out[cursor++] = 1U; out[cursor++] = 2U;
    if (to != NULL) {
        out[cursor++] = funding ? 5U : 9U;
        if (!funding) {
            (void)memcpy(out + cursor, env->program, 32U); cursor += 32U;
            write_u16(out + cursor, (uint16_t)seed_length); cursor += 2U;
            (void)memcpy(out + cursor, seed, seed_length); cursor += seed_length;
            (void)memcpy(out + cursor, env->pda[owner], 32U); cursor += 32U;
        }
        (void)memcpy(out + cursor, env->asset_id, 32U); cursor += 32U;
        (void)memcpy(out + cursor, to, 32U); cursor += 32U;
        (void)memset(out + cursor, 0, 8U); write_u64(out + cursor + 8U, amount); cursor += 16U;
    }
    out[cursor++] = 7U; out[cursor++] = 8U;
    return cursor;
}

static int pay5_call(pay5_native *env, unsigned caller, const char *method,
                     const uint8_t *input, size_t input_length,
                     const uint8_t *grants, size_t grants_length,
                     lxp_result expected, uint64_t debit, bool payment)
{
    static const uint8_t access[] = "LayerX/programs/access-declaration/v1\0";
    size_t cursor = CALL_FIXED_BYTES;
    size_t method_length = strlen(method);
    (void)memcpy(env->payload, env->program, 32U);
    write_u16(env->payload + 32U, 2U);
    write_u16(env->payload + 34U, (uint16_t)method_length);
    write_u32(env->payload + 36U, (uint32_t)input_length);
    write_u16(env->payload + 40U, (uint16_t)grants_length);
    write_u32(env->payload + 42U, sizeof(access));
    write_u32(env->payload + 46U, 70U);
    for (size_t i = 0U; i < LX_PROGRAMS_CALL_BUDGET_FIELDS; ++i)
        write_u64(env->payload + 50U + i * 8U, call_budget[i]);
    (void)memcpy(env->payload + cursor, method, method_length); cursor += method_length;
    (void)memcpy(env->payload + cursor, input, input_length); cursor += input_length;
    (void)memcpy(env->payload + cursor, grants, grants_length); cursor += grants_length;
    (void)memcpy(env->payload + cursor, access, sizeof(access)); cursor += sizeof(access);
    return pay5_execute(env, caller, LX_PROGRAMS_CALL, cursor, expected, debit, payment, method);
}

static size_t pay5_request(uint8_t *out, uint8_t method, const uint8_t *first,
                           const uint8_t *second, bool has_amount, uint64_t amount)
{
    size_t length = (first == NULL ? 0U : 32U) + (second == NULL ? 0U : 32U) + (has_amount ? 16U : 0U);
    out[0] = 'L'; out[1] = 'X'; out[2] = 20U; out[3] = method;
    out[4] = 1U; out[5] = 0x20U; write_u32(out + 6U, (uint32_t)length);
    size_t cursor = 10U;
    if (first != NULL) { (void)memcpy(out + cursor, first, 32U); cursor += 32U; }
    if (second != NULL) { (void)memcpy(out + cursor, second, 32U); cursor += 32U; }
    if (has_amount) { (void)memset(out + cursor, 0, 8U); write_u64(out + cursor + 8U, amount); cursor += 16U; }
    return cursor;
}

static int pay5_amount(pay5_native *env, const uint8_t *key, size_t length, uint64_t expected)
{
    lxp_module_ctx ctx;
    uint8_t ns[33];
    (void)memcpy(ns, env->program, 32U); ns[32] = 1U;
    PAY5_OK(lxp_arena_reset(&env->arena, 0U));
    PAY5_OK(lxp_module_ctx_init(&ctx, &env->kernel, LXP_MODULE_PROGRAMS, 10U, 0U,
        env->state.next_sequence, 1000000U, &env->arena, false));
    ctx.protocol_version = 3U;
    for (uint32_t i = 0U;; ++i) {
        const uint8_t *cell_key, *value;
        uint16_t key_length;
        uint32_t value_length, count;
        PAY5_OK(lxp_programs_storage_cell_at(&ctx, ns, sizeof(ns), i,
            &cell_key, &key_length, &value, &value_length, &count));
        if (key_length == length && memcmp(cell_key, key, length) == 0) {
            lxp_u128 amount;
            PAY5_ASSERT(value_length == 16U);
            PAY5_OK(lxp_u128_from_be(value, &amount));
            PAY5_ASSERT(amount.hi == 0U && amount.lo == expected);
            return 0;
        }
        PAY5_ASSERT(i + 1U < count);
    }
}

static lx_account *pay5_account(pay5_native *env, unsigned owner)
{
    for (size_t i = 0U; i < env->accounts.count; ++i)
        if (memcmp(env->accounts.accounts[i].id, env->pda[owner], 32U) == 0)
            return &env->accounts.accounts[i];
    return NULL;
}

static int pay5_token_roundtrip(void)
{
    static pay5_native env;
    uint8_t input[90], grants[512], key[74];
    size_t length, grants_length;
    PAY5_ASSERT(pay5_init(&env) == 0);
    PAY5_ASSERT(pay5_deploy(&env, "programs/fixtures/pay5/token-lxt20.wasm",
        "programs/fixtures/pay5/token-lxt20.interface", "token-deploy") == 0);
    for (size_t i = 0U; i < 3U; ++i)
        PAY5_ASSERT(pay5_register(&env, env.identities.identities[i].did_id, 32U) == 0);
    grants_length = pay5_grants(&env, grants, 0U, env.pda[0], 1000000U, true, NULL, 0U);
    length = pay5_request(input, 0U, NULL, NULL, false, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "initialize", input, length, grants, grants_length, LXP_OK, 1000000U, true) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 1000000U);
    PAY5_ASSERT(pay5_amount(&env, (const uint8_t *)"supply", 6U, 1000000U) == 0);
    grants_length = pay5_grants(&env, grants, 0U, NULL, 0U, false, NULL, 0U);
    length = pay5_request(input, 2U, env.identities.identities[2].did_id, NULL, true, 0U);
    PAY5_ASSERT(pay5_call(&env, 1U, "approve", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 2U, env.identities.identities[2].did_id, NULL, true, 100U);
    PAY5_ASSERT(pay5_call(&env, 0U, "approve", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 3U, env.identities.identities[0].did_id, env.pda[1], true, 40U);
    PAY5_ASSERT(pay5_call(&env, 2U, "transfer_from", input, length, grants, grants_length, LXP_ERR_NON_CANONICAL, 0U, false) == 0);
    (void)memcpy(key, "allowance:", 10U);
    (void)memcpy(key + 10U, env.identities.identities[0].did_id, 32U);
    (void)memcpy(key + 42U, env.identities.identities[2].did_id, 32U);
    PAY5_ASSERT(pay5_amount(&env, key, sizeof(key), 100U) == 0);
    grants_length = pay5_grants(&env, grants, 0U, env.pda[1], 40U, false, env.identities.identities[0].did_id, 32U);
    env.assets[1].paused = true;
    PAY5_ASSERT(pay5_call(&env, 2U, "transfer_from", input, length, grants, grants_length,
        LXP_ERR_PROGRAM_REFUSED, 0U, false) == 0);
    PAY5_ASSERT(pay5_amount(&env, key, sizeof(key), 100U) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 1000000U && pay5_account(&env, 1U)->balance.lo == 0U);
    env.assets[1].paused = false;
    PAY5_ASSERT(pay5_call(&env, 2U, "transfer_from", input, length, grants, grants_length, LXP_OK, 0U, true) == 0);
    PAY5_ASSERT(pay5_amount(&env, key, sizeof(key), 60U) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 999960U && pay5_account(&env, 1U)->balance.lo == 40U);
    length = pay5_request(input, 1U, env.pda[0], NULL, true, 15U);
    grants_length = pay5_grants(&env, grants, 1U, env.pda[0], 15U, false, env.identities.identities[1].did_id, 32U);
    PAY5_ASSERT(pay5_call(&env, 1U, "transfer", input, length, grants, grants_length, LXP_OK, 0U, true) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 999975U && pay5_account(&env, 1U)->balance.lo == 25U);
    grants_length = pay5_grants(&env, grants, 0U, NULL, 0U, false, NULL, 0U);
    length = pay5_request(input, 4U, env.identities.identities[1].did_id, NULL, false, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "balance_of", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 5U, env.identities.identities[0].did_id, env.identities.identities[2].did_id, false, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "allowance", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 6U, NULL, NULL, false, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "total_supply", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 7U, NULL, NULL, false, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "metadata", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 2U, env.identities.identities[2].did_id, NULL, true, 0U);
    PAY5_ASSERT(pay5_call(&env, 0U, "approve", input, length, grants, grants_length, LXP_OK, 0U, false) == 0);
    length = pay5_request(input, 3U, env.identities.identities[0].did_id, env.pda[1], true, 1U);
    grants_length = pay5_grants(&env, grants, 0U, env.pda[1], 1U, false, env.identities.identities[0].did_id, 32U);
    PAY5_ASSERT(pay5_call(&env, 2U, "transfer_from", input, length, grants, grants_length, LXP_ERR_PROGRAM_REFUSED, 0U, false) == 0);
    PAY5_ASSERT(pay5_amount(&env, key, sizeof(key), 0U) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 999975U && pay5_account(&env, 1U)->balance.lo == 25U);
    PAY5_ASSERT(pay5_amount(&env, (const uint8_t *)"supply", 6U, 1000000U) == 0);
    (void)memcpy(key, "balance:", 8U);
    (void)memcpy(key + 8U, env.pda[0], 32U);
    PAY5_ASSERT(pay5_amount(&env, key, 40U, 999975U) == 0);
    (void)memcpy(key + 8U, env.pda[1], 32U);
    PAY5_ASSERT(pay5_amount(&env, key, 40U, 25U) == 0);
    PAY5_OK(lxp_history_close(&env.history));
    PAY5_OK(lxp_log_close(&env.feed_log));
    PAY5_OK(lxp_log_close(&env.canonical_log));
    PAY5_ASSERT(pthread_mutex_destroy(&env.mutex) == 0);
    while (env.kernel.blob_count != 0U) free(env.kernel.blobs[--env.kernel.blob_count].bytes);
    PAY5_OK(lxp_state_store_destroy(&env.state));
    return 0;
}
static int pay5_merchant_roundtrip(void)
{
    static pay5_native env;
    static const uint8_t seed[] = "payments-merchant";
    uint8_t input[130] = {0U, 1U};
    uint8_t funding[256], first[512], second[512], grants[1024];
    size_t funding_length, first_length, second_length, cursor;
    PAY5_ASSERT(pay5_init(&env) == 0);
    PAY5_ASSERT(pay5_deploy(&env, "programs/fixtures/pay5/payments-merchant.wasm", NULL, "merchant-deploy") == 0);
    PAY5_OK(lxp_programs_account_derive(env.program, seed, sizeof(seed) - 1U, env.pda[0]));
    PAY5_ASSERT(pay5_register(&env, seed, sizeof(seed) - 1U) == 0);
    (void)memcpy(input + 2U, env.asset_id, 32U);
    (void)memcpy(input + 34U, env.payment_account[1]->id, 32U);
    (void)memcpy(input + 66U, env.payment_account[2]->id, 32U);
    write_u64(input + 106U, 100U); write_u64(input + 122U, 10U);
    funding_length = pay5_grants(&env, funding, 0U, env.pda[0], 100U, true, NULL, 0U);
    first_length = pay5_grants(&env, first, 0U, env.payment_account[1]->id, 90U, false, seed, sizeof(seed) - 1U);
    second_length = pay5_grants(&env, second, 0U, env.payment_account[2]->id, 10U, false, seed, sizeof(seed) - 1U);
    write_u16(grants, 7U); grants[2] = 1U; grants[3] = 2U; cursor = 4U;
    (void)memcpy(grants + cursor, funding + 4U, funding_length - 6U); cursor += funding_length - 6U;
    const uint8_t *low = memcmp(env.payment_account[1]->id, env.payment_account[2]->id, 32U) < 0 ? first : second;
    const uint8_t *high = low == first ? second : first;
    PAY5_ASSERT(first_length == second_length);
    (void)memcpy(grants + cursor, low + 4U, first_length - 6U); cursor += first_length - 6U;
    (void)memcpy(grants + cursor, high + 4U, second_length - 6U); cursor += second_length - 6U;
    grants[cursor++] = 7U; grants[cursor++] = 8U;
    lxp_u128 merchant_before = env.payment_account[1]->balance, collector_before = env.payment_account[2]->balance;
    PAY5_ASSERT(pay5_call(&env, 0U, "layerx_call", input, sizeof(input), funding, funding_length,
        LXP_ERR_NON_CANONICAL, 0U, false) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 0U);
    PAY5_ASSERT(lxp_u128_cmp(merchant_before, env.payment_account[1]->balance) == 0 &&
        lxp_u128_cmp(collector_before, env.payment_account[2]->balance) == 0);
    env.assets[1].paused = true;
    PAY5_ASSERT(pay5_call(&env, 0U, "layerx_call", input, sizeof(input), grants, cursor,
        LXP_ERR_PROGRAM_REFUSED, 0U, false) == 0);
    PAY5_ASSERT(pay5_account(&env, 0U)->balance.lo == 0U);
    PAY5_ASSERT(lxp_u128_cmp(merchant_before, env.payment_account[1]->balance) == 0 &&
        lxp_u128_cmp(collector_before, env.payment_account[2]->balance) == 0);
    env.assets[1].paused = false;
    PAY5_ASSERT(pay5_call(&env, 0U, "layerx_call", input, sizeof(input), grants, cursor,
        LXP_OK, 100U, true) == 0);
    PAY5_ASSERT(env.payment_account[1]->balance.lo == merchant_before.lo + 90U &&
        env.payment_account[2]->balance.lo == collector_before.lo + 10U && pay5_account(&env, 0U)->balance.lo == 0U);
    PAY5_ASSERT(!lxp_ct_is_zero(env.receipt.program_outcome.transfer_root, 32U));
    PAY5_OK(lxp_history_close(&env.history));
    PAY5_OK(lxp_log_close(&env.feed_log));
    PAY5_OK(lxp_log_close(&env.canonical_log));
    PAY5_ASSERT(pthread_mutex_destroy(&env.mutex) == 0);
    while (env.kernel.blob_count != 0U) free(env.kernel.blobs[--env.kernel.blob_count].bytes);
    PAY5_OK(lxp_state_store_destroy(&env.state));
    return 0;
}
#undef PAY5_OK
#undef PAY5_ASSERT
