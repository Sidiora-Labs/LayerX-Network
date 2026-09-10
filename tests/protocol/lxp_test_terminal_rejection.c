#define _POSIX_C_SOURCE 200809L
#include "lxp_daemon_batch_wal.h"
#include "layerx/lxp_daemon.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lx_asset.h"
#include <unistd.h>
#define main terminal_rejection_activity_fixture_main
#include "../programs/test_call_activity.c"
#undef main

#define CHECK(expression) do { if (!(expression)) { \
    fprintf(stderr, "terminal rejection failure at %d: %s\n", __LINE__, #expression); \
    return 1; } } while (0)

static const uint8_t terminal_actor_seed[32] = {0x51U};
static const uint8_t terminal_did[] = "did:lxp:terminal-rejection";
static const uint8_t terminal_actor_name[] = "agent:did:lxp:terminal-rejection:main";
static const uint8_t terminal_treasury_name[] = "system:fees";
static const uint8_t terminal_recipient_name[] = "agent:did:lxp:terminal-recipient:main";

typedef struct terminal_fixture {
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
    lxp_log feed_log;
    lxp_log canonical_log;
    lxp_history history;
    lx_programs_state_feed_store feed;
    pthread_mutex_t feed_mutex;
    lxp_arena arena;
    uint8_t *storage;
    uint8_t actor_public_key[32];
    uint8_t actor_id[32];
    uint8_t recipient_id[32];
    char directory[128];
} terminal_fixture;

static int terminal_sign(const uint8_t seed[32], const uint8_t *message,
                         size_t length, uint8_t signature[64])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, seed, 32U);
    EVP_MD_CTX *context = EVP_MD_CTX_new();
    size_t signature_length = 64U;
    int ok = key != NULL && context != NULL &&
        EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(context, signature, &signature_length, message, length) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int terminal_fixture_open(terminal_fixture *f)
{
    static const uint64_t parameters = 1U;
    uint8_t treasury_id[32], grant[32] = {0};
    lxp_genesis_manifest manifest = {0};
    lx_programs_fee_genesis_parameters fees = {0};
    memset(f, 0, sizeof(*f));
    f->storage = malloc(4U * LXP_MAX_BATCH_BODY_BYTES);
    CHECK(f->storage != NULL);
    CHECK(lxp_arena_init(&f->arena, f->storage, 4U * LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
    CHECK(executed_public_key(terminal_actor_seed, f->actor_public_key) == 0);
    CHECK(executed_public_key(executed_sequencer_seed, f->authorization.public_key) == 0);
    memcpy(f->authorization.sequencer_id, f->authorization.public_key, 32U);
    f->authorization.authorized = 1U;
    f->authorization.first_batch_number = 1U;
    f->authorization.last_batch_number = 100U;
    CHECK(lx_account_registry_init(&f->accounts) == LXP_OK);
    CHECK(lx_account_id_from_string(terminal_actor_name,
        sizeof(terminal_actor_name) - 1U, f->actor_id) == LXP_OK);
    CHECK(lx_account_id_from_string(terminal_treasury_name,
        sizeof(terminal_treasury_name) - 1U, treasury_id) == LXP_OK);
    CHECK(lx_account_id_from_string(terminal_recipient_name,
        sizeof(terminal_recipient_name) - 1U, f->recipient_id) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, terminal_actor_name, sizeof(terminal_actor_name) - 1U,
        f->actor_id, 1U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->actor) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, terminal_treasury_name, sizeof(terminal_treasury_name) - 1U,
        treasury_id, 2U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->treasury) == LXP_OK);
    CHECK(lx_account_open(&f->accounts, terminal_recipient_name, sizeof(terminal_recipient_name) - 1U,
        f->recipient_id, 3U, LX_ACCOUNT_OPEN_GENESIS, NULL, &f->recipient) == LXP_OK);
    f->asset.asset_id[0] = 9U;
    f->asset.registered = true;
    CHECK(lxp_ledger_bootstrap_balance(f->actor, f->asset.asset_id,
        (lxp_u128){0U, 1000U}, 1U) == LXP_OK);
    CHECK(lxp_ledger_bootstrap_balance(f->treasury, f->asset.asset_id,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    CHECK(lxp_ledger_bootstrap_balance(f->recipient, f->asset.asset_id,
        (lxp_u128){0U, 0U}, 0U) == LXP_OK);
    CHECK(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    CHECK(lxp_identity_register(&f->identities, terminal_did, sizeof(terminal_did) - 1U,
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
    f->scope.module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    f->scope.activity_ordinal_min = 1U;
    f->scope.activity_ordinal_max = 10U;
    f->scope.maximum_per_activity = (lxp_u128){UINT64_MAX, UINT64_MAX};
    f->scope.maximum_total = f->scope.maximum_per_activity;
    f->scope.maximum_per_period = f->scope.maximum_per_activity;
    f->authority.scope = &f->scope;
    f->authority.kind = LXP_AUTHORITY_OWNER;
    memcpy(f->authority.actor, f->identity->did_id, 32U);
    memcpy(f->authority.principal, f->actor_id, 32U);
    memcpy(f->authority.verified_key, f->actor_public_key, 32U);
    CHECK(lxp_authority_hash(f->authority.kind, grant, f->actor_public_key,
        f->authority.authority_hash) == LXP_OK);
    f->fees.version = 1U;
    f->fees.multiplier_basis_points = 10000U;
    {
        char path[192], database[192];
        strcpy(f->directory, "/tmp/lxp-terminal-rejection-XXXXXX");
        CHECK(mkdtemp(f->directory) != NULL);
        CHECK(pthread_mutex_init(&f->feed_mutex, NULL) == 0);
        CHECK(snprintf(path, sizeof(path), "%s/feed.log", f->directory) > 0);
        CHECK(lxp_log_open_or_create(&f->feed_log, path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
        CHECK(snprintf(path, sizeof(path), "%s/canonical.log", f->directory) > 0);
        CHECK(lxp_log_open_or_create(&f->canonical_log, path, LXP_MAX_BATCH_BODY_BYTES) == LXP_OK);
        CHECK(snprintf(database, sizeof(database), "%s/history.db", f->directory) > 0);
        CHECK(lxp_history_open(&f->history, &f->canonical_log, database,
            "migrations/0007_history_index.sql") == LXP_OK);
        CHECK(lxp_programs_state_feed_store_open(&f->feed, &f->feed_log, &f->canonical_log,
            &f->history, &f->arena, &f->feed_mutex) == LXP_OK);
        CHECK(lxp_programs_state_feed_store_anchor(&f->feed, f->state.next_sequence,
            f->kernel.current_state_root) == LXP_OK);
        f->runtime.state_feed = &f->feed.feed;
        CHECK(lxp_programs_bind_state_feed(&f->kernel, f->runtime.state_feed) == LXP_OK);
        CHECK(lxp_programs_state_feed_store_recover(&f->feed, &f->kernel) == LXP_OK);
    }
    return 0;
}

static int terminal_build_send(terminal_fixture *f, lxp_activity *activity,
                               uint8_t *payload, size_t *payload_length)
{
    lxp_send send;
    uint8_t material[144], message[512], preimage[32];
    size_t message_length;
    memset(&send, 0, sizeof(send));
    memcpy(send.from, f->actor_id, 32U);
    memcpy(send.to, f->recipient_id, 32U);
    memcpy(send.asset, f->asset.asset_id, 32U);
    send.amount.lo = 4U;
    send.sequence = f->actor->next_sequence;
    send.expires_at = 100U;
    send.idempotency_key[0] = 0x2AU;
    send.authorization.kind = LXP_AUTH_OWNER;
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    memcpy(send.authorization.controller, send.from, 32U);
    memcpy(send.authorization.public_key, f->actor_public_key, 32U);
    memcpy(material, send.from, 32U);
    memcpy(material + 32U, send.to, 32U);
    memcpy(material + 64U, send.asset, 32U);
    CHECK(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    memcpy(material + 112U, send.idempotency_key, 32U);
    CHECK(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    CHECK(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
    CHECK(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, preimage) == LXP_OK);
    CHECK(terminal_sign(terminal_actor_seed, preimage, 32U, send.authorization.signature) == 0);
    CHECK(lxp_send_encode(&send, payload, 512U, payload_length) == LXP_OK);
    memset(activity, 0, sizeof(*activity));
    activity->protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    activity->network_id = 7U;
    activity->activity_type = LX_ASSET_SEND;
    activity->actor_did = (lxp_byte_span){terminal_did, sizeof(terminal_did) - 1U};
    activity->authority = (lxp_byte_span){f->actor_public_key, 32U};
    activity->timestamp_bound = (lxp_timestamp_bound){1U, 100U};
    activity->account_sequence = send.sequence;
    activity->payload = (lxp_byte_span){payload, *payload_length};
    memcpy(activity->idempotency_key, send.idempotency_key, 32U);
    CHECK(lxp_hash_payload(payload, *payload_length, activity->payload_hash) == LXP_OK);
    CHECK(lxp_activity_signing_preimage(activity, preimage) == LXP_OK);
    {
        static uint8_t signature[64];
        CHECK(terminal_sign(terminal_actor_seed, preimage, 32U, signature) == 0);
        activity->signature = (lxp_byte_span){signature, 64U};
    }
    CHECK(lxp_activity_verify_signature(activity) == LXP_OK);
    return 0;
}

static void terminal_execution(const terminal_fixture *f, lxp_kernel_execution *execution,
                               uint64_t global_sequence, uint64_t batch_number)
{
    execution->network_id = 7U;
    execution->batch_number = batch_number;
    execution->batch_timestamp_ms = 10U;
    execution->maximum_timestamp_window = 100U;
    execution->epoch = 1U;
    execution->global_sequence = global_sequence;
    execution->recorded_module_version = lx_asset_module_iface()->abi_version;
    execution->parameter_version = 1U;
    execution->signature_valid = true;
    execution->identities = (lxp_identity_store *)&f->identities;
    execution->authority = (const lxp_authority_resolved *)&f->authority;
    execution->fee_parameters = (const lxp_fee_params *)&f->fees;
    execution->fee_balance = f->actor->balance;
    execution->gas_limit = UINT64_MAX;
    execution->arena = (lxp_arena *)&f->arena;
    execution->sequencer_private_key = executed_sequencer_seed;
}

static int classification_case(void)
{
    CHECK(!lxp_terminal_rejection_applies(LXP_OK));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_IDEMPOTENT_REPLAY));
    CHECK(!lxp_terminal_rejection_applies(LXP_FATAL_INVARIANT));
    CHECK(!lxp_terminal_rejection_applies(LXP_FATAL_REPLAY_DIVERGENCE));
    CHECK(!lxp_terminal_rejection_applies(LXP_FATAL_SUPPLY_MISMATCH));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_BATCH_GAP));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_DA_MISSING));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_IO));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_LOG_CORRUPT));
    CHECK(!lxp_terminal_rejection_applies(LXP_ERR_ARENA_EXHAUSTED));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_TRUNCATED));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_UNKNOWN_ACTIVITY));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_UNKNOWN_DID));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_BAD_SIGNATURE));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_IDENTITY_FROZEN));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_SEQUENCE_MISMATCH));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_INSUFFICIENT_BALANCE));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_OVERFLOW));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_FEE_LIMIT));
    CHECK(lxp_terminal_rejection_applies(LXP_ERR_PROGRAM_REFUSED));
    return 0;
}

static int terminal_wal_record(terminal_fixture *f, const lxp_byte_span *canonical,
    const lxp_kernel_prepared_batch *prepared, const lxp_kernel_execution *execution,
    lxp_byte_span receipt, uint8_t digest[32])
{
    lxp_daemon_batch_wal_input input = {0};
    lxp_batch_header header = {0};
    lxp_batch_body *body = malloc(sizeof(*body));
    lxp_batch_roots roots;
    lxp_merkle_proof proofs[1];
    lxp_byte_span receipts[1];
    lxp_byte_span artifacts[1] = {{NULL, 0U}};
    lxp_byte_span graphs[1] = {{NULL, 0U}};
    uint8_t leaves[1][32], root[32];
    CHECK(body != NULL);
    receipts[0] = receipt;
    CHECK(lxp_batch_roots_compute(&(lxp_batch_root_inputs){canonical, 1U, receipts, 1U,
        lxp_kernel_prepared_batch_events(prepared), 1U, NULL, 0U, NULL, 0U},
        &f->arena, &roots) == LXP_OK);
    header.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    header.network_id = 7U;
    header.epoch = 1U;
    header.batch_number = execution->batch_number;
    header.first_sequence = execution->global_sequence;
    header.last_sequence = execution->global_sequence;
    header.timestamp_ms = execution->batch_timestamp_ms;
    memcpy(header.previous_state_root,
        lxp_kernel_prepared_batch_base_boundary(prepared)->receipt_state_root, 32U);
    memcpy(header.resulting_state_root, lxp_kernel_prepared_batch_final_root(prepared), 32U);
    memcpy(header.activity_merkle_root, roots.activity_merkle_root, 32U);
    memcpy(header.receipt_merkle_root, roots.receipt_merkle_root, 32U);
    memcpy(header.event_merkle_root, roots.event_merkle_root, 32U);
    memcpy(header.oracle_root, roots.oracle_root, 32U);
    memcpy(header.data_availability_root, roots.data_availability_root, 32U);
    memcpy(header.sequencer_id, f->authorization.sequencer_id, 32U);
    CHECK(lxp_merkle_leaf_hash(receipts[0].bytes, receipts[0].length, leaves[0]) == LXP_OK);
    CHECK(lxp_merkle_proof_generate((const uint8_t (*)[32])leaves, 1U, 0U,
        &f->arena, &proofs[0], root) == LXP_OK);
    CHECK(memcmp(root, roots.receipt_merkle_root, 32U) == 0);
    input.protocol_version = header.protocol_version;
    input.network_id = header.network_id;
    input.epoch = header.epoch;
    input.batch_number = header.batch_number;
    input.timestamp_ms = header.timestamp_ms;
    input.parameter_version = execution->parameter_version;
    input.fee_schedule_version = lxp_kernel_prepared_batch_fee_schedule_version(prepared);
    input.metering_schedule_version = lxp_kernel_prepared_batch_metering_schedule_version(prepared);
    input.first_sequence = header.first_sequence;
    input.last_sequence = header.last_sequence;
    input.count = 1U;
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
    CHECK(lxp_da_body_from_kernels(&header,
        lxp_kernel_prepared_batch_base_kernel(prepared),
        lxp_kernel_prepared_batch_settled_kernel(prepared),
        canonical, 1U, receipts, 1U, input.events, 1U, NULL, 0U,
        &f->arena, body) == LXP_OK);
    header = body->header;
    input.state_diff = body->state_diff;
    input.recovery_metadata = body->recovery_metadata;
    CHECK(lxp_batch_sign(&header, executed_sequencer_seed, &f->authorization,
        input.header_signature, &f->arena) == LXP_OK);
    CHECK(lxp_batch_header_encode(&header, &f->arena, &input.canonical_header) == LXP_OK);
    CHECK(lxp_daemon_batch_wal_write_prepared(f->directory, &input, digest) == LXP_OK);
    free(body);
    return 0;
}

static int terminal_rejection_case(void)
{
    terminal_fixture *live = malloc(sizeof(*live));
    terminal_fixture *restarted = malloc(sizeof(*restarted));
    lxp_activity activity;
    lxp_activity replay_activity;
    lxp_kernel_execution execution;
    lxp_kernel_execution replay_execution;
    lxp_kernel_execution rejected;
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_daemon_batch_wal_record *loaded = NULL;
    lxp_daemon_batch_wal_recovery recovery;
    lxp_kernel_batch_boundary live_boundary;
    lxp_kernel_batch_boundary replay_boundary;
    lxp_batch_roots roots, replay_roots;
    lxp_byte_span canonical, replay_canonical, encoded, replay_encoded;
    const lxp_receipt *decoded;
    lxp_receipt replayed;
    lxp_receipt duplicate;
    uint8_t payload[512], replay_payload[512];
    uint8_t canonical_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t receipt_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t batch_id[32], replay_batch_id[32], digest[32];
    uint8_t base_root[32];
    uint64_t first_sequence;
    uint64_t actor_sequence_before;
    lxp_u128 actor_balance_before;
    lxp_u128 recipient_balance_before;
    lxp_u128 treasury_balance_before;
    size_t payload_length = 0U, replay_payload_length = 0U, receipt_length;
    bool present = false;
    CHECK(live != NULL && restarted != NULL);
    CHECK(terminal_fixture_open(live) == 0);
    CHECK(terminal_fixture_open(restarted) == 0);
    CHECK(memcmp(live->kernel.current_state_root,
                 restarted->kernel.current_state_root, 32U) == 0);
    CHECK(terminal_build_send(live, &activity, payload, &payload_length) == 0);
    CHECK(terminal_build_send(restarted, &replay_activity, replay_payload,
                              &replay_payload_length) == 0);
    CHECK(payload_length == replay_payload_length);
    CHECK(memcmp(payload, replay_payload, payload_length) == 0);
    CHECK(lxp_activity_encode(&activity, &live->arena, &canonical) == LXP_OK);
    CHECK(canonical.length <= sizeof(canonical_bytes));
    memcpy(canonical_bytes, canonical.bytes, canonical.length);
    canonical = (lxp_byte_span){canonical_bytes, canonical.length};
    first_sequence = live->state.next_sequence;
    memcpy(base_root, live->kernel.current_state_root, 32U);
    actor_sequence_before = live->actor->next_sequence;
    actor_balance_before = live->actor->balance;
    recipient_balance_before = live->recipient->balance;
    treasury_balance_before = live->treasury->balance;
    memset(&execution, 0, sizeof(execution));
    terminal_execution(live, &execution, first_sequence, 1U);
    CHECK(lxp_daemon_batch_bind_prefix(&canonical, 1U, live->kernel.current_state_root,
        first_sequence, 1U, &live->arena, &execution, &roots, batch_id) == LXP_OK);

    /* An acknowledged activity whose apply refuses becomes a canonical receipt. */
    CHECK(lxp_kernel_prepare_terminal_rejection(&live->kernel, &activity, &execution,
        LXP_ERR_IDENTITY_FROZEN, &prepared) == LXP_OK);
    CHECK(lxp_kernel_prepared_batch_count(prepared) == 1U);
    decoded = lxp_kernel_prepared_batch_receipts(prepared);
    CHECK(decoded[0].result_code == LXP_ERR_IDENTITY_FROZEN);
    CHECK(decoded[0].global_sequence == first_sequence);
    CHECK(decoded[0].protocol_version == LXP_PROTOCOL_VERSION_STATE_COMMITMENT);
    CHECK(decoded[0].module_id == LXP_MODULE_ASSET);
    CHECK(decoded[0].module_version == lx_asset_module_iface()->abi_version);
    CHECK(decoded[0].parameter_version == 1U);
    CHECK(decoded[0].fee_charged.hi == 0U && decoded[0].fee_charged.lo == 0U);
    CHECK(decoded[0].effects.count == 0U);
    CHECK(memcmp(decoded[0].previous_state_root, base_root, 32U) == 0);
    CHECK(memcmp(decoded[0].batch_id, batch_id, 32U) == 0);
    CHECK(memcmp(decoded[0].activity_root, roots.activity_merkle_root, 32U) == 0);
    CHECK(memcmp(decoded[0].resulting_state_root, base_root, 32U) != 0);
    CHECK(lxp_receipt_verify(&decoded[0], live->authorization.public_key,
                             &live->arena) == LXP_OK);
    CHECK(lxp_receipt_encode(&decoded[0], true, &live->arena, &encoded) == LXP_OK);
    CHECK(encoded.length <= sizeof(receipt_bytes));
    memcpy(receipt_bytes, encoded.bytes, encoded.length);
    receipt_length = encoded.length;
    encoded = (lxp_byte_span){receipt_bytes, receipt_length};
    /* The prepared batch stages the refusal; the live kernel is untouched. */
    CHECK(live->state.next_sequence == first_sequence);
    CHECK(memcmp(live->kernel.current_state_root, base_root, 32U) == 0);

    /* The refusal reaches the write-ahead log before the kernel commits. */
    CHECK(terminal_wal_record(live, &canonical, prepared, &execution,
                              encoded, digest) == 0);
    CHECK(lxp_daemon_batch_wal_load(live->directory, &live->authorization,
                                    &loaded, &present) == LXP_OK && present);
    CHECK(lxp_daemon_batch_wal_classify(loaded,
        lxp_kernel_prepared_batch_base_boundary(prepared), &recovery) == LXP_OK &&
        recovery == LXP_DAEMON_BATCH_WAL_DISCARD_BASE);
    CHECK(lxp_daemon_batch_wal_view(loaded)->count == 1U);
    CHECK(lxp_daemon_batch_wal_view(loaded)->receipts[0].length == receipt_length);
    CHECK(memcmp(lxp_daemon_batch_wal_view(loaded)->receipts[0].bytes,
                 receipt_bytes, receipt_length) == 0);

    /* The offered global sequence is consumed by the refusal. */
    CHECK(lxp_kernel_commit_prepared_batch(&live->kernel, &live->identities,
                                           prepared, digest) == LXP_OK);
    CHECK(live->state.next_sequence == first_sequence + 1U);
    CHECK(memcmp(live->kernel.current_state_root,
                 decoded[0].resulting_state_root, 32U) == 0);
    CHECK(lxp_kernel_batch_boundary_read(&live->kernel, &live_boundary) == LXP_OK);
    CHECK(lxp_daemon_batch_wal_classify(loaded, &live_boundary, &recovery) == LXP_OK &&
        recovery == LXP_DAEMON_BATCH_WAL_FINALIZE_SETTLED);
    /* No module effect, no fee and no actor sequence is consumed by a refusal. */
    CHECK(live->actor->next_sequence == actor_sequence_before);
    CHECK(live->actor->balance.hi == actor_balance_before.hi &&
          live->actor->balance.lo == actor_balance_before.lo);
    CHECK(live->recipient->balance.hi == recipient_balance_before.hi &&
          live->recipient->balance.lo == recipient_balance_before.lo);
    CHECK(live->treasury->balance.hi == treasury_balance_before.hi &&
          live->treasury->balance.lo == treasury_balance_before.lo);

    /* Publication advances the programs state feed frontier. */
    CHECK(lxp_kernel_finalize_prepared_batch_publication(&live->kernel, &activity,
                                                         prepared, digest) == LXP_OK);
    CHECK(live->feed.scanned_through_sequence == first_sequence);
    CHECK(memcmp(live->feed.head_state_root, decoded[0].resulting_state_root, 32U) == 0);

    /* The recorded refusal is the anti-replay guard for the same activity. */
    memset(&rejected, 0, sizeof(rejected));
    terminal_execution(live, &rejected, live->state.next_sequence, 2U);
    memset(&duplicate, 0, sizeof(duplicate));
    CHECK(lxp_kernel_execute_activity(&live->kernel, &activity, &rejected,
                                      &duplicate) == LXP_ERR_IDEMPOTENT_REPLAY);
    CHECK(duplicate.result_code == LXP_ERR_IDENTITY_FROZEN);
    CHECK(duplicate.global_sequence == first_sequence);
    CHECK(live->state.next_sequence == first_sequence + 1U);

    /* A restarted node replays the same refusal from the same base state. */
    CHECK(lxp_activity_encode(&replay_activity, &restarted->arena,
                              &replay_canonical) == LXP_OK);
    CHECK(replay_canonical.length == canonical.length);
    CHECK(memcmp(replay_canonical.bytes, canonical.bytes, canonical.length) == 0);
    memset(&replay_execution, 0, sizeof(replay_execution));
    terminal_execution(restarted, &replay_execution, first_sequence, 1U);
    CHECK(lxp_daemon_batch_bind_prefix(&replay_canonical, 1U,
        restarted->kernel.current_state_root, first_sequence, 1U,
        &restarted->arena, &replay_execution, &replay_roots,
        replay_batch_id) == LXP_OK);
    CHECK(memcmp(replay_batch_id, batch_id, 32U) == 0);
    /* Outcomes that are not terminal rejections stay fail-stop. */
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &replay_execution, LXP_OK, &replayed) == LXP_ERR_NON_CANONICAL);
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &replay_execution, LXP_ERR_IDEMPOTENT_REPLAY, &replayed) == LXP_ERR_NON_CANONICAL);
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &replay_execution, LXP_FATAL_INVARIANT, &replayed) == LXP_ERR_NON_CANONICAL);
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &replay_execution, LXP_ERR_IO, &replayed) == LXP_ERR_NON_CANONICAL);
    memset(&rejected, 0, sizeof(rejected));
    terminal_execution(restarted, &rejected, first_sequence + 1U, 1U);
    memcpy(rejected.batch_id, replay_execution.batch_id, 32U);
    memcpy(rejected.activity_root, replay_execution.activity_root, 32U);
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &rejected, LXP_ERR_IDENTITY_FROZEN, &replayed) == LXP_FATAL_INVARIANT);
    CHECK(restarted->state.next_sequence == first_sequence);
    CHECK(memcmp(restarted->kernel.current_state_root, base_root, 32U) == 0);
    /* The replayed refusal reproduces the canonical receipt byte for byte. */
    memset(&replayed, 0, sizeof(replayed));
    CHECK(lxp_kernel_terminal_rejection(&restarted->kernel, &replay_activity,
        &replay_execution, LXP_ERR_IDENTITY_FROZEN, &replayed) == LXP_OK);
    CHECK(lxp_receipt_encode(&replayed, true, &restarted->arena,
                             &replay_encoded) == LXP_OK);
    CHECK(replay_encoded.length == receipt_length);
    CHECK(memcmp(replay_encoded.bytes, receipt_bytes, receipt_length) == 0);
    CHECK(lxp_kernel_batch_boundary_read(&restarted->kernel, &replay_boundary) == LXP_OK);
    CHECK(replay_boundary.next_sequence == live_boundary.next_sequence);
    CHECK(memcmp(replay_boundary.receipt_state_root,
                 live_boundary.receipt_state_root, 32U) == 0);
    CHECK(memcmp(replay_boundary.canonical_state_root,
                 live_boundary.canonical_state_root, 32U) == 0);
    CHECK(restarted->feed.scanned_through_sequence == first_sequence);
    lxp_daemon_batch_wal_destroy(loaded);
    lxp_kernel_prepared_batch_destroy(prepared);
    free(live->storage);
    free(restarted->storage);
    free(live);
    free(restarted);
    return 0;
}

static int terminal_maintenance_case(void)
{
    terminal_fixture *f = malloc(sizeof(*f));
    lxp_activity activity;
    lxp_kernel_execution execution;
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_programs_occupancy_receipt sweep;
    lxp_batch_roots roots;
    lxp_byte_span canonical, maintenance;
    const lxp_receipt *decoded;
    uint8_t payload[512];
    uint8_t canonical_bytes[LXP_MAX_ACTIVITY_BYTES];
    uint8_t batch_id[32], digest[32], settled_root[32];
    uint64_t first_sequence;
    size_t payload_length = 0U;
    CHECK(f != NULL);
    CHECK(terminal_fixture_open(f) == 0);
    CHECK(terminal_build_send(f, &activity, payload, &payload_length) == 0);
    CHECK(lxp_activity_encode(&activity, &f->arena, &canonical) == LXP_OK);
    CHECK(canonical.length <= sizeof(canonical_bytes));
    memcpy(canonical_bytes, canonical.bytes, canonical.length);
    canonical = (lxp_byte_span){canonical_bytes, canonical.length};
    first_sequence = f->state.next_sequence;
    memset(&execution, 0, sizeof(execution));
    terminal_execution(f, &execution, first_sequence, 1U);
    CHECK(lxp_daemon_batch_bind_prefix(&canonical, 1U, f->kernel.current_state_root,
        first_sequence, 1U, &f->arena, &execution, &roots, batch_id) == LXP_OK);
    CHECK(lxp_kernel_prepare_terminal_rejection(&f->kernel, &activity, &execution,
        LXP_ERR_SEQUENCE_MISMATCH, &prepared) == LXP_OK);
    /* The occupancy sweep the batch coordinator runs after every batch runs
     * after a terminal rejection too and chains onto its refusal receipt. */
    CHECK(lxp_kernel_prepare_batch_maintenance(prepared, &activity, &execution) == LXP_OK);
    maintenance = lxp_kernel_prepared_batch_maintenance(prepared);
    CHECK(maintenance.bytes != NULL && maintenance.length != 0U);
    CHECK(lxp_programs_occupancy_receipt_decode(maintenance.bytes,
                                                maintenance.length, &sweep) == LXP_OK);
    decoded = lxp_kernel_prepared_batch_receipts(prepared);
    CHECK(decoded != NULL);
    CHECK(decoded[0].result_code == LXP_ERR_SEQUENCE_MISMATCH);
    CHECK(decoded[0].global_sequence == first_sequence);
    CHECK(sweep.batch_number == execution.batch_number);
    CHECK(sweep.global_sequence == decoded[0].global_sequence + 1U);
    CHECK(memcmp(sweep.previous_state_root, decoded[0].resulting_state_root, 32U) == 0);
    memcpy(settled_root, lxp_kernel_prepared_batch_final_root(prepared), 32U);
    CHECK(memcmp(settled_root, sweep.resulting_state_root, 32U) == 0);
    memcpy(digest, lxp_kernel_prepared_batch_publication_digest(prepared), 32U);
    CHECK(lxp_kernel_commit_prepared_batch(&f->kernel, &f->identities, prepared,
                                           digest) == LXP_OK);
    /* The refusal consumes the offered sequence and the sweep the next one. */
    CHECK(f->state.next_sequence == first_sequence + 2U);
    CHECK(memcmp(f->kernel.current_state_root, settled_root, 32U) == 0);
    CHECK(lxp_kernel_finalize_batch_publication_maintenance(&f->kernel, &activity,
        decoded, 1U, maintenance, digest) == LXP_OK);
    CHECK(f->feed.scanned_through_sequence == first_sequence + 1U);
    CHECK(memcmp(f->feed.head_state_root, settled_root, 32U) == 0);
    lxp_kernel_prepared_batch_destroy(prepared);
    free(f->storage);
    free(f);
    return 0;
}

int main(void)
{
    if (classification_case() != 0) return 1;
    if (terminal_rejection_case() != 0) return 1;
    if (terminal_maintenance_case() != 0) return 1;
    printf("terminal rejection tests passed\n");
    return 0;
}
