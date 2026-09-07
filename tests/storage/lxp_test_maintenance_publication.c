#define _POSIX_C_SOURCE 200809L
#include "lxp_daemon_batch_wal.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_bridge_credit.h"
#include "../bridge/files.h"
#include <unistd.h>
#define main maintenance_activity_fixture_main
#include "../programs/test_call_activity.c"
#undef main

#define CHECK(expression) do { if (!(expression)) { \
    fprintf(stderr, "maintenance publication failure at %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static const uint8_t maintenance_actor_seed[32] = {0x33U};
static const uint8_t maintenance_did[] = "did:lxp:maintenance-publication";

typedef struct maintenance_fixture {
    const lxp_activity *input_activity;
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lxp_identity *identity;
    lx_account_registry accounts;
    lx_account *actor;
    lx_account *treasury;
    lx_account *recipient;
    lxp_transfer_asset_state asset;
    lx_asset_record asset_record;
    lx_asset_runtime asset_runtime;
    lx_programs_transfer_runtime runtime;
    lxp_authority_scope scope;
    lxp_authority_resolved authority;
    lxp_fee_params fees;
    lxp_sequencer_authorization authorization;
    lxp_daemon_receipt_authority_store receipt_authority;
    lxp_log authority_log;
    lxp_log feed_log;
    lxp_log canonical_log;
    lxp_log evidence_log;
    lxp_history history;
    lx_programs_state_feed_store feed;
    lxp_daemon_evidence_store evidence;
    pthread_mutex_t feed_mutex;
    lxp_arena arena;
    uint8_t *storage;
    uint8_t actor_public_key[32];
    char directory[128];
    char authority_path[160];
} maintenance_fixture;

static int maintenance_fixture_logs(maintenance_fixture *f, uint32_t network_id)
{
    CHECK(executed_public_key(executed_sequencer_seed, f->authorization.public_key) == 0);
    memcpy(f->authorization.sequencer_id, f->authorization.public_key, 32U);
    f->authorization.authorized = 1U;
    f->authorization.first_batch_number = 1U;
    f->authorization.last_batch_number = 100U;
    strcpy(f->directory, "/tmp/lxp-maintenance-publication-XXXXXX");
    CHECK(mkdtemp(f->directory) != NULL);
    CHECK(snprintf(f->authority_path, sizeof(f->authority_path), "%s/authority.log", f->directory) > 0);
    CHECK(lxp_log_open_or_create(&f->authority_log, f->authority_path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    CHECK(lxp_daemon_receipt_authority_open(&f->receipt_authority, &f->authority_log,
        &f->authorization) == LXP_OK);
    {
        char path[160], database[160];
        uint8_t anchor[32] = {1U};
        CHECK(pthread_mutex_init(&f->feed_mutex, NULL) == 0);
        CHECK(snprintf(path, sizeof(path), "%s/feed.log", f->directory) > 0);
        CHECK(lxp_log_open_or_create(&f->feed_log, path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
        CHECK(snprintf(path, sizeof(path), "%s/canonical.log", f->directory) > 0);
        CHECK(lxp_log_open_or_create(&f->canonical_log, path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
        CHECK(snprintf(database, sizeof(database), "%s/history.db", f->directory) > 0);
        CHECK(lxp_history_open(&f->history, &f->canonical_log, database, "migrations/0007_history_index.sql") == LXP_OK);
        CHECK(lxp_programs_state_feed_store_open(&f->feed, &f->feed_log, &f->canonical_log,
            &f->history, &f->arena, &f->feed_mutex) == LXP_OK);
        CHECK(lxp_programs_state_feed_store_anchor(&f->feed, f->state.next_sequence, f->kernel.current_state_root) == LXP_OK);
        f->runtime.state_feed = &f->feed.feed;
        CHECK(lxp_programs_bind_state_feed(&f->kernel, f->runtime.state_feed) == LXP_OK);
        CHECK(lxp_programs_state_feed_store_recover(&f->feed, &f->kernel) == LXP_OK);
        CHECK(snprintf(path, sizeof(path), "%s/evidence.log", f->directory) > 0);
        CHECK(lxp_log_open_or_create(&f->evidence_log, path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
        CHECK(lxp_daemon_evidence_open(&f->evidence, &f->evidence_log, network_id, &f->authorization,
            anchor, true, NULL, NULL, &f->arena) == LXP_OK);
    }
    return 0;
}
static int maintenance_fixture_open(maintenance_fixture *f)
{
    static const uint8_t actor_name[] = "agent:did:lxp:maintenance-publication:main";
    static const uint8_t treasury_name[] = "system:fees";
    static const uint8_t recipient_name[] = "agent:did:lxp:recipient:main";
    uint8_t actor_id[32], treasury_id[32], recipient_id[32], grant[32] = {0};
    static const uint64_t parameters = 1U;
    lxp_genesis_manifest manifest = {0};
    lx_programs_fee_genesis_parameters fees = {0};
    memset(f, 0, sizeof(*f));
    f->authority_log.descriptor = -1;
    f->storage = malloc(4U * LXP_MAX_BATCH_BODY_BYTES);
    CHECK(f->storage != NULL);
    CHECK(lxp_arena_init(&f->arena, f->storage, 4U * LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    CHECK(executed_public_key(maintenance_actor_seed, f->actor_public_key) == 0);
    CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    CHECK(lx_account_id_from_string(actor_name, sizeof(actor_name) - 1U, actor_id) == LXP_OK);
    CHECK(lx_account_id_from_string(treasury_name, sizeof(treasury_name) - 1U, treasury_id) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, actor_name, sizeof(actor_name) - 1U,
        actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->actor) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, treasury_name, sizeof(treasury_name) - 1U,
        treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->treasury) == LXP_OK);
    CHECK(lx_account_id_from_string(recipient_name, sizeof(recipient_name) - 1U, recipient_id) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, recipient_name, sizeof(recipient_name) - 1U,
        recipient_id, 3U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->recipient) == LXP_OK);
    f->asset.asset_id[0] = 9U;
    f->asset.registered = true;
    CHECK(lxp_ledger_bootstrap_balance(f->actor, f->asset.asset_id,
        (lxp_u128){0U, UINT64_MAX}, 1U) == LXP_OK);
    CHECK(lxp_ledger_bootstrap_balance(f->treasury, f->asset.asset_id,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    CHECK(lxp_ledger_bootstrap_balance(f->recipient, f->asset.asset_id,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    CHECK(lxp_identity_register(&f->identities, maintenance_did, sizeof(maintenance_did) - 1U,
        f->actor_public_key, &f->identity) == LXP_OK);
    CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal, &parameters, 1U) == LXP_OK);
    CHECK(install_metering_v1(&f->kernel) == LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, programs_module_registration_v4()) == LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, lx_asset_module_iface()) == LXP_OK);
    memcpy(f->asset_record.asset_id, f->asset.asset_id, 32U);
    f->asset_runtime = (lx_asset_runtime){&f->accounts, &f->asset_record, 1U,
        &f->asset, 1U, 7U, 3U};
    f->actor->has_authority_key = true;
    memcpy(f->actor->authority_key, f->actor_public_key, 32U);
    CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET, &f->asset_runtime) == LXP_OK);
    f->runtime.accounts = &f->accounts;
    f->runtime.assets = &f->asset;
    f->runtime.asset_count = 1U;
    f->runtime.fee_schedule = (lx_programs_fee_schedule){1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U};
    memcpy(f->runtime.occupancy_asset_id, f->asset.asset_id, 32U);
    f->runtime.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
    f->runtime.metering_schedule_context = &f->kernel;
    f->runtime.resolve_occupancy_parameters = lxp_programs_fee_governance_resolve_runtime;
    f->runtime.occupancy_parameter_context = &f->kernel;
    CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_PROGRAMS, &f->runtime) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(&f->kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    memcpy(manifest.signer_public_key, f->actor_public_key, 32U);
    fees.schedule = f->runtime.fee_schedule;
    memcpy(fees.occupancy_asset_id, f->asset.asset_id, 32U);
    fees.target_occupancy_byte_batches = 3U;
    fees.response_denominator = 1U;
    fees.maximum_change_numerator = 1U;
    fees.maximum_change_denominator = 1U;
    fees.minimum_fee_units_per_occupancy_byte_batch = 1U;
    fees.maximum_fee_units_per_occupancy_byte_batch = 10U;
    CHECK(lxp_programs_fee_genesis_append(&manifest, &fees) == LXP_OK);
    CHECK(lxp_programs_fee_genesis_materialize(&manifest, &f->kernel) == LXP_OK);
    CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    f->scope.module_mask = UINT64_C(1) << LXP_MODULE_PROGRAMS;
    f->scope.activity_ordinal_min = 1U;
    f->scope.activity_ordinal_max = 10U;
    f->scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    f->scope.maximum_total = f->scope.maximum_per_activity;
    f->scope.maximum_per_period = f->scope.maximum_per_activity;
    f->authority.scope = &f->scope;
    f->authority.kind = LXP_AUTHORITY_OWNER;
    memcpy(f->authority.actor, f->identity->did_id, 32U);
    memcpy(f->authority.principal, actor_id, 32U);
    memcpy(f->authority.verified_key, f->actor_public_key, 32U);
    CHECK(lxp_authority_hash(f->authority.kind, grant, f->actor_public_key,
        f->authority.authority_hash) == LXP_OK);
    f->fees.version = 1U;
    f->fees.multiplier_basis_points = 10000U;
    return maintenance_fixture_logs(f, 7U);
}

static int maintenance_publish(maintenance_fixture *f, uint32_t type,
    const uint8_t *payload, size_t payload_length, size_t count, uint64_t batch_number)
{
    lxp_activity *activities = calloc(count, sizeof(*activities));
    lxp_kernel_execution *executions = calloc(count, sizeof(*executions));
    lxp_byte_span canonical[64], receipts[65], artifacts[64], graphs[64];
    uint8_t *canonical_storage[64] = {NULL}, *receipt_storage[64] = {NULL};
    lxp_merkle_proof proofs[64];
    uint8_t signatures[64][64], leaves[65][32], root[32], batch_id[32], durable[32];
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_daemon_batch_wal_record *loaded = NULL;
    lxp_daemon_batch_wal_input input = {0};
    lxp_batch_roots roots;
    lxp_batch_header header = {0};
    lxp_kernel_batch_boundary live;
    lxp_daemon_batch_wal_recovery recovery;
    const lxp_receipt *decoded;
    lxp_programs_occupancy_receipt maintenance;
    size_t retry = 0U;
    bool present = false;
    uint64_t first_sequence = f->state.next_sequence;
    CHECK(count > 0U && count <= 64U && activities != NULL && executions != NULL);
    CHECK(lxp_arena_reset(&f->arena, 0U) == LXP_OK);
    f->scope.module_mask = UINT64_C(1) << lxp_activity_module_id(type);
    for (size_t i = 0U; i < count; ++i) {
        EVP_PKEY *key;
        EVP_MD_CTX *ctx;
        size_t signature_length = 64U;
        uint8_t preimage[32];
        if (f->input_activity != NULL) {
            CHECK(count == 1U);
            activities[i] = *f->input_activity;
        } else {
        fill_activity(&activities[i], type, payload, payload_length,
            maintenance_did, sizeof(maintenance_did) - 1U, f->actor_public_key);
        activities[i].protocol_version = 3U;
        activities[i].network_id = 7U;
        activities[i].account_sequence = f->identity->next_sequence + i;
        write_u64(activities[i].idempotency_key, activities[i].account_sequence + 1U);
        activities[i].fee_limit = type == LX_PROGRAMS_CALL ? (lxp_u128){0U, 1000000000U} : (lxp_u128){0U, 0U};
        CHECK(lxp_activity_signing_preimage(&activities[i], preimage) == LXP_OK);
        key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, maintenance_actor_seed, 32U);
        ctx = EVP_MD_CTX_new();
        CHECK(key != NULL && ctx != NULL);
        CHECK(EVP_DigestSignInit(ctx, NULL, NULL, NULL, key) == 1 &&
            EVP_DigestSign(ctx, signatures[i], &signature_length, preimage, 32U) == 1 && signature_length == 64U);
        EVP_MD_CTX_free(ctx);
        EVP_PKEY_free(key);
        activities[i].signature = (lxp_byte_span){signatures[i], 64U};
        }
        {
            size_t mark = lxp_arena_mark(&f->arena);
            CHECK(lxp_activity_encode(&activities[i], &f->arena, &canonical[i]) == LXP_OK);
            canonical_storage[i] = malloc(canonical[i].length);
            CHECK(canonical_storage[i] != NULL);
            memcpy(canonical_storage[i], canonical[i].bytes, canonical[i].length);
            canonical[i].bytes = canonical_storage[i];
            CHECK(lxp_arena_reset(&f->arena, mark) == LXP_OK);
        }
        executions[i].network_id = activities[i].network_id;
        executions[i].batch_number = batch_number;
        executions[i].batch_timestamp_ms = f->input_activity != NULL ?
            activities[i].timestamp_bound.not_before : 10U;
        executions[i].maximum_timestamp_window = f->input_activity != NULL ?
            activities[i].timestamp_bound.not_after - activities[i].timestamp_bound.not_before : 100U;
        executions[i].epoch = 1U;
        executions[i].global_sequence = first_sequence + i;
        executions[i].recorded_module_version = type == LXP_BRIDGE_CREDIT ? 1U : type == LX_ASSET_SEND ?
            lx_asset_module_iface()->abi_version : LX_PROGRAMS_SANDBOX_DESTROY_ABI_VERSION;
        executions[i].parameter_version = 1U;
        executions[i].signature_valid = true;
        executions[i].identities = &f->identities;
        executions[i].authority = &f->authority;
        executions[i].fee_parameters = &f->fees;
        executions[i].fee_balance = f->actor != NULL ? f->actor->balance : (lxp_u128){0U, 0U};
        executions[i].gas_limit = UINT64_MAX;
        executions[i].arena = &f->arena;
        executions[i].sequencer_private_key = executed_sequencer_seed;
    }
    CHECK(lxp_daemon_batch_bind_prefix(canonical, count, f->kernel.current_state_root,
        first_sequence, batch_number, &f->arena, executions, &roots, batch_id) == LXP_OK);
    if (type == LX_PROGRAMS_CALL) {
        lxp_result prepared_status = lxp_kernel_prepare_activity_batch(&f->kernel, activities, executions,
            count, 4U, &prepared, &retry);
        if (prepared_status != LXP_OK) fprintf(stderr, "CALL preparation result %d, prefix %zu\n", (int)prepared_status, retry);
        CHECK(prepared_status == LXP_OK);
        CHECK(retry == 0U);
    } else {
        CHECK(count == 1U);
        lxp_result prepared_status = lxp_kernel_prepare_serial_activity_batch(&f->kernel, activities, executions, &prepared);
        if (prepared_status != LXP_OK) fprintf(stderr, "serial preparation result %d\n", (int)prepared_status);
        CHECK(prepared_status == LXP_OK);
    }
    {
        lxp_result maintenance_status = lxp_kernel_prepare_batch_maintenance(prepared, activities, executions);
        if (maintenance_status != LXP_OK) fprintf(stderr, "maintenance preparation result %d\n", (int)maintenance_status);
        CHECK(maintenance_status == LXP_OK);
    }
    CHECK(f->state.next_sequence == first_sequence);
    decoded = lxp_kernel_prepared_batch_receipts(prepared);
    for (size_t i = 0U; i < count; ++i) {
        if (decoded[i].result_code != LXP_OK)
            fprintf(stderr, "maintained activity type %u result %d\n", type, (int)decoded[i].result_code);
        CHECK(decoded[i].result_code == LXP_OK);
        size_t mark = lxp_arena_mark(&f->arena);
        CHECK(lxp_receipt_verify(&decoded[i], f->authorization.public_key, &f->arena) == LXP_OK);
        CHECK(lxp_arena_reset(&f->arena, mark) == LXP_OK);
        CHECK(lxp_receipt_encode(&decoded[i], true, &f->arena, &receipts[i]) == LXP_OK);
        receipt_storage[i] = malloc(receipts[i].length);
        CHECK(receipt_storage[i] != NULL);
        memcpy(receipt_storage[i], receipts[i].bytes, receipts[i].length);
        receipts[i].bytes = receipt_storage[i];
        CHECK(lxp_arena_reset(&f->arena, mark) == LXP_OK);
        artifacts[i] = decoded[i].program_outcome.terminal_payload;
        graphs[i] = decoded[i].program_outcome.call_graph_payload;
    }
    receipts[count] = lxp_kernel_prepared_batch_maintenance(prepared);
    CHECK(lxp_programs_occupancy_receipt_decode(receipts[count].bytes, receipts[count].length, &maintenance) == LXP_OK);
    CHECK(maintenance.global_sequence == first_sequence + count && maintenance.batch_number == batch_number);
    CHECK(lxp_batch_roots_compute(&(lxp_batch_root_inputs){canonical, count, receipts, count + 1U,
        lxp_kernel_prepared_batch_events(prepared), count, NULL, 0U, NULL, 0U}, &f->arena, &roots) == LXP_OK);
    header.protocol_version = 3U;
    header.network_id = activities[0].network_id;
    header.epoch = 1U;
    header.batch_number = batch_number;
    header.first_sequence = first_sequence;
    header.last_sequence = first_sequence + count;
    header.timestamp_ms = executions[0].batch_timestamp_ms;
    memcpy(header.previous_state_root, f->kernel.current_state_root, 32U);
    memcpy(header.resulting_state_root, maintenance.resulting_state_root, 32U);
    memcpy(header.activity_merkle_root, roots.activity_merkle_root, 32U);
    memcpy(header.receipt_merkle_root, roots.receipt_merkle_root, 32U);
    memcpy(header.event_merkle_root, roots.event_merkle_root, 32U);
    memcpy(header.oracle_root, roots.oracle_root, 32U);
    memcpy(header.data_availability_root, roots.data_availability_root, 32U);
    memcpy(header.sequencer_id, f->authorization.sequencer_id, 32U);
    input.protocol_version = 3U;
    input.network_id = header.network_id;
    input.epoch = 1U;
    input.batch_number = batch_number;
    input.timestamp_ms = header.timestamp_ms;
    input.parameter_version = 1U;
    input.fee_schedule_version = lxp_kernel_prepared_batch_fee_schedule_version(prepared);
    input.metering_schedule_version = lxp_kernel_prepared_batch_metering_schedule_version(prepared);
    input.first_sequence = first_sequence;
    input.last_sequence = header.last_sequence;
    input.count = count;
    input.base = *lxp_kernel_prepared_batch_base_boundary(prepared);
    input.settled = *lxp_kernel_prepared_batch_final_boundary(prepared);
    memcpy(input.publication_digest, lxp_kernel_prepared_batch_publication_digest(prepared), 32U);
    input.authorization = f->authorization;
    input.activities = canonical;
    input.receipts = receipts;
    input.events = lxp_kernel_prepared_batch_events(prepared);
    input.terminal_payloads = artifacts;
    input.call_graphs = graphs;
    input.receipt_proofs = proofs;
    input.maintenance = receipts[count];
    CHECK(lxp_batch_sign(&header, executed_sequencer_seed, &f->authorization, input.header_signature, &f->arena) == LXP_OK);
    CHECK(lxp_batch_header_encode(&header, &f->arena, &input.canonical_header) == LXP_OK);
    for (size_t i = 0U; i <= count; ++i)
        CHECK(lxp_merkle_leaf_hash(receipts[i].bytes, receipts[i].length, leaves[i]) == LXP_OK);
    for (size_t i = 0U; i <= count; ++i) {
        lxp_merkle_proof *proof = i == count ? &input.maintenance_proof : &proofs[i];
        CHECK(lxp_merkle_proof_generate((const uint8_t (*)[32])leaves, count + 1U, i,
            &f->arena, proof, root) == LXP_OK);
        CHECK(memcmp(root, roots.receipt_merkle_root, 32U) == 0);
    }
    CHECK(lxp_daemon_batch_wal_write_prepared(f->directory, &input, durable) == LXP_OK);
    CHECK(lxp_daemon_batch_wal_load(f->directory, &f->authorization, &loaded, &present) == LXP_OK && present);
    CHECK(lxp_daemon_batch_wal_classify(loaded, &input.base, &recovery) == LXP_OK &&
        recovery == LXP_DAEMON_BATCH_WAL_DISCARD_BASE);
    CHECK(lxp_daemon_batch_wal_view(loaded)->count == count &&
        lxp_daemon_batch_wal_view(loaded)->maintenance.length == input.maintenance.length);
    CHECK(lxp_kernel_commit_prepared_batch(&f->kernel, &f->identities, prepared, durable) == LXP_OK);
    CHECK(lxp_kernel_batch_boundary_read(&f->kernel, &live) == LXP_OK);
    CHECK(lxp_daemon_batch_wal_classify(loaded, &live, &recovery) == LXP_OK &&
        recovery == LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED);
    CHECK(lxp_kernel_finalize_prepared_batch_publication(&f->kernel, activities, prepared, durable) == LXP_OK);
    for (size_t i = 0U; i < count; ++i)
        CHECK(lxp_daemon_receipt_authority_append_artifacts(&f->receipt_authority,
            receipts[i].bytes, receipts[i].length, input.canonical_header.bytes,
            input.canonical_header.length, input.header_signature, &proofs[i], &f->arena,
            artifacts[i], graphs[i]) == LXP_OK);
    {
        uint8_t bad_signature[64];
        lxp_merkle_proof bad_proof = input.maintenance_proof;
        memcpy(bad_signature, input.header_signature, 64U);
        bad_signature[0] ^= 1U;
        CHECK(lxp_daemon_receipt_authority_append_maintenance(&f->receipt_authority,
            input.maintenance.bytes, input.maintenance.length, input.canonical_header.bytes,
            input.canonical_header.length, bad_signature, &input.maintenance_proof, &f->arena) != LXP_OK);
        bad_proof.leaf_index = 0U;
        CHECK(lxp_daemon_receipt_authority_append_maintenance(&f->receipt_authority,
            input.maintenance.bytes, input.maintenance.length, input.canonical_header.bytes,
            input.canonical_header.length, input.header_signature, &bad_proof, &f->arena) != LXP_OK);
        CHECK(f->receipt_authority.last_global_sequence == first_sequence + count - 1U);
    }
    CHECK(lxp_daemon_receipt_authority_append_maintenance(&f->receipt_authority,
        input.maintenance.bytes, input.maintenance.length, input.canonical_header.bytes,
        input.canonical_header.length, input.header_signature, &input.maintenance_proof, &f->arena) == LXP_OK);
    {
        lxp_daemon_receipt_evidence activity_evidence, selected;
        bool selected_present;
        uint8_t digest[32];
        size_t mark = lxp_arena_mark(&f->arena);
        CHECK(lxp_receipt_digest(&decoded[0], &f->arena, digest) == LXP_OK);
        CHECK(lxp_daemon_receipt_authority_lookup(&f->receipt_authority,
            digest, &f->arena, &activity_evidence) == LXP_OK);
        CHECK(lxp_daemon_receipt_authority_batch_maintenance(&f->receipt_authority,
            &activity_evidence, &f->arena, &selected, &selected_present) == LXP_OK && selected_present);
        CHECK(selected.format_version == 3U && selected.receipt_proof.leaf_index == count &&
            selected.receipt_proof.leaf_count == count + 1U &&
            selected.canonical_receipt.length == input.maintenance.length &&
            memcmp(selected.canonical_receipt.bytes, input.maintenance.bytes, input.maintenance.length) == 0);
        activity_evidence.header_signature[0] ^= 1U;
        CHECK(lxp_daemon_receipt_authority_batch_maintenance(&f->receipt_authority,
            &activity_evidence, &f->arena, &selected, &selected_present) == LXP_ERR_CONTEXT_MISMATCH);
        CHECK(lxp_arena_reset(&f->arena, mark) == LXP_OK);
    }
    CHECK(f->feed.scanned_through_sequence == maintenance.global_sequence);
    CHECK(memcmp(f->feed.head_state_root, maintenance.resulting_state_root, 32U) == 0);
    CHECK(lxp_daemon_account_evidence_publish_batch_maintenance(&f->evidence, &f->kernel,
        input.maintenance, &input.maintenance_proof, &f->authorization, input.canonical_header,
        input.header_signature, &f->arena) == LXP_OK);
    {
        lxp_daemon_account_evidence account;
        CHECK(lxp_daemon_account_evidence_lookup(&f->evidence, f->authority.principal,
            maintenance.resulting_state_root, &f->arena, &account) == LXP_OK);
        CHECK(account.format_version == 2U && account.observed_sequence == maintenance.global_sequence);
        CHECK(account.canonical_receipt.length == input.maintenance.length &&
            memcmp(account.canonical_receipt.bytes, input.maintenance.bytes, input.maintenance.length) == 0);
    }
    CHECK(lxp_daemon_batch_wal_transition(f->directory, loaded, &live, LXP_DAEMON_BATCH_WAL_COMMITTED) == LXP_OK);
    CHECK(lxp_daemon_batch_wal_retire(f->directory, loaded, &live) == LXP_OK);
    CHECK(f->state.next_sequence == first_sequence + count + 1U);
    CHECK(f->receipt_authority.last_global_sequence == header.last_sequence);
    lxp_daemon_batch_wal_destroy(loaded);
    lxp_kernel_prepared_batch_destroy(prepared);
    for (size_t i = 0U; i < count; ++i) {
        free(canonical_storage[i]);
        free(receipt_storage[i]);
    }
    free(executions);
    free(activities);
    return 0;
}

static int maintenance_noncall(maintenance_fixture *f)
{
    static const uint8_t entry[] = {0x41U, 0U, 0x0bU};
    uint8_t program[32] = {0x71U}, code_hash[32], account[32], wasm[512], payload[2048];
    size_t wasm_length = candidate_module(wasm, entry, sizeof(entry));
    size_t length = deploy_payload(payload, program, f->authority.principal, wasm, wasm_length,
        code_hash, LX_PROGRAMS_ACCOUNT_ABI_VERSION, INTERFACE_CAPABILITIES_NONE);
    CHECK(maintenance_publish(f, LX_PROGRAMS_DEPLOY, payload, length, 1U, 6U) == 0);
    memset(payload, 0, sizeof(payload));
    memcpy(payload, program, 32U);
    memcpy(payload + 32U, "LXPA1", 5U);
    memcpy(payload + 37U, f->asset.asset_id, 32U);
    payload[72U] = 5U;
    memcpy(payload + 73U, "vault", 5U);
    CHECK(maintenance_publish(f, LX_PROGRAMS_ACCOUNT, payload, 78U, 1U, 7U) == 0);
    CHECK(lxp_programs_account_derive(program, (const uint8_t *)"vault", 5U, account) == LXP_OK);
    memcpy(payload, program, 32U);
    payload[32U] = 1U;
    memcpy(payload + 33U, account, 32U);
    memcpy(payload + 65U, f->asset.asset_id, 32U);
    memcpy(payload + 97U, f->actor->id, 32U);
    write_u16(payload + 129U, 5U);
    memcpy(payload + 131U, "vault", 5U);
    CHECK(maintenance_publish(f, LX_PROGRAMS_WIND_DOWN, payload, 136U, 1U, 8U) == 0);
    memcpy(payload, program, 32U);
    payload[32U] = 2U;
    memcpy(payload + 33U, program, 32U);
    write_u64(payload + 65U, f->state.next_sequence + 1U);
    CHECK(maintenance_publish(f, LX_PROGRAMS_WIND_DOWN, payload, 73U, 1U, 9U) == 0);
    payload[32U] = 3U;
    CHECK(maintenance_publish(f, LX_PROGRAMS_WIND_DOWN, payload, 33U, 1U, 10U) == 0);
    {
        lxp_send send = {0};
        uint8_t material[144], message[512], digest[32];
        size_t message_length, signature_length = 64U;
        EVP_PKEY *key;
        EVP_MD_CTX *ctx;
        lxp_u128 before = f->recipient->balance;
        memcpy(send.from, f->actor->id, 32U);
        memcpy(send.to, f->recipient->id, 32U);
        memcpy(send.asset, f->asset.asset_id, 32U);
        send.amount.lo = 1U;
        send.sequence = f->actor->next_sequence;
        send.expires_at = 100U;
        write_u64(send.idempotency_key, f->identity->next_sequence + 1U);
        send.idempotency_key[31U] = 1U;
        send.authorization.kind = LXP_AUTH_OWNER;
        send.authorization.network_id = 7U;
        send.authorization.protocol_version = 3U;
        memcpy(send.authorization.controller, send.from, 32U);
        memcpy(material, send.from, 32U);
        memcpy(material + 32U, send.to, 32U);
        memcpy(material + 64U, send.asset, 32U);
        CHECK(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
        memcpy(material + 112U, send.idempotency_key, 32U);
        CHECK(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
        memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
        memcpy(send.authorization.public_key, f->actor_public_key, 32U);
        CHECK(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
        CHECK(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
        key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, maintenance_actor_seed, 32U);
        ctx = EVP_MD_CTX_new();
        CHECK(key != NULL && ctx != NULL && EVP_DigestSignInit(ctx, NULL, NULL, NULL, key) == 1);
        CHECK(EVP_DigestSign(ctx, send.authorization.signature, &signature_length, digest, 32U) == 1 &&
            signature_length == 64U);
        EVP_MD_CTX_free(ctx);
        EVP_PKEY_free(key);
        CHECK(lxp_send_encode(&send, payload, sizeof(payload), &length) == LXP_OK);
        CHECK(maintenance_publish(f, LX_ASSET_SEND, payload, length, 1U, 11U) == 0);
        CHECK(lxp_u128_add(before, send.amount, &before) == LXP_OK);
        CHECK(lxp_u128_cmp(f->recipient->balance, before) == 0);
    }
    CHECK(f->state.next_sequence == 86U && f->receipt_authority.last_global_sequence == 85U);
    return 0;
}

static int maintenance_bridge(const char *manifest_path, const char *activity_path)
{
    maintenance_fixture *f = calloc(1U, sizeof(*f));
    lxp_genesis_manifest *manifest = calloc(1U, sizeof(*manifest));
    lxp_activity activity;
    lxp_bridge_profile profile;
    lxp_bridge_credit credit;
    lx_account *recipient;
    uint8_t *manifest_bytes, *activity_bytes;
    size_t manifest_length, activity_length;
    uint8_t name[LX_ACCOUNT_NAME_MAX], grant[32] = {0}, nullifier[32];
    size_t name_length;
    lxp_u128 amount;
    bool present;
    CHECK(f != NULL && manifest != NULL);
    CHECK(read_file(manifest_path, LXP_GENESIS_MAX_ENCODED_BYTES, false,
        &manifest_bytes, &manifest_length) == 0);
    CHECK(read_file(activity_path, LXP_MAX_ACTIVITY_BYTES, false,
        &activity_bytes, &activity_length) == 0);
    f->storage = malloc(4U * LXP_MAX_BATCH_BODY_BYTES);
    CHECK(f->storage != NULL && lxp_arena_init(&f->arena, f->storage,
        4U * LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    CHECK(lxp_genesis_parse(manifest_bytes, manifest_length, LXP_GENESIS_INPUT_MANIFEST, manifest) == LXP_OK);
    CHECK(lxp_genesis_verify_signature(manifest, &f->arena) == LXP_OK);
    CHECK(lxp_bridge_genesis_profile(manifest, &profile, &present) == LXP_OK && present);
    CHECK(lxp_activity_decode(activity_bytes, activity_length, &activity) == LXP_OK);
    CHECK(lxp_activity_verify_signature(&activity) == LXP_OK && activity.activity_type == LXP_BRIDGE_CREDIT);
    CHECK(activity.payload.length == sizeof(credit.bytes));
    memcpy(credit.bytes, activity.payload.bytes, sizeof(credit.bytes));
    CHECK(lxp_bridge_credit_verify(&profile, &credit, manifest->network_id, 3U, nullifier) == LXP_OK);
    CHECK(lxp_u128_from_be(credit.bytes + 191U, &amount) == LXP_OK);
    CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    CHECK(lxp_state_store_bind_accounts(&f->state, &f->accounts) == LXP_OK);
    CHECK(lxp_kernel_create(&f->kernel, &f->state, &f->journal, manifest, 1U) == LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, programs_module_registration_v4()) == LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, lx_asset_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_register_module(&f->kernel, lxp_bridge_module_iface()) == LXP_OK);
    CHECK(lxp_kernel_set_capabilities(&f->kernel, NULL, lxp_kernel_canonical_ledger_apply) == LXP_OK);
    CHECK(lxp_genesis_materialize(manifest, &f->arena, &f->kernel) == LXP_OK);
    for (size_t i = 0U; i < f->accounts.count; ++i)
        CHECK(lxp_u128_is_zero(f->accounts.accounts[i].balance));
    memcpy(f->asset.asset_id, profile.bytes + 97U, 32U);
    f->asset.registered = true;
    memcpy(f->asset_record.asset_id, f->asset.asset_id, 32U);
    f->asset_runtime = (lx_asset_runtime){&f->accounts, &f->asset_record, 1U,
        &f->asset, 1U, manifest->network_id, 3U};
    CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET, &f->asset_runtime) == LXP_OK);
    f->runtime.accounts = &f->accounts;
    f->runtime.assets = &f->asset;
    f->runtime.asset_count = 1U;
    f->runtime.resolve_metering_schedule = lxp_programs_metering_resolve_runtime;
    f->runtime.metering_schedule_context = &f->kernel;
    f->runtime.resolve_occupancy_parameters = lxp_programs_fee_governance_resolve_runtime;
    f->runtime.occupancy_parameter_context = &f->kernel;
    CHECK(lxp_programs_fee_governance_resolve_runtime(&f->kernel, 0U,
        &f->runtime.fee_schedule, f->runtime.occupancy_asset_id) == LXP_OK);
    CHECK(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_PROGRAMS, &f->runtime) == LXP_OK);
    CHECK(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    CHECK(memcmp(f->kernel.current_state_root, manifest->genesis_state_root, 32U) == 0);
    CHECK(activity.authority.length == 32U);
    CHECK(lxp_identity_register(&f->identities, activity.actor_did.bytes, activity.actor_did.length,
        activity.authority.bytes, &f->identity) == LXP_OK);
    CHECK(activity.actor_did.length <= sizeof(name) - 11U);
    memcpy(name, "agent:", 6U);
    memcpy(name + 6U, activity.actor_did.bytes, activity.actor_did.length);
    name_length = 6U + activity.actor_did.length;
    memcpy(name + name_length, ":main", 5U);
    name_length += 5U;
    CHECK(lx_account_id_from_string(name, name_length, f->authority.principal) == LXP_OK);
    f->scope.activity_ordinal_min = 1U;
    f->scope.activity_ordinal_max = 1U;
    f->scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    f->scope.maximum_total = f->scope.maximum_per_activity;
    f->scope.maximum_per_period = f->scope.maximum_per_activity;
    f->authority.scope = &f->scope;
    f->authority.kind = LXP_AUTHORITY_OWNER;
    memcpy(f->authority.actor, f->identity->did_id, 32U);
    memcpy(f->authority.verified_key, activity.authority.bytes, 32U);
    CHECK(lxp_authority_hash(f->authority.kind, grant, f->authority.verified_key,
        f->authority.authority_hash) == LXP_OK);
    f->fees.version = 1U;
    f->fees.multiplier_basis_points = 10000U;
    CHECK(maintenance_fixture_logs(f, manifest->network_id) == 0);
    f->input_activity = &activity;
    CHECK(maintenance_publish(f, LXP_BRIDGE_CREDIT, activity.payload.bytes,
        activity.payload.length, 1U, 1U) == 0);
    CHECK(lx_account_lookup(&f->accounts, name, name_length, credit.bytes + 107U, &recipient) == LXP_OK);
    CHECK(lxp_u128_cmp(recipient->balance, amount) == 0);
    CHECK(f->state.next_sequence == 3U && f->receipt_authority.last_global_sequence == 2U);
    CHECK(lxp_history_close(&f->history) == LXP_OK);
    CHECK(lxp_log_close(&f->feed_log) == LXP_OK && lxp_log_close(&f->canonical_log) == LXP_OK &&
        lxp_log_close(&f->evidence_log) == LXP_OK && lxp_log_close(&f->authority_log) == LXP_OK);
    CHECK(pthread_mutex_destroy(&f->feed_mutex) == 0);
    while (f->kernel.blob_count != 0U) free(f->kernel.blobs[--f->kernel.blob_count].bytes);
    CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f->storage);
    free(f);
    free(manifest);
    free(manifest_bytes);
    free(activity_bytes);
    puts("real custody credit privately prepared and published with authenticated maintenance");
    return 0;
}

int main(int argc, char **argv)
{
    if (argc == 3) return maintenance_bridge(argv[1], argv[2]);
    CHECK(argc == 1);
    maintenance_fixture *f = calloc(1U, sizeof(*f));
    static const uint8_t entry[] = {0x41U, 0U, 0x0bU};
    static const uint8_t upgraded_entry[] = {0x41U, 7U, 0x0bU};
    uint8_t wasm[512], upgraded[128], payload[2048], call[STAGED_CALL_FIXTURE_BYTES];
    uint8_t program[32] = {0x31U}, code_hash[32], upgraded_hash[32];
    size_t wasm_length = candidate_module(wasm, entry, sizeof(entry));
    size_t upgraded_length = candidate_module(upgraded, upgraded_entry, sizeof(upgraded_entry));
    size_t length;
    lxp_daemon_receipt_authority_store reopened;
    CHECK(f != NULL && maintenance_fixture_open(f) == 0);
    length = deploy_payload(payload, program, f->authority.principal, wasm, wasm_length,
        code_hash, LX_PROGRAMS_ABI_VERSION, INTERFACE_CAPABILITIES_NONE);
    CHECK(maintenance_publish(f, LX_PROGRAMS_DEPLOY, payload, length, 1U, 1U) == 0);
    length = staged_call_payload(call, program);
    CHECK(length == sizeof(call));
    CHECK(maintenance_publish(f, LX_PROGRAMS_CALL, call, length, 1U, 2U) == 0);
    length = upgrade_payload(payload, program, code_hash, upgraded, upgraded_length,
        upgraded_hash, LX_PROGRAMS_ABI_VERSION, INTERFACE_CAPABILITIES_NONE, false);
    CHECK(maintenance_publish(f, LX_PROGRAMS_UPGRADE, payload, length, 1U, 3U) == 0);
    length = staged_call_payload(call, program);
    CHECK(maintenance_publish(f, LX_PROGRAMS_CALL, call, length, 1U, 4U) == 0);
    CHECK(maintenance_publish(f, LX_PROGRAMS_CALL, call, length, 64U, 5U) == 0);
    CHECK(f->state.next_sequence == 74U);
    CHECK(lxp_log_close(&f->authority_log) == LXP_OK);
    CHECK(lxp_log_open_or_create(&f->authority_log, f->authority_path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    CHECK(lxp_daemon_receipt_authority_open(&reopened, &f->authority_log, &f->authorization) == LXP_OK);
    CHECK(reopened.last_global_sequence == 73U);
    CHECK(lxp_programs_state_feed_store_open(&f->feed, &f->feed_log, &f->canonical_log,
        &f->history, &f->arena, &f->feed_mutex) == LXP_OK);
    CHECK(lxp_programs_state_feed_store_recover(&f->feed, &f->kernel) == LXP_OK);
    CHECK(f->feed.scanned_through_sequence == 73U &&
        memcmp(f->feed.head_state_root, f->kernel.current_state_root, 32U) == 0);
    CHECK(maintenance_noncall(f) == 0);
    CHECK(lxp_history_close(&f->history) == LXP_OK);
    CHECK(lxp_log_close(&f->feed_log) == LXP_OK);
    CHECK(lxp_log_close(&f->canonical_log) == LXP_OK);
    CHECK(lxp_log_close(&f->evidence_log) == LXP_OK);
    CHECK(pthread_mutex_destroy(&f->feed_mutex) == 0);
    CHECK(lxp_log_close(&f->authority_log) == LXP_OK);
    while (f->kernel.blob_count != 0U) free(f->kernel.blobs[--f->kernel.blob_count].bytes);
    CHECK(lxp_state_store_destroy(&f->state) == LXP_OK);
    free(f->storage);
    free(f);
    puts("real lifecycle maintenance, mixed WAL recovery, combined authority and 64-activity batch passed");
    return 0;
}
