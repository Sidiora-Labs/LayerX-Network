#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

#include "../../src/modules/programs/storage.h"

#include <string.h>

static lxp_result parameter_version(void *context, uint64_t epoch,
                                    uint32_t *version)
{
    const uint32_t *configured = (const uint32_t *)context;
    (void)epoch;
    if (configured == NULL || version == NULL || *configured == 0U)
        return LXP_ERR_NON_CANONICAL;
    *version = *configured;
    return LXP_OK;
}

static lxp_result occupancy_parameters(
    void *context, uint32_t version, lx_programs_fee_schedule *schedule,
    uint8_t asset_id[32])
{
    const lx_programs_transfer_runtime *runtime =
        (const lx_programs_transfer_runtime *)context;
    if (runtime == NULL || schedule == NULL || asset_id == NULL ||
        runtime->fee_schedule.version != version)
        return LXP_ERR_VERSION_UNSUPPORTED;
    *schedule = runtime->fee_schedule;
    (void)memcpy(asset_id, runtime->occupancy_asset_id, 32U);
    return LXP_OK;
}

static lxp_result empty_batch(lxp_replay_engine *engine, lxp_kernel *kernel,
                              uint64_t batch_number, uint64_t sequence,
                              lxp_arena *arena,
                              lxp_replay_batch_result *result)
{
    lxp_batch_body body;
    lxp_byte_span empty;
    lxp_result status;
    (void)memset(&body, 0, sizeof(body));
    body.header.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    body.header.network_id = 1U;
    body.header.epoch = kernel->epoch;
    body.header.batch_number = batch_number;
    body.header.first_sequence = sequence;
    body.header.last_sequence = sequence;
    body.header.timestamp_ms = batch_number * 1000U;
    (void)memcpy(body.header.previous_state_root,
                 kernel->current_state_root, 32U);
    status = lxp_replay_section_encode(NULL, 0U, arena, &empty);
    if (status != LXP_OK) return status;
    body.activities = empty;
    body.oracle_inputs = empty;
    return lxp_replay_batch(engine, &body, body.header.previous_state_root,
                            arena, result);
}

static lxp_result seed_principal_storage(lxp_kernel *kernel, lxp_arena *arena)
{
    lxp_module_ctx ctx;
    lxp_programs_storage_cell cell;
    uint8_t namespace_bytes[65] = {0x21U};
    uint8_t cell_key[1] = {0x31U};
    uint8_t cell_value[2] = {0x41U, 0x42U};
    uint8_t root[32];
    lxp_result status;
    namespace_bytes[32] = 0U;
    namespace_bytes[33] = 0x51U;
    cell = (lxp_programs_storage_cell){
        cell_key, (uint16_t)sizeof(cell_key), cell_value,
        (uint32_t)sizeof(cell_value)
    };
    status = lxp_module_ctx_init(&ctx, kernel, LXP_MODULE_PROGRAMS,
                                 0U, 0U, 0U, UINT64_MAX, arena, true);
    if (status == LXP_OK)
        status = lxp_programs_storage_stage_final(
            &ctx, namespace_bytes, (uint16_t)sizeof(namespace_bytes),
            &cell, 1U);
    if (status == LXP_OK) status = lxp_module_ctx_commit(&ctx);
    if (status == LXP_OK) status = lxp_state_root(kernel, root);
    if (status == LXP_OK)
        (void)memcpy(kernel->current_state_root, root, sizeof(root));
    return status;
}

int main(void)
{
    static uint8_t arena_bytes[524288];
    lxp_arena arena;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_replay_engine engine;
    lxp_replay_batch_result first;
    lxp_replay_batch_result second;
    lx_account_registry accounts;
    lxp_transfer_asset_state asset = {{0x41U}, true, false};
    lx_programs_transfer_runtime runtime;
    lxp_programs_occupancy_receipt decoded;
    uint32_t parameters = 1U;
    (void)memset(&runtime, 0, sizeof(runtime));
    runtime.accounts = &accounts;
    runtime.assets = &asset;
    runtime.asset_count = 1U;
    runtime.fee_schedule = (lx_programs_fee_schedule){
        1U, 1U, 1U, 2U, 4U, 1U, 1U, 1U
    };
    (void)memcpy(runtime.occupancy_asset_id, asset.asset_id, 32U);
    runtime.resolve_occupancy_parameters = occupancy_parameters;
    runtime.occupancy_parameter_context = &runtime;
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_PROGRAMS,
                                       &runtime) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        seed_principal_storage(&kernel, &arena) != LXP_OK ||
        lxp_replay_engine_init(&engine, parameter_version, &parameters) !=
            LXP_OK ||
        lxp_programs_replay_engine_bind(&engine, &kernel) != LXP_OK)
        return 1;
    if (empty_batch(&engine, &kernel, 1U, 0U, &arena, &first) != LXP_OK ||
        first.activity_count != 0U || first.receipt_count != 1U ||
        first.batch_maintenance_output.result_code != LXP_OK ||
        first.batch_maintenance_output.canonical_receipt.length == 0U ||
        memcmp(first.resulting_state_root, kernel.current_state_root, 32U) != 0 ||
        state.next_sequence != 1U || !state.account_root_required ||
        lxp_programs_occupancy_receipt_decode(
            first.batch_maintenance_output.canonical_receipt.bytes,
            first.batch_maintenance_output.canonical_receipt.length,
            &decoded) != LXP_OK ||
        decoded.batch_number != 1U || decoded.global_sequence != 0U ||
        decoded.parameter_version != 1U || decoded.schedule_version != 1U ||
        decoded.byte_batches.hi != 0U || decoded.byte_batches.lo != 3U ||
        !lxp_u128_is_zero(decoded.fee_units) ||
        lxp_ct_is_zero(decoded.settlement_evidence_digest, 32U) ||
        lxp_ct_is_zero(decoded.resulting_state_root, 32U))
        return 1;
    if (engine.batch_finalize(
            engine.batch_finalize_context, &(lxp_batch_header){
                .protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY,
                .batch_number = 1U,
                .first_sequence = 0U,
                .last_sequence = 0U,
                .timestamp_ms = 1000U
            }, parameters, 0U, kernel.current_state_root, &arena,
            &second.batch_maintenance_output) != LXP_ERR_IDEMPOTENT_REPLAY)
        return 1;
    runtime.fee_schedule.version = 2U;
    runtime.fee_schedule.occupancy_byte_batch = 2U;
    parameters = 2U;
    if (empty_batch(&engine, &kernel, 2U, 1U, &arena, &second) != LXP_OK ||
        second.receipt_count != 1U ||
        lxp_programs_occupancy_receipt_decode(
            second.batch_maintenance_output.canonical_receipt.bytes,
            second.batch_maintenance_output.canonical_receipt.length,
            &decoded) != LXP_OK || decoded.global_sequence != 1U ||
        decoded.schedule_version != 2U || state.next_sequence != 2U ||
        decoded.schedule_prices[6] != 2U || decoded.byte_batches.hi != 0U ||
        decoded.byte_batches.lo != 3U || !lxp_u128_is_zero(decoded.fee_units) ||
        memcmp(second.resulting_state_root,
               first.resulting_state_root, 32U) == 0)
        return 1;
    runtime.fee_schedule.occupancy_byte_batch = 3U;
    if (empty_batch(&engine, &kernel, 2U, 1U, &arena, &second) !=
        LXP_ERR_IDEMPOTENT_REPLAY)
        return 1;
    runtime.fee_schedule.version = 4U;
    parameters = 4U;
    if (empty_batch(&engine, &kernel, 3U, 2U, &arena, &second) !=
            LXP_ERR_VERSION_UNSUPPORTED || state.next_sequence != 2U)
        return 1;
    return 0;
}
