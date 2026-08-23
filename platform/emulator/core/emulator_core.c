#include "emulator_core.h"

#include "layerx/lx_asset.h"
#include "layerx/lx_budget.h"
#include "layerx/lx_escrow.h"
#include "layerx/lx_perps.h"
#include "layerx/lx_service.h"
#include "layerx/lx_stream.h"
#include "layerx/lxp_activity.h"
#include "layerx/lxp_authority.h"
#include "layerx/lxp_batch.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_ledger.h"
#include "layerx/lxp_receipt.h"
#include "layerx/lxp_snapshot.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stdlib.h>
#include <string.h>

enum {
    PLATFORM_EMULATOR_ARENA_BYTES = 16 * 1024 * 1024,
    PLATFORM_EMULATOR_RECEIPT_BYTES = 64 * 1024,
    PLATFORM_EMULATOR_SNAPSHOT_BYTES = 24 * 1024 * 1024,
    PLATFORM_EMULATOR_SNAPSHOT_VERSION = 1,
    PLATFORM_EMULATOR_FAULT_REJECT = 1,
    PLATFORM_EMULATOR_FAULT_DROP_RECEIPT = 2,
    PLATFORM_EMULATOR_FAULT_CORRUPT_RECEIPT = 3
};

static const uint8_t snapshot_magic[8] = { 'L', 'X', 'E', 'M', 'U', '0', '1', 0 };

typedef struct platform_snapshot_header {
    uint8_t magic[8];
    uint32_t version;
    uint32_t network_id;
    uint64_t timestamp_ms;
    uint64_t batch_number;
    uint64_t global_sequence;
    uint64_t identity_count;
    uint64_t account_count;
    uint64_t core_length;
    lxp_snapshot_manifest_record manifest;
    uint8_t wrapper_digest[32];
} platform_snapshot_header;

struct platform_emulator {
    uint32_t network_id;
    uint64_t timestamp_ms;
    uint64_t batch_number;
    uint64_t global_sequence;
    uint64_t parameter_set;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_identity_store identities;
    lx_account_registry accounts;
    lx_asset_registry assets;
    lx_asset_record native_asset;
    lxp_transfer_asset_state native_asset_state;
    lxp_kernel kernel;
    lxp_fee_params fee_parameters;
    lxp_arena arena;
    uint8_t *arena_bytes;
    uint8_t receipt_bytes[PLATFORM_EMULATOR_RECEIPT_BYTES];
    uint8_t *snapshot_bytes;
    size_t snapshot_length;
    uint64_t reject_count;
    uint64_t drop_count;
    uint64_t corrupt_count;
    bool state_initialized;
};

static lxp_result apply_transfer_set(lxp_kernel *kernel,
                                     const lxp_transfer_set *set,
                                     lxp_receipt *receipt)
{
    lxp_transfer_set_result applied;
    lxp_transfer_context context;
    lxp_result status;
    (void)kernel;
    if (set == NULL || receipt == NULL) return LXP_ERR_NON_CANONICAL;
    context = set->context;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &applied);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, applied.transfer_set_root, 32U);
    return status;
}

static uint8_t emulator_fee_token;

static lxp_result prepare_fee(lxp_kernel *kernel, const lxp_activity *activity,
                              const lxp_authority_resolved *authority,
                              lxp_u128 fee, void **transaction)
{
    (void)kernel;
    (void)activity;
    (void)authority;
    if (!lxp_u128_is_zero(fee)) return LXP_ERR_FEE_UNPAYABLE;
    *transaction = &emulator_fee_token;
    return LXP_OK;
}

static void commit_fee(lxp_kernel *kernel, void *transaction)
{
    (void)kernel;
    (void)transaction;
}

static void rollback_fee(lxp_kernel *kernel, void *transaction)
{
    (void)kernel;
    (void)transaction;
}

static lxp_result register_modules(platform_emulator *emulator)
{
    const lxp_module_iface *modules[] = {
        lx_asset_module_iface(), lx_budget_module_iface(),
        lx_escrow_module_iface(), lx_stream_module_iface(),
        lx_service_module_iface(), lx_perps_module_iface()
    };
    size_t i;
    lxp_result status = LXP_OK;
    for (i = 0U; i < sizeof(modules) / sizeof(modules[0]) && status == LXP_OK;
         ++i)
        status = lxp_kernel_register_module(&emulator->kernel, modules[i]);
    return status;
}

static void put_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static lxp_result batch_identifier(const platform_emulator *emulator,
                                   uint8_t batch_id[32])
{
    uint8_t material[4U + 8U + 8U + 32U];
    material[0] = (uint8_t)(emulator->network_id >> 24U);
    material[1] = (uint8_t)(emulator->network_id >> 16U);
    material[2] = (uint8_t)(emulator->network_id >> 8U);
    material[3] = (uint8_t)emulator->network_id;
    put_u64(material + 4U, emulator->batch_number);
    put_u64(material + 12U, emulator->timestamp_ms);
    (void)memcpy(material + 20U, emulator->kernel.current_state_root, 32U);
    return lxp_hash_domain(LXP_DOMAIN_BATCH_HEADER, material,
                           sizeof(material), batch_id);
}

platform_emulator *platform_emulator_create(uint32_t network_id,
                                             uint64_t timestamp_ms)
{
    platform_emulator *emulator;
    lxp_result status;
    if (network_id == 0U || timestamp_ms == 0U) return NULL;
    emulator = calloc(1U, sizeof(*emulator));
    if (emulator == NULL) return NULL;
    emulator->arena_bytes = malloc(PLATFORM_EMULATOR_ARENA_BYTES);
    emulator->snapshot_bytes = malloc(PLATFORM_EMULATOR_SNAPSHOT_BYTES);
    if (emulator->arena_bytes == NULL || emulator->snapshot_bytes == NULL) {
        platform_emulator_destroy(emulator);
        return NULL;
    }
    emulator->network_id = network_id;
    emulator->timestamp_ms = timestamp_ms;
    emulator->global_sequence = 1U;
    emulator->parameter_set = 1U;
    emulator->fee_parameters.version = 1U;
    emulator->fee_parameters.multiplier_basis_points = 10000U;
    status = lxp_state_store_init(&emulator->state, emulator->global_sequence);
    emulator->state_initialized = status == LXP_OK;
    if (status == LXP_OK)
        status = lx_account_registry_init(&emulator->accounts);
    if (status == LXP_OK)
        status = lx_asset_registry_init(&emulator->assets, 0U);
    if (status == LXP_OK)
        status = lxp_arena_init(&emulator->arena, emulator->arena_bytes,
                                PLATFORM_EMULATOR_ARENA_BYTES);
    if (status == LXP_OK)
        status = lxp_kernel_create(&emulator->kernel, &emulator->state,
                                   &emulator->journal,
                                   &emulator->parameter_set, 0U);
    if (status == LXP_OK) status = register_modules(emulator);
    if (status == LXP_OK)
        status = lxp_kernel_set_capabilities(&emulator->kernel, NULL,
                                             apply_transfer_set);
    if (status == LXP_OK)
        status = lxp_kernel_set_fee_transaction(
            &emulator->kernel,
            &(lxp_kernel_fee_transaction){ prepare_fee, commit_fee,
                                           rollback_fee });
    if (status == LXP_OK) {
        emulator->native_asset.asset_id[0] = 1U;
        emulator->native_asset.symbol_length = 3U;
        (void)memcpy(emulator->native_asset.symbol, "LXP", 4U);
        emulator->native_asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
        emulator->native_asset.custody_reference[0] = 1U;
        emulator->native_asset.custody_reference_length = 1U;
        status = lx_asset_transfer_state(&emulator->native_asset,
                                         &emulator->native_asset_state);
    }
    if (status == LXP_OK)
        status = lxp_state_root(&emulator->kernel,
                                emulator->kernel.current_state_root);
    if (status != LXP_OK) {
        platform_emulator_destroy(emulator);
        return NULL;
    }
    return emulator;
}

void platform_emulator_destroy(platform_emulator *emulator)
{
    if (emulator == NULL) return;
    if (emulator->state_initialized)
        (void)lxp_state_store_destroy(&emulator->state);
    free(emulator->arena_bytes);
    free(emulator->snapshot_bytes);
    free(emulator);
}

const char *platform_emulator_error_name(int32_t result)
{
    return lxp_result_name(result);
}

int32_t platform_emulator_set_time(platform_emulator *emulator,
                                   uint64_t timestamp_ms)
{
    if (emulator == NULL || timestamp_ms < emulator->timestamp_ms)
        return LXP_ERR_NON_MONOTONIC_TIME;
    emulator->timestamp_ms = timestamp_ms;
    return LXP_OK;
}

int32_t platform_emulator_advance_time(platform_emulator *emulator,
                                       uint64_t delta_ms)
{
    if (emulator == NULL || UINT64_MAX - emulator->timestamp_ms < delta_ms)
        return LXP_ERR_OVERFLOW;
    emulator->timestamp_ms += delta_ms;
    return LXP_OK;
}

int32_t platform_emulator_inject_failure(platform_emulator *emulator,
                                         uint32_t kind, uint64_t count)
{
    if (emulator == NULL) return LXP_ERR_NON_CANONICAL;
    switch (kind) {
    case PLATFORM_EMULATOR_FAULT_REJECT: emulator->reject_count = count; break;
    case PLATFORM_EMULATOR_FAULT_DROP_RECEIPT: emulator->drop_count = count; break;
    case PLATFORM_EMULATOR_FAULT_CORRUPT_RECEIPT: emulator->corrupt_count = count; break;
    default: return LXP_ERR_UNKNOWN_FIELD;
    }
    return LXP_OK;
}

int32_t platform_emulator_prefund(platform_emulator *emulator,
                                  const uint8_t *did, size_t did_length,
                                  const uint8_t public_key[32],
                                  uint64_t amount_hi, uint64_t amount_lo)
{
    uint8_t account_name[LX_ACCOUNT_NAME_MAX];
    uint8_t account_id[32];
    lx_account *account;
    lxp_identity *identity;
    lxp_result status;
    static const uint8_t prefix[] = "agent:";
    static const uint8_t suffix[] = ":main";
    if (emulator == NULL || did == NULL || public_key == NULL ||
        did_length == 0U || did_length > LXP_MAX_DID_LENGTH ||
        sizeof(prefix) - 1U + did_length + sizeof(suffix) - 1U >
            sizeof(account_name)) return LXP_ERR_NON_CANONICAL;
    status = lxp_identity_register(&emulator->identities, did, did_length,
                                   public_key, &identity);
    if (status != LXP_OK) return status;
    (void)memcpy(account_name, prefix, sizeof(prefix) - 1U);
    (void)memcpy(account_name + sizeof(prefix) - 1U, did, did_length);
    (void)memcpy(account_name + sizeof(prefix) - 1U + did_length, suffix,
                 sizeof(suffix) - 1U);
    status = lx_account_id_from_string(account_name,
        sizeof(prefix) - 1U + did_length + sizeof(suffix) - 1U, account_id);
    if (status == LXP_OK)
        status = lx_account_open(&emulator->accounts, account_name,
            sizeof(prefix) - 1U + did_length + sizeof(suffix) - 1U,
            account_id, emulator->global_sequence, LX_ACCOUNT_OPEN_GENESIS,
            NULL, &account);
    if (status == LXP_OK) {
        (void)memcpy(account->authority_key, public_key, 32U);
        account->has_authority_key = true;
        status = lxp_ledger_bootstrap_balance(
            account, emulator->native_asset.asset_id,
            (lxp_u128){ amount_hi, amount_lo }, 0U);
    }
    (void)identity;
    return status;
}

static lxp_result owner_authority(platform_emulator *emulator,
                                  const lxp_activity *activity,
                                  lxp_authority_resolved *authority)
{
    lxp_identity *identity;
    uint8_t actor[32];
    uint8_t grant_id[32] = { 0 };
    lxp_result status = lxp_identity_resolve(&emulator->identities,
        activity->actor_did.bytes, activity->actor_did.length, &identity);
    if (status != LXP_OK) return status;
    if (activity->authority.length != 32U ||
        memcmp(activity->authority.bytes, identity->primary_key, 32U) != 0)
        return LXP_ERR_BAD_SIGNATURE;
    status = lxp_did_id_derive(activity->actor_did.bytes,
                               activity->actor_did.length, actor);
    if (status != LXP_OK) return status;
    (void)memset(authority, 0, sizeof(*authority));
    (void)memcpy(authority->actor, actor, 32U);
    (void)memcpy(authority->principal, actor, 32U);
    (void)memcpy(authority->verified_key, identity->primary_key, 32U);
    authority->kind = LXP_AUTHORITY_OWNER;
    return lxp_authority_hash(authority->kind, grant_id,
                              authority->verified_key,
                              authority->authority_hash);
}

int32_t platform_emulator_execute(platform_emulator *emulator,
                                  const uint8_t *activity_bytes, size_t length,
                                  platform_emulator_receipt *output)
{
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_kernel_execution execution;
    lxp_receipt receipt;
    lxp_byte_span canonical_activity;
    lxp_byte_span canonical_receipt;
    lxp_batch_root_inputs root_inputs;
    lxp_batch_roots roots;
    lxp_result status;
    if (emulator == NULL || activity_bytes == NULL || length == 0U ||
        output == NULL) return LXP_ERR_NON_CANONICAL;
    if (emulator->reject_count != 0U) {
        --emulator->reject_count;
        return LXP_ERR_IO;
    }
    status = lxp_arena_reset(&emulator->arena, 0U);
    if (status == LXP_OK)
        status = lxp_activity_decode(activity_bytes, length, &activity);
    if (status == LXP_OK)
        status = lxp_activity_check_envelope(&activity, emulator->network_id);
    if (status == LXP_OK) status = lxp_activity_verify_signature(&activity);
    if (status == LXP_OK)
        status = owner_authority(emulator, &activity, &authority);
    (void)memset(&root_inputs, 0, sizeof(root_inputs));
    canonical_activity = (lxp_byte_span){ activity_bytes, length };
    root_inputs.activities = &canonical_activity;
    root_inputs.activity_count = 1U;
    if (status == LXP_OK)
        status = lxp_batch_roots_compute(&root_inputs, &emulator->arena,
                                         &roots);
    (void)memset(&execution, 0, sizeof(execution));
    execution.network_id = emulator->network_id;
    execution.batch_timestamp_ms = emulator->timestamp_ms;
    execution.maximum_timestamp_window = UINT64_C(86400000);
    execution.epoch = 0U;
    execution.global_sequence = emulator->global_sequence;
    execution.recorded_module_version = 1U;
    execution.parameter_version = 1U;
    execution.signature_valid = true;
    execution.identities = &emulator->identities;
    execution.authority = &authority;
    execution.fee_parameters = &emulator->fee_parameters;
    execution.fee_balance = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    execution.gas_limit = UINT64_C(1000000);
    execution.arena = &emulator->arena;
    (void)memcpy(execution.activity_root, roots.activity_merkle_root, 32U);
    if (status == LXP_OK)
        status = batch_identifier(emulator, execution.batch_id);
    if (status == LXP_OK)
        status = lxp_kernel_execute_activity(&emulator->kernel, &activity,
                                             &execution, &receipt);
    if (status != LXP_OK) return status;
    status = lxp_receipt_encode(&receipt, false, &emulator->arena,
                                &canonical_receipt);
    if (status != LXP_OK || canonical_receipt.length >
        sizeof(emulator->receipt_bytes))
        return status != LXP_OK ? status : LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(emulator->receipt_bytes, canonical_receipt.bytes,
                 canonical_receipt.length);
    if (emulator->corrupt_count != 0U && canonical_receipt.length != 0U) {
        --emulator->corrupt_count;
        emulator->receipt_bytes[canonical_receipt.length - 1U] ^= UINT8_C(1);
    }
    (void)memcpy(output->activity_id, receipt.activity_id, 32U);
    (void)memcpy(output->batch_id, receipt.batch_id, 32U);
    (void)memcpy(output->state_root, receipt.resulting_state_root, 32U);
    output->global_sequence = receipt.global_sequence;
    output->result_code = receipt.result_code;
    output->bytes = emulator->receipt_bytes;
    output->length = canonical_receipt.length;
    ++emulator->global_sequence;
    ++emulator->batch_number;
    if (emulator->drop_count != 0U) {
        --emulator->drop_count;
        return LXP_ERR_IO;
    }
    return LXP_OK;
}

int32_t platform_emulator_inspect(const platform_emulator *emulator,
                                  platform_emulator_state *state)
{
    lxp_result status;
    if (emulator == NULL || state == NULL) return LXP_ERR_NON_CANONICAL;
    status = lxp_state_root(&emulator->kernel, state->state_root);
    if (status != LXP_OK) return status;
    state->next_sequence = emulator->state.next_sequence;
    state->batch_number = emulator->batch_number;
    state->timestamp_ms = emulator->timestamp_ms;
    state->cell_count = emulator->state.count;
    state->account_count = emulator->accounts.count;
    return LXP_OK;
}

int32_t platform_emulator_cell(const platform_emulator *emulator, size_t index,
                               uint8_t key[32], uint64_t *value_hi,
                               uint64_t *value_lo)
{
    if (emulator == NULL || key == NULL || value_hi == NULL ||
        value_lo == NULL || index >= emulator->state.count)
        return LXP_ERR_UNKNOWN_FIELD;
    (void)memcpy(key, emulator->state.cells[index].key, 32U);
    *value_hi = emulator->state.cells[index].value.hi;
    *value_lo = emulator->state.cells[index].value.lo;
    return LXP_OK;
}

int32_t platform_emulator_account(const platform_emulator *emulator,
                                  size_t index, uint8_t id[32],
                                  const uint8_t **name, size_t *name_length,
                                  uint64_t *balance_hi, uint64_t *balance_lo)
{
    const lx_account *account;
    if (emulator == NULL || id == NULL || name == NULL || name_length == NULL ||
        balance_hi == NULL || balance_lo == NULL ||
        index >= emulator->accounts.count) return LXP_ERR_UNKNOWN_FIELD;
    account = &emulator->accounts.accounts[index];
    (void)memcpy(id, account->id, 32U);
    *name = account->name;
    *name_length = account->name_length;
    *balance_hi = account->balance.hi;
    *balance_lo = account->balance.lo;
    return LXP_OK;
}

int32_t platform_emulator_snapshot_export(platform_emulator *emulator,
                                          const uint8_t **bytes,
                                          size_t *length)
{
    platform_snapshot_header header;
    lxp_byte_span core;
    size_t identity_bytes;
    size_t account_bytes;
    size_t total;
    lxp_result status;
    if (emulator == NULL || bytes == NULL || length == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_arena_reset(&emulator->arena, 0U);
    if (status == LXP_OK)
        status = lxp_snapshot_write(&emulator->kernel,
            emulator->global_sequence == 0U ? 0U : emulator->global_sequence - 1U,
            &emulator->arena, &core);
    if (status != LXP_OK) return status;
    (void)memset(&header, 0, sizeof(header));
    (void)memcpy(header.magic, snapshot_magic, sizeof(snapshot_magic));
    header.version = PLATFORM_EMULATOR_SNAPSHOT_VERSION;
    header.network_id = emulator->network_id;
    header.timestamp_ms = emulator->timestamp_ms;
    header.batch_number = emulator->batch_number;
    header.global_sequence = emulator->global_sequence;
    header.identity_count = emulator->identities.count;
    header.account_count = emulator->accounts.count;
    header.core_length = core.length;
    status = lxp_snapshot_manifest_build(core.bytes, core.length,
        emulator->global_sequence == 0U ? 0U : emulator->global_sequence - 1U,
        emulator->kernel.current_state_root, &header.manifest);
    identity_bytes = emulator->identities.count * sizeof(lxp_identity);
    account_bytes = emulator->accounts.count * sizeof(lx_account);
    total = sizeof(header) + identity_bytes + account_bytes + core.length;
    if (status != LXP_OK || total > PLATFORM_EMULATOR_SNAPSHOT_BYTES)
        return status != LXP_OK ? status : LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(emulator->snapshot_bytes, &header, sizeof(header));
    (void)memcpy(emulator->snapshot_bytes + sizeof(header),
                 emulator->identities.identities, identity_bytes);
    (void)memcpy(emulator->snapshot_bytes + sizeof(header) + identity_bytes,
                 emulator->accounts.accounts, account_bytes);
    (void)memcpy(emulator->snapshot_bytes + sizeof(header) + identity_bytes +
                 account_bytes, core.bytes, core.length);
    status = lxp_hash_domain(LXP_DOMAIN_SNAPSHOT, emulator->snapshot_bytes,
                             total, header.wrapper_digest);
    if (status != LXP_OK) return status;
    (void)memcpy(emulator->snapshot_bytes +
                     offsetof(platform_snapshot_header, wrapper_digest),
                 header.wrapper_digest, sizeof(header.wrapper_digest));
    emulator->snapshot_length = total;
    *bytes = emulator->snapshot_bytes;
    *length = total;
    return LXP_OK;
}

int32_t platform_emulator_snapshot_import(platform_emulator *emulator,
                                          const uint8_t *bytes,
                                          size_t length)
{
    platform_snapshot_header header;
    size_t identity_bytes;
    size_t account_bytes;
    size_t expected;
    const uint8_t *core;
    uint8_t expected_digest[32];
    uint8_t actual_digest[32];
    lxp_result status;
    if (emulator == NULL || bytes == NULL || length < sizeof(header))
        return LXP_ERR_TRUNCATED;
    (void)memcpy(&header, bytes, sizeof(header));
    if (memcmp(header.magic, snapshot_magic, sizeof(snapshot_magic)) != 0 ||
        header.version != PLATFORM_EMULATOR_SNAPSHOT_VERSION ||
        header.network_id != emulator->network_id ||
        header.identity_count > LXP_IDENTITY_STORE_CAPACITY ||
        header.account_count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_SNAPSHOT_MISMATCH;
    identity_bytes = (size_t)header.identity_count * sizeof(lxp_identity);
    account_bytes = (size_t)header.account_count * sizeof(lx_account);
    if (header.core_length > SIZE_MAX - sizeof(header) - identity_bytes -
        account_bytes) return LXP_ERR_LENGTH_LIMIT;
    expected = sizeof(header) + identity_bytes + account_bytes +
               (size_t)header.core_length;
    if (expected != length) return LXP_ERR_TRAILING_BYTES;
    if (length > PLATFORM_EMULATOR_SNAPSHOT_BYTES)
        return LXP_ERR_LENGTH_LIMIT;
    (void)memcpy(expected_digest, header.wrapper_digest,
                 sizeof(expected_digest));
    (void)memcpy(emulator->snapshot_bytes, bytes, length);
    (void)memset(emulator->snapshot_bytes +
                     offsetof(platform_snapshot_header, wrapper_digest),
                 0, sizeof(header.wrapper_digest));
    status = lxp_hash_domain(LXP_DOMAIN_SNAPSHOT, emulator->snapshot_bytes,
                             length, actual_digest);
    if (status != LXP_OK ||
        lxp_ct_memcmp(expected_digest, actual_digest, 32U) != 0)
        return status != LXP_OK ? status : LXP_ERR_SNAPSHOT_MISMATCH;
    core = emulator->snapshot_bytes + sizeof(header) + identity_bytes +
           account_bytes;
    status = lxp_snapshot_load(core, (size_t)header.core_length,
                               &header.manifest, header.manifest.state_root,
                               &emulator->kernel);
    if (status != LXP_OK) return status;
    (void)memcpy(emulator->identities.identities,
                 emulator->snapshot_bytes + sizeof(header),
                 identity_bytes);
    emulator->identities.count = (size_t)header.identity_count;
    (void)memcpy(emulator->accounts.accounts,
                 emulator->snapshot_bytes + sizeof(header) + identity_bytes,
                 account_bytes);
    emulator->accounts.count = (size_t)header.account_count;
    emulator->timestamp_ms = header.timestamp_ms;
    emulator->batch_number = header.batch_number;
    emulator->global_sequence = header.global_sequence;
    return LXP_OK;
}
