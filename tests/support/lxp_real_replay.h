#ifndef LXP_REAL_REPLAY_H
#define LXP_REAL_REPLAY_H

#include "layerx/lxp_kernel.h"
#include "layerx/lxp_da.h"
#include "layerx/programs.h"
#include "layerx/lxp_genesis.h"
#include "layerx/lx_asset.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <openssl/evp.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#define REAL_REQUIRE(condition) do { if (!(condition)) { \
    (void)fprintf(stderr, "state commitment check failed at line %d\n", __LINE__); \
    return 1; } } while (0)

typedef struct lxp_real_replay_fixture {
    lxp_kernel kernel;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lx_asset_runtime runtime;
    lx_programs_transfer_runtime programs_runtime;
    lx_asset_record asset;
    lxp_transfer_asset_state transfer_asset;
    lxp_arena arena;
    uint8_t arena_bytes[4U * 1024U * 1024U];
    uint64_t parameters;
    uint8_t public_key[32];
    uint8_t signature[64];
    uint8_t payload[512];
    lxp_activity activity;
    lxp_kernel_execution execution;
    lxp_authority_resolved authority;
    lxp_fee_params fees;
    lxp_receipt receipt;
    lxp_replay_engine engine;
} lxp_real_replay_fixture;

static const uint8_t lxp_real_replay_seed[32] = {1U};
static const uint8_t lxp_real_replay_did[] = "did:key:alice";

static int lxp_real_replay_sign(const uint8_t digest[32], uint8_t signature[64],
                        uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL, lxp_real_replay_seed, 32U);
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    size_t key_length = 32U;
    size_t signature_length = 64U;
    int ok = key != NULL && ctx != NULL &&
        EVP_PKEY_get_raw_public_key(key, public_key, &key_length) == 1 &&
        key_length == 32U && EVP_DigestSignInit(ctx, NULL, NULL, NULL, key) == 1 &&
        EVP_DigestSign(ctx, signature, &signature_length, digest, 32U) == 1 &&
        signature_length == 64U;
    EVP_MD_CTX_free(ctx);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static inline int lxp_real_replay_prepare(lxp_real_replay_fixture *f, uint16_t version, bool success)
{
    static const uint8_t from_name[] = "agent:did:key:alice:main";
    static const uint8_t to_name[] = "agent:did:key:bob:main";
    uint8_t digest[32] = {0};
    uint8_t material[144];
    uint8_t message[512];
    size_t message_length;
    size_t payload_length;
    lx_account *from;
    lx_account *to;
    lxp_identity *identity;
    lxp_send send;
    (void)memset(&send, 0, sizeof(send));
    REAL_REQUIRE(lxp_real_replay_sign(digest, f->signature, f->public_key) == 0);
    REAL_REQUIRE(lxp_arena_init(&f->arena, f->arena_bytes, sizeof(f->arena_bytes)) == LXP_OK);
    REAL_REQUIRE(lxp_state_store_init(&f->state, 1U) == LXP_OK);
    f->parameters = 1U;
    REAL_REQUIRE(lxp_kernel_create(&f->kernel, &f->state, &f->journal,
                              &f->parameters, 1U) == LXP_OK);
    REAL_REQUIRE(lxp_kernel_set_capabilities(&f->kernel, NULL,
                                        lxp_kernel_canonical_ledger_apply) == LXP_OK);
    REAL_REQUIRE(lxp_kernel_register_module(&f->kernel, lx_asset_module_iface()) == LXP_OK);
    REAL_REQUIRE(lxp_identity_register(&f->identities, lxp_real_replay_did, sizeof(lxp_real_replay_did) - 1U,
                                  f->public_key, &identity) == LXP_OK);
    REAL_REQUIRE(lx_account_id_from_string(from_name, sizeof(from_name) - 1U, send.from) == LXP_OK);
    REAL_REQUIRE(lx_account_id_from_string(to_name, sizeof(to_name) - 1U, send.to) == LXP_OK);
    send.asset[0] = 3U;
    send.amount.lo = 1U;
    send.expires_at = 100U;
    send.idempotency_key[0] = 7U;
    send.authorization.kind = LXP_AUTH_OWNER;
    send.authorization.network_id = 7U;
    send.authorization.protocol_version = version;
    (void)memcpy(send.authorization.controller, send.from, 32U);
    (void)memcpy(material, send.from, 32U);
    (void)memcpy(material + 32U, send.to, 32U);
    (void)memcpy(material + 64U, send.asset, 32U);
    REAL_REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    (void)memcpy(material + 112U, send.idempotency_key, 32U);
    REAL_REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    (void)memcpy(send.authorization.public_key, f->public_key, 32U);
    REAL_REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
    REAL_REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
    REAL_REQUIRE(lxp_real_replay_sign(digest, send.authorization.signature, f->public_key) == 0);
    REAL_REQUIRE(lxp_send_encode(&send, f->payload, sizeof(f->payload), &payload_length) == LXP_OK);
    if (success) {
        REAL_REQUIRE(lx_account_registry_init(&f->accounts) == LXP_OK);
        REAL_REQUIRE(lx_account_open(&f->accounts, from_name, sizeof(from_name) - 1U,
                                send.from, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &from) == LXP_OK);
        REAL_REQUIRE(lx_account_open(&f->accounts, to_name, sizeof(to_name) - 1U,
                                send.to, 1U, LX_ACCOUNT_OPEN_CREDIT, NULL, &to) == LXP_OK);
        REAL_REQUIRE(lxp_ledger_bootstrap_balance(from, send.asset, (lxp_u128){0U, 10U}, 0U) == LXP_OK);
        REAL_REQUIRE(lxp_ledger_bootstrap_balance(to, send.asset, (lxp_u128){0U, 0U}, 0U) == LXP_OK);
        from->has_authority_key = true;
        (void)memcpy(from->authority_key, f->public_key, 32U);
        (void)memcpy(f->asset.asset_id, send.asset, 32U);
        (void)memcpy(f->transfer_asset.asset_id, send.asset, 32U);
        f->transfer_asset.registered = true;
        f->runtime = (lx_asset_runtime){&f->accounts, &f->asset, 1U,
            &f->transfer_asset, 1U, 7U, version};
        REAL_REQUIRE(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_ASSET, &f->runtime) == LXP_OK);
    }
    f->activity.protocol_version = version;
    f->activity.network_id = 7U;
    f->activity.activity_type = LX_ASSET_SEND;
    f->activity.actor_did = (lxp_byte_span){lxp_real_replay_did, sizeof(lxp_real_replay_did) - 1U};
    f->activity.authority = (lxp_byte_span){f->public_key, 32U};
    f->activity.signature = (lxp_byte_span){f->signature, 64U};
    f->activity.timestamp_bound = (lxp_timestamp_bound){1U, 100U};
    f->activity.payload = (lxp_byte_span){f->payload, payload_length};
    (void)memcpy(f->activity.idempotency_key, send.idempotency_key, 32U);
    REAL_REQUIRE(lxp_hash_payload(f->payload, payload_length, f->activity.payload_hash) == LXP_OK);
    REAL_REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    REAL_REQUIRE(lxp_real_replay_sign(digest, f->signature, f->public_key) == 0);
    REAL_REQUIRE(lxp_activity_verify_signature(&f->activity) == LXP_OK);
    f->authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(f->authority.principal, send.from, 32U);
    (void)memcpy(f->authority.verified_key, f->public_key, 32U);
    (void)memcpy(f->authority.actor, identity->did_id, 32U);
    f->fees.version = 1U;
    f->fees.base_fee.lo = success ? 0U : 1U;
    f->fees.multiplier_basis_points = 10000U;
    f->execution.network_id = 7U;
    f->execution.epoch = f->kernel.epoch;
    f->execution.batch_number = 1U;
    f->execution.batch_timestamp_ms = 10U;
    f->execution.maximum_timestamp_window = 100U;
    f->execution.global_sequence = 1U;
    f->execution.recorded_module_version = 1U;
    f->execution.parameter_version = 1U;
    f->execution.signature_valid = true;
    f->execution.identities = &f->identities;
    f->execution.authority = &f->authority;
    f->execution.fee_parameters = &f->fees;
    f->execution.gas_limit = 10000U;
    f->execution.arena = &f->arena;
    f->execution.sequencer_private_key = lxp_real_replay_seed;
    f->execution.batch_id[0] = 5U;
    REAL_REQUIRE(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    return 0;
}

static inline lxp_result lxp_real_replay_parameters(void *context, uint64_t epoch,
                                                     uint32_t *version)
{
    const lxp_real_replay_fixture *f = context;
    if (f == NULL || epoch != f->kernel.epoch || version == NULL)
        return LXP_ERR_CONTEXT_MISMATCH;
    *version = (uint32_t)f->parameters;
    return LXP_OK;
}

static inline lxp_result lxp_real_replay_transition(
    void *context, uint16_t version, uint32_t parameters, uint64_t timestamp,
    uint64_t sequence, lxp_byte_span bytes, const uint8_t previous[32],
    lxp_arena *arena, lxp_replay_activity_output *output)
{
    lxp_real_replay_fixture *f = context;
    lxp_activity activity;
    lxp_result status;
    if (f == NULL || output == NULL ||
        lxp_ct_memcmp(previous, f->kernel.current_state_root, 32U) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    status = lxp_activity_decode(bytes.bytes, bytes.length, &activity);
    if (status == LXP_OK && (activity.protocol_version != version ||
        parameters != f->parameters)) status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status != LXP_OK) return status;
    f->execution.batch_timestamp_ms = timestamp;
    f->execution.global_sequence = sequence;
    f->execution.arena = arena;
    status = lxp_kernel_execute_activity(&f->kernel, &activity,
                                         &f->execution, &f->receipt);
    if (status != LXP_OK) { fprintf(stderr, "replay kernel activity %u sequence %llu result %d\n", activity.activity_type, (unsigned long long)sequence, status); return status; }
    (void)memset(output, 0, sizeof(*output));
    output->result_code = f->receipt.result_code;
    output->fee_charged = f->receipt.fee_charged;
    (void)memcpy(output->resulting_state_root, f->receipt.resulting_state_root, 32U);
    status = lxp_receipt_encode(&f->receipt, true, arena, &output->canonical_receipt);
    if (status == LXP_OK)
        status = lxp_programs_project_receipt_events(&f->receipt, arena,
                                                     &output->canonical_events);
    if (status != LXP_OK) fprintf(stderr, "replay projection activity %u result %d\n", activity.activity_type, status);
    return status;
}

static const uint8_t fee_active_key[] = "progfee/active/v1";
static const uint8_t fee_history_prefix[] = "progfee/history/v1/";
static lxp_result lxp_real_seed_fee_governance(
    lxp_kernel *kernel, const lx_programs_transfer_runtime *runtime)
{
    lxp_genesis_manifest manifest;
    lx_programs_fee_genesis_parameters parameters;
    size_t index;
    lxp_result status;
    if (kernel == NULL || runtime == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(&manifest, 0, sizeof(manifest));
    manifest.signer_public_key[0] = 1U;
    (void)memset(&parameters, 0, sizeof(parameters));
    parameters.schedule = runtime->fee_schedule;
    (void)memcpy(parameters.occupancy_asset_id,
                 runtime->occupancy_asset_id, 32U);
    parameters.target_occupancy_byte_batches = 3U;
    parameters.response_denominator = 1U;
    parameters.maximum_change_numerator = 1U;
    parameters.maximum_change_denominator = 1U;
    parameters.minimum_fee_units_per_occupancy_byte_batch = 1U;
    parameters.maximum_fee_units_per_occupancy_byte_batch = 10U;
    status = lxp_programs_fee_genesis_append(&manifest, &parameters);
    if (status != LXP_OK) return status;
    if (kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV -
                                      manifest.module_value_count)
        return LXP_ERR_LENGTH_LIMIT;
    for (index = 0U; index < manifest.module_value_count; ++index) {
        const lxp_genesis_module_value *value = &manifest.module_values[index];
        lxp_module_kv_entry *entry =
            &kernel->module_kv[kernel->module_kv_count++];
        size_t key_length;
        if (memcmp(value->key, fee_active_key,
                   sizeof(fee_active_key) - 1U) == 0)
            key_length = sizeof(fee_active_key) - 1U;
        else if (memcmp(value->key, fee_history_prefix,
                        sizeof(fee_history_prefix) - 1U) == 0)
            key_length = sizeof(fee_history_prefix) - 1U + 4U;
        else
            return LXP_FATAL_INVARIANT;
        (void)memset(entry, 0, sizeof(*entry));
        entry->module_id = value->module_id;
        entry->key_length = (uint16_t)key_length;
        entry->value_length = (uint32_t)value->value_length;
        (void)memcpy(entry->key, value->key, key_length);
        (void)memcpy(entry->value, value->value, value->value_length);
    }
    return LXP_OK;
}

static inline int lxp_real_replay_init(lxp_real_replay_fixture *f)
{
    REAL_REQUIRE(lxp_real_replay_prepare(f, LXP_PROTOCOL_VERSION_STATE_COMMITMENT, true) == 0);
    f->programs_runtime.accounts = &f->accounts;
    f->programs_runtime.assets = &f->transfer_asset;
    f->programs_runtime.asset_count = 1U;
    f->programs_runtime.fee_schedule = (lx_programs_fee_schedule){1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U};
    (void)memcpy(f->programs_runtime.occupancy_asset_id, f->asset.asset_id, 32U);
    f->programs_runtime.resolve_occupancy_parameters = lxp_programs_fee_governance_resolve_runtime;
    f->programs_runtime.occupancy_parameter_context = &f->kernel;
    REAL_REQUIRE(lxp_kernel_register_module(&f->kernel, programs_module_registration()) == LXP_OK);
    REAL_REQUIRE(lxp_kernel_bind_module_runtime(&f->kernel, LXP_MODULE_PROGRAMS, &f->programs_runtime) == LXP_OK);
    REAL_REQUIRE(lxp_real_seed_fee_governance(&f->kernel, &f->programs_runtime) == LXP_OK);
    REAL_REQUIRE(lxp_state_root(&f->kernel, f->kernel.current_state_root) == LXP_OK);
    REAL_REQUIRE(lxp_replay_engine_init(&f->engine, lxp_real_replay_parameters, f) == LXP_OK);
    REAL_REQUIRE(lxp_programs_replay_engine_bind(&f->engine, &f->kernel) == LXP_OK);
    REAL_REQUIRE(lxp_replay_engine_register(&f->engine, LXP_PROTOCOL_VERSION_STATE_COMMITMENT,
                                             lxp_real_replay_transition) == LXP_OK);
    return 0;
}

static inline int lxp_real_replay_activity(lxp_real_replay_fixture *f,
                                           uint64_t account_sequence,
                                           lxp_arena *arena, lxp_byte_span *bytes)
{
    lxp_send send;
    uint8_t material[144], message[512], digest[32];
    size_t message_length, payload_length;
    REAL_REQUIRE(lxp_send_decode(f->payload, f->activity.payload.length, &send) == LXP_OK);
    send.idempotency_key[0] = (uint8_t)(account_sequence + 7U);
    send.sequence = account_sequence;
    (void)memcpy(material, send.from, 32U);
    (void)memcpy(material + 32U, send.to, 32U);
    (void)memcpy(material + 64U, send.asset, 32U);
    REAL_REQUIRE(lxp_u128_to_be(send.amount, material + 96U) == LXP_OK);
    (void)memcpy(material + 112U, send.idempotency_key, 32U);
    REAL_REQUIRE(lxp_hash_context_value(material, sizeof(material), send.context_hash) == LXP_OK);
    (void)memcpy(send.authorization.signed_context_hash, send.context_hash, 32U);
    REAL_REQUIRE(lxp_send_authorization_message(&send, message, sizeof(message), &message_length) == LXP_OK);
    REAL_REQUIRE(lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE, message, message_length, digest) == LXP_OK);
    REAL_REQUIRE(lxp_real_replay_sign(digest, send.authorization.signature, f->public_key) == 0);
    REAL_REQUIRE(lxp_send_encode(&send, f->payload, sizeof(f->payload), &payload_length) == LXP_OK);
    f->activity.payload.length = payload_length;
    f->activity.account_sequence = account_sequence;
    (void)memcpy(f->activity.idempotency_key, send.idempotency_key, 32U);
    REAL_REQUIRE(lxp_hash_payload(f->payload, payload_length, f->activity.payload_hash) == LXP_OK);
    REAL_REQUIRE(lxp_activity_signing_preimage(&f->activity, digest) == LXP_OK);
    REAL_REQUIRE(lxp_real_replay_sign(digest, f->signature, f->public_key) == 0);
    REAL_REQUIRE(lxp_activity_encode(&f->activity, arena, bytes) == LXP_OK);
    return 0;
}

static inline int lxp_real_replay_build(lxp_real_replay_fixture *f,
    uint64_t batch, const lxp_byte_span *activities, size_t count,
    const lxp_byte_span *oracles, size_t oracle_count,
    lxp_arena *arena, lxp_batch_body *body)
{
    lxp_kernel *before = malloc(sizeof(*before));
    lxp_state_store *state = malloc(sizeof(*state));
    lx_account_registry *accounts = malloc(sizeof(*accounts));
    lxp_replay_activity_output outputs[8];
    lxp_byte_span receipts[9], events[8];
    lxp_replay_activity_output maintenance;
    lxp_batch_roots roots;
    lxp_batch_header header = {0};
    size_t i;
    REAL_REQUIRE(before != NULL && state != NULL && accounts != NULL && count <= 8U && count != 0U);
    *before = f->kernel;
    *state = f->state;
    *accounts = f->accounts;
    state->accounts = accounts;
    before->state = state;
    header.protocol_version = LXP_PROTOCOL_VERSION_STATE_COMMITMENT;
    header.network_id = 7U;
    header.epoch = f->kernel.epoch;
    header.batch_number = batch;
    header.first_sequence = f->state.next_sequence;
    header.last_sequence = header.first_sequence + count;
    header.timestamp_ms = 10U;
    f->execution.batch_number = batch;
    (void)memcpy(header.previous_state_root, f->kernel.current_state_root, 32U);
    for (i = 0U; i < count; ++i) {
        REAL_REQUIRE(lxp_real_replay_transition(f, header.protocol_version,
            (uint32_t)f->parameters, header.timestamp_ms, header.first_sequence + i,
            activities[i], f->kernel.current_state_root, arena, &outputs[i]) == LXP_OK);
        REAL_REQUIRE(outputs[i].result_code == LXP_OK);
        receipts[i] = outputs[i].canonical_receipt;
        events[i] = outputs[i].canonical_events;
    }
    REAL_REQUIRE(lxp_programs_replay_finalize(&f->kernel, &header, (uint32_t)f->parameters,
        header.last_sequence, f->kernel.current_state_root, arena, &maintenance) == LXP_OK);
    receipts[count] = maintenance.canonical_receipt;
    (void)memcpy(header.resulting_state_root, f->kernel.current_state_root, 32U);
    REAL_REQUIRE(lxp_batch_roots_compute(&(lxp_batch_root_inputs){activities, count,
        receipts, count + 1U, events, count, oracles, oracle_count, NULL, 0U}, arena, &roots) == LXP_OK);
    (void)memcpy(header.activity_merkle_root, roots.activity_merkle_root, 32U);
    (void)memcpy(header.receipt_merkle_root, roots.receipt_merkle_root, 32U);
    (void)memcpy(header.event_merkle_root, roots.event_merkle_root, 32U);
    (void)memcpy(header.oracle_root, roots.oracle_root, 32U);
    REAL_REQUIRE(lxp_da_body_from_kernels(&header, before, &f->kernel,
        activities, count, receipts, count + 1U, events, count, oracles, oracle_count,
        arena, body) == LXP_OK);
    free(accounts);
    free(state);
    free(before);
    return 0;
}

#undef REAL_REQUIRE
#endif
