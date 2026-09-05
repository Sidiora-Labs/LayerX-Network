#define _POSIX_C_SOURCE 200809L

#define main layerx_programs_call_fixture_main
#include "../../../tests/programs/test_call_activity.c"
#undef main

#include "layerx/lxp_snapshot.h"
#include "../../../cmd/layerxd/lxp_daemon_batch_wal.h"

#include <openssl/evp.h>
#include <time.h>
#include <unistd.h>

enum { DIFFERENTIAL_BATCH_SIZE = 32, DIFFERENTIAL_WORKERS = 8 };
static lxp_u128 observed_call_fee;
static lxp_u128 observed_call_limit;

typedef enum differential_workload {
    DIFFERENTIAL_LOW_CONFLICT,
    DIFFERENTIAL_ALL_CONFLICTING,
    DIFFERENTIAL_PLANNING_REFUSAL
} differential_workload;

typedef struct differential_fixture {
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lx_programs_transfer_runtime runtime;
    lxp_transfer_asset_state fee_asset_state;
    lxp_fee_params fees;
    lxp_authority_resolved authorities[DIFFERENTIAL_BATCH_SIZE];
    lxp_authority_scope scopes[DIFFERENTIAL_BATCH_SIZE];
    lxp_u128 actor_initial_balances[DIFFERENTIAL_BATCH_SIZE];
    uint8_t dids[DIFFERENTIAL_BATCH_SIZE][32];
    size_t did_lengths[DIFFERENTIAL_BATCH_SIZE];
    uint8_t account_names[DIFFERENTIAL_BATCH_SIZE][32];
    size_t account_name_lengths[DIFFERENTIAL_BATCH_SIZE];
    uint8_t private_keys[DIFFERENTIAL_BATCH_SIZE][32];
    uint8_t keys[DIFFERENTIAL_BATCH_SIZE][32];
    uint8_t program_id[32];
    uint8_t fee_asset[32];
    uint8_t sequencer_private_key[32];
    lxp_sequencer_authorization sequencer_authorization;
    uint64_t parameters;
} differential_fixture;

typedef struct differential_run {
    lxp_receipt receipts[DIFFERENTIAL_BATCH_SIZE];
    lxp_byte_span events[DIFFERENTIAL_BATCH_SIZE];
    uint8_t receipt_storage[DIFFERENTIAL_BATCH_SIZE]
                           [LXP_MAX_ACTIVITY_BYTES];
    uint8_t event_storage[DIFFERENTIAL_BATCH_SIZE][8192];
    lxp_byte_span canonical_receipts[DIFFERENTIAL_BATCH_SIZE];
    uint8_t root[32];
    uint8_t canonical_root[32];
    uint8_t prepared_root[32];
    uint64_t elapsed_ns;
} differential_run;

typedef struct differential_checkpoint {
    const char *directory;
    lxp_kernel *kernel;
    uint8_t *storage;
    size_t storage_length;
    char path[128];
    bool stored;
} differential_checkpoint;

static lxp_result persist_differential_checkpoint(
    void *context, const lxp_kernel_batch_boundary *settled)
{
    differential_checkpoint *checkpoint =
        (differential_checkpoint *)context;
    lxp_snapshot_manifest_record manifest;
    lxp_byte_span snapshot;
    lxp_arena arena;
    uint64_t sequence;
    lxp_result status;
    if (checkpoint == NULL || settled == NULL ||
        checkpoint->directory == NULL || checkpoint->kernel == NULL ||
        checkpoint->storage == NULL || settled->next_sequence == 0U)
        return LXP_ERR_NON_CANONICAL;
    sequence = settled->next_sequence - 1U;
    status = lxp_arena_init(&arena, checkpoint->storage,
                            checkpoint->storage_length);
    if (status == LXP_OK)
        status = lxp_snapshot_write(checkpoint->kernel, sequence,
                                    &arena, &snapshot);
    if (status == LXP_OK)
        status = lxp_snapshot_manifest(
            snapshot.bytes, snapshot.length, sequence,
            settled->canonical_state_root, settled->receipt_state_root,
            &manifest);
    if (status == LXP_OK)
        status = lxp_snapshot_store_write(
            checkpoint->directory, &manifest,
            snapshot.bytes, snapshot.length);
    if (status == LXP_OK &&
        snprintf(checkpoint->path, sizeof(checkpoint->path),
                 "%s/%020llu.lxs", checkpoint->directory,
                 (unsigned long long)sequence) <= 0)
        status = LXP_ERR_LENGTH_LIMIT;
    if (status == LXP_OK) checkpoint->stored = true;
    return status;
}

static bool cleanup_differential_directory(
    const char *directory, differential_checkpoint *checkpoint,
    lxp_daemon_batch_wal_record *record,
    lxp_kernel_prepared_batch *prepared)
{
    char path[160];
    lxp_daemon_batch_wal_destroy(record);
    lxp_kernel_prepared_batch_destroy(prepared);
    if (checkpoint != NULL && checkpoint->stored)
        (void)unlink(checkpoint->path);
    if (directory == NULL) return true;
    if (snprintf(path, sizeof(path), "%s/prepared-batch.lxw", directory) > 0)
        (void)unlink(path);
    if (snprintf(path, sizeof(path), "%s/prepared-batch.lxw.tmp", directory) > 0)
        (void)unlink(path);
    return rmdir(directory) == 0;
}

static int public_key_for(const uint8_t private_key[32],
                          uint8_t public_key[32])
{
    EVP_PKEY *key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                  private_key, 32U);
    size_t length = 32U;
    int ok = key != NULL && EVP_PKEY_get_raw_public_key(
        key, public_key, &length) == 1 && length == 32U;
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static int sign_activity(lxp_activity *activity,
                         const uint8_t private_key[32],
                         uint8_t signature[64])
{
    uint8_t preimage[32];
    EVP_PKEY *key = NULL;
    EVP_MD_CTX *context = NULL;
    size_t length = 64U;
    int ok = lxp_activity_signing_preimage(activity, preimage) == LXP_OK;
    if (ok) key = EVP_PKEY_new_raw_private_key(EVP_PKEY_ED25519, NULL,
                                                private_key, 32U);
    if (key != NULL) context = EVP_MD_CTX_new();
    ok = ok && key != NULL && context != NULL &&
         EVP_DigestSignInit(context, NULL, NULL, NULL, key) == 1 &&
         EVP_DigestSign(context, signature, &length,
                        preimage, sizeof(preimage)) == 1 && length == 64U;
    EVP_MD_CTX_free(context);
    EVP_PKEY_free(key);
    return ok ? 0 : 1;
}

static size_t explicit_empty_call_payload(uint8_t *out,
                                          const uint8_t program_id[32])
{
    static const uint8_t declaration_domain[] =
        "LayerX/programs/access-declaration/v1\0";
    static const uint8_t access_set_domain[] =
        "LayerX/programs/access-set/v1\0";
    static const uint8_t capabilities[] = {0U, 1U, 3U};
    uint8_t declaration[(sizeof(declaration_domain) - 1U) + 1U + 4U +
                        (sizeof(access_set_domain) - 1U) + 2U + 4U];
    size_t offset = 0U;
    (void)memcpy(declaration + offset, declaration_domain,
                 sizeof(declaration_domain) - 1U);
    offset += sizeof(declaration_domain) - 1U;
    declaration[offset++] = 1U;
    write_u32(declaration + offset,
              (uint32_t)((sizeof(access_set_domain) - 1U) + 2U + 4U));
    offset += 4U;
    (void)memcpy(declaration + offset, access_set_domain,
                 sizeof(access_set_domain) - 1U);
    offset += sizeof(access_set_domain) - 1U;
    write_u16(declaration + offset, 0U);
    offset += 2U;
    write_u16(declaration + offset, 0U);
    offset += 2U;
    write_u16(declaration + offset, 0U);
    offset += 2U;
    offset = call_payload_with_access(out, program_id, capabilities,
                                      sizeof(capabilities), declaration,
                                      offset);
    write_u16(out + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    return offset;
}

static size_t call_payload_with_marker(uint8_t *out, size_t length,
                                       uint8_t marker)
{
    size_t calldata_offset = CALL_FIXED_BYTES + sizeof("layerx_call") - 1U;
    (void)memmove(out + calldata_offset + 1U,
                  out + calldata_offset, length - calldata_offset);
    out[calldata_offset] = marker;
    write_u32(out + 36U, 1U);
    return length + 1U;
}

static size_t event_emitting_module(uint8_t *out)
{
    static const uint8_t header[] =
        {0U, 0x61U, 0x73U, 0x6dU, 1U, 0U, 0U, 0U};
    uint8_t section[256];
    size_t cursor = 0U;
    size_t length = 0U;
    append_bytes(out, &cursor, header, sizeof(header));
    section[length++] = 3U;
    section[length++] = 0x60U; section[length++] = 4U;
    section[length++] = 0x7fU; section[length++] = 0x7fU;
    section[length++] = 0x7fU; section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 1U;
    section[length++] = 0x7fU; section[length++] = 1U;
    section[length++] = 0x7fU;
    section[length++] = 0x60U; section[length++] = 2U;
    section[length++] = 0x7fU; section[length++] = 0x7fU;
    section[length++] = 1U; section[length++] = 0x7fU;
    append_section(out, &cursor, 1U, section, length);
    length = 0U;
    section[length++] = 1U;
    append_name(section, &length, "layerx_v1");
    append_name(section, &length, "event_emit");
    section[length++] = 0U; section[length++] = 0U;
    append_section(out, &cursor, 2U, section, length);
    { static const uint8_t functions[] = {2U, 1U, 2U};
      append_section(out, &cursor, 3U, functions, sizeof(functions)); }
    { static const uint8_t memory[] = {1U, 1U, 1U, 1U};
      append_section(out, &cursor, 5U, memory, sizeof(memory)); }
    length = 0U; section[length++] = 3U;
    append_name(section, &length, "layerx_reserve");
    section[length++] = 0U; section[length++] = 1U;
    append_name(section, &length, "layerx_call");
    section[length++] = 0U; section[length++] = 2U;
    append_name(section, &length, "memory");
    section[length++] = 2U; section[length++] = 0U;
    append_section(out, &cursor, 7U, section, length);
    length = 0U; section[length++] = 2U;
    { static const uint8_t reserve[] = {4U, 0U, 0x41U, 0U, 0x0bU};
      append_bytes(section, &length, reserve, sizeof(reserve)); }
    { static const uint8_t call[] = {
        15U, 0U, 0x41U, 0U, 0x41U, 1U, 0x20U, 0U, 0x20U, 1U,
        0x10U, 0U, 0x1aU, 0x41U, 0U, 0x0bU
      }; append_bytes(section, &length, call, sizeof(call)); }
    append_section(out, &cursor, 10U, section, length);
    length = 0U; section[length++] = 1U; section[length++] = 0U;
    section[length++] = 0x41U; section[length++] = 0U;
    section[length++] = 0x0bU; section[length++] = 1U;
    section[length++] = (uint8_t)'E';
    append_section(out, &cursor, 11U, section, length);
    return cursor;
}

static size_t disjoint_read_call_payload(
    uint8_t *out, const uint8_t program_id[32],
    const uint8_t principal[32], uint8_t key_tag)
{
    static const uint8_t declaration_domain[] =
        "LayerX/programs/access-declaration/v1\0";
    static const uint8_t access_set_domain[] =
        "LayerX/programs/access-set/v1\0";
    static const uint8_t capabilities[] = {0U, 1U, 3U};
    enum { ENTRY_BYTES = 32 + 1 + 32 + 1 + 1 + 2 + 32 };
    uint8_t declaration[(sizeof(declaration_domain) - 1U) + 1U + 4U +
                        (sizeof(access_set_domain) - 1U) + 2U + ENTRY_BYTES +
                        4U];
    uint8_t key[32] = {0U};
    size_t offset = 0U;
    key[31] = key_tag;
    (void)memcpy(declaration + offset, declaration_domain,
                 sizeof(declaration_domain) - 1U);
    offset += sizeof(declaration_domain) - 1U;
    declaration[offset++] = 1U;
    write_u32(declaration + offset,
              (uint32_t)((sizeof(access_set_domain) - 1U) + 2U +
                         ENTRY_BYTES + 4U));
    offset += 4U;
    (void)memcpy(declaration + offset, access_set_domain,
                 sizeof(access_set_domain) - 1U);
    offset += sizeof(access_set_domain) - 1U;
    write_u16(declaration + offset, 1U);
    offset += 2U;
    (void)memcpy(declaration + offset, program_id, 32U);
    offset += 32U;
    declaration[offset++] = 0U;
    (void)memcpy(declaration + offset, principal, 32U);
    offset += 32U;
    declaration[offset++] = 0U;
    declaration[offset++] = 0U;
    write_u16(declaration + offset, 32U);
    offset += 2U;
    (void)memcpy(declaration + offset, key, 32U);
    offset += 32U;
    write_u16(declaration + offset, 0U);
    offset += 2U;
    write_u16(declaration + offset, 0U);
    offset += 2U;
    offset = call_payload_with_access(out, program_id, capabilities,
                                      sizeof(capabilities), declaration,
                                      offset);
    write_u16(out + 32U, LX_PROGRAMS_ACCOUNT_ABI_VERSION);
    return offset;
}

static uint64_t elapsed_ns(const struct timespec *start,
                           const struct timespec *finish)
{
    uint64_t seconds = (uint64_t)(finish->tv_sec - start->tv_sec);
    uint64_t nanoseconds;
    if (finish->tv_nsec >= start->tv_nsec)
        nanoseconds = (uint64_t)(finish->tv_nsec - start->tv_nsec);
    else {
        --seconds;
        nanoseconds = UINT64_C(1000000000) +
            (uint64_t)finish->tv_nsec - (uint64_t)start->tv_nsec;
    }
    return seconds * UINT64_C(1000000000) + nanoseconds;
}

static int differential_fixture_init_balanced(
    differential_fixture *fixture, lxp_u128 actor_zero_balance,
    lxp_u128 treasury_balance)
{
    static const uint8_t treasury_name[] = "system:fees";
    uint8_t wasm[256];
    uint8_t payload[DEPLOY_FIXED_BYTES + INTERFACE_MAX_FIXTURE_BYTES +
                    sizeof(wasm)];
    uint8_t deployment_signature[64];
    uint8_t code_hash[32];
    uint8_t treasury_id[32];
    static uint8_t arena_bytes[LXP_MAX_ACTIVITY_BYTES + 4096U];
    lxp_arena arena;
    lxp_activity deployment;
    lxp_kernel_execution execution;
    lxp_receipt receipt;
    lx_account *treasury;
    size_t wasm_length;
    size_t payload_length;
    size_t index;
    (void)memset(fixture, 0, sizeof(*fixture));
    fixture->parameters = 1U;
    fixture->fee_asset[0] = 9U;
    fixture->sequencer_private_key[0] = 0x71U;
    if (public_key_for(fixture->sequencer_private_key,
                       fixture->sequencer_authorization.public_key) != 0)
        return 1;
    (void)memcpy(fixture->sequencer_authorization.sequencer_id,
                 fixture->sequencer_authorization.public_key, 32U);
    fixture->sequencer_authorization.first_batch_number = 1U;
    fixture->sequencer_authorization.last_batch_number = 100U;
    fixture->sequencer_authorization.authorized = 1U;
    (void)memset(fixture->program_id, 0x31, 32U);
    if (lx_account_registry_init(&fixture->accounts) != LXP_OK ||
        lxp_state_store_init(&fixture->state, 1U) != LXP_OK ||
        lxp_state_store_bind_accounts(&fixture->state, &fixture->accounts) !=
            LXP_OK ||
        lxp_kernel_create(&fixture->kernel, &fixture->state,
                          &fixture->journal, &fixture->parameters, 1U) != LXP_OK ||
        install_metering_v1(&fixture->kernel) != LXP_OK ||
        lxp_kernel_register_module(&fixture->kernel,
                                   programs_module_registration_v2()) != LXP_OK)
        return 1;
    for (index = 0U; index < DIFFERENTIAL_BATCH_SIZE; ++index) {
        lx_account *actor;
        lxp_identity *identity;
        uint8_t actor_id[32];
        int length = snprintf((char *)fixture->dids[index],
                              sizeof(fixture->dids[index]),
                              "did:lxp:parallel-%02zu", index);
        if (length <= 0 || (size_t)length >= sizeof(fixture->dids[index]))
            return 1;
        fixture->did_lengths[index] = (size_t)length;
        length = snprintf((char *)fixture->account_names[index],
                          sizeof(fixture->account_names[index]),
                          "agent:parallel-%02zu:main", index);
        if (length <= 0 ||
            (size_t)length >= sizeof(fixture->account_names[index]))
            return 1;
        fixture->account_name_lengths[index] = (size_t)length;
        fixture->private_keys[index][0] = (uint8_t)(index + 1U);
        if (public_key_for(fixture->private_keys[index],
                           fixture->keys[index]) != 0)
            return 1;
        if (lxp_identity_register(&fixture->identities,
                                  fixture->dids[index],
                                  fixture->did_lengths[index],
                                  fixture->keys[index], &identity) != LXP_OK ||
            identity == NULL ||
            lx_account_id_from_string(fixture->account_names[index],
                                      fixture->account_name_lengths[index],
                                      actor_id) != LXP_OK ||
            lx_account_open(&fixture->accounts,
                            fixture->account_names[index],
                            fixture->account_name_lengths[index], actor_id,
                            index + 1U, LX_ACCOUNT_OPEN_GENESIS, NULL,
                            &actor) != LXP_OK ||
            lxp_ledger_bootstrap_balance(actor, fixture->fee_asset,
                                         index == 0U ? actor_zero_balance :
                                             (lxp_u128){0U, UINT64_MAX},
                                         index + 1U) != LXP_OK)
            return 1;
        fixture->actor_initial_balances[index] =
            index == 0U ? actor_zero_balance :
                (lxp_u128){0U, UINT64_MAX};
        fixture->scopes[index].module_mask =
            UINT64_C(1) << LXP_MODULE_PROGRAMS;
        fixture->scopes[index].activity_ordinal_min = 1U;
        fixture->scopes[index].activity_ordinal_max = 9U;
        fixture->scopes[index].maximum_per_activity =
            (lxp_u128){UINT64_MAX, UINT64_MAX};
        fixture->scopes[index].maximum_total =
            (lxp_u128){UINT64_MAX, UINT64_MAX};
        fixture->scopes[index].maximum_per_period =
            (lxp_u128){UINT64_MAX, UINT64_MAX};
        (void)memcpy(fixture->authorities[index].actor,
                     identity->did_id, 32U);
        (void)memcpy(fixture->authorities[index].principal, actor_id, 32U);
        fixture->authorities[index].kind = LXP_AUTHORITY_OWNER;
        (void)memcpy(fixture->authorities[index].verified_key,
                     fixture->keys[index], 32U);
        fixture->authorities[index].scope = &fixture->scopes[index];
        if (lxp_authority_hash(
                fixture->authorities[index].kind,
                (const uint8_t[32]){0U},
                fixture->authorities[index].verified_key,
                fixture->authorities[index].authority_hash) != LXP_OK)
            return 1;
    }
    if (lx_account_id_from_string(treasury_name, sizeof(treasury_name) - 1U,
                                  treasury_id) != LXP_OK ||
        lx_account_open(&fixture->accounts, treasury_name,
                        sizeof(treasury_name) - 1U, treasury_id,
                        DIFFERENTIAL_BATCH_SIZE + 1U,
                        LX_ACCOUNT_OPEN_GENESIS, NULL, &treasury) != LXP_OK ||
        lxp_ledger_bootstrap_balance(treasury, fixture->fee_asset,
                                     treasury_balance, 0U) != LXP_OK)
        return 1;
    fixture->fee_asset_state.registered = true;
    (void)memcpy(fixture->fee_asset_state.asset_id, fixture->fee_asset, 32U);
    fixture->runtime.accounts = &fixture->accounts;
    fixture->runtime.assets = &fixture->fee_asset_state;
    fixture->runtime.asset_count = 1U;
    fixture->runtime.fee_schedule =
        (lx_programs_fee_schedule){1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U};
    fixture->runtime.resolve_metering_schedule =
        lxp_programs_metering_resolve_runtime;
    fixture->runtime.metering_schedule_context = &fixture->kernel;
    (void)memcpy(fixture->runtime.occupancy_asset_id,
                 fixture->fee_asset, 32U);
    fixture->runtime.resolve_occupancy_parameters = occupancy_parameters;
    fixture->runtime.occupancy_parameter_context = &fixture->runtime;
    if (lxp_kernel_bind_module_runtime(&fixture->kernel, LXP_MODULE_PROGRAMS,
                                       &fixture->runtime) != LXP_OK ||
        lxp_programs_bind_fee_transaction(&fixture->kernel) != LXP_OK)
        return 1;
    fixture->fees.version = 1U;
    fixture->fees.multiplier_basis_points = 10000U;
    wasm_length = event_emitting_module(wasm);
    payload_length = deploy_payload(
        payload, fixture->program_id, fixture->authorities[0].principal,
        wasm, wasm_length, code_hash, LX_PROGRAMS_ACCOUNT_ABI_VERSION,
        INTERFACE_CAPABILITIES_EMIT_EVENT);
    fill_activity(&deployment, LX_PROGRAMS_DEPLOY, payload, payload_length,
                  fixture->dids[0], fixture->did_lengths[0], fixture->keys[0]);
    if (sign_activity(&deployment, fixture->private_keys[0],
                      deployment_signature) != 0)
        return 1;
    deployment.signature =
        (lxp_byte_span){deployment_signature, sizeof(deployment_signature)};
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK)
        return 1;
    (void)memset(&execution, 0, sizeof(execution));
    execution.network_id = 7U;
    execution.batch_number = 1U;
    execution.batch_timestamp_ms = 10U;
    execution.maximum_timestamp_window = 100U;
    execution.global_sequence = 1U;
    execution.epoch = 1U;
    execution.recorded_module_version = LX_PROGRAMS_ACCOUNT_ABI_VERSION;
    execution.parameter_version = 1U;
    execution.signature_valid = true;
    execution.identities = &fixture->identities;
    execution.authority = &fixture->authorities[0];
    execution.fee_parameters = &fixture->fees;
    execution.gas_limit = 1000000U;
    execution.arena = &arena;
    return lxp_kernel_execute_activity(&fixture->kernel, &deployment,
                                       &execution, &receipt) == LXP_OK &&
           receipt.result_code == LXP_OK ? 0 : 1;
}

static int differential_fixture_init(differential_fixture *fixture)
{
    return differential_fixture_init_balanced(
        fixture, (lxp_u128){0U, UINT64_MAX}, (lxp_u128){0U, 0U});
}

static int execute_workload(differential_fixture *fixture,
                            differential_workload workload,
                            size_t activity_count, uint32_t workers,
                            lxp_u128 fee_limit,
                            bool expect_retry, bool commit_retry_prefix,
                            lxp_result *retry_status,
                            size_t *retry_prefix, differential_run *run)
{
    static uint8_t payloads[DIFFERENTIAL_BATCH_SIZE]
                           [CALL_FIXED_BYTES + 256U];
    static uint8_t arena_storage[DIFFERENTIAL_BATCH_SIZE]
                                [2U * LXP_MAX_ACTIVITY_BYTES + 4096U];
    static lxp_arena arenas[DIFFERENTIAL_BATCH_SIZE];
    static lxp_activity activities[DIFFERENTIAL_BATCH_SIZE];
    static uint8_t signatures[DIFFERENTIAL_BATCH_SIZE][64];
    static lxp_kernel_execution executions[DIFFERENTIAL_BATCH_SIZE];
    static uint8_t batch_arena_storage[
        (2U * DIFFERENTIAL_BATCH_SIZE + 4U) * LXP_MAX_ACTIVITY_BYTES];
    static uint8_t snapshot_storage[16U * 1024U * 1024U];
    lxp_byte_span canonical_activities[DIFFERENTIAL_BATCH_SIZE];
    lxp_byte_span canonical_receipts[DIFFERENTIAL_BATCH_SIZE];
    lxp_merkle_proof proofs[DIFFERENTIAL_BATCH_SIZE];
    uint8_t receipt_hashes[DIFFERENTIAL_BATCH_SIZE][32];
    lxp_kernel_prepared_batch *prepared = NULL;
    lxp_daemon_batch_wal_record *wal_record = NULL;
    const lxp_receipt *prepared_receipts;
    const lxp_byte_span *prepared_events;
    const lxp_kernel_batch_boundary *base;
    const lxp_kernel_batch_boundary *settled;
    lxp_daemon_batch_wal_input wal_input;
    lxp_batch_roots roots;
    lxp_batch_roots scheduling_roots;
    lxp_batch_header header;
    lxp_byte_span canonical_header;
    lxp_arena batch_arena;
    differential_checkpoint checkpoint;
    uint8_t header_signature[64];
    uint8_t proof_root[32];
    uint8_t batch_id[32];
    uint8_t offered_activity_root[32];
    uint8_t offered_batch_id[32];
    char directory[] = "/tmp/lxp-program-parallel-XXXXXX";
    struct timespec start;
    struct timespec finish;
    size_t index;
    size_t retry_prefix_count = 0U;
    lxp_result status;
    if (activity_count == 0U || activity_count > DIFFERENTIAL_BATCH_SIZE)
        return 1;
    (void)memset(run, 0, sizeof(*run));
    (void)memset(&checkpoint, 0, sizeof(checkpoint));
    for (index = 0U; index < activity_count; ++index) {
        size_t actor = workload == DIFFERENTIAL_ALL_CONFLICTING ? 0U : index;
        size_t payload_length = workload == DIFFERENTIAL_LOW_CONFLICT ?
            disjoint_read_call_payload(
                payloads[index], fixture->program_id,
                fixture->authorities[actor].principal,
                (uint8_t)(index + 1U)) :
            explicit_empty_call_payload(payloads[index], fixture->program_id);
        payload_length = call_payload_with_marker(
            payloads[index], payload_length, (uint8_t)(index + 1U));
        fill_activity(&activities[index], LX_PROGRAMS_CALL, payloads[index],
                      payload_length, fixture->dids[actor],
                      fixture->did_lengths[actor], fixture->keys[actor]);
        activities[index].account_sequence =
            workload == DIFFERENTIAL_ALL_CONFLICTING ? index + 1U :
            (actor == 0U ? 1U : 0U);
        activities[index].idempotency_key[30] = (uint8_t)(index >> 8U);
        activities[index].idempotency_key[31] = (uint8_t)(index + 2U);
        activities[index].fee_limit = fee_limit;
        if (sign_activity(&activities[index], fixture->private_keys[actor],
                          signatures[index]) != 0)
            return 1;
        activities[index].signature =
            (lxp_byte_span){signatures[index], sizeof(signatures[index])};
        if (lxp_arena_init(&arenas[index], arena_storage[index],
                           sizeof(arena_storage[index])) != LXP_OK)
            return 1;
        (void)memset(&executions[index], 0, sizeof(executions[index]));
        executions[index].network_id = 7U;
        executions[index].batch_number = 2U;
        executions[index].batch_timestamp_ms = 20U;
        executions[index].maximum_timestamp_window = 100U;
        executions[index].global_sequence = index + 2U;
        executions[index].epoch = 1U;
        executions[index].recorded_module_version =
            LX_PROGRAMS_ACCOUNT_ABI_VERSION;
        executions[index].parameter_version = 1U;
        executions[index].signature_valid = true;
        executions[index].identities = &fixture->identities;
        executions[index].authority = &fixture->authorities[actor];
        executions[index].fee_parameters = &fixture->fees;
        executions[index].fee_balance =
            fixture->actor_initial_balances[actor];
        executions[index].gas_limit = 1000000U;
        executions[index].arena = &arenas[index];
        executions[index].sequencer_private_key =
            fixture->sequencer_private_key;
    }
    if (lxp_arena_init(&batch_arena, batch_arena_storage,
                       sizeof(batch_arena_storage)) != LXP_OK ||
        mkdtemp(directory) == NULL)
        return 1;
    for (index = 0U; index < activity_count; ++index)
        if (lxp_activity_encode(&activities[index], &batch_arena,
                                &canonical_activities[index]) != LXP_OK) {
            cleanup_differential_directory(
                directory, &checkpoint, wal_record, prepared);
            return 1;
        }
    if (lxp_daemon_batch_bind_prefix(
            canonical_activities, activity_count,
            fixture->kernel.current_state_root, 2U, 2U,
            &batch_arena, executions, &scheduling_roots,
            batch_id) != LXP_OK) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    (void)memcpy(offered_activity_root,
                 scheduling_roots.activity_merkle_root, 32U);
    (void)memcpy(offered_batch_id, batch_id, 32U);
    if (clock_gettime(CLOCK_MONOTONIC, &start) != 0) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    status = lxp_kernel_prepare_activity_batch(
        &fixture->kernel, activities, executions, activity_count,
        workers, &prepared, &retry_prefix_count);
    if (expect_retry && status != LXP_OK && prepared == NULL) {
        if (retry_status != NULL) *retry_status = status;
        if (retry_prefix != NULL) *retry_prefix = retry_prefix_count;
        if (commit_retry_prefix && retry_prefix_count != 0U) {
            activity_count = retry_prefix_count;
            status = lxp_daemon_batch_bind_prefix(
                canonical_activities, activity_count,
                fixture->kernel.current_state_root, 2U, 2U,
                &batch_arena, executions, &scheduling_roots, batch_id);
            if (status == LXP_OK &&
                (memcmp(offered_activity_root,
                        scheduling_roots.activity_merkle_root, 32U) == 0 ||
                 memcmp(offered_batch_id, batch_id, 32U) == 0))
                status = LXP_FATAL_INVARIANT;
            retry_prefix_count = 0U;
            if (status == LXP_OK)
                status = lxp_kernel_prepare_activity_batch(
                    &fixture->kernel, activities, executions,
                    activity_count, workers, &prepared,
                    &retry_prefix_count);
            expect_retry = false;
            if (status == LXP_OK) {
                /* Continue through the exact WAL/checkpoint/finalize path
                 * with the retained original prefix. */
            } else {
                cleanup_differential_directory(
                    directory, &checkpoint, wal_record, prepared);
                return 1;
            }
        } else {
            return cleanup_differential_directory(
                       directory, &checkpoint, wal_record, prepared) ? 0 : 1;
        }
    }
    if (expect_retry) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    prepared_receipts = status == LXP_OK ?
        lxp_kernel_prepared_batch_receipts(prepared) : NULL;
    prepared_events = status == LXP_OK ?
        lxp_kernel_prepared_batch_events(prepared) : NULL;
    base = status == LXP_OK ?
        lxp_kernel_prepared_batch_base_boundary(prepared) : NULL;
    settled = status == LXP_OK ?
        lxp_kernel_prepared_batch_final_boundary(prepared) : NULL;
    if (status == LXP_OK &&
        (lxp_kernel_prepared_batch_count(prepared) != activity_count ||
         retry_prefix_count != 0U ||
         prepared_receipts == NULL || prepared_events == NULL ||
         base == NULL || settled == NULL))
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK)
        (void)memcpy(run->prepared_root,
                     lxp_kernel_prepared_batch_final_root(prepared), 32U);
    for (index = 0U; status == LXP_OK && index < activity_count; ++index)
        if (memcmp(prepared_receipts[index].activity_root,
                   scheduling_roots.activity_merkle_root, 32U) != 0 ||
            memcmp(prepared_receipts[index].batch_id, batch_id, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
    for (index = 0U; status == LXP_OK &&
                     index < activity_count; ++index)
        status = lxp_receipt_encode(&prepared_receipts[index], true,
                                    &batch_arena,
                                    &canonical_receipts[index]);
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(
            &(lxp_batch_root_inputs){
                canonical_activities, activity_count,
                canonical_receipts, activity_count,
                prepared_events, activity_count,
                NULL, 0U, NULL, 0U},
            &batch_arena, &roots);
    for (index = 0U; status == LXP_OK &&
                     index < activity_count; ++index)
        status = lxp_merkle_leaf_hash(canonical_receipts[index].bytes,
                                      canonical_receipts[index].length,
                                      receipt_hashes[index]);
    for (index = 0U; status == LXP_OK &&
                     index < activity_count; ++index) {
        status = lxp_merkle_proof_generate(
            (const uint8_t (*)[32])receipt_hashes,
            activity_count, index, &batch_arena,
            &proofs[index], proof_root);
        if (status == LXP_OK &&
            memcmp(proof_root, roots.receipt_merkle_root, 32U) != 0)
            status = LXP_FATAL_INVARIANT;
    }
    (void)memset(&header, 0, sizeof(header));
    header.protocol_version = activities[0].protocol_version;
    header.network_id = 7U;
    header.epoch = 1U;
    header.batch_number = 2U;
    header.first_sequence = prepared_receipts == NULL ? 0U :
        prepared_receipts[0].global_sequence;
    header.last_sequence = prepared_receipts == NULL ? 0U :
        prepared_receipts[activity_count - 1U].global_sequence;
    if (prepared_receipts != NULL) {
        (void)memcpy(header.previous_state_root,
                     prepared_receipts[0].previous_state_root, 32U);
        (void)memcpy(header.resulting_state_root,
                     prepared_receipts[activity_count - 1U]
                         .resulting_state_root, 32U);
    }
    (void)memcpy(header.activity_merkle_root, roots.activity_merkle_root, 32U);
    (void)memcpy(header.receipt_merkle_root, roots.receipt_merkle_root, 32U);
    (void)memcpy(header.event_merkle_root, roots.event_merkle_root, 32U);
    (void)memcpy(header.oracle_root, roots.oracle_root, 32U);
    (void)memcpy(header.data_availability_root,
                 roots.data_availability_root, 32U);
    header.timestamp_ms = 20U;
    (void)memcpy(header.sequencer_id,
                 fixture->sequencer_authorization.sequencer_id, 32U);
    if (status == LXP_OK)
        status = lxp_batch_sign(
            &header, fixture->sequencer_private_key,
            &fixture->sequencer_authorization, header_signature,
            &batch_arena);
    if (status == LXP_OK)
        status = lxp_batch_header_encode(&header, &batch_arena,
                                         &canonical_header);
    (void)memset(&wal_input, 0, sizeof(wal_input));
    if (status == LXP_OK) {
        wal_input.protocol_version = header.protocol_version;
        wal_input.network_id = header.network_id;
        wal_input.epoch = header.epoch;
        wal_input.batch_number = header.batch_number;
        wal_input.timestamp_ms = header.timestamp_ms;
        wal_input.parameter_version = 1U;
        wal_input.fee_schedule_version =
            prepared_receipts[0].program_outcome.fee_schedule_version;
        wal_input.metering_schedule_version =
            prepared_receipts[0].program_outcome.metering_schedule_version;
        wal_input.first_sequence = header.first_sequence;
        wal_input.last_sequence = header.last_sequence;
        wal_input.count = activity_count;
        wal_input.base = *base;
        wal_input.settled = *settled;
        (void)memcpy(wal_input.publication_digest,
                     lxp_kernel_prepared_batch_publication_digest(prepared),
                     32U);
        wal_input.authorization = fixture->sequencer_authorization;
        wal_input.canonical_header = canonical_header;
        (void)memcpy(wal_input.header_signature, header_signature, 64U);
        wal_input.activities = canonical_activities;
        wal_input.receipts = canonical_receipts;
        wal_input.events = prepared_events;
        wal_input.receipt_proofs = proofs;
        checkpoint.directory = directory;
        checkpoint.kernel = &fixture->kernel;
        checkpoint.storage = snapshot_storage;
        checkpoint.storage_length = sizeof(snapshot_storage);
        status = lxp_daemon_batch_wal_commit_kernel(
            directory, &wal_input, &fixture->kernel, &fixture->identities,
            activities, prepared, persist_differential_checkpoint,
            &checkpoint, &wal_record);
    }
    if (clock_gettime(CLOCK_MONOTONIC, &finish) != 0) status = LXP_ERR_IO;
    run->elapsed_ns = elapsed_ns(&start, &finish);
    if (status == LXP_OK) {
        (void)memcpy(run->root, fixture->kernel.current_state_root, 32U);
        status = lxp_state_root(&fixture->kernel, run->canonical_root);
    }
    if (status != LXP_OK) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    for (index = 0U; index < activity_count; ++index) {
        lxp_arena receipt_arena;
        run->receipts[index] = prepared_receipts[index];
        run->events[index] = prepared_events[index];
        if (!run->receipts[index].program_outcome.present ||
            lxp_arena_init(&receipt_arena, run->receipt_storage[index],
                           sizeof(run->receipt_storage[index])) != LXP_OK ||
            lxp_receipt_encode(&run->receipts[index], true, &receipt_arena,
                               &run->canonical_receipts[index]) != LXP_OK) {
            cleanup_differential_directory(
                directory, &checkpoint, wal_record, prepared);
            return 1;
        }
        if (run->events[index].length > sizeof(run->event_storage[index])) {
            cleanup_differential_directory(
                directory, &checkpoint, wal_record, prepared);
            return 1;
        }
        if (run->events[index].length != 0U)
            (void)memcpy(run->event_storage[index], run->events[index].bytes,
                         run->events[index].length);
        run->events[index].bytes = run->event_storage[index];
        if (workload == DIFFERENTIAL_PLANNING_REFUSAL) {
            if (run->receipts[index].result_code != LXP_ERR_UNKNOWN_FIELD ||
                run->events[index].length != 4U ||
                memcmp(run->events[index].bytes, "\0\0\0\0", 4U) != 0) {
                cleanup_differential_directory(
                    directory, &checkpoint, wal_record, prepared);
                return 1;
            }
        } else if (run->events[index].length <= 4U ||
            (index != 0U &&
             run->events[index].length == run->events[index - 1U].length &&
             memcmp(run->events[index].bytes,
                    run->events[index - 1U].bytes,
                    run->events[index].length) == 0)) {
            cleanup_differential_directory(
                directory, &checkpoint, wal_record, prepared);
            return 1;
        }
    }
    if (lxp_daemon_batch_wal_transition(
            directory, wal_record,
            lxp_kernel_prepared_batch_final_boundary(prepared),
            LXP_DAEMON_BATCH_WAL_COMMITTED) != LXP_OK) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    if (lxp_daemon_batch_wal_retire(
            directory, wal_record,
            lxp_kernel_prepared_batch_final_boundary(prepared)) != LXP_OK) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    if (!checkpoint.stored) {
        cleanup_differential_directory(
            directory, &checkpoint, wal_record, prepared);
        return 1;
    }
    return cleanup_differential_directory(
               directory, &checkpoint, wal_record, prepared) ? 0 : 1;
}

static lxp_result differential_fixture_destroy(differential_fixture *fixture)
{
    size_t index;
    lxp_result status = lxp_state_store_destroy(&fixture->state);
    if (status != LXP_OK) return status;
    for (index = 0U; index < fixture->kernel.blob_count; ++index) {
        free(fixture->kernel.blobs[index].bytes);
        fixture->kernel.blobs[index].bytes = NULL;
    }
    fixture->kernel.blob_count = 0U;
    fixture->kernel.blob_total_bytes = 0U;
    return LXP_OK;
}

static int qualify_zero_prefix_fee_limit(void)
{
    static differential_fixture serial_fixture;
    static differential_fixture parallel_fixture;
    static differential_run serial_run;
    static differential_run parallel_run;
    lxp_result serial_status = LXP_OK;
    lxp_result parallel_status = LXP_OK;
    size_t serial_prefix = SIZE_MAX;
    size_t parallel_prefix = SIZE_MAX;
    lxp_u128 impossible_limit = {UINT64_MAX, UINT64_MAX};
    if (differential_fixture_init(&serial_fixture) != 0 ||
        differential_fixture_init(&parallel_fixture) != 0 ||
        execute_workload(
            &serial_fixture, DIFFERENTIAL_ALL_CONFLICTING, 1U, 1U,
            impossible_limit, true, false,
            &serial_status, &serial_prefix,
            &serial_run) != 0 ||
        execute_workload(
            &parallel_fixture, DIFFERENTIAL_ALL_CONFLICTING, 1U,
            DIFFERENTIAL_WORKERS, impossible_limit, true, false,
            &parallel_status, &parallel_prefix, &parallel_run) != 0 ||
        serial_status == LXP_OK || serial_status != parallel_status ||
        serial_prefix != 0U || parallel_prefix != 0U)
        return 1;
    return differential_fixture_destroy(&serial_fixture) == LXP_OK &&
           differential_fixture_destroy(&parallel_fixture) == LXP_OK ? 0 : 1;
}

static int exact_runs_equal(const differential_run *serial,
                            const differential_run *parallel)
{
    size_t index;
    if (memcmp(serial->root, parallel->root, 32U) != 0) return 1;
    if (memcmp(serial->canonical_root, parallel->canonical_root, 32U) != 0)
        return 1;
    if (memcmp(serial->prepared_root, parallel->prepared_root, 32U) != 0 ||
        memcmp(serial->root, serial->prepared_root, 32U) != 0 ||
        memcmp(parallel->root, parallel->prepared_root, 32U) != 0)
        return 1;
    for (index = 0U; index < DIFFERENTIAL_BATCH_SIZE; ++index) {
        if (serial->receipts[index].global_sequence !=
                parallel->receipts[index].global_sequence ||
            serial->receipts[index].program_outcome.cpu_fuel !=
                parallel->receipts[index].program_outcome.cpu_fuel ||
            serial->canonical_receipts[index].length !=
                parallel->canonical_receipts[index].length ||
            memcmp(serial->canonical_receipts[index].bytes,
                   parallel->canonical_receipts[index].bytes,
                   serial->canonical_receipts[index].length) != 0 ||
            serial->events[index].length != parallel->events[index].length ||
            (serial->events[index].length != 0U &&
             memcmp(serial->events[index].bytes,
                    parallel->events[index].bytes,
                    serial->events[index].length) != 0))
            return 1;
    }
    return 0;
}

static int derive_call_limit(const lxp_receipt *receipt, lxp_u128 *limit)
{
    size_t index;
    if (receipt == NULL || limit == NULL ||
        !receipt->program_outcome.present)
        return 1;
    *limit = (lxp_u128){0U, 0U};
    for (index = 0U; index < LX_PROGRAMS_CALL_BUDGET_FIELDS; ++index) {
        lxp_u256 product;
        lxp_u128 component;
        if (lxp_u128_mul(
                (lxp_u128){0U, call_budget[index]},
                (lxp_u128){
                    0U, receipt->program_outcome.fee_schedule_prices[index]},
                &product) != LXP_OK ||
            product.words[2] != 0U || product.words[3] != 0U)
            return 1;
        component = (lxp_u128){product.words[1], product.words[0]};
        if (lxp_u128_add(*limit, component, limit) != LXP_OK)
            return 1;
    }
    return 0;
}

static int qualify_workload(differential_workload workload)
{
    static differential_fixture serial_fixture;
    static differential_fixture parallel_fixture;
    static differential_run serial;
    static differential_run parallel;
    lxp_u128 fee_limit = workload == DIFFERENTIAL_LOW_CONFLICT ?
        (lxp_u128){0U, UINT64_MAX} : observed_call_limit;
    if (workload == DIFFERENTIAL_ALL_CONFLICTING &&
        lxp_u128_is_zero(fee_limit))
        return 1;
    if (differential_fixture_init(&serial_fixture) != 0 ||
        differential_fixture_init(&parallel_fixture) != 0 ||
        execute_workload(&serial_fixture, workload,
                         DIFFERENTIAL_BATCH_SIZE, 1U,
                         fee_limit,
                         false, false, NULL, NULL,
                         &serial) != 0 ||
        execute_workload(&parallel_fixture, workload,
                         DIFFERENTIAL_BATCH_SIZE,
                         DIFFERENTIAL_WORKERS,
                         fee_limit,
                         false, false, NULL, NULL,
                         &parallel) != 0 ||
        exact_runs_equal(&serial, &parallel) != 0)
        return 1;
    observed_call_fee = serial.receipts[0].fee_charged;
    if (derive_call_limit(&serial.receipts[0], &observed_call_limit) != 0 ||
        lxp_u128_cmp(observed_call_limit, observed_call_fee) < 0)
        return 1;
    /* Elapsed values are source-harness measurements only. They are excluded
     * from every consensus comparison and no baseline number is embedded. */
    (void)fprintf(stderr,
                  "program batch differential workload=%s serial_ns=%llu parallel_ns=%llu\n",
                  workload == DIFFERENTIAL_LOW_CONFLICT ? "low-conflict" :
                  "all-conflicting",
                  (unsigned long long)serial.elapsed_ns,
                  (unsigned long long)parallel.elapsed_ns);
    return differential_fixture_destroy(&serial_fixture) == LXP_OK &&
           differential_fixture_destroy(&parallel_fixture) == LXP_OK ? 0 : 1;
}

static int execute_scalar_prefix(differential_fixture *fixture,
                                 size_t activity_count,
                                 differential_run *run)
{
    static uint8_t payloads[DIFFERENTIAL_BATCH_SIZE]
                           [CALL_FIXED_BYTES + 256U];
    static uint8_t signatures[DIFFERENTIAL_BATCH_SIZE][64];
    static uint8_t arena_storage[DIFFERENTIAL_BATCH_SIZE]
                                [2U * LXP_MAX_ACTIVITY_BYTES + 4096U];
    static uint8_t root_arena_storage[
        DIFFERENTIAL_BATCH_SIZE * LXP_MAX_ACTIVITY_BYTES];
    lxp_activity activities[DIFFERENTIAL_BATCH_SIZE];
    lxp_kernel_execution executions[DIFFERENTIAL_BATCH_SIZE];
    lxp_byte_span canonical_activities[DIFFERENTIAL_BATCH_SIZE];
    lxp_batch_roots scheduling_roots;
    lxp_arena arenas[DIFFERENTIAL_BATCH_SIZE];
    lxp_arena root_arena;
    uint8_t batch_id[32];
    size_t index;
    if (activity_count == 0U || activity_count > DIFFERENTIAL_BATCH_SIZE ||
        lxp_arena_init(&root_arena, root_arena_storage,
                       sizeof(root_arena_storage)) != LXP_OK)
        return 1;
    (void)memset(run, 0, sizeof(*run));
    for (index = 0U; index < activity_count; ++index) {
        size_t length = explicit_empty_call_payload(
            payloads[index], fixture->program_id);
        length = call_payload_with_marker(
            payloads[index], length, (uint8_t)(index + 1U));
        fill_activity(&activities[index], LX_PROGRAMS_CALL,
                      payloads[index], length, fixture->dids[0],
                      fixture->did_lengths[0], fixture->keys[0]);
        activities[index].account_sequence = index + 1U;
        activities[index].idempotency_key[30] = (uint8_t)(index >> 8U);
        activities[index].idempotency_key[31] = (uint8_t)(index + 2U);
        activities[index].fee_limit = observed_call_limit;
        if (sign_activity(&activities[index], fixture->private_keys[0],
                          signatures[index]) != 0)
            return 1;
        activities[index].signature =
            (lxp_byte_span){signatures[index], sizeof(signatures[index])};
        if (lxp_arena_init(&arenas[index], arena_storage[index],
                           sizeof(arena_storage[index])) != LXP_OK ||
            lxp_activity_encode(&activities[index], &root_arena,
                                &canonical_activities[index]) != LXP_OK)
            return 1;
    }
    for (index = 0U; index < activity_count; ++index) {
        (void)memset(&executions[index], 0, sizeof(executions[index]));
        executions[index].network_id = 7U;
        executions[index].batch_number = 2U;
        executions[index].batch_timestamp_ms = 20U;
        executions[index].maximum_timestamp_window = 100U;
        executions[index].epoch = 1U;
        executions[index].global_sequence = index + 2U;
        executions[index].recorded_module_version =
            LX_PROGRAMS_ACCOUNT_ABI_VERSION;
        executions[index].parameter_version = 1U;
        executions[index].signature_valid = true;
        executions[index].identities = &fixture->identities;
        executions[index].authority = &fixture->authorities[0];
        executions[index].fee_parameters = &fixture->fees;
        executions[index].fee_balance = fixture->actor_initial_balances[0];
        executions[index].gas_limit = 1000000U;
        executions[index].arena = &arenas[index];
        executions[index].sequencer_private_key =
            fixture->sequencer_private_key;
    }
    if (lxp_daemon_batch_bind_prefix(
            canonical_activities, activity_count,
            fixture->kernel.current_state_root, 2U, 2U,
            &root_arena, executions, &scheduling_roots,
            batch_id) != LXP_OK)
        return 1;
    for (index = 0U; index < activity_count; ++index) {
        if (lxp_kernel_execute_activity(
                &fixture->kernel, &activities[index], &executions[index],
                &run->receipts[index]) != LXP_OK ||
            lxp_programs_project_receipt_events(
                &run->receipts[index], &arenas[index],
                &run->events[index]) != LXP_OK)
            return 1;
        {
            lxp_arena receipt_arena;
            if (lxp_arena_init(&receipt_arena, run->receipt_storage[index],
                               sizeof(run->receipt_storage[index])) != LXP_OK ||
                lxp_receipt_encode(
                    &run->receipts[index], true, &receipt_arena,
                    &run->canonical_receipts[index]) != LXP_OK ||
                run->events[index].length >
                    sizeof(run->event_storage[index]))
                return 1;
        }
        if (run->events[index].length != 0U)
            (void)memcpy(run->event_storage[index],
                         run->events[index].bytes,
                         run->events[index].length);
        run->events[index].bytes = run->event_storage[index];
    }
    (void)memcpy(run->root, fixture->kernel.current_state_root, 32U);
    if (lxp_state_root(&fixture->kernel, run->canonical_root) != LXP_OK)
        return 1;
    (void)memcpy(run->prepared_root, run->root, 32U);
    return 0;
}

static int qualify_retry_balance_case(lxp_u128 actor_balance,
                                      lxp_u128 treasury_balance)
{
    enum { RETRY_PREFIX = 8, RETRY_OFFER = RETRY_PREFIX + 1 };
    static differential_fixture serial_refusal;
    static differential_fixture parallel_refusal;
    static differential_fixture scalar_prefix;
    static differential_run serial_run;
    static differential_run parallel_run;
    static differential_run scalar_run;
    lxp_result serial_status = LXP_OK;
    lxp_result parallel_status = LXP_OK;
    size_t serial_prefix_count = 0U;
    size_t parallel_prefix_count = 0U;
    if (differential_fixture_init_balanced(
            &serial_refusal, actor_balance, treasury_balance) != 0 ||
        differential_fixture_init_balanced(
            &parallel_refusal, actor_balance, treasury_balance) != 0 ||
        execute_workload(
            &serial_refusal, DIFFERENTIAL_ALL_CONFLICTING,
            RETRY_OFFER, 1U, observed_call_limit,
            true, true, &serial_status,
            &serial_prefix_count, &serial_run) != 0 ||
        execute_workload(
            &parallel_refusal, DIFFERENTIAL_ALL_CONFLICTING,
            RETRY_OFFER, DIFFERENTIAL_WORKERS, observed_call_limit,
            true, true, &parallel_status,
            &parallel_prefix_count, &parallel_run) != 0 ||
        serial_status == LXP_OK || serial_status != parallel_status ||
        serial_prefix_count != RETRY_PREFIX ||
        parallel_prefix_count != RETRY_PREFIX)
        return 1;
    if (differential_fixture_init_balanced(
            &scalar_prefix, actor_balance, treasury_balance) != 0 ||
        execute_scalar_prefix(&scalar_prefix, RETRY_PREFIX,
                              &scalar_run) != 0 ||
        exact_runs_equal(&serial_run, &parallel_run) != 0 ||
        exact_runs_equal(&serial_run, &scalar_run) != 0)
        return 1;
    return differential_fixture_destroy(&serial_refusal) == LXP_OK &&
           differential_fixture_destroy(&parallel_refusal) == LXP_OK &&
           differential_fixture_destroy(&scalar_prefix) == LXP_OK ? 0 : 1;
}

static int qualify_retry_contract(void)
{
    enum { RETRY_PREFIX = 8 };
    lxp_u128 prefix_fee = {0U, 0U};
    lxp_u128 fee_limit_headroom;
    lxp_u128 actor_balance;
    lxp_u128 treasury_balance;
    lxp_u128 maximum = {UINT64_MAX, UINT64_MAX};
    size_t index;
    if (lxp_u128_is_zero(observed_call_fee) ||
        lxp_u128_sub(observed_call_limit, observed_call_fee,
                     &fee_limit_headroom) != LXP_OK)
        return 1;
    for (index = 0U; index < RETRY_PREFIX; ++index)
        if (lxp_u128_add(prefix_fee, observed_call_fee,
                         &prefix_fee) != LXP_OK)
            return 1;
    if (lxp_u128_add(prefix_fee, fee_limit_headroom,
                     &actor_balance) != LXP_OK ||
        qualify_retry_balance_case(actor_balance,
                                   (lxp_u128){0U, 0U}) != 0 ||
        lxp_u128_sub(maximum, prefix_fee, &treasury_balance) != LXP_OK ||
        qualify_retry_balance_case(maximum, treasury_balance) != 0)
        return 1;
    return 0;
}

static int qualify_planning_refusal(void)
{
    static differential_fixture serial_fixture;
    static differential_fixture parallel_fixture;
    static differential_run serial_run;
    static differential_run parallel_run;
    if (differential_fixture_init(&serial_fixture) != 0 ||
        differential_fixture_init(&parallel_fixture) != 0)
        return 1;
    serial_fixture.program_id[0] ^= 0x80U;
    parallel_fixture.program_id[0] ^= 0x80U;
    if (execute_workload(
            &serial_fixture, DIFFERENTIAL_PLANNING_REFUSAL, 1U, 1U,
            observed_call_limit, false, false, NULL, NULL, &serial_run) != 0 ||
        execute_workload(
            &parallel_fixture, DIFFERENTIAL_PLANNING_REFUSAL, 1U,
            DIFFERENTIAL_WORKERS, observed_call_limit, false, false,
            NULL, NULL, &parallel_run) != 0 ||
        serial_run.receipts[0].result_code != LXP_ERR_UNKNOWN_FIELD ||
        parallel_run.receipts[0].result_code != LXP_ERR_UNKNOWN_FIELD ||
        serial_run.receipts[0].effects.count != 0U ||
        parallel_run.receipts[0].effects.count != 0U ||
        exact_runs_equal(&serial_run, &parallel_run) != 0)
        return 1;
    return differential_fixture_destroy(&serial_fixture) == LXP_OK &&
           differential_fixture_destroy(&parallel_fixture) == LXP_OK ? 0 : 1;
}

int main(void)
{
    if (qualify_workload(DIFFERENTIAL_LOW_CONFLICT) != 0) {
        (void)fputs("low-conflict differential failed\n", stderr);
        return 1;
    }
    if (qualify_workload(DIFFERENTIAL_ALL_CONFLICTING) != 0) {
        (void)fputs("all-conflicting differential failed\n", stderr);
        return 1;
    }
    if (qualify_retry_contract() != 0) {
        (void)fputs("retry differential failed\n", stderr);
        return 1;
    }
    if (qualify_zero_prefix_fee_limit() != 0) {
        (void)fputs("zero-prefix differential failed\n", stderr);
        return 1;
    }
    if (qualify_planning_refusal() != 0) {
        (void)fputs("planning-refusal differential failed\n", stderr);
        return 1;
    }
    return 0;
}
