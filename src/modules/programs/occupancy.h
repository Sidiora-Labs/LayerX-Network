#ifndef LAYERX_PROGRAMS_OCCUPANCY_INTERNAL_H
#define LAYERX_PROGRAMS_OCCUPANCY_INTERNAL_H

#include "layerx/programs.h"
#include "layerx/lxp_kernel.h"

enum {
    LXP_PROGRAMS_OCCUPANCY_MAX_LEDGER_BYTES = 60000,
    LXP_PROGRAMS_OCCUPANCY_MAX_EVIDENCE_BYTES = 65536,
    LXP_PROGRAMS_OCCUPANCY_MAX_CHUNKS = 60,
    LXP_PROGRAMS_OCCUPANCY_MAX_POSITIONS = 256
};

typedef struct lxp_programs_occupancy_activation_position {
    uint8_t namespace_bytes[65];
    uint8_t namespace_length;
    uint8_t payer[32];
    uint64_t persistent_bytes;
} lxp_programs_occupancy_activation_position;

typedef struct lxp_programs_occupancy_payer {
    uint8_t principal[32];
    lxp_u128 due;
    lxp_u128 paid;
    lxp_u128 arrears;
    lxp_u128 verified_due;
    lxp_u128 verified_paid;
    lxp_u128 verified_arrears;
    bool frozen;
} lxp_programs_occupancy_payer;

typedef struct lxp_programs_occupancy_bridge {
    lxp_module_ctx *ctx;
    const uint8_t *current_ledger;
    uint32_t current_ledger_length;
    uint16_t current_ledger_chunks;
    uint64_t current_batch;
    uint32_t current_schedule_version;
    uint64_t current_schedule_prices[7];
    uint8_t current_asset_id[32];
    uint64_t finalized_batch;
    uint64_t global_sequence;
    uint32_t parameter_version;
    uint64_t resolved_schedule_prices[7];
    uint8_t resolved_asset_id[32];
    uint8_t *next_ledger;
    uint32_t next_ledger_length;
    uint32_t next_ledger_written;
    uint8_t *evidence;
    uint32_t evidence_length;
    uint32_t evidence_written;
    lxp_programs_occupancy_payer payers[LXP_PROGRAMS_OCCUPANCY_MAX_PAYERS];
    uint16_t payer_count;
    uint16_t payers_written;
    uint64_t batch_number;
    uint32_t schedule_version;
    lxp_u128 byte_batches;
    lxp_u128 fee_units;
    lxp_u128 paid_fee_units;
    lxp_u128 arrears_fee_units;
    lxp_programs_occupancy_receipt receipt;
    lxp_programs_occupancy_activation_position
        activation_positions[LXP_PROGRAMS_OCCUPANCY_MAX_POSITIONS];
    uint16_t activation_count;
    uint8_t authorized_root_program[32];
    uint8_t authorized_payer[32];
    uint8_t authorized_activity_binding[32];
    lxp_u128 authorized_responsibility_ceiling;
    bool begun;
    bool applied;
    bool finalizing;
    bool uninitialized;
    bool call_authorized;
} lxp_programs_occupancy_bridge;

lxp_result lxp_programs_occupancy_bridge_init(
    lxp_programs_occupancy_bridge *bridge, lxp_module_ctx *ctx);
lxp_result lxp_programs_occupancy_bind_call(
    lxp_programs_occupancy_bridge *bridge, const uint8_t root_program[32],
    const uint64_t budget[LX_PROGRAMS_CALL_BUDGET_FIELDS]);
lxp_result lxp_ctx_emit_programs_maintenance_transfer_set(
    lxp_module_ctx *ctx, const lxp_transfer_set *set, lxp_receipt *receipt);
lxp_result layerx_programs_occupancy_ledger_length(uint64_t token);
lxp_result layerx_programs_occupancy_ledger_byte(uint64_t token,
                                                 uint32_t offset);
lxp_result layerx_programs_occupancy_activation_count(uint64_t token);
lxp_result layerx_programs_occupancy_activation_record_length(
    uint64_t token, uint16_t index);
lxp_result layerx_programs_occupancy_activation_record_byte(
    uint64_t token, uint16_t index, uint16_t offset);
lxp_result layerx_programs_occupancy_output_begin(
    uint64_t token, uint64_t batch_number, uint32_t parameter_version,
    uint32_t schedule_version,
    uint32_t ledger_length, uint32_t evidence_length, uint16_t payer_count,
    uint64_t byte_batches_hi, uint64_t byte_batches_lo,
    uint64_t fee_units_hi, uint64_t fee_units_lo,
    uint64_t paid_hi, uint64_t paid_lo,
    uint64_t arrears_hi, uint64_t arrears_lo);
lxp_result layerx_programs_occupancy_output_payer(
    uint64_t token, uint16_t index,
    uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t due_hi, uint64_t due_lo,
    uint64_t paid_hi, uint64_t paid_lo,
    uint64_t arrears_hi, uint64_t arrears_lo, uint8_t frozen);
lxp_result layerx_programs_occupancy_payer_available(
    uint64_t token, uint64_t p0, uint64_t p1, uint64_t p2, uint64_t p3,
    uint64_t fee_hi, uint64_t fee_lo);
lxp_result layerx_programs_occupancy_output_byte(
    uint64_t token, uint16_t section, uint32_t offset, uint8_t byte);
lxp_result layerx_programs_occupancy_output_apply(uint64_t token);
lxp_result layerx_programs_occupancy_finalize_rust(
    uint64_t token, uint64_t batch_number, uint32_t parameter_version,
    uint32_t schedule_version,
    uint64_t fee_cpu, uint64_t fee_memory_byte,
    uint64_t fee_storage_read_byte, uint64_t fee_storage_write_byte,
    uint64_t fee_output_value, uint64_t fee_output_byte,
    uint64_t fee_occupancy_byte_batch);

#endif
