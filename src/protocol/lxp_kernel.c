#include "layerx/lxp_kernel.h"

#include "layerx/lxp_admission.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/programs.h"

#include "../modules/programs/event.h"

#include <limits.h>
#include <stdlib.h>
#include <string.h>

struct lxp_kernel_batch_snapshot {
    lxp_state_snapshot *state;
    lxp_identity_store identities;
    lxp_verified_receipt_index verified_receipts;
    lxp_kernel kernel;
    lxp_state_journal journal;
    lx_programs_transfer_runtime programs_runtime;
    lxp_transfer_asset_state *assets;
    lx_programs_fee_schedule fee_schedule;
    lx_programs_metering_schedule metering_schedule;
    lxp_fee_params fee_parameters;
    uint8_t occupancy_asset_id[32];
    uint8_t active_level_token[32];
    uint64_t level_generation;
};

struct lxp_prepared_transition {
    lxp_prepared_module_transition *module;
    lxp_result result_code;
    lxp_u128 fee_charged;
    uint8_t activity_id[32];
    uint16_t protocol_version;
    uint16_t module_id;
    uint32_t module_version;
    uint32_t parameter_version;
    uint32_t fee_schedule_version;
    uint32_t metering_schedule_version;
    uint8_t level_snapshot_token[32];
    uint8_t execution_binding[32];
};

struct lxp_kernel_prepared_batch {
    lxp_kernel_batch_snapshot *base;
    lxp_kernel_batch_snapshot *settled;
    lxp_receipt *receipts;
    lxp_byte_span *events;
    uint8_t **event_bytes;
    uint8_t publication_digest[32];
    lxp_kernel_batch_boundary base_boundary;
    lxp_kernel_batch_boundary final_boundary;
    size_t count;
    bool committed;
};

static lxp_result snapshot_bind_call_admission(
    lxp_module_ctx *ctx, const lxp_kernel_batch_snapshot *snapshot,
    const lxp_kernel_execution *execution, const uint8_t activity_id[32],
    lxp_u128 signed_fee_limit)
{
    if (ctx == NULL || snapshot == NULL || execution == NULL ||
        execution->authority == NULL || activity_id == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(ctx->activity_id, activity_id, 32U);
    (void)memcpy(ctx->call_admission.activity_binding, activity_id, 32U);
    (void)memcpy(ctx->call_admission.payer,
                 execution->authority->principal, 32U);
    ctx->call_admission.available_fee_units = execution->fee_balance;
    ctx->call_admission.signed_fee_limit = signed_fee_limit;
    ctx->call_admission.fee_schedule_version =
        snapshot->fee_schedule.version;
    ctx->call_admission.metering_schedule_version =
        snapshot->metering_schedule.version;
    (void)memcpy(ctx->call_admission.metering_schedule_coefficients,
                 snapshot->metering_schedule.coefficients,
                 sizeof(snapshot->metering_schedule.coefficients));
    ctx->call_admission.fee_schedule_prices[0] = snapshot->fee_schedule.cpu;
    ctx->call_admission.fee_schedule_prices[1] =
        snapshot->fee_schedule.memory_byte;
    ctx->call_admission.fee_schedule_prices[2] =
        snapshot->fee_schedule.storage_read_byte;
    ctx->call_admission.fee_schedule_prices[3] =
        snapshot->fee_schedule.storage_write_byte;
    ctx->call_admission.fee_schedule_prices[4] =
        snapshot->fee_schedule.output_value;
    ctx->call_admission.fee_schedule_prices[5] =
        snapshot->fee_schedule.output_byte;
    ctx->call_admission.fee_schedule_prices[6] =
        snapshot->fee_schedule.occupancy_byte_batch;
    ctx->call_admission.parameter_version = execution->parameter_version;
    ctx->call_admission.present = true;
    return LXP_OK;
}

static lxp_result snapshot_metering_schedule(
    void *context, uint32_t recorded_version, uint64_t batch_number,
    lx_programs_metering_schedule *schedule)
{
    const lxp_kernel_batch_snapshot *snapshot =
        (const lxp_kernel_batch_snapshot *)context;
    if (snapshot == NULL || schedule == NULL ||
        recorded_version != snapshot->metering_schedule.version ||
        batch_number < snapshot->metering_schedule.activation_batch)
        return LXP_ERR_VERSION_UNSUPPORTED;
    *schedule = snapshot->metering_schedule;
    return LXP_OK;
}

static lxp_result snapshot_occupancy_parameters(
    void *context, uint32_t recorded_version,
    lx_programs_fee_schedule *schedule, uint8_t occupancy_asset_id[32])
{
    const lxp_kernel_batch_snapshot *snapshot =
        (const lxp_kernel_batch_snapshot *)context;
    if (snapshot == NULL || schedule == NULL || occupancy_asset_id == NULL ||
        recorded_version != snapshot->fee_schedule.version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    *schedule = snapshot->fee_schedule;
    (void)memcpy(occupancy_asset_id,
                 snapshot->programs_runtime.occupancy_asset_id, 32U);
    return LXP_OK;
}

static void kernel_snapshot_release(lxp_kernel_batch_snapshot *snapshot)
{
    size_t index;
    if (snapshot == NULL) return;
    for (index = 0U; index < snapshot->kernel.blob_count; ++index) {
        free(snapshot->kernel.blobs[index].bytes);
        snapshot->kernel.blobs[index].bytes = NULL;
    }
    free(snapshot->assets);
    snapshot->assets = NULL;
    lxp_state_snapshot_destroy(snapshot->state);
    snapshot->state = NULL;
    free(snapshot);
}

static lxp_result kernel_snapshot_copy_blobs(
    lxp_kernel_batch_snapshot *target, const lxp_kernel *source)
{
    size_t index;
    size_t total = 0U;
    if (source->blob_count > LXP_KERNEL_MAX_BLOBS ||
        source->blob_total_bytes > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < source->blob_count; ++index)
        target->kernel.blobs[index].bytes = NULL;
    for (index = 0U; index < source->blob_count; ++index) {
        lxp_module_blob *blob = &target->kernel.blobs[index];
        *blob = source->blobs[index];
        blob->bytes = NULL;
        if (source->blobs[index].length == 0U ||
            source->blobs[index].length > LXP_KERNEL_MAX_BLOB_BYTES ||
            total > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES -
                        source->blobs[index].length ||
            source->blobs[index].bytes == NULL)
            return LXP_FATAL_INVARIANT;
        total += source->blobs[index].length;
        blob->bytes = (uint8_t *)malloc(source->blobs[index].length);
        if (blob->bytes == NULL) return LXP_ERR_ARENA_EXHAUSTED;
        (void)memcpy(blob->bytes, source->blobs[index].bytes,
                     source->blobs[index].length);
    }
    return total == source->blob_total_bytes ? LXP_OK :
        LXP_FATAL_INVARIANT;
}

static lxp_result kernel_snapshot_finish(
    lxp_kernel_batch_snapshot *snapshot,
    const lx_programs_transfer_runtime *runtime)
{
    lxp_result status;
    snapshot->kernel.state =
        lxp_state_snapshot_store_for_prepare(snapshot->state);
    snapshot->kernel.journal = &snapshot->journal;
    snapshot->kernel.parameter_set = NULL;
    snapshot->kernel.read_parameter = NULL;
    snapshot->kernel.apply_transfer_set = lxp_kernel_canonical_ledger_apply;
    snapshot->kernel.check_supply = NULL;
    snapshot->kernel.observe_commit = NULL;
    snapshot->kernel.commit_observer_context = NULL;
    snapshot->kernel.publication_poisoned = false;
    snapshot->kernel.poisoned_sequence = 0U;
    (void)memset(snapshot->kernel.poisoned_activity_id, 0, 32U);
    (void)memset(snapshot->kernel.poisoned_state_root, 0, 32U);
    (void)memset(snapshot->kernel.module_runtime, 0,
                 sizeof(snapshot->kernel.module_runtime));
    snapshot->programs_runtime = *runtime;
    snapshot->programs_runtime.accounts =
        lxp_state_snapshot_accounts_for_prepare(snapshot->state);
    snapshot->programs_runtime.assets = snapshot->assets;
    snapshot->programs_runtime.state_feed = NULL;
    (void)memcpy(snapshot->programs_runtime.occupancy_asset_id,
                 snapshot->occupancy_asset_id, 32U);
    snapshot->programs_runtime.resolve_metering_schedule =
        snapshot_metering_schedule;
    snapshot->programs_runtime.metering_schedule_context = snapshot;
    snapshot->programs_runtime.resolve_occupancy_parameters =
        snapshot_occupancy_parameters;
    snapshot->programs_runtime.occupancy_parameter_context = snapshot;
    snapshot->kernel.module_runtime[LXP_MODULE_PROGRAMS] =
        &snapshot->programs_runtime;
    status = lxp_state_store_bind_accounts(
        snapshot->kernel.state, snapshot->programs_runtime.accounts);
    if (status != LXP_OK) return status;
    return lxp_programs_bind_fee_transaction(&snapshot->kernel);
}

lxp_result lxp_kernel_batch_snapshot_create(
    const lxp_kernel *kernel, const lxp_identity_store *identities,
    const lxp_verified_receipt_index *verified_receipts,
    const lxp_kernel_execution *batch_execution,
    lxp_kernel_batch_snapshot **snapshot_out)
{
    const lx_programs_transfer_runtime *runtime;
    lxp_kernel_batch_snapshot *snapshot;
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || identities == NULL ||
        batch_execution == NULL || snapshot_out == NULL ||
        batch_execution->fee_parameters == NULL ||
        batch_execution->batch_number == 0U ||
        identities->count > LXP_IDENTITY_STORE_CAPACITY ||
        (verified_receipts != NULL &&
         verified_receipts->count > LXP_VERIFIED_RECEIPT_INDEX_MAX) ||
        kernel->module_kv_count > LXP_KERNEL_MAX_MODULE_KV ||
        kernel->blob_count > LXP_KERNEL_MAX_BLOBS ||
        kernel->publication_poisoned)
        return LXP_ERR_NON_CANONICAL;
    *snapshot_out = NULL;
    runtime = (const lx_programs_transfer_runtime *)
        kernel->module_runtime[LXP_MODULE_PROGRAMS];
    if (runtime == NULL || runtime->accounts == NULL ||
        runtime->assets == NULL || runtime->asset_count == 0U ||
        runtime->asset_count > LXP_KERNEL_MAX_TRANSFER_ASSETS ||
        runtime->asset_count > SIZE_MAX / sizeof(lxp_transfer_asset_state) ||
        runtime->resolve_metering_schedule == NULL ||
        runtime->resolve_occupancy_parameters == NULL)
        return LXP_ERR_MODULE_DISABLED;
    snapshot = (lxp_kernel_batch_snapshot *)calloc(1U, sizeof(*snapshot));
    if (snapshot == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    snapshot->kernel = *kernel;
    snapshot->fee_parameters = *batch_execution->fee_parameters;
    snapshot->identities = *identities;
    if (verified_receipts != NULL) {
        snapshot->verified_receipts = *verified_receipts;
        snapshot->verified_receipts.fallback = NULL;
        snapshot->verified_receipts.fallback_context = NULL;
    }
    status = lxp_state_snapshot_create(kernel->state, &snapshot->state);
    if (status != LXP_OK) {
        kernel_snapshot_release(snapshot);
        return status;
    }
    snapshot->assets = (lxp_transfer_asset_state *)malloc(
        runtime->asset_count * sizeof(*snapshot->assets));
    if (snapshot->assets == NULL) {
        kernel_snapshot_release(snapshot);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    (void)memcpy(snapshot->assets, runtime->assets,
                 runtime->asset_count * sizeof(*snapshot->assets));
    status = runtime->resolve_metering_schedule(
        runtime->metering_schedule_context,
        batch_execution->recorded_metering_schedule_version,
        batch_execution->batch_number, &snapshot->metering_schedule);
    if (status == LXP_OK)
        status = runtime->resolve_occupancy_parameters(
            runtime->occupancy_parameter_context,
            batch_execution->recorded_fee_schedule_version,
            &snapshot->fee_schedule,
            snapshot->occupancy_asset_id);
    if (status == LXP_OK)
        status = kernel_snapshot_copy_blobs(snapshot, kernel);
    if (status == LXP_OK)
        status = kernel_snapshot_finish(snapshot, runtime);
    if (status != LXP_OK) {
        kernel_snapshot_release(snapshot);
        return status;
    }
    *snapshot_out = snapshot;
    return LXP_OK;
}

lxp_result lxp_kernel_batch_snapshot_clone(
    const lxp_kernel_batch_snapshot *source,
    lxp_kernel_batch_snapshot **snapshot_out)
{
    lxp_kernel_batch_snapshot *snapshot;
    lxp_result status;
    if (source == NULL || source->state == NULL || snapshot_out == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (source->identities.count > LXP_IDENTITY_STORE_CAPACITY ||
        source->verified_receipts.count > LXP_VERIFIED_RECEIPT_INDEX_MAX ||
        source->kernel.module_kv_count > LXP_KERNEL_MAX_MODULE_KV ||
        source->kernel.blob_count > LXP_KERNEL_MAX_BLOBS ||
        source->programs_runtime.asset_count == 0U ||
        source->programs_runtime.asset_count >
            LXP_KERNEL_MAX_TRANSFER_ASSETS ||
        source->programs_runtime.asset_count >
            SIZE_MAX / sizeof(lxp_transfer_asset_state))
        return LXP_FATAL_INVARIANT;
    *snapshot_out = NULL;
    snapshot = (lxp_kernel_batch_snapshot *)calloc(1U, sizeof(*snapshot));
    if (snapshot == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    snapshot->kernel = source->kernel;
    snapshot->identities = source->identities;
    snapshot->verified_receipts = source->verified_receipts;
    snapshot->fee_schedule = source->fee_schedule;
    snapshot->metering_schedule = source->metering_schedule;
    snapshot->fee_parameters = source->fee_parameters;
    snapshot->level_generation = source->level_generation;
    (void)memcpy(snapshot->active_level_token,
                 source->active_level_token, 32U);
    (void)memcpy(snapshot->occupancy_asset_id,
                 source->occupancy_asset_id, 32U);
    status = lxp_state_snapshot_clone(source->state, &snapshot->state);
    if (status == LXP_OK) {
        snapshot->assets = (lxp_transfer_asset_state *)malloc(
            source->programs_runtime.asset_count * sizeof(*snapshot->assets));
        if (snapshot->assets == NULL)
            status = LXP_ERR_ARENA_EXHAUSTED;
        else
            (void)memcpy(snapshot->assets, source->assets,
                         source->programs_runtime.asset_count *
                             sizeof(*snapshot->assets));
    }
    if (status == LXP_OK)
        status = kernel_snapshot_copy_blobs(snapshot, &source->kernel);
    if (status == LXP_OK)
        status = kernel_snapshot_finish(snapshot, &source->programs_runtime);
    if (status != LXP_OK) {
        kernel_snapshot_release(snapshot);
        return status;
    }
    *snapshot_out = snapshot;
    return LXP_OK;
}

void lxp_kernel_batch_snapshot_destroy(lxp_kernel_batch_snapshot *snapshot)
{
    kernel_snapshot_release(snapshot);
}

static lxp_result level_token_mix(uint8_t chain[32], const void *bytes,
                                  size_t length)
{
    uint8_t leaf[32];
    uint8_t input[64];
    lxp_result status = lxp_hash_domain(
        LXP_DOMAIN_CONTEXT_HASH, (const uint8_t *)bytes, length, leaf);
    if (status != LXP_OK) return status;
    (void)memcpy(input, chain, 32U);
    (void)memcpy(input + 32U, leaf, 32U);
    return lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, input, sizeof(input),
                           chain);
}

static lxp_result level_token_u64(uint8_t chain[32], uint64_t value)
{
    uint8_t bytes[8];
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - index * 8U));
    return level_token_mix(chain, bytes, sizeof(bytes));
}

static lxp_result level_token_identity(uint8_t chain[32],
                                       const lxp_identity *identity)
{
    lxp_result status;
#define TOKEN_FIELD(field) do { \
    status = level_token_mix(chain, &(field), sizeof(field)); \
    if (status != LXP_OK) return status; \
} while (0)
#define TOKEN_U64(field) do { \
    status = level_token_u64(chain, (field)); \
    if (status != LXP_OK) return status; \
} while (0)
    TOKEN_FIELD(identity->did_id);
    TOKEN_U64(identity->status);
    TOKEN_FIELD(identity->primary_key);
    TOKEN_FIELD(identity->pending_key);
    TOKEN_FIELD(identity->has_pending_key);
    TOKEN_U64(identity->rotation_announced_at);
    TOKEN_U64(identity->rotation_effective_at);
    TOKEN_U64(identity->rotation_lapse_at);
    TOKEN_U64(identity->rotation_effective_sequence);
    TOKEN_FIELD(identity->superseded_key);
    TOKEN_FIELD(identity->has_superseded_key);
    TOKEN_U64(identity->next_sequence);
    TOKEN_U64(identity->revocation_sequence);
    TOKEN_FIELD(identity->recovery_root);
    TOKEN_U64(identity->recovery_threshold);
    TOKEN_FIELD(identity->recovery_pending_key);
    TOKEN_U64(identity->recovery_approvals);
    TOKEN_U64(identity->recovery_effective_at);
    TOKEN_U64(identity->recovery_lapse_at);
    TOKEN_FIELD(identity->recovery_vetoed);
    TOKEN_FIELD(identity->evm_payout_address);
    TOKEN_FIELD(identity->has_evm_payout_binding);
#undef TOKEN_U64
#undef TOKEN_FIELD
    return LXP_OK;
}

static lxp_result kernel_execution_binding(
    const lxp_kernel_execution *execution, uint8_t binding[32])
{
    uint8_t scalar[16];
    lxp_result status;
    if (execution == NULL || execution->authority == NULL || binding == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(binding, 0, 32U);
#define BIND_U64(value) do { \
    status = level_token_u64(binding, (uint64_t)(value)); \
    if (status != LXP_OK) return status; \
} while (0)
#define BIND_BYTES(bytes, length) do { \
    status = level_token_mix(binding, (bytes), (length)); \
    if (status != LXP_OK) return status; \
} while (0)
    BIND_U64(execution->network_id);
    BIND_U64(execution->batch_number);
    BIND_U64(execution->batch_timestamp_ms);
    BIND_U64(execution->maximum_timestamp_window);
    BIND_U64(execution->epoch);
    BIND_U64(execution->global_sequence);
    BIND_U64(execution->recorded_module_version);
    BIND_U64(execution->recorded_metering_schedule_version);
    BIND_U64(execution->recorded_fee_schedule_version);
    BIND_U64(execution->parameter_version);
    BIND_U64(execution->signature_valid ? 1U : 0U);
    BIND_U64(execution->fee_meter.canonical_encoded_bytes);
    BIND_U64(execution->fee_meter.execution_units);
    BIND_U64(execution->fee_meter.storage_units);
    BIND_U64(execution->fee_meter.exact_program_fee_present ? 1U : 0U);
    BIND_U64(execution->fee_meter.program_fee_schedule_version);
    lxp_u128_to_be(execution->fee_meter.exact_program_fee_units, scalar);
    BIND_BYTES(scalar, sizeof(scalar));
    lxp_u128_to_be(execution->fee_balance, scalar);
    BIND_BYTES(scalar, sizeof(scalar));
    BIND_U64(execution->gas_limit);
    BIND_BYTES(execution->batch_id, 32U);
    BIND_BYTES(execution->activity_root, 32U);
    BIND_BYTES(execution->authority->actor, 32U);
    BIND_BYTES(execution->authority->principal, 32U);
    BIND_U64(execution->authority->kind);
    BIND_BYTES(execution->authority->verified_key, 32U);
    BIND_BYTES(execution->authority->authority_hash, 32U);
#undef BIND_BYTES
#undef BIND_U64
    return LXP_OK;
}

lxp_result lxp_kernel_batch_snapshot_begin_level(
    lxp_kernel_batch_snapshot *snapshot)
{
    static const uint8_t domain[] = "LXP/kernel/parallel-level/v1";
    uint8_t input[32U * 6U + 8U];
    uint8_t state_root[32];
    uint8_t identity_root[32];
    uint8_t receipt_root[32];
    uint8_t schedule_root[32];
    uint8_t asset_root[32];
    uint8_t prior_token[32];
    uint8_t scalar[16];
    size_t offset = 0U;
    size_t index;
    lxp_result status;
    if (snapshot == NULL || snapshot->state == NULL ||
        snapshot->level_generation == UINT64_MAX)
        return LXP_ERR_NON_CANONICAL;
    (void)memset(input, 0, sizeof(input));
    status = lxp_state_root(&snapshot->kernel, state_root);
    (void)memset(identity_root, 0, sizeof(identity_root));
    for (index = 0U; status == LXP_OK &&
                     index < snapshot->identities.count; ++index)
        status = level_token_identity(identity_root,
                                      &snapshot->identities.identities[index]);
    (void)memset(receipt_root, 0, sizeof(receipt_root));
    for (index = 0U; status == LXP_OK &&
                     index < snapshot->verified_receipts.count; ++index) {
        const lxp_verified_receipt_facts *facts =
            &snapshot->verified_receipts.entries[index];
        uint8_t amount[16];
        status = level_token_mix(receipt_root, facts->receipt_digest, 32U);
        if (status == LXP_OK)
            status = level_token_u64(receipt_root,
                                     (uint64_t)(uint32_t)facts->result_code);
        if (status == LXP_OK)
            status = level_token_u64(receipt_root, facts->global_sequence);
        if (status == LXP_OK)
            status = level_token_u64(receipt_root, facts->timestamp);
        if (status == LXP_OK)
            status = level_token_mix(receipt_root, facts->asset, 32U);
        lxp_u128_to_be(facts->amount, amount);
        if (status == LXP_OK)
            status = level_token_mix(receipt_root, amount, sizeof(amount));
        if (status == LXP_OK)
            status = level_token_mix(receipt_root,
                                     facts->resulting_state_root, 32U);
    }
    (void)memset(schedule_root, 0, sizeof(schedule_root));
#define SCHEDULE_U64(value) do { \
    if (status == LXP_OK) status = level_token_u64(schedule_root, (value)); \
} while (0)
    SCHEDULE_U64(snapshot->fee_schedule.version);
    SCHEDULE_U64(snapshot->fee_schedule.cpu);
    SCHEDULE_U64(snapshot->fee_schedule.memory_byte);
    SCHEDULE_U64(snapshot->fee_schedule.storage_read_byte);
    SCHEDULE_U64(snapshot->fee_schedule.storage_write_byte);
    SCHEDULE_U64(snapshot->fee_schedule.output_value);
    SCHEDULE_U64(snapshot->fee_schedule.output_byte);
    SCHEDULE_U64(snapshot->fee_schedule.occupancy_byte_batch);
    SCHEDULE_U64(snapshot->metering_schedule.version);
    for (index = 0U; index < LX_PROGRAMS_METERING_COEFFICIENTS; ++index)
        SCHEDULE_U64(snapshot->metering_schedule.coefficients[index]);
    SCHEDULE_U64(snapshot->metering_schedule.activation_batch);
    SCHEDULE_U64(snapshot->metering_schedule.authority_kind);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root,
                                 snapshot->metering_schedule.authority_digest,
                                 32U);
    SCHEDULE_U64(snapshot->fee_parameters.version);
    lxp_u128_to_be(snapshot->fee_parameters.base_fee, scalar);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root, scalar, sizeof(scalar));
    lxp_u128_to_be(snapshot->fee_parameters.per_activity_type_unit, scalar);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root, scalar, sizeof(scalar));
    lxp_u128_to_be(snapshot->fee_parameters.per_encoded_byte, scalar);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root, scalar, sizeof(scalar));
    lxp_u128_to_be(snapshot->fee_parameters.per_execution_unit, scalar);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root, scalar, sizeof(scalar));
    lxp_u128_to_be(snapshot->fee_parameters.per_storage_unit, scalar);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root, scalar, sizeof(scalar));
    SCHEDULE_U64(snapshot->fee_parameters.multiplier_basis_points);
    if (status == LXP_OK)
        status = level_token_mix(schedule_root,
                                 snapshot->occupancy_asset_id, 32U);
#undef SCHEDULE_U64
    (void)memset(asset_root, 0, sizeof(asset_root));
    for (index = 0U; status == LXP_OK &&
                     index < snapshot->programs_runtime.asset_count; ++index) {
        status = level_token_mix(asset_root,
                                 snapshot->assets[index].asset_id, 32U);
        if (status == LXP_OK)
            status = level_token_u64(asset_root,
                                     snapshot->assets[index].registered ?
                                         1U : 0U);
        if (status == LXP_OK)
            status = level_token_u64(asset_root,
                                     snapshot->assets[index].paused ?
                                         1U : 0U);
    }
    if (status != LXP_OK) return status;
    (void)memcpy(prior_token, snapshot->active_level_token, 32U);
    (void)memcpy(input + offset, domain,
                 sizeof(domain) > 32U ? 32U : sizeof(domain));
    offset += 32U;
    (void)memcpy(input + offset, state_root, 32U); offset += 32U;
    (void)memcpy(input + offset, identity_root, 32U); offset += 32U;
    (void)memcpy(input + offset, receipt_root, 32U); offset += 32U;
    (void)memcpy(input + offset, schedule_root, 32U); offset += 32U;
    (void)memcpy(input + offset, asset_root, 32U); offset += 32U;
    for (index = 0U; index < 8U; ++index)
        input[offset + index] = (uint8_t)(snapshot->level_generation >>
                                          (56U - index * 8U));
    offset += 8U;
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, input, offset,
                             snapshot->active_level_token);
    if (status == LXP_OK &&
        lxp_ct_memcmp(prior_token, snapshot->active_level_token, 32U) == 0)
        return LXP_FATAL_INVARIANT;
    if (status == LXP_OK) ++snapshot->level_generation;
    return status;
}

lxp_result lxp_kernel_batch_schedule_item(
    const lxp_kernel_batch_snapshot *snapshot,
    const lxp_activity *activity, const lxp_kernel_execution *execution,
    lxp_arena *arena, lxp_programs_schedule_item *item)
{
    lxp_kernel_batch_snapshot *view = NULL;
    const lxp_module_registration *registration = NULL;
    lxp_module_ctx ctx;
    lxp_programs_call_schedule_descriptor descriptor;
    lx_account_registry *accounts;
    lx_account *payer = NULL;
    lxp_byte_span encoded;
    uint8_t activity_id[32];
    void *decoded = NULL;
    size_t index;
    lxp_result status;
    if (snapshot == NULL || activity == NULL || execution == NULL ||
        execution->authority == NULL || arena == NULL || item == NULL ||
        activity->activity_type != LX_PROGRAMS_CALL)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_kernel_batch_snapshot_clone(snapshot, &view);
    if (status == LXP_OK)
        status = lxp_module_version_for_epoch(
            &view->kernel, LXP_MODULE_PROGRAMS, execution->epoch,
            execution->recorded_module_version, &registration);
    if (status == LXP_OK)
        status = lxp_activity_encode(activity, arena, &encoded);
    if (status == LXP_OK)
        status = lxp_activity_id(encoded.bytes, encoded.length, activity_id);
    if (status == LXP_OK)
        status = lxp_module_ctx_init(
            &ctx, &view->kernel, LXP_MODULE_PROGRAMS,
            execution->batch_timestamp_ms, execution->epoch,
            execution->global_sequence, execution->gas_limit, arena, false);
    if (status == LXP_OK) {
        ctx.protocol_version = activity->protocol_version;
        ctx.batch_number = execution->batch_number;
        ctx.verified_receipts = &view->verified_receipts;
        status = snapshot_bind_call_admission(
            &ctx, view, execution, activity_id, activity->fee_limit);
    }
    if (status == LXP_OK)
        status = registration->iface->decode(
            &ctx, lxp_activity_type_ordinal(activity->activity_type),
            activity->payload.bytes, activity->payload.length, &decoded);
    if (status == LXP_OK)
        status = registration->iface->validate(
            &ctx, activity, execution->authority, decoded);
    if (status == LXP_OK)
        status = lxp_programs_call_schedule_decode(
            activity, execution->authority, &ctx.call_admission, decoded,
            &descriptor);
    accounts = status == LXP_OK ?
        lxp_state_snapshot_accounts_for_prepare(view->state) : NULL;
    if (status == LXP_OK)
        for (index = 0U; index < accounts->count; ++index)
            if (memcmp(accounts->accounts[index].id,
                       descriptor.payer, 32U) == 0) {
                payer = &accounts->accounts[index];
                break;
            }
    if (status == LXP_OK)
        status = lxp_programs_call_schedule_item_prepare(
            &descriptor,
            payer != NULL && payer->has_asset ? payer->asset_id :
                (const uint8_t[32]){0},
            view->occupancy_asset_id,
            payer != NULL && payer->has_asset, item);
    if (registration != NULL && registration->iface->release != NULL &&
        decoded != NULL)
        registration->iface->release(&ctx, decoded);
    lxp_kernel_batch_snapshot_destroy(view);
    return status;
}

static lxp_result kernel_snapshot_payer_balance(
    const lxp_kernel_batch_snapshot *snapshot,
    const lxp_authority_resolved *authority, lxp_u128 *balance)
{
    const lx_account_registry *accounts;
    size_t index;
    if (snapshot == NULL || authority == NULL || balance == NULL)
        return LXP_ERR_NON_CANONICAL;
    accounts = lxp_state_snapshot_accounts(snapshot->state);
    if (accounts == NULL) return LXP_FATAL_INVARIANT;
    for (index = 0U; index < accounts->count; ++index)
        if (accounts->accounts[index].kind == LX_ACCOUNT_AGENT_MAIN &&
            lxp_ct_memcmp(accounts->accounts[index].id,
                          authority->principal, 32U) == 0) {
            *balance = accounts->accounts[index].balance;
            return LXP_OK;
        }
    return LXP_ERR_AUTH_SCOPE;
}

static const char *const module_names[LXP_MODULE_RESERVED_COUNT] = {
    "asset", "escrow", "budget", "stream", "service", "perps",
    "governance", "bridge", "programs"
};

static bool registration_active(const lxp_module_registration *registration,
                                uint64_t epoch)
{
    return registration->enabled && epoch >= registration->enabled_epoch &&
           epoch < registration->disabled_epoch;
}

lxp_result lxp_kernel_set_fee_transaction(
    lxp_kernel *kernel, const lxp_kernel_fee_transaction *transaction)
{
    if (kernel == NULL || transaction == NULL || transaction->prepare == NULL ||
        transaction->commit == NULL || transaction->rollback == NULL)
        return LXP_ERR_NON_CANONICAL;
    kernel->fee_transaction = *transaction;
    return LXP_OK;
}

lxp_result lxp_kernel_set_supply_checker(lxp_kernel *kernel,
                                         lxp_kernel_supply_checker checker)
{
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    kernel->check_supply = checker;
    return LXP_OK;
}

lxp_result lxp_kernel_set_commit_observer(
    lxp_kernel *kernel, lxp_kernel_commit_observer observer, void *context)
{
    if (kernel == NULL || observer == NULL || context == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (kernel->observe_commit != NULL)
        return LXP_ERR_NON_CANONICAL;
    kernel->observe_commit = observer;
    kernel->commit_observer_context = context;
    return LXP_OK;
}

lxp_result lxp_kernel_clear_commit_observer(
    lxp_kernel *kernel, void *exact_context)
{
    if (kernel == NULL || exact_context == NULL ||
        kernel->observe_commit == NULL ||
        kernel->commit_observer_context != exact_context ||
        kernel->publication_poisoned || kernel->batch_publication_pending)
        return LXP_ERR_NON_CANONICAL;
    kernel->observe_commit = NULL;
    kernel->commit_observer_context = NULL;
    return LXP_OK;
}

lxp_result lxp_kernel_recover_commit_observer(
    lxp_kernel *kernel, const lxp_activity *canonical_activity,
    const lxp_receipt *canonical_receipt)
{
    lxp_result status;
    if (kernel == NULL || canonical_activity == NULL ||
        canonical_receipt == NULL || !kernel->publication_poisoned ||
        kernel->batch_publication_pending ||
        kernel->observe_commit == NULL ||
        canonical_receipt->global_sequence != kernel->poisoned_sequence ||
        lxp_ct_memcmp(canonical_receipt->activity_id,
                      kernel->poisoned_activity_id, 32U) != 0 ||
        lxp_ct_memcmp(canonical_receipt->resulting_state_root,
                      kernel->poisoned_state_root, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = kernel->observe_commit(kernel->commit_observer_context, kernel,
                                    canonical_activity, canonical_receipt);
    if (status != LXP_OK) return status;
    kernel->publication_poisoned = false;
    kernel->poisoned_sequence = 0U;
    (void)memset(kernel->poisoned_activity_id, 0,
                 sizeof(kernel->poisoned_activity_id));
    (void)memset(kernel->poisoned_state_root, 0,
                 sizeof(kernel->poisoned_state_root));
    return LXP_OK;
}

lxp_result lxp_kernel_restore_commit_observer_pending(
    lxp_kernel *kernel, const lxp_activity *canonical_activity,
    const lxp_receipt *canonical_receipt)
{
    uint8_t activity_id[32];
    lxp_result status;
    if (kernel == NULL || canonical_activity == NULL ||
        canonical_receipt == NULL || kernel->publication_poisoned ||
        kernel->batch_publication_pending ||
        kernel->observe_commit == NULL ||
        canonical_receipt->global_sequence == 0U ||
        lxp_ct_is_zero(canonical_receipt->resulting_state_root, 32U))
        return LXP_ERR_NON_CANONICAL;
    {
        lxp_byte_span canonical;
        uint8_t bytes[LXP_MAX_ACTIVITY_BYTES];
        lxp_arena arena;
        status = lxp_arena_init(&arena, bytes, sizeof(bytes));
        if (status == LXP_OK)
            status = lxp_activity_encode(canonical_activity, &arena,
                                         &canonical);
        if (status == LXP_OK)
            status = lxp_activity_id(canonical.bytes, canonical.length,
                                     activity_id);
    }
    if (status != LXP_OK ||
        lxp_ct_memcmp(activity_id, canonical_receipt->activity_id, 32U) != 0)
        return status != LXP_OK ? status : LXP_ERR_CONTEXT_MISMATCH;
    kernel->publication_poisoned = true;
    kernel->poisoned_sequence = canonical_receipt->global_sequence;
    (void)memcpy(kernel->poisoned_activity_id,
                 canonical_receipt->activity_id, 32U);
    (void)memcpy(kernel->poisoned_state_root,
                 canonical_receipt->resulting_state_root, 32U);
    return LXP_OK;
}

static void close_failed_fee_transaction(lxp_kernel *kernel,
                                         void *fee_transaction,
                                         lxp_result status)
{
    if (lxp_result_is_fatal(status))
        kernel->fee_transaction.commit(kernel, fee_transaction);
    else
        kernel->fee_transaction.rollback(kernel, fee_transaction);
}

lxp_result lxp_kernel_bind_module_runtime(lxp_kernel *kernel,
                                          uint16_t module_id,
                                          void *runtime)
{
    lxp_result status;
    if (kernel == NULL || runtime == NULL || module_id == 0U ||
        module_id > LXP_MODULE_RESERVED_COUNT)
        return LXP_ERR_NON_CANONICAL;
    if (module_id == LXP_MODULE_PROGRAMS) {
        lx_programs_transfer_runtime *programs_runtime =
            (lx_programs_transfer_runtime *)runtime;
        if (programs_runtime->accounts == NULL)
            return LXP_ERR_NON_CANONICAL;
        status = lxp_state_store_bind_accounts(
            kernel->state, programs_runtime->accounts);
        if (status != LXP_OK) return status;
        if (programs_runtime->state_feed != NULL) {
            status = lxp_programs_bind_state_feed(
                kernel, programs_runtime->state_feed);
            if (status != LXP_OK) return status;
        }
    }
    kernel->module_runtime[module_id] = runtime;
    if (module_id == LXP_MODULE_PROGRAMS &&
        kernel->fee_transaction.prepare == NULL)
        return lxp_programs_bind_fee_transaction(kernel);
    return LXP_OK;
}

static lxp_result validate_iface(const lxp_module_iface *iface)
{
    const char *expected;
    size_t expected_length;
    const char *terminator;
    size_t i;
    if (iface == NULL || iface->module_id == 0U ||
        iface->module_id > LXP_MODULE_RESERVED_COUNT ||
        iface->abi_version == 0U || iface->name == NULL ||
        iface->activity_types == NULL || iface->activity_type_count == 0U ||
        iface->activity_type_count > LXP_MODULE_MAX_ACTIVITY_TYPES ||
        iface->genesis == NULL || iface->decode == NULL ||
        iface->validate == NULL || iface->execute == NULL ||
        iface->epoch_begin == NULL || iface->epoch_end == NULL ||
        iface->state_root == NULL) return LXP_ERR_UNKNOWN_MODULE;
    terminator = memchr(iface->name, '\0', LXP_MODULE_MAX_NAME + 1U);
    if (terminator == NULL)
        return LXP_ERR_LENGTH_LIMIT;
    expected = module_names[iface->module_id - 1U];
    expected_length = strlen(expected);
    if ((size_t)(terminator - iface->name) != expected_length ||
        memcmp(iface->name, expected, expected_length) != 0)
        return LXP_ERR_UNKNOWN_MODULE;
    for (i = 0U; i < iface->activity_type_count; ++i) {
        if (lxp_activity_module_id(iface->activity_types[i]) !=
            iface->module_id) return LXP_ERR_UNKNOWN_ACTIVITY;
        if (i != 0U && iface->activity_types[i - 1U] >=
            iface->activity_types[i]) return LXP_ERR_UNSORTED_SEQUENCE;
    }
    return LXP_OK;
}

lxp_result lxp_kernel_create(lxp_kernel *kernel, lxp_state_store *state,
                             lxp_state_journal *journal,
                             const void *parameter_set, uint64_t epoch)
{
    if (kernel == NULL || state == NULL || journal == NULL ||
        parameter_set == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(kernel, 0, sizeof(*kernel));
    kernel->state = state;
    kernel->journal = journal;
    kernel->parameter_set = parameter_set;
    kernel->epoch = epoch;
    return LXP_OK;
}

lxp_result lxp_kernel_set_epoch(lxp_kernel *kernel, uint64_t epoch)
{
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    if (epoch < kernel->epoch) return LXP_ERR_TIMESTAMP_REGRESSION;
    kernel->epoch = epoch;
    return LXP_OK;
}

lxp_result lxp_kernel_set_capabilities(
    lxp_kernel *kernel, lxp_kernel_parameter_reader read_parameter,
    lxp_kernel_transfer_applier apply_transfer_set)
{
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    kernel->read_parameter = read_parameter;
    kernel->apply_transfer_set = apply_transfer_set;
    return LXP_OK;
}

static lxp_result program_spend_authorized_set(
    const lxp_transfer_set *set, lxp_transfer_set *authorized,
    lxp_transfer_source_authority
        authorities[LXP_MAX_TRANSFER_SET_LEGS])
{
    const lxp_transfer_leg *leg = NULL;
    size_t authority_index;
    size_t program_spend_count = 0U;
    uint8_t root[32];
    lxp_result status;
    if (set == NULL || authorized == NULL || authorities == NULL ||
        set->leg_count == 0U ||
        set->leg_count > LXP_MAX_TRANSFER_SET_LEGS ||
        set->context.source_authorities == NULL ||
        set->context.source_authority_count == 0U ||
        set->context.source_authority_count > set->leg_count ||
        set->context.source_authority_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    *authorized = *set;
    for (authority_index = 0U;
         authority_index < set->context.source_authority_count;
         ++authority_index) {
        const lxp_transfer_source_authority *authority =
            &set->context.source_authorities[authority_index];
        const lxp_transfer_leg *matching_leg = NULL;
        size_t matching_leg_count = 0U;
        size_t leg_index;
        size_t prior_authority_index;
        for (prior_authority_index = 0U;
             prior_authority_index < authority_index;
             ++prior_authority_index)
            if (lxp_ct_memcmp(
                    authority->authorized_from,
                    set->context.source_authorities[prior_authority_index]
                        .authorized_from,
                    32U) == 0)
                return LXP_ERR_UNAUTHORIZED_DEBIT;
        for (leg_index = 0U; leg_index < set->leg_count; ++leg_index)
            if (set->legs[leg_index].from != NULL &&
                lxp_ct_memcmp(authority->authorized_from,
                              set->legs[leg_index].from->id, 32U) == 0) {
                ++matching_leg_count;
                matching_leg = &set->legs[leg_index];
            }
        if (matching_leg_count != 1U)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        if (authority->debit_authority_kind != LXP_AUTH_PROGRAM_SPEND)
            continue;
        ++program_spend_count;
        leg = matching_leg;
    }
    if (program_spend_count == 0U)
        return set->context.program_spend_token == 0U ?
                   LXP_ERR_UNKNOWN_FIELD : LXP_ERR_UNAUTHORIZED_DEBIT;
    if (program_spend_count != 1U || leg == NULL ||
        set->context.origin_module_id != LXP_MODULE_PROGRAMS ||
        set->context.program_spend_token == 0U ||
        set->context.source_authorities == NULL)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_transfer_set_root(set->legs, set->leg_count, root);
    if (status == LXP_OK)
        status = layerx_programs_consume_program_spend_authorization(
            set->context.program_spend_token,
            set->context.origin_module_id, leg->from->id, leg->to->id,
            leg->asset_id, leg->amount.hi, leg->amount.lo, leg->reason,
            leg->supply_mode, root);
    if (status != LXP_OK) return LXP_ERR_UNAUTHORIZED_DEBIT;
    authorized->context.program_spend_token = 0U;
    authorized->context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(authorities, set->context.source_authorities,
                 set->context.source_authority_count * sizeof(authorities[0]));
    authorized->context.source_authorities = authorities;
    for (authority_index = 0U;
         authority_index < authorized->context.source_authority_count;
         ++authority_index)
        if (authorities[authority_index]
                .debit_authority_kind == LXP_AUTH_PROGRAM_SPEND) {
            authorities[authority_index].debit_authority_kind =
                LXP_AUTH_OWNER;
        }
    return LXP_OK;
}

lxp_result lxp_kernel_canonical_ledger_apply(
    lxp_kernel *kernel, const lxp_transfer_set *set, lxp_receipt *receipt)
{
    lxp_transfer_context context;
    lxp_transfer_set_result result;
    lxp_result status;
    if (kernel == NULL || set == NULL || receipt == NULL)
        return LXP_ERR_NON_CANONICAL;
    context = set->context;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root,
                     result.transfer_set_root, 32U);
    return status;
}

lxp_result lxp_kernel_apply_transfer_set(
    lxp_kernel *kernel, const lxp_transfer_set *set, lxp_receipt *receipt)
{
    lxp_transfer_set authorized;
    lxp_transfer_source_authority authorities[LXP_MAX_TRANSFER_SET_LEGS];
    lxp_result status;
    size_t index;
    bool program_spend = false;
    if (kernel == NULL || set == NULL || receipt == NULL ||
        kernel->apply_transfer_set == NULL)
        return LXP_ERR_BALANCE_BYPASS;
    if (set->leg_count == 0U ||
        set->leg_count > LXP_MAX_TRANSFER_SET_LEGS ||
        set->context.source_authorities == NULL ||
        set->context.source_authority_count == 0U ||
        set->context.source_authority_count > set->leg_count ||
        set->context.source_authority_count > LXP_MAX_TRANSFER_SET_LEGS)
        return LXP_ERR_NON_CANONICAL;
    for (index = 0U; index < set->context.source_authority_count; ++index)
        if (set->context.source_authorities[index].debit_authority_kind ==
            LXP_AUTH_PROGRAM_SPEND)
            program_spend = true;
    if (!program_spend) {
        if (set->context.program_spend_token != 0U)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        return kernel->apply_transfer_set(kernel, set, receipt);
    }
    if (kernel->apply_transfer_set != lxp_kernel_canonical_ledger_apply)
        return LXP_ERR_BALANCE_BYPASS;
    status = program_spend_authorized_set(set, &authorized, authorities);
    if (status != LXP_OK) return status;
    return kernel->apply_transfer_set(kernel, &authorized, receipt);
}

lxp_result lxp_kernel_register_module(lxp_kernel *kernel,
                                      const lxp_module_iface *iface)
{
    lxp_module_registration *registration;
    size_t i;
    lxp_result status;
    if (kernel == NULL) return LXP_ERR_NON_CANONICAL;
    status = validate_iface(iface);
    if (status != LXP_OK) return status;
    for (i = 0U; i < kernel->module_count; ++i) {
        lxp_module_registration *current = &kernel->modules[i];
        if (current->module_id != iface->module_id ||
            !registration_active(current, kernel->epoch)) continue;
        if (iface->abi_version <= current->abi_version)
            return LXP_ERR_VERSION_UNSUPPORTED;
        current->disabled_epoch = kernel->epoch;
    }
    if (kernel->module_count == LXP_KERNEL_MAX_MODULE_REGISTRATIONS)
        return LXP_ERR_ARENA_EXHAUSTED;
    registration = &kernel->modules[kernel->module_count];
    (void)memset(registration, 0, sizeof(*registration));
    registration->iface = iface;
    registration->module_id = iface->module_id;
    registration->abi_version = iface->abi_version;
    (void)memcpy(registration->name, iface->name, strlen(iface->name) + 1U);
    registration->activity_type_count = iface->activity_type_count;
    (void)memcpy(registration->activity_types, iface->activity_types,
                 iface->activity_type_count * sizeof(iface->activity_types[0]));
    registration->enabled_epoch = kernel->epoch;
    registration->disabled_epoch = UINT64_MAX;
    registration->enabled = true;
    ++kernel->module_count;
    return LXP_OK;
}

lxp_result lxp_kernel_module_by_id(
    const lxp_kernel *kernel, uint16_t module_id, uint64_t epoch,
    const lxp_module_registration **registration)
{
    size_t i;
    const lxp_module_registration *found = NULL;
    if (kernel == NULL || registration == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (module_id == 0U || module_id > LXP_MODULE_RESERVED_COUNT)
        return LXP_ERR_UNKNOWN_MODULE;
    for (i = 0U; i < kernel->module_count; ++i) {
        const lxp_module_registration *candidate = &kernel->modules[i];
        if (candidate->module_id == module_id &&
            registration_active(candidate, epoch) &&
            (found == NULL || candidate->abi_version > found->abi_version))
            found = candidate;
    }
    if (found == NULL) return LXP_ERR_MODULE_DISABLED;
    *registration = found;
    return LXP_OK;
}

lxp_result lxp_kernel_module_for_activity(
    const lxp_kernel *kernel, uint32_t activity_type, uint64_t epoch,
    const lxp_module_registration **registration)
{
    const lxp_module_registration *candidate;
    uint16_t module_id = lxp_activity_module_id(activity_type);
    size_t left = 0U;
    size_t right;
    lxp_result status = lxp_kernel_module_by_id(kernel, module_id, epoch,
                                                &candidate);
    if (status != LXP_OK) return status;
    right = candidate->activity_type_count;
    while (left < right) {
        size_t middle = left + (right - left) / 2U;
        if (candidate->activity_types[middle] < activity_type)
            left = middle + 1U;
        else
            right = middle;
    }
    if (left == candidate->activity_type_count ||
        candidate->activity_types[left] != activity_type)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    *registration = candidate;
    return LXP_OK;
}

lxp_result lxp_module_version_for_epoch(
    const lxp_kernel *kernel, uint16_t module_id, uint64_t epoch,
    uint32_t recorded_version,
    const lxp_module_registration **registration)
{
    size_t i;
    if (kernel == NULL || registration == NULL || recorded_version == 0U)
        return LXP_ERR_NON_CANONICAL;
    for (i = 0U; i < kernel->module_count; ++i) {
        const lxp_module_registration *candidate = &kernel->modules[i];
        if (candidate->module_id == module_id &&
            candidate->abi_version == recorded_version &&
            registration_active(candidate, epoch)) {
            *registration = candidate;
            return LXP_OK;
        }
    }
    return LXP_ERR_VERSION_UNSUPPORTED;
}

lxp_result lxp_kernel_dispatch(const lxp_module_registration *registration,
                               lxp_module_ctx *ctx,
                               const lxp_activity *activity,
                               const lxp_authority_resolved *authority,
                               lxp_effect_buffer *effects,
                               lxp_result *module_result)
{
    void *decoded = NULL;
    lxp_result status;
    if (registration == NULL || ctx == NULL || activity == NULL ||
        authority == NULL || effects == NULL || module_result == NULL)
        return LXP_ERR_NON_CANONICAL;
    status = registration->iface->decode(
        ctx, lxp_activity_type_ordinal(activity->activity_type),
        activity->payload.bytes, activity->payload.length, &decoded);
    if (status == LXP_OK) {
        status = lxp_module_ctx_set_mutable(ctx, false);
        if (status == LXP_OK)
            status = registration->iface->validate(ctx, activity, authority,
                                                   decoded);
    }
    if (status == LXP_OK) status = lxp_module_ctx_set_mutable(ctx, true);
    if (status == LXP_OK)
        status = registration->iface->execute(ctx, activity, authority,
                                              decoded, effects);
    if (registration->iface->release != NULL)
        registration->iface->release(ctx, decoded);
    if (lxp_result_is_fatal(status)) return status;
    *module_result = status;
    return LXP_OK;
}

static lxp_result receipt_state_root(const lxp_kernel *kernel,
                                     const lxp_module_ctx *module_ctx,
                                     const lxp_receipt *receipt,
                                     uint8_t root[32])
{
    uint8_t input[32U + 32U + 8U + 4U + 16U + 4U + 32U];
    uint8_t module_root[32];
    size_t offset = 0U;
    size_t i;
    (void)memcpy(input + offset, kernel->current_state_root, 32U);
    offset += 32U;
    (void)memcpy(input + offset, receipt->activity_id, 32U);
    offset += 32U;
    for (i = 0U; i < 8U; ++i)
        input[offset + i] = (uint8_t)(receipt->global_sequence >>
                                     (56U - 8U * i));
    offset += 8U;
    input[offset++] = (uint8_t)((uint32_t)receipt->result_code >> 24U);
    input[offset++] = (uint8_t)((uint32_t)receipt->result_code >> 16U);
    input[offset++] = (uint8_t)((uint32_t)receipt->result_code >> 8U);
    input[offset++] = (uint8_t)(uint32_t)receipt->result_code;
    lxp_u128_to_be(receipt->fee_charged, input + offset);
    offset += 16U;
    input[offset++] = (uint8_t)(receipt->module_version >> 24U);
    input[offset++] = (uint8_t)(receipt->module_version >> 16U);
    input[offset++] = (uint8_t)(receipt->module_version >> 8U);
    input[offset++] = (uint8_t)receipt->module_version;
    if (module_ctx != NULL && module_ctx->commit_prepared) {
        lxp_result status = lxp_module_ctx_preview_root(module_ctx,
                                                        module_root);
        if (status != LXP_OK) return status;
    } else {
        lxp_result status = lxp_state_subtree_root(kernel, receipt->module_id,
                                                   module_root);
        if (status != LXP_OK) return status;
    }
    (void)memcpy(input + offset, module_root, sizeof(module_root));
    offset += sizeof(module_root);
    return lxp_hash_domain(LXP_DOMAIN_RECEIPT, input, offset, root);
}

typedef struct legacy_program_outcome_v2 {
    bool present;
    uint8_t encoding_version;
    uint8_t terminal_kind;
    lxp_result result_code;
    uint16_t runtime_version;
    uint16_t abi_version;
    uint32_t fee_schedule_version;
    uint64_t cpu_fuel;
    uint64_t memory_bytes;
    uint64_t storage_read_bytes;
    uint64_t storage_write_bytes;
    uint32_t output_values;
    uint64_t output_bytes;
    lxp_u128 occupancy_byte_batches;
    lxp_u128 occupancy_fee_units;
    uint64_t fee_schedule_prices[7];
    uint8_t occupancy_asset_id[32];
    uint8_t occupancy_evidence_digest[32];
    uint8_t occupancy_transfer_root[32];
    lxp_u128 fee_units;
    uint8_t call_graph_root[32];
    uint8_t terminal_payload_root[32];
    uint8_t transfer_root[32];
} legacy_program_outcome_v2;

typedef struct compact_receipt {
    uint16_t protocol_version;
    uint8_t activity_id[32];
    uint64_t global_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t activity_root[32];
    lxp_result result_code;
    lxp_u128 fee_charged;
    uint8_t batch_id[32];
    uint16_t module_id;
    uint32_t module_version;
    uint32_t parameter_version;
    legacy_program_outcome_v2 program_outcome;
} compact_receipt;

typedef struct legacy_program_outcome_v1 {
    bool present;
    uint8_t terminal_kind;
    lxp_result result_code;
    uint16_t runtime_version;
    uint16_t abi_version;
    uint32_t fee_schedule_version;
    uint64_t cpu_fuel;
    uint64_t memory_bytes;
    uint64_t storage_read_bytes;
    uint64_t storage_write_bytes;
    uint32_t output_values;
    uint64_t output_bytes;
    lxp_u128 fee_units;
    uint8_t call_graph_root[32];
    uint8_t terminal_payload_root[32];
    uint8_t transfer_root[32];
} legacy_program_outcome_v1;

typedef struct legacy_program_compact_receipt_v1 {
    uint16_t protocol_version;
    uint8_t activity_id[32];
    uint64_t global_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    uint8_t activity_root[32];
    lxp_result result_code;
    lxp_u128 fee_charged;
    uint8_t batch_id[32];
    uint16_t module_id;
    uint32_t module_version;
    uint32_t parameter_version;
    legacy_program_outcome_v1 program_outcome;
} legacy_program_compact_receipt_v1;

typedef struct legacy_compact_receipt {
    uint16_t protocol_version;
    uint8_t activity_id[32];
    uint64_t global_sequence;
    uint8_t previous_state_root[32];
    uint8_t resulting_state_root[32];
    lxp_result result_code;
    lxp_u128 fee_charged;
    uint8_t batch_id[32];
    uint16_t module_id;
    uint32_t module_version;
    uint32_t parameter_version;
} legacy_compact_receipt;

enum {
    COMPACT_RECEIPT_V2_BYTES = 560,
    COMPACT_RECEIPT_V3_BYTES = 564
};
static const uint8_t compact_receipt_v2_magic[5] = {
    'L', 'X', 'R', 'C', '2'
};
static const uint8_t compact_receipt_v3_magic[5] = {
    'L', 'X', 'R', 'C', '3'
};

static void compact_write_u16(uint8_t *bytes, uint16_t value)
{
    bytes[0] = (uint8_t)(value >> 8U);
    bytes[1] = (uint8_t)value;
}

static void compact_write_u32(uint8_t *bytes, uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static void compact_write_u64(uint8_t *bytes, uint64_t value)
{
    size_t index;
    for (index = 0U; index < 8U; ++index)
        bytes[index] = (uint8_t)(value >> (56U - 8U * index));
}

static uint16_t compact_read_u16(const uint8_t *bytes)
{
    return (uint16_t)(((uint16_t)bytes[0] << 8U) | bytes[1]);
}

static uint32_t compact_read_u32(const uint8_t *bytes)
{
    return ((uint32_t)bytes[0] << 24U) | ((uint32_t)bytes[1] << 16U) |
           ((uint32_t)bytes[2] << 8U) | bytes[3];
}

static uint64_t compact_read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

static lxp_result compact_receipt_encode(
    const lxp_receipt *receipt, bool metered, uint8_t *bytes)
{
    const lxp_program_outcome *outcome;
    size_t offset = 0U;
    size_t index;
    if (receipt == NULL || bytes == NULL ||
        !lxp_protocol_version_supported(receipt->protocol_version) ||
        (metered && (!receipt->program_outcome.present ||
                     receipt->program_outcome.encoding_version != 3U)) ||
        (!metered && receipt->program_outcome.present))
        return LXP_ERR_NON_CANONICAL;
    outcome = &receipt->program_outcome;
#define COMPACT_COPY(value, length) do { \
    (void)memcpy(bytes + offset, (value), (length)); \
    offset += (length); \
} while (0)
#define COMPACT_U16(value) do { \
    compact_write_u16(bytes + offset, (value)); offset += 2U; \
} while (0)
#define COMPACT_U32(value) do { \
    compact_write_u32(bytes + offset, (value)); offset += 4U; \
} while (0)
#define COMPACT_U64(value) do { \
    compact_write_u64(bytes + offset, (value)); offset += 8U; \
} while (0)
#define COMPACT_U128(value) do { \
    lxp_result compact_status = lxp_u128_to_be((value), bytes + offset); \
    if (compact_status != LXP_OK) return compact_status; \
    offset += 16U; \
} while (0)
    COMPACT_COPY(metered ? compact_receipt_v3_magic :
                           compact_receipt_v2_magic,
                 sizeof(compact_receipt_v2_magic));
    COMPACT_U16(receipt->protocol_version);
    COMPACT_COPY(receipt->activity_id, 32U);
    COMPACT_U64(receipt->global_sequence);
    COMPACT_COPY(receipt->previous_state_root, 32U);
    COMPACT_COPY(receipt->resulting_state_root, 32U);
    COMPACT_COPY(receipt->activity_root, 32U);
    COMPACT_U32((uint32_t)receipt->result_code);
    COMPACT_U128(receipt->fee_charged);
    COMPACT_COPY(receipt->batch_id, 32U);
    COMPACT_U16(receipt->module_id);
    COMPACT_U32(receipt->module_version);
    COMPACT_U32(receipt->parameter_version);
    bytes[offset++] = outcome->present ? 1U : 0U;
    bytes[offset++] = outcome->encoding_version;
    bytes[offset++] = outcome->terminal_kind;
    COMPACT_U32((uint32_t)outcome->result_code);
    COMPACT_U16(outcome->runtime_version);
    COMPACT_U16(outcome->abi_version);
    COMPACT_U32(outcome->fee_schedule_version);
    if (metered) COMPACT_U32(outcome->metering_schedule_version);
    COMPACT_U64(outcome->cpu_fuel);
    COMPACT_U64(outcome->memory_bytes);
    COMPACT_U64(outcome->storage_read_bytes);
    COMPACT_U64(outcome->storage_write_bytes);
    COMPACT_U32(outcome->output_values);
    COMPACT_U64(outcome->output_bytes);
    COMPACT_U128(outcome->occupancy_byte_batches);
    COMPACT_U128(outcome->occupancy_fee_units);
    for (index = 0U; index < 7U; ++index)
        COMPACT_U64(outcome->fee_schedule_prices[index]);
    COMPACT_COPY(outcome->occupancy_asset_id, 32U);
    COMPACT_COPY(outcome->occupancy_evidence_digest, 32U);
    COMPACT_COPY(outcome->occupancy_transfer_root, 32U);
    COMPACT_U128(outcome->fee_units);
    COMPACT_COPY(outcome->call_graph_root, 32U);
    COMPACT_COPY(outcome->terminal_payload_root, 32U);
    COMPACT_COPY(outcome->transfer_root, 32U);
#undef COMPACT_U128
#undef COMPACT_U64
#undef COMPACT_U32
#undef COMPACT_U16
#undef COMPACT_COPY
    return offset == (metered ? COMPACT_RECEIPT_V3_BYTES :
                                COMPACT_RECEIPT_V2_BYTES) ?
        LXP_OK : LXP_FATAL_INVARIANT;
}

static lxp_result compact_receipt_decode(
    const uint8_t *bytes, size_t length, bool metered,
    lxp_receipt *receipt)
{
    lxp_program_outcome *outcome;
    size_t offset = 0U;
    size_t index;
    if (bytes == NULL || receipt == NULL ||
        length != (metered ? COMPACT_RECEIPT_V3_BYTES :
                             COMPACT_RECEIPT_V2_BYTES) ||
        memcmp(bytes, metered ? compact_receipt_v3_magic :
                                compact_receipt_v2_magic,
               sizeof(compact_receipt_v2_magic)) != 0)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    (void)memset(receipt, 0, sizeof(*receipt));
#define COMPACT_READ_COPY(value, length) do { \
    (void)memcpy((value), bytes + offset, (length)); \
    offset += (length); \
} while (0)
#define COMPACT_READ_U16(value) do { \
    (value) = compact_read_u16(bytes + offset); offset += 2U; \
} while (0)
#define COMPACT_READ_U32(value) do { \
    (value) = compact_read_u32(bytes + offset); offset += 4U; \
} while (0)
#define COMPACT_READ_U64(value) do { \
    (value) = compact_read_u64(bytes + offset); offset += 8U; \
} while (0)
#define COMPACT_READ_U128(value) do { \
    if (lxp_u128_from_be(bytes + offset, &(value)) != LXP_OK) \
        return LXP_FATAL_REPLAY_DIVERGENCE; \
    offset += 16U; \
} while (0)
    offset += sizeof(compact_receipt_v2_magic);
    COMPACT_READ_U16(receipt->protocol_version);
    COMPACT_READ_COPY(receipt->activity_id, 32U);
    COMPACT_READ_U64(receipt->global_sequence);
    COMPACT_READ_COPY(receipt->previous_state_root, 32U);
    COMPACT_READ_COPY(receipt->resulting_state_root, 32U);
    COMPACT_READ_COPY(receipt->activity_root, 32U);
    receipt->result_code =
        (lxp_result)(int32_t)compact_read_u32(bytes + offset);
    offset += 4U;
    COMPACT_READ_U128(receipt->fee_charged);
    COMPACT_READ_COPY(receipt->batch_id, 32U);
    COMPACT_READ_U16(receipt->module_id);
    COMPACT_READ_U32(receipt->module_version);
    COMPACT_READ_U32(receipt->parameter_version);
    outcome = &receipt->program_outcome;
    if (bytes[offset] > 1U) return LXP_FATAL_REPLAY_DIVERGENCE;
    outcome->present = bytes[offset++] != 0U;
    outcome->encoding_version = bytes[offset++];
    outcome->terminal_kind = bytes[offset++];
    outcome->result_code =
        (lxp_result)(int32_t)compact_read_u32(bytes + offset);
    offset += 4U;
    COMPACT_READ_U16(outcome->runtime_version);
    COMPACT_READ_U16(outcome->abi_version);
    COMPACT_READ_U32(outcome->fee_schedule_version);
    outcome->metering_schedule_version =
        LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
    if (metered)
        COMPACT_READ_U32(outcome->metering_schedule_version);
    COMPACT_READ_U64(outcome->cpu_fuel);
    COMPACT_READ_U64(outcome->memory_bytes);
    COMPACT_READ_U64(outcome->storage_read_bytes);
    COMPACT_READ_U64(outcome->storage_write_bytes);
    COMPACT_READ_U32(outcome->output_values);
    COMPACT_READ_U64(outcome->output_bytes);
    COMPACT_READ_U128(outcome->occupancy_byte_batches);
    COMPACT_READ_U128(outcome->occupancy_fee_units);
    for (index = 0U; index < 7U; ++index)
        COMPACT_READ_U64(outcome->fee_schedule_prices[index]);
    COMPACT_READ_COPY(outcome->occupancy_asset_id, 32U);
    COMPACT_READ_COPY(outcome->occupancy_evidence_digest, 32U);
    COMPACT_READ_COPY(outcome->occupancy_transfer_root, 32U);
    COMPACT_READ_U128(outcome->fee_units);
    COMPACT_READ_COPY(outcome->call_graph_root, 32U);
    COMPACT_READ_COPY(outcome->terminal_payload_root, 32U);
    COMPACT_READ_COPY(outcome->transfer_root, 32U);
#undef COMPACT_READ_U128
#undef COMPACT_READ_U64
#undef COMPACT_READ_U32
#undef COMPACT_READ_U16
#undef COMPACT_READ_COPY
    if (offset != length ||
        !lxp_protocol_version_supported(receipt->protocol_version) ||
        (outcome->present && outcome->encoding_version != 1U &&
         outcome->encoding_version != 2U &&
         outcome->encoding_version != 3U) ||
        (outcome->present && receipt->module_id != LXP_MODULE_PROGRAMS) ||
        (receipt->protocol_version == LXP_PROTOCOL_VERSION_OCCUPANCY &&
         outcome->present && outcome->encoding_version != 2U &&
         outcome->encoding_version != 3U) ||
        (receipt->protocol_version == LXP_PROTOCOL_VERSION_LEGACY &&
         outcome->present && outcome->encoding_version != 1U &&
         outcome->encoding_version != 3U) ||
        (metered && outcome->present && outcome->encoding_version != 3U) ||
        (!metered && outcome->encoding_version == 3U) ||
        (outcome->present && outcome->metering_schedule_version == 0U))
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (outcome->present &&
        lxp_program_outcome_validate_for_protocol(
            outcome, receipt->protocol_version) != LXP_OK)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    return LXP_OK;
}

static lxp_result receipt_restore_compact(const uint8_t *bytes, size_t length,
                                          lxp_receipt *receipt)
{
    const compact_receipt *compact;
    if (bytes == NULL || receipt == NULL)
        return LXP_FATAL_REPLAY_DIVERGENCE;
    if (length == COMPACT_RECEIPT_V3_BYTES &&
        memcmp(bytes, compact_receipt_v3_magic,
               sizeof(compact_receipt_v3_magic)) == 0)
        return compact_receipt_decode(bytes, length, true, receipt);
    if (length == COMPACT_RECEIPT_V2_BYTES &&
        memcmp(bytes, compact_receipt_v2_magic,
               sizeof(compact_receipt_v2_magic)) == 0)
        return compact_receipt_decode(bytes, length, false, receipt);
    if (length == sizeof(legacy_compact_receipt)) {
        const legacy_compact_receipt *legacy =
            (const legacy_compact_receipt *)bytes;
        (void)memset(receipt, 0, sizeof(*receipt));
        receipt->protocol_version = legacy->protocol_version;
        (void)memcpy(receipt->activity_id, legacy->activity_id, 32U);
        receipt->global_sequence = legacy->global_sequence;
        (void)memcpy(receipt->previous_state_root,
                     legacy->previous_state_root, 32U);
        (void)memcpy(receipt->resulting_state_root,
                     legacy->resulting_state_root, 32U);
        receipt->result_code = legacy->result_code;
        receipt->fee_charged = legacy->fee_charged;
        (void)memcpy(receipt->batch_id, legacy->batch_id, 32U);
        receipt->module_id = legacy->module_id;
        receipt->module_version = legacy->module_version;
        receipt->parameter_version = legacy->parameter_version;
        return LXP_OK;
    }
    if (length == sizeof(legacy_program_compact_receipt_v1)) {
        const legacy_program_compact_receipt_v1 *legacy =
            (const legacy_program_compact_receipt_v1 *)bytes;
        (void)memset(receipt, 0, sizeof(*receipt));
        receipt->protocol_version = legacy->protocol_version;
        (void)memcpy(receipt->activity_id, legacy->activity_id, 32U);
        receipt->global_sequence = legacy->global_sequence;
        (void)memcpy(receipt->previous_state_root,
                     legacy->previous_state_root, 32U);
        (void)memcpy(receipt->resulting_state_root,
                     legacy->resulting_state_root, 32U);
        (void)memcpy(receipt->activity_root, legacy->activity_root, 32U);
        receipt->result_code = legacy->result_code;
        receipt->fee_charged = legacy->fee_charged;
        (void)memcpy(receipt->batch_id, legacy->batch_id, 32U);
        receipt->module_id = legacy->module_id;
        receipt->module_version = legacy->module_version;
        receipt->parameter_version = legacy->parameter_version;
        receipt->program_outcome.present = legacy->program_outcome.present;
        receipt->program_outcome.encoding_version = 1U;
        receipt->program_outcome.terminal_kind =
            legacy->program_outcome.terminal_kind;
        receipt->program_outcome.result_code =
            legacy->program_outcome.result_code;
        receipt->program_outcome.runtime_version =
            legacy->program_outcome.runtime_version;
        receipt->program_outcome.abi_version =
            legacy->program_outcome.abi_version;
        receipt->program_outcome.fee_schedule_version =
            legacy->program_outcome.fee_schedule_version;
        receipt->program_outcome.metering_schedule_version =
            LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
        receipt->program_outcome.cpu_fuel = legacy->program_outcome.cpu_fuel;
        receipt->program_outcome.memory_bytes =
            legacy->program_outcome.memory_bytes;
        receipt->program_outcome.storage_read_bytes =
            legacy->program_outcome.storage_read_bytes;
        receipt->program_outcome.storage_write_bytes =
            legacy->program_outcome.storage_write_bytes;
        receipt->program_outcome.output_values =
            legacy->program_outcome.output_values;
        receipt->program_outcome.output_bytes =
            legacy->program_outcome.output_bytes;
        receipt->program_outcome.fee_units = legacy->program_outcome.fee_units;
        (void)memcpy(receipt->program_outcome.call_graph_root,
                     legacy->program_outcome.call_graph_root, 32U);
        (void)memcpy(receipt->program_outcome.terminal_payload_root,
                     legacy->program_outcome.terminal_payload_root, 32U);
        (void)memcpy(receipt->program_outcome.transfer_root,
                     legacy->program_outcome.transfer_root, 32U);
        return !receipt->program_outcome.present ||
            lxp_program_outcome_validate_for_protocol(
                &receipt->program_outcome,
                receipt->protocol_version) == LXP_OK ?
                LXP_OK : LXP_FATAL_REPLAY_DIVERGENCE;
    }
    if (length != sizeof(*compact)) return LXP_FATAL_REPLAY_DIVERGENCE;
    compact = (const compact_receipt *)bytes;
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->protocol_version = compact->protocol_version;
    (void)memcpy(receipt->activity_id, compact->activity_id, 32U);
    receipt->global_sequence = compact->global_sequence;
    (void)memcpy(receipt->previous_state_root, compact->previous_state_root,
                 32U);
    (void)memcpy(receipt->resulting_state_root, compact->resulting_state_root,
                 32U);
    (void)memcpy(receipt->activity_root, compact->activity_root, 32U);
    receipt->result_code = compact->result_code;
    receipt->fee_charged = compact->fee_charged;
    (void)memcpy(receipt->batch_id, compact->batch_id, 32U);
    receipt->module_id = compact->module_id;
    receipt->module_version = compact->module_version;
    receipt->parameter_version = compact->parameter_version;
    receipt->program_outcome.present = compact->program_outcome.present;
    receipt->program_outcome.encoding_version =
        compact->program_outcome.encoding_version;
    receipt->program_outcome.terminal_kind =
        compact->program_outcome.terminal_kind;
    receipt->program_outcome.result_code = compact->program_outcome.result_code;
    receipt->program_outcome.runtime_version =
        compact->program_outcome.runtime_version;
    receipt->program_outcome.abi_version = compact->program_outcome.abi_version;
    receipt->program_outcome.fee_schedule_version =
        compact->program_outcome.fee_schedule_version;
    receipt->program_outcome.metering_schedule_version =
        LXP_PROGRAM_METERING_SCHEDULE_VERSION_V1;
    receipt->program_outcome.cpu_fuel = compact->program_outcome.cpu_fuel;
    receipt->program_outcome.memory_bytes = compact->program_outcome.memory_bytes;
    receipt->program_outcome.storage_read_bytes =
        compact->program_outcome.storage_read_bytes;
    receipt->program_outcome.storage_write_bytes =
        compact->program_outcome.storage_write_bytes;
    receipt->program_outcome.output_values =
        compact->program_outcome.output_values;
    receipt->program_outcome.output_bytes = compact->program_outcome.output_bytes;
    receipt->program_outcome.occupancy_byte_batches =
        compact->program_outcome.occupancy_byte_batches;
    receipt->program_outcome.occupancy_fee_units =
        compact->program_outcome.occupancy_fee_units;
    (void)memcpy(receipt->program_outcome.fee_schedule_prices,
                 compact->program_outcome.fee_schedule_prices,
                 sizeof(receipt->program_outcome.fee_schedule_prices));
    (void)memcpy(receipt->program_outcome.occupancy_asset_id,
                 compact->program_outcome.occupancy_asset_id, 32U);
    (void)memcpy(receipt->program_outcome.occupancy_evidence_digest,
                 compact->program_outcome.occupancy_evidence_digest, 32U);
    (void)memcpy(receipt->program_outcome.occupancy_transfer_root,
                 compact->program_outcome.occupancy_transfer_root, 32U);
    receipt->program_outcome.fee_units = compact->program_outcome.fee_units;
    (void)memcpy(receipt->program_outcome.call_graph_root,
                 compact->program_outcome.call_graph_root, 32U);
    (void)memcpy(receipt->program_outcome.terminal_payload_root,
                 compact->program_outcome.terminal_payload_root, 32U);
    (void)memcpy(receipt->program_outcome.transfer_root,
                 compact->program_outcome.transfer_root, 32U);
    return !receipt->program_outcome.present ||
        lxp_program_outcome_validate_for_protocol(
            &receipt->program_outcome,
            receipt->protocol_version) == LXP_OK ?
            LXP_OK : LXP_FATAL_REPLAY_DIVERGENCE;
}

static lxp_result receipt_store(lxp_state_journal *journal,
                                const lxp_activity *activity,
                                const lxp_receipt *receipt)
{
    uint8_t compact[COMPACT_RECEIPT_V3_BYTES];
    lxp_result status;
    if (!lxp_protocol_version_supported(receipt->protocol_version) ||
        (receipt->program_outcome.present &&
         receipt->program_outcome.encoding_version != 3U))
        return LXP_ERR_VERSION_UNSUPPORTED;
    if (receipt->protocol_version == LXP_PROTOCOL_VERSION_LEGACY &&
        !receipt->program_outcome.present) {
        legacy_program_compact_receipt_v1 legacy;
        (void)memset(&legacy, 0, sizeof(legacy));
        legacy.protocol_version = receipt->protocol_version;
        (void)memcpy(legacy.activity_id, receipt->activity_id, 32U);
        legacy.global_sequence = receipt->global_sequence;
        (void)memcpy(legacy.previous_state_root,
                     receipt->previous_state_root, 32U);
        (void)memcpy(legacy.resulting_state_root,
                     receipt->resulting_state_root, 32U);
        (void)memcpy(legacy.activity_root, receipt->activity_root, 32U);
        legacy.result_code = receipt->result_code;
        legacy.fee_charged = receipt->fee_charged;
        (void)memcpy(legacy.batch_id, receipt->batch_id, 32U);
        legacy.module_id = receipt->module_id;
        legacy.module_version = receipt->module_version;
        legacy.parameter_version = receipt->parameter_version;
        return lxp_idempotency_record(
            journal, activity->actor_did.bytes, activity->actor_did.length,
            activity->idempotency_key,
            (const uint8_t *)&legacy, sizeof(legacy));
    }
    status = compact_receipt_encode(
        receipt, receipt->program_outcome.present, compact);
    if (status != LXP_OK) return status;
    return lxp_idempotency_record(journal, activity->actor_did.bytes,
                                  activity->actor_did.length,
                                  activity->idempotency_key,
                                  compact,
                                  receipt->program_outcome.present ?
                                      COMPACT_RECEIPT_V3_BYTES :
                                      COMPACT_RECEIPT_V2_BYTES);
}

static bool activity_declared(const lxp_module_registration *registration,
                              uint32_t activity_type)
{
    size_t i;
    for (i = 0U; i < registration->activity_type_count; ++i)
        if (registration->activity_types[i] == activity_type) return true;
    return false;
}

static void store_u32(uint8_t bytes[4], uint32_t value)
{
    bytes[0] = (uint8_t)(value >> 24U);
    bytes[1] = (uint8_t)(value >> 16U);
    bytes[2] = (uint8_t)(value >> 8U);
    bytes[3] = (uint8_t)value;
}

static lxp_result synthesize_program_call_failure(
    const lxp_activity *activity, const uint8_t activity_id[32],
    const lxp_kernel_execution *execution, lxp_result module_result,
    lxp_u128 pre_runtime_fee, uint32_t fee_schedule_version,
    uint32_t metering_schedule_version,
    lxp_program_outcome *outcome)
{
    static const uint8_t graph_domain[] =
        "LXP/programs/empty-call-graph/v1";
    static const uint8_t failure_domain[] =
        "LXP/programs/pre-runtime-failure/v1";
    uint8_t payload_hash[32];
    uint8_t failure_input[sizeof(failure_domain) + 32U + 32U + 4U + 4U + 4U];
    size_t offset = 0U;
    lxp_result status;
    if (activity == NULL || activity_id == NULL || execution == NULL ||
        outcome == NULL || module_result == LXP_OK ||
        lxp_result_is_fatal(module_result))
        return LXP_FATAL_INVARIANT;
    status = lxp_hash_payload(activity->payload.bytes,
                              activity->payload.length, payload_hash);
    if (status != LXP_OK) return status;
    (void)memset(outcome, 0, sizeof(*outcome));
    outcome->present = true;
    outcome->encoding_version = 3U;
    outcome->terminal_kind = LXP_PROGRAM_TERMINAL_FAILURE;
    outcome->result_code = module_result;
    outcome->runtime_version = 1U;
    outcome->abi_version = 1U;
    if (activity->payload.bytes != NULL && activity->payload.length >= 34U) {
        uint16_t requested_abi =
            (uint16_t)(((uint16_t)activity->payload.bytes[32] << 8U) |
                       activity->payload.bytes[33]);
        if (requested_abi != 0U) outcome->abi_version = requested_abi;
    }
    outcome->fee_schedule_version = fee_schedule_version;
    outcome->metering_schedule_version = metering_schedule_version;
    outcome->cpu_fuel = execution->fee_meter.execution_units;
    outcome->storage_write_bytes = execution->fee_meter.storage_units;
    outcome->fee_units = pre_runtime_fee;
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, graph_domain,
                             sizeof(graph_domain), outcome->call_graph_root);
    if (status != LXP_OK) return status;
    (void)memcpy(failure_input + offset, failure_domain,
                 sizeof(failure_domain));
    offset += sizeof(failure_domain);
    (void)memcpy(failure_input + offset, activity_id, 32U);
    offset += 32U;
    (void)memcpy(failure_input + offset, payload_hash, 32U);
    offset += 32U;
    store_u32(failure_input + offset, (uint32_t)module_result);
    offset += 4U;
    store_u32(failure_input + offset, execution->recorded_module_version);
    offset += 4U;
    store_u32(failure_input + offset, execution->parameter_version);
    offset += 4U;
    return lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, failure_input, offset,
                           outcome->terminal_payload_root);
}

void lxp_prepared_transition_destroy(lxp_prepared_transition *prepared)
{
    if (prepared == NULL) return;
    lxp_prepared_module_transition_destroy(prepared->module);
    free(prepared);
}

lxp_result lxp_kernel_prepare_activity(
    const lxp_kernel_batch_snapshot *snapshot,
    const lxp_activity *activity, const lxp_kernel_execution *execution,
    lxp_arena *worker_arena, lxp_prepared_transition **prepared_out)
{
    lxp_kernel_batch_snapshot *work = NULL;
    lxp_prepared_transition *prepared = NULL;
    const lxp_module_registration *registration;
    lxp_kernel_execution private_execution;
    lxp_identity *identity;
    const uint8_t *prior_receipt;
    size_t prior_receipt_length;
    lxp_admission_context admission_context;
    lxp_admission_result admission;
    lxp_fee_policy_decision admission_policy;
    lxp_fee_policy_decision fee_policy;
    lxp_module_ctx module_ctx;
    lxp_effect_buffer effects;
    lxp_result module_result;
    lxp_result status;
    lxp_u128 pre_runtime_fee;
    lxp_u128 fee;
    lxp_fee_meter actual_fee_meter;
    const lxp_program_outcome *program_outcome;
    lxp_program_outcome synthetic_outcome;
    lxp_byte_span encoded;
    bool module_ctx_initialized = false;
    if (snapshot == NULL || activity == NULL || execution == NULL ||
        worker_arena == NULL || prepared_out == NULL ||
        execution->authority == NULL || execution->fee_parameters == NULL ||
        execution->global_sequence == 0U ||
        lxp_ct_is_zero(snapshot->active_level_token, 32U) ||
        lxp_activity_module_id(activity->activity_type) !=
            LXP_MODULE_PROGRAMS ||
        lxp_activity_type_ordinal(activity->activity_type) != 3U)
        return LXP_ERR_NON_CANONICAL;
    *prepared_out = NULL;
    status = lxp_kernel_batch_snapshot_clone(snapshot, &work);
    if (status != LXP_OK) return status;
    prepared = (lxp_prepared_transition *)calloc(1U, sizeof(*prepared));
    if (prepared == NULL) {
        lxp_kernel_batch_snapshot_destroy(work);
        return LXP_ERR_ARENA_EXHAUSTED;
    }
    private_execution = *execution;
    private_execution.fee_parameters = &work->fee_parameters;
    private_execution.identities = &work->identities;
    private_execution.verified_receipts = &work->verified_receipts;
    private_execution.arena = worker_arena;
    private_execution.sequencer_private_key = NULL;
    private_execution.canonical_events_out = NULL;
    status = lxp_module_version_for_epoch(
        &work->kernel, LXP_MODULE_PROGRAMS, execution->epoch,
        execution->recorded_module_version, &registration);
    if (status == LXP_OK &&
        !activity_declared(registration, activity->activity_type))
        status = LXP_ERR_UNKNOWN_ACTIVITY;
    if (status == LXP_OK)
        status = lxp_identity_resolve(&work->identities,
                                      activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK)
        status = lxp_idempotency_lookup(
            work->kernel.state, activity->actor_did.bytes,
            activity->actor_did.length, activity->idempotency_key,
            &prior_receipt, &prior_receipt_length);
    if (status != LXP_OK) goto done;
    admission_context = (lxp_admission_context){
        execution->network_id, execution->batch_timestamp_ms,
        execution->maximum_timestamp_window, identity->next_sequence,
        execution->signature_valid, false,
        lxp_u128_cmp(execution->fee_balance, activity->fee_limit) >= 0
    };
    admission = lxp_admit_activity(activity, &admission_context);
    status = lxp_fee_admission_check(admission, activity->fee_limit,
                                     execution->fee_balance,
                                     &admission_policy);
    if (status == LXP_OK && admission_policy.result_code != LXP_OK)
        status = admission_policy.result_code;
    if (status == LXP_OK)
        status = lxp_fee_compute(&work->fee_parameters,
                                 activity->activity_type,
                                 execution->fee_meter, &pre_runtime_fee);
    fee = (lxp_u128){0U, 0U};
    if (status == LXP_OK)
        status = lxp_fee_rejection_policy(
            &admission_policy, LXP_OK, fee, activity->fee_limit,
            &fee_policy);
    if (status == LXP_OK)
        status = lxp_activity_encode(activity, worker_arena, &encoded);
    if (status == LXP_OK)
        status = lxp_activity_id(encoded.bytes, encoded.length,
                                 prepared->activity_id);
    if (status != LXP_OK) goto done;
    (void)memset(&effects, 0, sizeof(effects));
    status = lxp_effect_buffer_init(&effects);
    module_result = fee_policy.result_code;
    if (status == LXP_OK && fee_policy.apply_module_effects) {
        status = lxp_module_ctx_init(
            &module_ctx, &work->kernel, registration->module_id,
            execution->batch_timestamp_ms, execution->epoch,
            execution->global_sequence, execution->gas_limit,
            worker_arena, true);
        if (status == LXP_OK) module_ctx_initialized = true;
        if (status == LXP_OK) {
            module_ctx.protocol_version = activity->protocol_version;
            module_ctx.batch_number = execution->batch_number;
            module_ctx.verified_receipts = &work->verified_receipts;
            status = snapshot_bind_call_admission(
                &module_ctx, work, execution, prepared->activity_id,
                activity->fee_limit);
            if (status == LXP_OK)
                status = lxp_module_ctx_bind_effects(&module_ctx, &effects);
        }
        if (status == LXP_OK)
            status = lxp_kernel_dispatch(registration, &module_ctx, activity,
                                         execution->authority, &effects,
                                         &module_result);
        program_outcome = status == LXP_OK ?
            lxp_ctx_program_outcome(&module_ctx) : NULL;
        if (status == LXP_OK && program_outcome == NULL &&
            module_result != LXP_OK && !lxp_result_is_fatal(module_result)) {
            status = synthesize_program_call_failure(
                activity, prepared->activity_id, &private_execution,
                module_result, pre_runtime_fee, work->fee_schedule.version,
                work->metering_schedule.version, &synthetic_outcome);
            if (status == LXP_OK) {
                synthetic_outcome.fee_schedule_version =
                    work->fee_schedule.version;
                synthetic_outcome.metering_schedule_version =
                    work->metering_schedule.version;
                (void)memcpy(synthetic_outcome.fee_schedule_prices,
                             module_ctx.call_admission.fee_schedule_prices,
                             sizeof(synthetic_outcome.fee_schedule_prices));
                status = lxp_ctx_bind_program_outcome(&module_ctx,
                                                      &synthetic_outcome);
                program_outcome = &module_ctx.program_outcome;
            }
        } else if (status == LXP_OK && program_outcome == NULL) {
            status = LXP_FATAL_INVARIANT;
        }
        if (status == LXP_OK) {
            actual_fee_meter = execution->fee_meter;
            actual_fee_meter.exact_program_fee_present = true;
            actual_fee_meter.program_fee_schedule_version =
                program_outcome->fee_schedule_version;
            actual_fee_meter.exact_program_fee_units =
                program_outcome->fee_units;
            status = lxp_fee_compute(&work->fee_parameters,
                                     activity->activity_type,
                                     actual_fee_meter, &fee);
        }
        if (status == LXP_OK)
            status = lxp_fee_rejection_policy(
                &admission_policy, module_result, fee, activity->fee_limit,
                &fee_policy);
        if (status == LXP_OK && !fee_policy.apply_module_effects) {
            lxp_module_ctx_rollback(&module_ctx);
            status = lxp_effect_buffer_init(&effects);
            module_ctx.next_effect_ordinal = 0U;
        }
    }
    if (status == LXP_OK && module_ctx_initialized)
        status = lxp_module_ctx_export_prepared(
            &module_ctx, &effects, snapshot->active_level_token,
            &prepared->module);
    if (status == LXP_OK && prepared->module == NULL)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK) {
        prepared->result_code = fee_policy.result_code;
        prepared->fee_charged = fee_policy.fee_charged;
        prepared->protocol_version = activity->protocol_version;
        prepared->module_id = registration->module_id;
        prepared->module_version = registration->abi_version;
        prepared->parameter_version = execution->parameter_version;
        prepared->fee_schedule_version = work->fee_schedule.version;
        prepared->metering_schedule_version = work->metering_schedule.version;
        (void)memcpy(prepared->level_snapshot_token,
                     snapshot->active_level_token, 32U);
        status = kernel_execution_binding(execution,
                                          prepared->execution_binding);
    }
    if (status == LXP_OK) {
        *prepared_out = prepared;
        prepared = NULL;
    }
done:
    if (module_ctx_initialized) lxp_module_ctx_rollback(&module_ctx);
    lxp_prepared_transition_destroy(prepared);
    lxp_kernel_batch_snapshot_destroy(work);
    return status;
}

static lxp_result kernel_snapshot_replace(
    lxp_kernel_batch_snapshot *target,
    lxp_kernel_batch_snapshot *replacement)
{
    lxp_kernel_batch_snapshot *retired =
        (lxp_kernel_batch_snapshot *)malloc(sizeof(*retired));
    if (retired == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    *retired = *target;
    *target = *replacement;
    free(replacement);
    target->kernel.state = lxp_state_snapshot_store_for_prepare(target->state);
    target->kernel.journal = &target->journal;
    target->programs_runtime.accounts =
        lxp_state_snapshot_accounts_for_prepare(target->state);
    target->programs_runtime.metering_schedule_context = target;
    target->programs_runtime.occupancy_parameter_context = target;
    target->kernel.module_runtime[LXP_MODULE_PROGRAMS] =
        &target->programs_runtime;
    kernel_snapshot_release(retired);
    return LXP_OK;
}

lxp_result lxp_kernel_snapshot_apply_prepared(
    lxp_kernel_batch_snapshot *snapshot, const lxp_activity *activity,
    const lxp_kernel_execution *execution,
    const lxp_prepared_transition *prepared, lxp_receipt *receipt,
    lxp_byte_span *canonical_events)
{
    lxp_kernel_batch_snapshot *candidate = NULL;
    const lxp_module_registration *registration;
    lxp_module_ctx module_ctx;
    lxp_effect_buffer effects;
    lxp_identity *identity;
    lxp_byte_span encoded;
    lxp_byte_span projected = {NULL, 0U};
    lxp_result status;
    uint8_t execution_binding[32];
    bool module_ctx_initialized = false;
    bool fee_transaction_open = false;
    void *fee_transaction = NULL;
    if (snapshot == NULL || activity == NULL || execution == NULL ||
        prepared == NULL || prepared->module == NULL || receipt == NULL ||
        execution->authority == NULL || execution->fee_parameters == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (canonical_events != NULL)
        *canonical_events = (lxp_byte_span){NULL, 0U};
    status = kernel_execution_binding(execution, execution_binding);
    if (status != LXP_OK) return status;
    if (prepared->protocol_version != activity->protocol_version ||
        prepared->parameter_version != execution->parameter_version ||
        prepared->module_version != execution->recorded_module_version ||
        prepared->fee_schedule_version !=
            execution->recorded_fee_schedule_version ||
        prepared->metering_schedule_version !=
            execution->recorded_metering_schedule_version ||
        lxp_ct_is_zero(snapshot->active_level_token, 32U) ||
        lxp_ct_memcmp(prepared->level_snapshot_token,
                      snapshot->active_level_token, 32U) != 0 ||
        lxp_ct_memcmp(prepared->execution_binding,
                      execution_binding, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_activity_encode(activity, execution->arena, &encoded);
    if (status == LXP_OK) {
        uint8_t activity_id[32];
        status = lxp_activity_id(encoded.bytes, encoded.length, activity_id);
        if (status == LXP_OK &&
            lxp_ct_memcmp(activity_id, prepared->activity_id, 32U) != 0)
            status = LXP_ERR_CONTEXT_MISMATCH;
    }
    if (status == LXP_OK)
        status = lxp_kernel_batch_snapshot_clone(snapshot, &candidate);
    if (status == LXP_OK)
        status = lxp_identity_resolve(&candidate->identities,
                                      activity->actor_did.bytes,
                                      activity->actor_did.length, &identity);
    if (status == LXP_OK)
        status = lxp_identity_consume_sequence(identity,
                                               activity->account_sequence);
    if (status == LXP_OK)
        status = lxp_module_version_for_epoch(
            &candidate->kernel, prepared->module_id, execution->epoch,
            prepared->module_version, &registration);
    if (status == LXP_OK)
        status = lxp_state_journal_open(candidate->kernel.state,
                                        execution->global_sequence,
                                        candidate->kernel.journal);
    if (status == LXP_OK && prepared->module != NULL) {
        status = lxp_effect_buffer_init(&effects);
        if (status == LXP_OK)
            status = lxp_module_ctx_init(
                &module_ctx, &candidate->kernel, prepared->module_id,
                execution->batch_timestamp_ms, execution->epoch,
                execution->global_sequence, execution->gas_limit,
                execution->arena, true);
        if (status == LXP_OK) module_ctx_initialized = true;
        if (status == LXP_OK) {
            module_ctx.protocol_version = activity->protocol_version;
            module_ctx.batch_number = execution->batch_number;
            module_ctx.verified_receipts = &candidate->verified_receipts;
            status = snapshot_bind_call_admission(
                &module_ctx, candidate, execution, prepared->activity_id,
                activity->fee_limit);
            if (status == LXP_OK)
                status = lxp_module_ctx_bind_effects(&module_ctx, &effects);
        }
        if (status == LXP_OK)
            status = lxp_module_ctx_import_prepared(
                &module_ctx, prepared->module,
                candidate->active_level_token, &effects);
    } else if (status == LXP_OK) {
        status = lxp_effect_buffer_init(&effects);
    }
    if (status == LXP_OK && !lxp_u128_is_zero(prepared->fee_charged)) {
        status = candidate->kernel.fee_transaction.prepare(
            &candidate->kernel, activity, execution->authority,
            prepared->fee_charged, &fee_transaction);
        fee_transaction_open = status == LXP_OK;
        if (status == LXP_OK && fee_transaction == NULL)
            status = LXP_FATAL_INVARIANT;
    }
    if (status == LXP_OK) {
        (void)memset(receipt, 0, sizeof(*receipt));
        receipt->protocol_version = prepared->protocol_version;
        (void)memcpy(receipt->activity_id, prepared->activity_id, 32U);
        receipt->global_sequence = execution->global_sequence;
        (void)memcpy(receipt->previous_state_root,
                     candidate->kernel.current_state_root, 32U);
        receipt->result_code = prepared->result_code;
        receipt->fee_charged = prepared->fee_charged;
        receipt->module_id = prepared->module_id;
        receipt->module_version = prepared->module_version;
        receipt->parameter_version = prepared->parameter_version;
        (void)memcpy(receipt->batch_id, execution->batch_id, 32U);
        (void)memcpy(receipt->activity_root, execution->activity_root, 32U);
        receipt->effects = effects;
        status = receipt_state_root(
            &candidate->kernel,
            module_ctx_initialized ? &module_ctx : NULL,
            receipt, receipt->resulting_state_root);
    }
    if (status == LXP_OK)
        status = lxp_receipt_build(
            receipt, receipt->activity_id, receipt->global_sequence,
            receipt->previous_state_root, receipt->resulting_state_root,
            receipt->activity_root, receipt->result_code, &effects,
            receipt->fee_charged, receipt->batch_id, receipt->module_id,
            receipt->module_version, receipt->parameter_version);
    if (status == LXP_OK) receipt->timestamp = execution->batch_timestamp_ms;
    if (status == LXP_OK && module_ctx_initialized) {
        const lxp_program_outcome *outcome =
            lxp_ctx_program_outcome(&module_ctx);
        if (outcome == NULL) status = LXP_FATAL_INVARIANT;
        else {
            status = lxp_receipt_bind_program_outcome(receipt, outcome);
            if (status == LXP_OK &&
                outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS)
                (void)memcpy(receipt->transfer_set_root,
                             outcome->transfer_root, 32U);
        }
    }
    if (status == LXP_OK && execution->sequencer_private_key != NULL)
        status = lxp_receipt_sign(receipt,
                                  execution->sequencer_private_key,
                                  execution->arena);
    if (status == LXP_OK)
        status = receipt_store(candidate->kernel.journal, activity, receipt);
    if (status == LXP_OK && module_ctx_initialized &&
        receipt->program_outcome.terminal_kind ==
            LXP_PROGRAM_TERMINAL_SUCCESS && canonical_events != NULL)
        status = lxp_programs_project_committed_events(
            &effects, execution->arena, &projected);
    if (status == LXP_OK)
        status = lxp_state_journal_commit(candidate->kernel.journal);
    if (status == LXP_OK && module_ctx_initialized)
        status = lxp_module_ctx_commit(&module_ctx);
    if (status == LXP_OK && fee_transaction_open) {
        candidate->kernel.fee_transaction.commit(&candidate->kernel,
                                                  fee_transaction);
        fee_transaction_open = false;
        fee_transaction = NULL;
    }
    if (status == LXP_OK) {
        (void)memcpy(candidate->kernel.current_state_root,
                     receipt->resulting_state_root, 32U);
        module_ctx_initialized = false;
        status = kernel_snapshot_replace(snapshot, candidate);
        if (status == LXP_OK) {
            candidate = NULL;
            if (canonical_events != NULL) *canonical_events = projected;
        }
    }
    if (module_ctx_initialized) lxp_module_ctx_rollback(&module_ctx);
    if (fee_transaction_open)
        candidate->kernel.fee_transaction.rollback(&candidate->kernel,
                                                    fee_transaction);
    if (candidate != NULL && candidate->journal.open)
        (void)lxp_state_journal_rollback(&candidate->journal);
    lxp_kernel_batch_snapshot_destroy(candidate);
    return status;
}

static bool kernel_snapshot_matches_live(
    const lxp_kernel_batch_snapshot *base, const lxp_kernel *live,
    const lxp_identity_store *identities)
{
    size_t index;
    if (base == NULL || live == NULL || identities == NULL ||
        live->module_kv_count != base->kernel.module_kv_count ||
        live->blob_count != base->kernel.blob_count ||
        live->blob_total_bytes != base->kernel.blob_total_bytes ||
        identities->count != base->identities.count ||
        lxp_ct_memcmp(live->current_state_root,
                      base->kernel.current_state_root, 32U) != 0 ||
        memcmp(live->module_kv, base->kernel.module_kv,
               live->module_kv_count * sizeof(live->module_kv[0])) != 0 ||
        memcmp(identities->identities, base->identities.identities,
               identities->count * sizeof(identities->identities[0])) != 0)
        return false;
    for (index = 0U; index < live->blob_count; ++index)
        if (live->blobs[index].module_id !=
                base->kernel.blobs[index].module_id ||
            live->blobs[index].length != base->kernel.blobs[index].length ||
            lxp_ct_memcmp(live->blobs[index].key,
                          base->kernel.blobs[index].key, 32U) != 0 ||
            live->blobs[index].bytes == NULL ||
            base->kernel.blobs[index].bytes == NULL ||
            memcmp(live->blobs[index].bytes,
                   base->kernel.blobs[index].bytes,
                   live->blobs[index].length) != 0)
            return false;
    return true;
}

static lxp_result kernel_settled_snapshot_validate(
    const lxp_kernel_batch_snapshot *settled)
{
    const lxp_state_store *store;
    lxp_receipt latest;
    bool have_latest = false;
    size_t index;
    size_t other;
    uint8_t canonical_root[32];
    lxp_result status;
    if (settled == NULL ||
        settled->identities.count > LXP_IDENTITY_STORE_CAPACITY ||
        settled->verified_receipts.count > LXP_VERIFIED_RECEIPT_INDEX_MAX)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < settled->identities.count; ++index) {
        const lxp_identity *identity = &settled->identities.identities[index];
        if (lxp_ct_is_zero(identity->did_id, 32U) ||
            identity->status < LXP_IDENTITY_ACTIVE ||
            identity->status > LXP_IDENTITY_RETIRED)
            return LXP_FATAL_INVARIANT;
        for (other = index + 1U; other < settled->identities.count; ++other)
            if (lxp_ct_memcmp(identity->did_id,
                              settled->identities.identities[other].did_id,
                              32U) == 0)
                return LXP_FATAL_INVARIANT;
    }
    for (index = 0U; index < settled->kernel.module_kv_count; ++index) {
        const lxp_module_kv_entry *entry =
            &settled->kernel.module_kv[index];
        if (entry->module_id == 0U ||
            entry->module_id > LXP_MODULE_RESERVED_COUNT ||
            entry->key_length == 0U ||
            entry->key_length > LXP_MODULE_MAX_KEY_BYTES ||
            entry->value_length > LXP_MODULE_MAX_VALUE_BYTES)
            return LXP_FATAL_INVARIANT;
        for (other = index + 1U;
             other < settled->kernel.module_kv_count; ++other)
            if (entry->module_id ==
                    settled->kernel.module_kv[other].module_id &&
                entry->key_length ==
                    settled->kernel.module_kv[other].key_length &&
                memcmp(entry->key, settled->kernel.module_kv[other].key,
                       entry->key_length) == 0)
                return LXP_FATAL_INVARIANT;
    }
    for (index = 0U; index < settled->kernel.blob_count; ++index) {
        const lxp_module_blob *blob = &settled->kernel.blobs[index];
        if (blob->module_id == 0U ||
            blob->module_id > LXP_MODULE_RESERVED_COUNT ||
            blob->length == 0U || blob->bytes == NULL)
            return LXP_FATAL_INVARIANT;
        for (other = index + 1U; other < settled->kernel.blob_count; ++other)
            if (blob->module_id == settled->kernel.blobs[other].module_id &&
                lxp_ct_memcmp(blob->key,
                              settled->kernel.blobs[other].key, 32U) == 0)
                return LXP_FATAL_INVARIANT;
    }
    status = lxp_state_root(&settled->kernel, canonical_root);
    if (status != LXP_OK) return status;
    store = lxp_state_snapshot_store(settled->state);
    if (store == NULL || store->next_sequence == 0U)
        return LXP_FATAL_INVARIANT;
    for (index = 0U; index < store->idempotency_count; ++index) {
        lxp_receipt candidate;
        status = receipt_restore_compact(
            store->idempotency[index].receipt,
            store->idempotency[index].receipt_length, &candidate);
        if (status != LXP_OK) return status;
        if (!have_latest || candidate.global_sequence > latest.global_sequence) {
            latest = candidate;
            have_latest = true;
        }
    }
    if (have_latest &&
        (latest.global_sequence == UINT64_MAX ||
         latest.global_sequence + 1U != store->next_sequence ||
         lxp_ct_memcmp(latest.resulting_state_root,
                       settled->kernel.current_state_root, 32U) != 0))
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

lxp_result lxp_kernel_batch_snapshot_commit(
    lxp_kernel *kernel, lxp_identity_store *identities,
    const lxp_kernel_batch_snapshot *base,
    const lxp_kernel_batch_snapshot *settled)
{
    uint8_t *blob_bytes[LXP_KERNEL_MAX_BLOBS] = {NULL};
    lxp_state_publication_guard *guard = NULL;
    size_t index;
    size_t blob_total = 0U;
    lxp_result status;
    if (kernel == NULL || identities == NULL || base == NULL ||
        settled == NULL || base == settled || kernel->publication_poisoned ||
        settled->kernel.module_kv_count > LXP_KERNEL_MAX_MODULE_KV ||
        settled->kernel.blob_count > LXP_KERNEL_MAX_BLOBS ||
        settled->identities.count > LXP_IDENTITY_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    if (!kernel_snapshot_matches_live(base, kernel, identities))
        return LXP_ERR_CONTEXT_MISMATCH;
    status = kernel_settled_snapshot_validate(settled);
    if (status != LXP_OK) return status;
    for (index = 0U; index < settled->kernel.blob_count; ++index) {
        if (settled->kernel.blobs[index].length == 0U ||
            settled->kernel.blobs[index].length > LXP_KERNEL_MAX_BLOB_BYTES ||
            blob_total > LXP_KERNEL_MAX_BLOB_TOTAL_BYTES -
                             settled->kernel.blobs[index].length)
            return LXP_FATAL_INVARIANT;
        blob_total += settled->kernel.blobs[index].length;
    }
    if (blob_total != settled->kernel.blob_total_bytes)
        return LXP_FATAL_INVARIANT;
    /* Allocate every replacement before crossing the no-refusal publication
     * boundary. */
    status = LXP_OK;
    for (index = 0U; index < settled->kernel.blob_count; ++index) {
        if (settled->kernel.blobs[index].bytes == NULL ||
            settled->kernel.blobs[index].length == 0U) {
            status = LXP_FATAL_INVARIANT;
            break;
        }
        blob_bytes[index] =
            (uint8_t *)malloc(settled->kernel.blobs[index].length);
        if (blob_bytes[index] == NULL) {
            status = LXP_ERR_ARENA_EXHAUSTED;
            break;
        }
        (void)memcpy(blob_bytes[index],
                     settled->kernel.blobs[index].bytes,
                     settled->kernel.blobs[index].length);
    }
    if (status != LXP_OK) {
        for (index = 0U; index < LXP_KERNEL_MAX_BLOBS; ++index)
            free(blob_bytes[index]);
        return status;
    }
    status = lxp_state_publication_guard_begin(
        base->state, settled->state, kernel->state, &guard);
    if (status != LXP_OK) {
        for (index = 0U; index < LXP_KERNEL_MAX_BLOBS; ++index)
            free(blob_bytes[index]);
        return status;
    }
    /* Revalidate every non-state domain while the live state guard excludes
     * gateway readers.  No live byte has changed yet. */
    if (!kernel_snapshot_matches_live(base, kernel, identities)) {
        status = lxp_state_publication_guard_end(guard);
        for (index = 0U; index < LXP_KERNEL_MAX_BLOBS; ++index)
            free(blob_bytes[index]);
        return status == LXP_OK ? LXP_ERR_CONTEXT_MISMATCH : status;
    }
    *identities = settled->identities;
    kernel->module_kv_count = settled->kernel.module_kv_count;
    (void)memcpy(kernel->module_kv, settled->kernel.module_kv,
                 settled->kernel.module_kv_count *
                     sizeof(kernel->module_kv[0]));
    for (index = 0U; index < kernel->blob_count; ++index)
        free(kernel->blobs[index].bytes);
    kernel->blob_count = settled->kernel.blob_count;
    kernel->blob_total_bytes = settled->kernel.blob_total_bytes;
    for (index = 0U; index < kernel->blob_count; ++index) {
        kernel->blobs[index] = settled->kernel.blobs[index];
        kernel->blobs[index].bytes = blob_bytes[index];
        blob_bytes[index] = NULL;
    }
    (void)memcpy(kernel->current_state_root,
                 settled->kernel.current_state_root, 32U);
    lxp_state_snapshot_publish_guarded(guard);
    return lxp_state_publication_guard_end(guard);
}

typedef struct lxp_kernel_prepare_worker {
    const lxp_kernel_batch_snapshot *snapshot;
    const lxp_activity *activity;
    const lxp_kernel_execution *execution;
    lxp_prepared_transition **prepared;
    uint8_t *arena_bytes;
    lxp_result status;
} lxp_kernel_prepare_worker;

enum { LXP_KERNEL_PREPARE_ARENA_BYTES = LXP_MAX_ACTIVITY_BYTES * 3U };

static void *kernel_prepare_worker_run(void *opaque)
{
    lxp_kernel_prepare_worker *worker =
        (lxp_kernel_prepare_worker *)opaque;
    lxp_arena arena;
    worker->status = lxp_arena_init(&arena, worker->arena_bytes,
                                    LXP_KERNEL_PREPARE_ARENA_BYTES);
    if (worker->status == LXP_OK)
        worker->status = lxp_kernel_prepare_activity(
            worker->snapshot, worker->activity, worker->execution, &arena,
            worker->prepared);
    return NULL;
}

lxp_result lxp_kernel_batch_publication_digest(
    const lxp_kernel_batch_boundary *base,
    const lxp_kernel_batch_boundary *final,
    const lxp_byte_span *canonical_activities,
    const lxp_byte_span *canonical_receipts,
    const lxp_byte_span *canonical_events, size_t activity_count,
    uint8_t digest[32])
{
    static const uint8_t domain[] = "LXP/kernel/prepared-batch/v1";
    size_t index;
    lxp_result status;
    if (base == NULL || final == NULL || canonical_activities == NULL ||
        canonical_receipts == NULL || canonical_events == NULL ||
        digest == NULL || activity_count == 0U ||
        activity_count > LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_hash_domain(LXP_DOMAIN_CONTEXT_HASH, domain,
                             sizeof(domain), digest);
    if (status == LXP_OK)
        status = level_token_mix(digest, base->receipt_state_root, 32U);
    if (status == LXP_OK)
        status = level_token_mix(digest, base->canonical_state_root, 32U);
    if (status == LXP_OK)
        status = level_token_u64(digest, base->next_sequence);
    if (status == LXP_OK)
        status = level_token_mix(digest, final->receipt_state_root, 32U);
    if (status == LXP_OK)
        status = level_token_mix(digest, final->canonical_state_root, 32U);
    if (status == LXP_OK)
        status = level_token_u64(digest, final->next_sequence);
    if (status == LXP_OK)
        status = level_token_u64(digest, activity_count);
    for (index = 0U; status == LXP_OK && index < activity_count; ++index) {
        status = level_token_mix(digest, canonical_activities[index].bytes,
                                 canonical_activities[index].length);
        if (status == LXP_OK)
            status = level_token_mix(digest, canonical_receipts[index].bytes,
                                     canonical_receipts[index].length);
        if (status == LXP_OK)
            status = level_token_mix(digest, canonical_events[index].bytes,
                                     canonical_events[index].length);
    }
    return status;
}

static lxp_result kernel_prepared_batch_digest(
    const lxp_activity *activities,
    const lxp_kernel_execution *executions,
    lxp_kernel_prepared_batch *batch)
{
    const lxp_state_store *base_state =
        lxp_state_snapshot_store(batch->base->state);
    const lxp_state_store *settled_state =
        lxp_state_snapshot_store(batch->settled->state);
    lxp_byte_span activity_bytes[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES];
    lxp_byte_span receipt_bytes[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES];
    size_t marks[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES];
    size_t index;
    lxp_result status;
    if (base_state == NULL || settled_state == NULL)
        return LXP_FATAL_INVARIANT;
    (void)memcpy(batch->base_boundary.receipt_state_root,
                 batch->base->kernel.current_state_root, 32U);
    (void)memcpy(batch->final_boundary.receipt_state_root,
                 batch->settled->kernel.current_state_root, 32U);
    batch->base_boundary.next_sequence = base_state->next_sequence;
    batch->final_boundary.next_sequence = settled_state->next_sequence;
    status = lxp_state_root(
        &batch->base->kernel, batch->base_boundary.canonical_state_root);
    if (status == LXP_OK)
        status = lxp_state_root(
            &batch->settled->kernel,
            batch->final_boundary.canonical_state_root);
    if (status != LXP_OK) return status;
    for (index = 0U; index < batch->count; ++index)
        marks[index] = lxp_arena_mark(executions[index].arena);
    for (index = 0U; index < batch->count; ++index) {
        status = lxp_activity_encode(&activities[index],
                                     executions[index].arena,
                                     &activity_bytes[index]);
        if (status == LXP_OK)
            status = lxp_receipt_encode(&batch->receipts[index], true,
                                        executions[index].arena,
                                        &receipt_bytes[index]);
        if (status != LXP_OK) break;
    }
    if (status == LXP_OK)
        status = lxp_kernel_batch_publication_digest(
            &batch->base_boundary, &batch->final_boundary, activity_bytes,
            receipt_bytes, batch->events, batch->count,
            batch->publication_digest);
    for (index = 0U; index < batch->count; ++index) {
        lxp_result reset_status =
            lxp_arena_reset(executions[index].arena, marks[index]);
        if (status == LXP_OK) status = reset_status;
    }
    return status;
}

lxp_result lxp_kernel_prepare_activity_batch(
    lxp_kernel *kernel, const lxp_activity *activities,
    const lxp_kernel_execution *executions, size_t offered_count,
    uint32_t maximum_workers, lxp_kernel_prepared_batch **batch_out,
    size_t *retry_prefix_count)
{
    lxp_kernel_batch_snapshot *base = NULL;
    lxp_kernel_batch_snapshot *settled = NULL;
    lxp_programs_schedule_item *items = NULL;
    lxp_kernel_execution *normalized = NULL;
    lxp_receipt *staged_receipts = NULL;
    lxp_byte_span *staged_events = NULL;
    lxp_arena *coordinator_arenas = NULL;
    uint8_t **coordinator_bytes = NULL;
    lxp_prepared_transition *prepared[
        LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES] = {NULL};
    uint16_t levels[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES] = {0};
    uint16_t maximum_level = 0U;
    size_t count = 0U;
    size_t planned_count = 0U;
    size_t settled_count = 0U;
    size_t sealed_count = 0U;
    size_t index;
    uint16_t level;
    lxp_kernel_prepared_batch *batch = NULL;
    lxp_result suffix_status = LXP_OK;
    bool suffix_is_semantic = false;
    bool status_is_semantic = false;
    bool stop_after_prefix = false;
    lxp_result status = LXP_OK;
    if (kernel == NULL || activities == NULL || executions == NULL ||
        batch_out == NULL || retry_prefix_count == NULL ||
        offered_count == 0U || maximum_workers == 0U)
        return LXP_ERR_NON_CANONICAL;
    *batch_out = NULL;
    *retry_prefix_count = 0U;
    while (count < offered_count &&
           count < LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES &&
           activities[count].activity_type == LX_PROGRAMS_CALL)
        ++count;
    if (count == 0U) return LXP_ERR_UNKNOWN_ACTIVITY;
    planned_count = count;
    if (executions[0].identities == NULL || executions[0].arena == NULL)
        return LXP_ERR_NON_CANONICAL;
    if (executions[0].global_sequence == 0U ||
        executions[0].global_sequence > UINT64_MAX - (count - 1U))
        return LXP_ERR_OVERFLOW;
    items = (lxp_programs_schedule_item *)calloc(count, sizeof(*items));
    normalized = (lxp_kernel_execution *)calloc(count, sizeof(*normalized));
    staged_receipts = (lxp_receipt *)calloc(count, sizeof(*staged_receipts));
    staged_events = (lxp_byte_span *)calloc(count, sizeof(*staged_events));
    coordinator_arenas = (lxp_arena *)calloc(count,
                                             sizeof(*coordinator_arenas));
    coordinator_bytes = (uint8_t **)calloc(count,
                                           sizeof(*coordinator_bytes));
    if (items == NULL || normalized == NULL || staged_receipts == NULL ||
        staged_events == NULL || coordinator_arenas == NULL ||
        coordinator_bytes == NULL) {
        status = LXP_ERR_ARENA_EXHAUSTED;
        goto done;
    }
    status = lxp_kernel_batch_snapshot_create(
        kernel, executions[0].identities, executions[0].verified_receipts,
        &executions[0], &base);
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        size_t arena_mark;
        if (executions[index].identities != executions[0].identities ||
            executions[index].batch_number != executions[0].batch_number ||
            executions[index].batch_timestamp_ms !=
                executions[0].batch_timestamp_ms ||
            executions[index].network_id != executions[0].network_id ||
            executions[index].epoch != executions[0].epoch ||
            executions[index].verified_receipts !=
                executions[0].verified_receipts ||
            executions[index].global_sequence !=
                executions[0].global_sequence + index ||
            lxp_ct_memcmp(executions[index].batch_id,
                          executions[0].batch_id, 32U) != 0 ||
            lxp_ct_memcmp(executions[index].activity_root,
                          executions[0].activity_root, 32U) != 0 ||
            executions[index].arena == NULL) {
            status = LXP_ERR_CONTEXT_MISMATCH;
            break;
        }
        normalized[index] = executions[index];
        coordinator_bytes[index] =
            (uint8_t *)malloc(LXP_KERNEL_PREPARE_ARENA_BYTES);
        if (coordinator_bytes[index] == NULL) {
            status = LXP_ERR_ARENA_EXHAUSTED;
            break;
        }
        status = lxp_arena_init(&coordinator_arenas[index],
                                coordinator_bytes[index],
                                LXP_KERNEL_PREPARE_ARENA_BYTES);
        if (status != LXP_OK) break;
        normalized[index].arena = &coordinator_arenas[index];
        normalized[index].recorded_fee_schedule_version =
            base->fee_schedule.version;
        normalized[index].recorded_metering_schedule_version =
            base->metering_schedule.version;
        normalized[index].fee_parameters = &base->fee_parameters;
        arena_mark = lxp_arena_mark(executions[index].arena);
        status = lxp_kernel_batch_schedule_item(
            base, &activities[index], &normalized[index],
            executions[index].arena, &items[index]);
        {
            lxp_result reset_status = lxp_arena_reset(
                executions[index].arena, arena_mark);
            if (status == LXP_OK) status = reset_status;
        }
    }
    if (status == LXP_OK)
        status = layerx_programs_schedule_plan(items, count, levels,
                                               &maximum_level);
    if (status == LXP_OK) {
        uint16_t canonical_level = levels[0];
        for (index = 1U; index < count; ++index) {
            if (levels[index] < canonical_level)
                levels[index] = canonical_level;
            else
                canonical_level = levels[index];
        }
        maximum_level = canonical_level;
        if (levels[0] != 0U) status = LXP_FATAL_INVARIANT;
        for (index = 1U; status == LXP_OK && index < count; ++index)
            if (levels[index] < levels[index - 1U] ||
                levels[index] > (uint16_t)(levels[index - 1U] + 1U))
                status = LXP_FATAL_INVARIANT;
    }
    if (status == LXP_OK)
        status = lxp_kernel_batch_snapshot_clone(base, &settled);
    for (level = 0U; status == LXP_OK && !stop_after_prefix &&
                    level <= maximum_level; ++level) {
        size_t level_indices[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES];
        size_t level_count = 0U;
        size_t cursor = 0U;
        size_t level_start_count = settled_count;
        status_is_semantic = false;
        status = lxp_kernel_batch_snapshot_begin_level(settled);
        for (index = 0U; status == LXP_OK && index < count; ++index)
            if (levels[index] == level)
                level_indices[level_count++] = index;
        for (index = 0U; status == LXP_OK && index < level_count; ++index) {
            size_t activity_index = level_indices[index];
            status = kernel_snapshot_payer_balance(
                settled, normalized[activity_index].authority,
                &normalized[activity_index].fee_balance);
        }
        while (status == LXP_OK && cursor < level_count) {
            pthread_t threads[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES];
            lxp_kernel_prepare_worker workers[
                LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES] = {{0}};
            bool launched[LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES] = {false};
            size_t width = level_count - cursor;
            size_t worker_index;
            if (width > maximum_workers) width = maximum_workers;
            for (worker_index = 0U; worker_index < width; ++worker_index) {
                size_t activity_index = level_indices[cursor + worker_index];
                workers[worker_index] = (lxp_kernel_prepare_worker){
                    settled, &activities[activity_index],
                    &normalized[activity_index], &prepared[activity_index],
                    (uint8_t *)malloc(LXP_KERNEL_PREPARE_ARENA_BYTES), LXP_OK
                };
                if (workers[worker_index].arena_bytes == NULL) {
                    status = LXP_ERR_ARENA_EXHAUSTED;
                    break;
                }
                if (maximum_workers > 1U &&
                    pthread_create(&threads[worker_index], NULL,
                                   kernel_prepare_worker_run,
                                   &workers[worker_index]) == 0)
                    launched[worker_index] = true;
                else
                    (void)kernel_prepare_worker_run(&workers[worker_index]);
            }
            for (worker_index = 0U; worker_index < width; ++worker_index) {
                if (workers[worker_index].arena_bytes == NULL) continue;
                if (launched[worker_index] &&
                    pthread_join(threads[worker_index], NULL) != 0)
                    abort();
                if (status == LXP_OK && workers[worker_index].status != LXP_OK) {
                    status = workers[worker_index].status;
                    status_is_semantic =
                        !lxp_result_is_fatal(status) &&
                        status != LXP_ERR_IO &&
                        status != LXP_ERR_ARENA_EXHAUSTED;
                }
                free(workers[worker_index].arena_bytes);
            }
            cursor += width;
        }
        if (status != LXP_OK && sealed_count != 0U &&
            status_is_semantic) {
            suffix_status = status;
            suffix_is_semantic = true;
            settled_count = sealed_count;
            status = LXP_OK;
            stop_after_prefix = true;
            continue;
        }
        for (index = 0U; status == LXP_OK && index < count; ++index)
            if (levels[index] == level) {
                if (index != settled_count) {
                    status = LXP_FATAL_INVARIANT;
                    break;
                }
                status = lxp_kernel_snapshot_apply_prepared(
                    settled, &activities[index], &normalized[index],
                    prepared[index], &staged_receipts[index],
                    &staged_events[index]);
                if (status != LXP_OK)
                    status_is_semantic =
                        !lxp_result_is_fatal(status) &&
                        status != LXP_ERR_IO &&
                        status != LXP_ERR_ARENA_EXHAUSTED;
                if (status == LXP_OK) ++settled_count;
            }
        if (settled_count > level_start_count &&
            (status == LXP_OK || settled_count < count)) {
            lxp_result seal_status =
                lxp_state_snapshot_seal_level(settled->state);
            if (seal_status != LXP_OK) {
                status = seal_status;
                suffix_is_semantic = false;
                status_is_semantic = false;
            } else {
                sealed_count = settled_count;
            }
        }
        if (status != LXP_OK && sealed_count != 0U &&
            status_is_semantic) {
            suffix_status = status;
            suffix_is_semantic = true;
            settled_count = sealed_count;
            status = LXP_OK;
            stop_after_prefix = true;
        } else if (status == LXP_OK && !stop_after_prefix &&
                   settled_count == count) {
            stop_after_prefix = true;
        }
    }
    if (status == LXP_OK && settled_count == 0U)
        status = LXP_FATAL_INVARIANT;
    if (status == LXP_OK && settled_count < count && suffix_is_semantic) {
        *retry_prefix_count = settled_count;
        status = suffix_status == LXP_OK ?
            LXP_FATAL_INVARIANT : suffix_status;
    } else if (status == LXP_OK && settled_count < count) {
        status = LXP_FATAL_INVARIANT;
    }
    if (status == LXP_OK) {
        batch = (lxp_kernel_prepared_batch *)calloc(1U, sizeof(*batch));
        if (batch == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
    }
    if (status == LXP_OK) {
        batch->event_bytes = (uint8_t **)calloc(count,
                                                sizeof(*batch->event_bytes));
        if (batch->event_bytes == NULL) status = LXP_ERR_ARENA_EXHAUSTED;
    }
    for (index = 0U; status == LXP_OK && index < count; ++index) {
        if (staged_events[index].length == 0U) continue;
        if (staged_events[index].bytes == NULL) {
            status = LXP_FATAL_INVARIANT;
            break;
        }
        batch->event_bytes[index] =
            (uint8_t *)malloc(staged_events[index].length);
        if (batch->event_bytes[index] == NULL) {
            status = LXP_ERR_ARENA_EXHAUSTED;
            break;
        }
        (void)memcpy(batch->event_bytes[index], staged_events[index].bytes,
                     staged_events[index].length);
        staged_events[index].bytes = batch->event_bytes[index];
    }
    for (index = 0U; status == LXP_OK && index < count; ++index)
        status = lxp_arena_reset(&coordinator_arenas[index], 0U);
    if (status == LXP_OK) {
        batch->base = base;
        base = NULL;
        batch->settled = settled;
        settled = NULL;
        batch->receipts = staged_receipts;
        staged_receipts = NULL;
        batch->events = staged_events;
        staged_events = NULL;
        batch->count = count;
        status = kernel_prepared_batch_digest(activities, normalized, batch);
    }
    if (status == LXP_OK) {
        *batch_out = batch;
        batch = NULL;
    }
done:
    for (index = 0U; index < planned_count; ++index)
        lxp_prepared_transition_destroy(prepared[index]);
    lxp_kernel_batch_snapshot_destroy(settled);
    lxp_kernel_batch_snapshot_destroy(base);
    free(normalized);
    free(items);
    free(staged_events);
    free(staged_receipts);
    if (coordinator_bytes != NULL)
        for (index = 0U; index < planned_count; ++index)
            free(coordinator_bytes[index]);
    free(coordinator_bytes);
    free(coordinator_arenas);
    lxp_kernel_prepared_batch_destroy(batch);
    return status;
}

size_t lxp_kernel_prepared_batch_count(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? 0U : batch->count;
}

const lxp_receipt *lxp_kernel_prepared_batch_receipts(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : batch->receipts;
}

const lxp_byte_span *lxp_kernel_prepared_batch_events(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : batch->events;
}

const uint8_t *lxp_kernel_prepared_batch_final_root(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : batch->settled->kernel.current_state_root;
}

const uint8_t *lxp_kernel_prepared_batch_publication_digest(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : batch->publication_digest;
}

const lxp_kernel_batch_boundary *lxp_kernel_prepared_batch_base_boundary(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : &batch->base_boundary;
}

const lxp_kernel_batch_boundary *lxp_kernel_prepared_batch_final_boundary(
    const lxp_kernel_prepared_batch *batch)
{
    return batch == NULL ? NULL : &batch->final_boundary;
}

lxp_result lxp_kernel_batch_boundary_read(
    const lxp_kernel *kernel, lxp_kernel_batch_boundary *boundary)
{
    lxp_result status;
    if (kernel == NULL || kernel->state == NULL || boundary == NULL)
        return LXP_ERR_NON_CANONICAL;
    (void)memcpy(boundary->receipt_state_root,
                 kernel->current_state_root, 32U);
    boundary->next_sequence = kernel->state->next_sequence;
    status = lxp_state_root(kernel, boundary->canonical_state_root);
    return status;
}

lxp_result lxp_kernel_commit_prepared_batch(
    lxp_kernel *kernel, lxp_identity_store *identities,
    lxp_kernel_prepared_batch *batch,
    const uint8_t fsynced_publication_digest[32])
{
    lxp_result status;
    if (kernel == NULL || identities == NULL || batch == NULL ||
        batch->base == NULL || batch->settled == NULL || batch->committed ||
        fsynced_publication_digest == NULL ||
        lxp_ct_is_zero(fsynced_publication_digest, 32U) ||
        lxp_ct_memcmp(fsynced_publication_digest,
                      batch->publication_digest, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_kernel_batch_snapshot_commit(
        kernel, identities, batch->base, batch->settled);
    if (status == LXP_OK) {
        batch->committed = true;
        kernel->publication_poisoned = true;
        kernel->batch_publication_pending = true;
        (void)memcpy(kernel->pending_batch_publication_digest,
                     batch->publication_digest, 32U);
        (void)memcpy(kernel->pending_batch_id,
                     batch->receipts[0].batch_id, 32U);
        (void)memcpy(kernel->pending_batch_base_receipt_root,
                     batch->base_boundary.receipt_state_root, 32U);
        kernel->pending_batch_first_sequence =
            batch->receipts[0].global_sequence;
        kernel->pending_batch_last_sequence =
            batch->receipts[batch->count - 1U].global_sequence;
        kernel->pending_batch_publication_index = 0U;
        kernel->poisoned_sequence = kernel->pending_batch_first_sequence;
        (void)memcpy(kernel->poisoned_activity_id,
                     batch->receipts[0].activity_id, 32U);
        (void)memcpy(kernel->poisoned_state_root,
                     batch->final_boundary.receipt_state_root, 32U);
    }
    return status;
}

lxp_result lxp_kernel_finalize_prepared_batch_publication(
    lxp_kernel *kernel, const lxp_activity *activities,
    const lxp_kernel_prepared_batch *batch,
    const uint8_t fsynced_publication_digest[32])
{
    if (kernel == NULL || activities == NULL || batch == NULL ||
        !batch->committed || !kernel->publication_poisoned ||
        !kernel->batch_publication_pending ||
        fsynced_publication_digest == NULL ||
        lxp_ct_memcmp(fsynced_publication_digest,
                      batch->publication_digest, 32U) != 0 ||
        lxp_ct_memcmp(kernel->pending_batch_publication_digest,
                      batch->publication_digest, 32U) != 0 ||
        kernel->pending_batch_first_sequence !=
            batch->receipts[0].global_sequence ||
        kernel->pending_batch_last_sequence !=
            batch->receipts[batch->count - 1U].global_sequence)
        return LXP_ERR_CONTEXT_MISMATCH;
    return lxp_kernel_finalize_batch_publication_records(
        kernel, activities, batch->receipts, batch->count,
        fsynced_publication_digest);
}

lxp_result lxp_kernel_restore_batch_publication_pending(
    lxp_kernel *kernel, const uint8_t fsynced_publication_digest[32],
    const uint8_t batch_id[32],
    const uint8_t base_receipt_state_root[32],
    const uint8_t final_receipt_state_root[32], uint64_t first_sequence,
    uint64_t last_sequence, uint32_t next_publication_index)
{
    if (kernel == NULL || fsynced_publication_digest == NULL ||
        batch_id == NULL || base_receipt_state_root == NULL ||
        final_receipt_state_root == NULL || kernel->publication_poisoned ||
        lxp_ct_is_zero(fsynced_publication_digest, 32U) ||
        lxp_ct_is_zero(batch_id, 32U) ||
        first_sequence == 0U || last_sequence < first_sequence ||
        last_sequence - first_sequence >=
            LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES ||
        (uint64_t)next_publication_index >
            last_sequence - first_sequence + 1U ||
        lxp_ct_memcmp(kernel->current_state_root,
                      final_receipt_state_root, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    kernel->publication_poisoned = true;
    kernel->batch_publication_pending = true;
    (void)memcpy(kernel->pending_batch_publication_digest,
                 fsynced_publication_digest, 32U);
    (void)memcpy(kernel->pending_batch_id, batch_id, 32U);
    (void)memcpy(kernel->pending_batch_base_receipt_root,
                 base_receipt_state_root, 32U);
    (void)memcpy(kernel->poisoned_state_root,
                 final_receipt_state_root, 32U);
    kernel->pending_batch_first_sequence = first_sequence;
    kernel->pending_batch_last_sequence = last_sequence;
    kernel->pending_batch_publication_index = next_publication_index;
    kernel->poisoned_sequence = first_sequence;
    return LXP_OK;
}

lxp_result lxp_kernel_finalize_batch_publication_records(
    lxp_kernel *kernel, const lxp_activity *activities,
    const lxp_receipt *receipts, size_t activity_count,
    const uint8_t fsynced_publication_digest[32])
{
    size_t index;
    uint8_t *verification_bytes;
    if (kernel == NULL || activities == NULL || receipts == NULL ||
        activity_count == 0U ||
        activity_count > LXP_PROGRAMS_SCHEDULE_MAX_ACTIVITIES ||
        !kernel->publication_poisoned ||
        !kernel->batch_publication_pending ||
        fsynced_publication_digest == NULL ||
        lxp_ct_memcmp(fsynced_publication_digest,
                      kernel->pending_batch_publication_digest, 32U) != 0 ||
        receipts[0].global_sequence !=
            kernel->pending_batch_first_sequence ||
        receipts[activity_count - 1U].global_sequence !=
            kernel->pending_batch_last_sequence ||
        kernel->pending_batch_last_sequence -
                kernel->pending_batch_first_sequence + 1U !=
            activity_count ||
        kernel->pending_batch_first_sequence >
            UINT64_MAX - (activity_count - 1U) ||
        lxp_ct_memcmp(receipts[activity_count - 1U].resulting_state_root,
                      kernel->poisoned_state_root, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    if (lxp_ct_memcmp(receipts[0].previous_state_root,
                      kernel->pending_batch_base_receipt_root, 32U) != 0)
        return LXP_ERR_CONTEXT_MISMATCH;
    verification_bytes = (uint8_t *)malloc(LXP_MAX_ACTIVITY_BYTES);
    if (verification_bytes == NULL) return LXP_ERR_ARENA_EXHAUSTED;
    for (index = 0U; index < activity_count; ++index) {
        lxp_arena arena;
        lxp_byte_span encoded;
        uint8_t activity_id[32];
        lxp_result status = lxp_arena_init(
            &arena, verification_bytes, LXP_MAX_ACTIVITY_BYTES);
        if (status == LXP_OK)
            status = lxp_activity_encode(&activities[index], &arena,
                                         &encoded);
        if (status == LXP_OK)
            status = lxp_activity_id(encoded.bytes, encoded.length,
                                     activity_id);
        if (status != LXP_OK ||
            !lxp_protocol_version_supported(
                receipts[index].protocol_version) ||
            receipts[index].global_sequence !=
                kernel->pending_batch_first_sequence + index ||
            lxp_ct_memcmp(receipts[index].batch_id,
                          kernel->pending_batch_id, 32U) != 0 ||
            (index != 0U &&
             lxp_ct_memcmp(receipts[index].previous_state_root,
                           receipts[index - 1U].resulting_state_root,
                           32U) != 0) ||
            lxp_ct_memcmp(activity_id, receipts[index].activity_id, 32U) != 0) {
            free(verification_bytes);
            return status != LXP_OK ? status : LXP_ERR_CONTEXT_MISMATCH;
        }
    }
    free(verification_bytes);
    if (kernel->pending_batch_publication_index > activity_count)
        return LXP_FATAL_INVARIANT;
    if (kernel->observe_commit != NULL)
        for (index = kernel->pending_batch_publication_index;
             index < activity_count; ++index) {
            lxp_result status = kernel->observe_commit(
                kernel->commit_observer_context, kernel,
                &activities[index], &receipts[index]);
            if (status != LXP_OK) return LXP_FATAL_INVARIANT;
            kernel->pending_batch_publication_index = (uint32_t)(index + 1U);
        }
    kernel->publication_poisoned = false;
    kernel->batch_publication_pending = false;
    kernel->poisoned_sequence = 0U;
    kernel->pending_batch_first_sequence = 0U;
    kernel->pending_batch_last_sequence = 0U;
    kernel->pending_batch_publication_index = 0U;
    (void)memset(kernel->pending_batch_publication_digest, 0, 32U);
    (void)memset(kernel->pending_batch_id, 0, 32U);
    (void)memset(kernel->pending_batch_base_receipt_root, 0, 32U);
    (void)memset(kernel->poisoned_activity_id, 0, 32U);
    (void)memset(kernel->poisoned_state_root, 0, 32U);
    return LXP_OK;
}

uint32_t lxp_kernel_batch_publication_next_index(const lxp_kernel *kernel)
{
    return kernel != NULL && kernel->batch_publication_pending ?
        kernel->pending_batch_publication_index : 0U;
}

void lxp_kernel_prepared_batch_destroy(lxp_kernel_prepared_batch *batch)
{
    size_t index;
    if (batch == NULL) return;
    if (batch->event_bytes != NULL)
        for (index = 0U; index < batch->count; ++index)
            free(batch->event_bytes[index]);
    free(batch->event_bytes);
    free(batch->events);
    free(batch->receipts);
    lxp_kernel_batch_snapshot_destroy(batch->settled);
    lxp_kernel_batch_snapshot_destroy(batch->base);
    (void)memset(batch, 0, sizeof(*batch));
    free(batch);
}

static lxp_result kernel_execute_prepared_call(
    lxp_kernel *kernel, const lxp_activity *activity,
    const lxp_kernel_execution *execution, lxp_receipt *receipt)
{
    lxp_kernel_batch_snapshot *base = NULL;
    lxp_kernel_batch_snapshot *settled = NULL;
    lxp_prepared_transition *prepared = NULL;
    lxp_byte_span events = {NULL, 0U};
    lxp_result status;
    size_t arena_mark = lxp_arena_mark(execution->arena);
    lxp_kernel_execution normalized = *execution;
    status = lxp_kernel_batch_snapshot_create(
        kernel, execution->identities, execution->verified_receipts,
        execution, &base);
    if (status == LXP_OK)
        status = lxp_kernel_batch_snapshot_begin_level(base);
    if (status == LXP_OK) {
        normalized.recorded_fee_schedule_version = base->fee_schedule.version;
        normalized.recorded_metering_schedule_version =
            base->metering_schedule.version;
    }
    if (status == LXP_OK)
        status = lxp_kernel_batch_snapshot_clone(base, &settled);
    if (status == LXP_OK)
        status = lxp_kernel_prepare_activity(
            base, activity, &normalized, execution->arena, &prepared);
    {
        lxp_result reset_status =
            lxp_arena_reset(execution->arena, arena_mark);
        if (status == LXP_OK) status = reset_status;
    }
    if (status == LXP_ERR_IDEMPOTENT_REPLAY) {
        const uint8_t *prior;
        size_t prior_length;
        lxp_result lookup = lxp_idempotency_lookup(
            kernel->state, activity->actor_did.bytes,
            activity->actor_did.length, activity->idempotency_key,
            &prior, &prior_length);
        if (lookup == LXP_ERR_IDEMPOTENT_REPLAY) {
            lookup = receipt_restore_compact(prior, prior_length, receipt);
            status = lookup == LXP_OK ? LXP_ERR_IDEMPOTENT_REPLAY : lookup;
        } else {
            status = lookup;
        }
    }
    if (status == LXP_OK)
        status = lxp_kernel_snapshot_apply_prepared(
            settled, activity, &normalized, prepared, receipt, &events);
    if (status == LXP_OK)
        status = lxp_state_snapshot_seal_level(settled->state);
    if (status == LXP_OK)
        status = lxp_kernel_batch_snapshot_commit(
            kernel, execution->identities, base, settled);
    if (status == LXP_OK && kernel->observe_commit != NULL) {
        status = kernel->observe_commit(kernel->commit_observer_context,
                                        kernel, activity, receipt);
        if (status != LXP_OK) {
            kernel->publication_poisoned = true;
            kernel->poisoned_sequence = receipt->global_sequence;
            (void)memcpy(kernel->poisoned_activity_id,
                         receipt->activity_id, 32U);
            (void)memcpy(kernel->poisoned_state_root,
                         receipt->resulting_state_root, 32U);
            status = LXP_FATAL_INVARIANT;
        }
    }
    if (status == LXP_OK && execution->canonical_events_out != NULL)
        *execution->canonical_events_out = events;
    lxp_prepared_transition_destroy(prepared);
    lxp_kernel_batch_snapshot_destroy(settled);
    lxp_kernel_batch_snapshot_destroy(base);
    return status;
}

lxp_result lxp_kernel_execute_activity(lxp_kernel *kernel,
                                       const lxp_activity *activity,
                                       const lxp_kernel_execution *execution,
                                       lxp_receipt *receipt)
{
    const lxp_module_registration *registration;
    const uint8_t *prior_receipt;
    size_t prior_receipt_length;
    lxp_identity *identity;
    lxp_admission_context admission_context;
    lxp_admission_result admission;
    lxp_fee_policy_decision admission_policy;
    lxp_fee_policy_decision fee_policy;
    lxp_module_ctx module_ctx;
    lxp_effect_buffer effects;
    lxp_result module_result;
    lxp_result status;
    lxp_u128 fee;
    lxp_u128 pre_runtime_fee = {0U, 0U};
    lxp_fee_meter actual_fee_meter;
    lxp_program_outcome synthetic_program_outcome;
    const lxp_program_outcome *program_outcome = NULL;
    lxp_byte_span encoded;
    lxp_byte_span projected_events = {NULL, 0U};
    uint8_t canonical_activity_id[32];
    size_t arena_mark;
    uint64_t identity_sequence_before;
    bool module_ctx_initialized = false;
    bool identity_sequence_consumed = false;
    bool fee_transaction_open = false;
    void *fee_transaction = NULL;
    bool programs_call;
    bool programs_state_activity;
    const lx_programs_transfer_runtime *programs_runtime = NULL;
    lx_programs_fee_schedule programs_fee_schedule;
    lx_programs_metering_schedule programs_metering_schedule;
    uint8_t programs_occupancy_asset_id[32];
    if (execution != NULL && execution->canonical_events_out != NULL)
        *execution->canonical_events_out = (lxp_byte_span){NULL, 0U};
    if (kernel == NULL || activity == NULL || execution == NULL ||
        receipt == NULL || execution->identities == NULL ||
        execution->authority == NULL || execution->fee_parameters == NULL ||
        execution->arena == NULL || execution->batch_number == 0U)
        return LXP_ERR_NON_CANONICAL;
    if (kernel->publication_poisoned) return LXP_FATAL_INVARIANT;
    if (lxp_activity_module_id(activity->activity_type) ==
            LXP_MODULE_PROGRAMS &&
        lxp_activity_type_ordinal(activity->activity_type) == 3U)
        return kernel_execute_prepared_call(kernel, activity, execution,
                                            receipt);
    arena_mark = lxp_arena_mark(execution->arena);
    programs_call = lxp_activity_module_id(activity->activity_type) ==
                        LXP_MODULE_PROGRAMS &&
                    lxp_activity_type_ordinal(activity->activity_type) == 3U;
    programs_state_activity =
        activity->activity_type == LX_PROGRAMS_ACCOUNT ||
        activity->activity_type == LX_PROGRAMS_WIND_DOWN;
    if (programs_state_activity &&
        (activity->protocol_version != LXP_PROTOCOL_VERSION_OCCUPANCY ||
         kernel->module_runtime[LXP_MODULE_PROGRAMS] == NULL ||
         ((const lx_programs_transfer_runtime *)
              kernel->module_runtime[LXP_MODULE_PROGRAMS])->state_feed == NULL ||
         kernel->observe_commit == NULL))
        return LXP_ERR_MODULE_DISABLED;
    if (programs_call) {
        programs_runtime = (const lx_programs_transfer_runtime *)
            kernel->module_runtime[LXP_MODULE_PROGRAMS];
        if (programs_runtime == NULL)
            return LXP_ERR_VERSION_UNSUPPORTED;
        if (programs_runtime->resolve_metering_schedule == NULL)
            return LXP_ERR_MODULE_DISABLED;
        (void)memset(&programs_metering_schedule, 0,
                     sizeof(programs_metering_schedule));
        status = programs_runtime->resolve_metering_schedule(
            programs_runtime->metering_schedule_context,
            execution->recorded_metering_schedule_version,
            execution->batch_number, &programs_metering_schedule);
        if (status != LXP_OK) return status;
        if (!lxp_program_metering_schedule_available(
                programs_metering_schedule.version))
            return LXP_ERR_VERSION_UNSUPPORTED;
        if (programs_runtime->resolve_occupancy_parameters == NULL)
            return LXP_ERR_MODULE_DISABLED;
        (void)memset(&programs_fee_schedule, 0,
                     sizeof(programs_fee_schedule));
        (void)memset(programs_occupancy_asset_id, 0,
                     sizeof(programs_occupancy_asset_id));
        status = programs_runtime->resolve_occupancy_parameters(
            programs_runtime->occupancy_parameter_context,
            execution->recorded_fee_schedule_version,
            &programs_fee_schedule,
            programs_occupancy_asset_id);
        if (status != LXP_OK) return status;
        if (programs_fee_schedule.version == 0U ||
            lxp_ct_is_zero(programs_occupancy_asset_id, 32U))
            return LXP_ERR_VERSION_UNSUPPORTED;
    }
    status = lxp_module_version_for_epoch(
        kernel, lxp_activity_module_id(activity->activity_type),
        execution->epoch, execution->recorded_module_version, &registration);
    if (status != LXP_OK) return status;
    if (!activity_declared(registration, activity->activity_type))
        return LXP_ERR_UNKNOWN_ACTIVITY;
    status = lxp_identity_resolve(execution->identities,
                                  activity->actor_did.bytes,
                                  activity->actor_did.length, &identity);
    if (status != LXP_OK) return status;
    status = lxp_idempotency_lookup(kernel->state,
                                    activity->actor_did.bytes,
                                    activity->actor_did.length,
                                    activity->idempotency_key,
                                    &prior_receipt,
                                    &prior_receipt_length);
    if (status == LXP_ERR_IDEMPOTENT_REPLAY) {
        status = receipt_restore_compact(prior_receipt,
                                         prior_receipt_length, receipt);
        return status == LXP_OK ? LXP_ERR_IDEMPOTENT_REPLAY : status;
    }
    if (status != LXP_OK) return status;
    admission_context = (lxp_admission_context){
        execution->network_id, execution->batch_timestamp_ms,
        execution->maximum_timestamp_window, identity->next_sequence,
        execution->signature_valid, false,
        lxp_u128_cmp(execution->fee_balance, activity->fee_limit) >= 0
    };
    admission = lxp_admit_activity(activity, &admission_context);
    status = lxp_fee_admission_check(admission, activity->fee_limit,
                                     execution->fee_balance,
                                     &admission_policy);
    if (status != LXP_OK) return status;
    if (admission_policy.result_code != LXP_OK)
        return admission_policy.result_code;
    status = lxp_fee_compute(execution->fee_parameters, activity->activity_type,
                             execution->fee_meter, &fee);
    if (status == LXP_OK) pre_runtime_fee = fee;
    if (status == LXP_OK && programs_call)
        fee = (lxp_u128){0U, 0U};
    if (status == LXP_OK)
        status = lxp_fee_rejection_policy(
            &admission_policy, LXP_OK, fee, activity->fee_limit, &fee_policy);
    if (status != LXP_OK) return status;
    status = lxp_activity_encode(activity, execution->arena, &encoded);
    if (status == LXP_OK)
        status = lxp_activity_id(encoded.bytes, encoded.length,
                                 canonical_activity_id);
    (void)lxp_arena_reset(execution->arena, arena_mark);
    if (status != LXP_OK) return status;
    status = lxp_state_journal_open(kernel->state,
                                    execution->global_sequence,
                                    kernel->journal);
    if (status != LXP_OK) return status;
    if (fee_policy.charge_fee && !programs_call)
        status = kernel->fee_transaction.prepare == NULL ?
                 LXP_FATAL_INVARIANT :
                 kernel->fee_transaction.prepare(
                     kernel, activity, execution->authority,
                     fee_policy.fee_charged,
                     &fee_transaction);
    if (status == LXP_OK && fee_policy.charge_fee && !programs_call)
        fee_transaction_open = true;
    if (status == LXP_OK && fee_transaction_open && fee_transaction == NULL)
        status = LXP_FATAL_INVARIANT;
    if (status != LXP_OK) {
        if (fee_transaction_open)
            kernel->fee_transaction.rollback(kernel, fee_transaction);
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    (void)memset(&effects, 0, sizeof(effects));
    status = lxp_effect_buffer_init(&effects);
    module_result = fee_policy.result_code;
    if (status == LXP_OK && fee_policy.apply_module_effects) {
        status = lxp_module_ctx_init(
            &module_ctx, kernel, registration->module_id,
            execution->batch_timestamp_ms, execution->epoch,
            execution->global_sequence, execution->gas_limit,
            execution->arena, false);
        if (status == LXP_OK) module_ctx_initialized = true;
        if (status == LXP_OK)
            module_ctx.protocol_version = activity->protocol_version;
        if (status == LXP_OK) module_ctx.batch_number = execution->batch_number;
        if (status == LXP_OK)
            module_ctx.verified_receipts = execution->verified_receipts;
        if (status == LXP_OK)
            (void)memcpy(module_ctx.activity_id, canonical_activity_id, 32U);
        if (status == LXP_OK && programs_call) {
            (void)memcpy(module_ctx.call_admission.activity_binding,
                         canonical_activity_id, 32U);
            (void)memcpy(module_ctx.call_admission.payer,
                         execution->authority->principal, 32U);
            module_ctx.call_admission.available_fee_units =
                execution->fee_balance;
            module_ctx.call_admission.signed_fee_limit = activity->fee_limit;
            module_ctx.call_admission.fee_schedule_version =
                programs_fee_schedule.version;
            module_ctx.call_admission.metering_schedule_version =
                programs_metering_schedule.version;
            (void)memcpy(
                module_ctx.call_admission.metering_schedule_coefficients,
                programs_metering_schedule.coefficients,
                sizeof(programs_metering_schedule.coefficients));
            module_ctx.call_admission.fee_schedule_prices[0] =
                programs_fee_schedule.cpu;
            module_ctx.call_admission.fee_schedule_prices[1] =
                programs_fee_schedule.memory_byte;
            module_ctx.call_admission.fee_schedule_prices[2] =
                programs_fee_schedule.storage_read_byte;
            module_ctx.call_admission.fee_schedule_prices[3] =
                programs_fee_schedule.storage_write_byte;
            module_ctx.call_admission.fee_schedule_prices[4] =
                programs_fee_schedule.output_value;
            module_ctx.call_admission.fee_schedule_prices[5] =
                programs_fee_schedule.output_byte;
            module_ctx.call_admission.fee_schedule_prices[6] =
                programs_fee_schedule.occupancy_byte_batch;
            module_ctx.call_admission.parameter_version =
                execution->parameter_version;
            module_ctx.call_admission.present = true;
        }
        if (status == LXP_OK)
            status = lxp_module_ctx_bind_effects(&module_ctx, &effects);
        if (status == LXP_OK)
            status = lxp_kernel_dispatch(registration, &module_ctx, activity,
                                         execution->authority, &effects,
                                         &module_result);
        if (status == LXP_OK && programs_call) {
            program_outcome = lxp_ctx_program_outcome(&module_ctx);
            if (program_outcome == NULL && module_result != LXP_OK &&
                !lxp_result_is_fatal(module_result)) {
                status = synthesize_program_call_failure(
                    activity, canonical_activity_id, execution,
                    module_result, pre_runtime_fee,
                    programs_fee_schedule.version,
                    programs_metering_schedule.version,
                    &synthetic_program_outcome);
                if (status == LXP_OK) {
                    synthetic_program_outcome.fee_schedule_version =
                        programs_fee_schedule.version;
                    synthetic_program_outcome.fee_schedule_prices[0] =
                        programs_fee_schedule.cpu;
                    synthetic_program_outcome.fee_schedule_prices[1] =
                        programs_fee_schedule.memory_byte;
                    synthetic_program_outcome.fee_schedule_prices[2] =
                        programs_fee_schedule.storage_read_byte;
                    synthetic_program_outcome.fee_schedule_prices[3] =
                        programs_fee_schedule.storage_write_byte;
                    synthetic_program_outcome.fee_schedule_prices[4] =
                        programs_fee_schedule.output_value;
                    synthetic_program_outcome.fee_schedule_prices[5] =
                        programs_fee_schedule.output_byte;
                    synthetic_program_outcome.fee_schedule_prices[6] =
                        programs_fee_schedule.occupancy_byte_batch;
                    program_outcome = &synthetic_program_outcome;
                }
            } else if (program_outcome == NULL) {
                status = LXP_FATAL_INVARIANT;
            }
            if (status == LXP_OK) {
                actual_fee_meter = execution->fee_meter;
                actual_fee_meter.exact_program_fee_present = true;
                actual_fee_meter.program_fee_schedule_version =
                    program_outcome->fee_schedule_version;
                actual_fee_meter.exact_program_fee_units =
                    program_outcome->fee_units;
                status = lxp_fee_compute(execution->fee_parameters,
                                         activity->activity_type,
                                         actual_fee_meter, &fee);
            } else {
                fee = pre_runtime_fee;
            }
        }
        if (status == LXP_OK)
            status = lxp_fee_rejection_policy(
                &admission_policy, module_result, fee, activity->fee_limit,
                &fee_policy);
        if (status == LXP_OK && !fee_policy.apply_module_effects)
            lxp_module_ctx_rollback(&module_ctx);
        if (status == LXP_OK && programs_call && fee_policy.charge_fee)
            status = kernel->fee_transaction.prepare == NULL ?
                     LXP_FATAL_INVARIANT :
                     kernel->fee_transaction.prepare(
                         kernel, activity, execution->authority,
                         fee_policy.fee_charged,
                         &fee_transaction);
        if (status == LXP_OK && programs_call && fee_policy.charge_fee)
            fee_transaction_open = true;
        if (status == LXP_OK && fee_transaction_open &&
            fee_transaction == NULL)
            status = LXP_FATAL_INVARIANT;
    }
    if (status != LXP_OK) {
        if (module_ctx_initialized) lxp_module_ctx_rollback(&module_ctx);
        if (fee_transaction_open)
            close_failed_fee_transaction(kernel, fee_transaction, status);
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    if (fee_policy.apply_module_effects)
        status = lxp_module_ctx_prepare_commit(&module_ctx);
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(&module_ctx);
        if (fee_transaction_open)
            close_failed_fee_transaction(kernel, fee_transaction, status);
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    (void)memset(receipt, 0, sizeof(*receipt));
    receipt->protocol_version = activity->protocol_version;
    (void)memcpy(receipt->activity_id, canonical_activity_id, 32U);
    receipt->global_sequence = execution->global_sequence;
    (void)memcpy(receipt->previous_state_root, kernel->current_state_root, 32U);
    receipt->result_code = fee_policy.result_code;
    receipt->fee_charged = fee_policy.fee_charged;
    receipt->module_id = registration->module_id;
    receipt->module_version = registration->abi_version;
    receipt->parameter_version = execution->parameter_version;
    (void)memcpy(receipt->batch_id, execution->batch_id, 32U);
    (void)memcpy(receipt->activity_root, execution->activity_root, 32U);
    if (fee_policy.apply_module_effects)
        receipt->effects = effects;
    status = receipt_state_root(kernel,
                                fee_policy.apply_module_effects ? &module_ctx : NULL,
                                receipt, receipt->resulting_state_root);
    if (status == LXP_OK)
        status = lxp_receipt_build(
            receipt, receipt->activity_id, execution->global_sequence,
            receipt->previous_state_root, receipt->resulting_state_root,
            execution->activity_root, fee_policy.result_code,
            fee_policy.apply_module_effects ? &effects :
                &(lxp_effect_buffer){ { { 0 } }, 0U },
            fee_policy.fee_charged, execution->batch_id, registration->module_id,
            registration->abi_version, execution->parameter_version);
    if (status == LXP_OK)
        receipt->timestamp = execution->batch_timestamp_ms;
    if (status == LXP_OK && programs_call &&
        program_outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS)
        (void)memcpy(receipt->transfer_set_root,
                     program_outcome->transfer_root, 32U);
    if (status == LXP_OK && programs_call)
        status = lxp_receipt_bind_program_outcome(receipt, program_outcome);
    if (status == LXP_OK && execution->sequencer_private_key != NULL)
        status = lxp_receipt_sign(receipt, execution->sequencer_private_key,
                                  execution->arena);
    if (status == LXP_OK) status = receipt_store(kernel->journal, activity,
                                                 receipt);
    if (status == LXP_OK && programs_call && fee_policy.apply_module_effects &&
        program_outcome != NULL &&
        program_outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS &&
        execution->canonical_events_out != NULL)
        status = lxp_programs_project_committed_events(
            &effects, execution->arena, &projected_events);
    identity_sequence_before = identity->next_sequence;
    if (status == LXP_OK) {
        status = lxp_identity_consume_sequence(identity,
                                               activity->account_sequence);
        identity_sequence_consumed = status == LXP_OK;
    }
    if (status == LXP_OK) {
        status = lxp_state_journal_commit(kernel->journal);
        if (status != LXP_OK && !kernel->journal->open) {
            lxp_result committed_status = LXP_OK;
            if (fee_policy.apply_module_effects)
                committed_status = lxp_module_ctx_commit(&module_ctx);
            else if (module_ctx_initialized)
                lxp_module_ctx_rollback(&module_ctx);
            if (fee_transaction_open)
                close_failed_fee_transaction(kernel, fee_transaction, status);
            if (committed_status == LXP_OK)
                (void)memcpy(kernel->current_state_root,
                             receipt->resulting_state_root, 32U);
            return committed_status == LXP_OK ? status : LXP_FATAL_INVARIANT;
        }
    }
    if (status == LXP_OK && fee_policy.apply_module_effects)
        status = lxp_module_ctx_commit(&module_ctx);
    else if (module_ctx_initialized)
        lxp_module_ctx_rollback(&module_ctx);
    if (status != LXP_OK) {
        if (identity_sequence_consumed)
            identity->next_sequence = identity_sequence_before;
        if (fee_transaction_open)
            close_failed_fee_transaction(kernel, fee_transaction, status);
        if (kernel->journal->open)
            (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    if (fee_transaction_open)
        kernel->fee_transaction.commit(kernel, fee_transaction);
    (void)memcpy(kernel->current_state_root, receipt->resulting_state_root, 32U);
    if (kernel->observe_commit != NULL) {
        status = kernel->observe_commit(kernel->commit_observer_context,
                                        kernel, activity, receipt);
        if (status != LXP_OK) {
            kernel->publication_poisoned = true;
            kernel->poisoned_sequence = receipt->global_sequence;
            (void)memcpy(kernel->poisoned_activity_id,
                         receipt->activity_id, 32U);
            (void)memcpy(kernel->poisoned_state_root,
                         receipt->resulting_state_root, 32U);
            return LXP_FATAL_INVARIANT;
        }
    }
    if (programs_call && fee_policy.apply_module_effects &&
        program_outcome != NULL &&
        program_outcome->terminal_kind == LXP_PROGRAM_TERMINAL_SUCCESS &&
        execution->canonical_events_out != NULL)
        *execution->canonical_events_out = projected_events;
    return LXP_OK;
}

uint8_t lxp_kernel_step_order(size_t index)
{
    static const uint8_t order[] = { 1U, 2U, 3U, 4U, 5U, 6U,
                                     7U, 8U, 9U, 10U, 11U, 12U };
    return index < sizeof(order) ? order[index] : 0U;
}
