#ifndef LAYERX_LXP_FEE_H
#define LAYERX_LXP_FEE_H

#include "layerx/lxp_admission.h"
#include "layerx/lxp_governance.h"
#include "layerx/lxp_transfer.h"

#include <stdbool.h>
#include <stdint.h>

typedef struct lxp_fee_meter {
    uint64_t canonical_encoded_bytes;
    uint64_t execution_units;
    uint64_t storage_units;
    bool exact_program_fee_present;
    uint32_t program_fee_schedule_version;
    lxp_u128 exact_program_fee_units;
} lxp_fee_meter;
#define lxp_fee_meter lxp_fee_meter

typedef struct lxp_fee_params {
    uint16_t version;
    lxp_u128 base_fee;
    lxp_u128 per_activity_type_unit;
    lxp_u128 per_encoded_byte;
    lxp_u128 per_execution_unit;
    lxp_u128 per_storage_unit;
    uint32_t multiplier_basis_points;
} lxp_fee_params;
#define lxp_fee_params lxp_fee_params

typedef struct lxp_meter_ctx {
    uint64_t execution_units;
    uint64_t net_storage_bytes;
    uint64_t execution_ceiling;
    uint64_t storage_ceiling;
    lxp_u128 storage_rate;
    lxp_u128 storage_fee;
    lxp_u128 fee_limit;
    uint32_t parameter_version;
    bool single_writer_bound;
    bool exhausted;
} lxp_meter_ctx;
#define lxp_meter_ctx lxp_meter_ctx

typedef struct lxp_fee_policy_decision {
    lxp_result result_code;
    lxp_u128 fee_charged;
    bool assign_global_sequence;
    bool consume_account_sequence;
    bool charge_fee;
    bool apply_module_effects;
} lxp_fee_policy_decision;
#define lxp_fee_policy_decision lxp_fee_policy_decision

typedef struct lxp_fee_replay_entry {
    lxp_u128 fee_charged;
    lxp_u128 actor_fee_debit;
    lxp_u128 treasury_fee_credit;
    lxp_u128 treasury_balance;
    uint8_t resulting_state_root[32];
    uint32_t parameter_version;
} lxp_fee_replay_entry;
#define lxp_fee_replay_entry lxp_fee_replay_entry

lxp_result lxp_fee_compute(const lxp_fee_params *parameters,
                           uint32_t activity_type, lxp_fee_meter meter,
                           lxp_u128 *fee);
lxp_result lxp_fee_limit_check(lxp_u128 computed_fee, lxp_u128 fee_limit,
                               lxp_u128 actor_spendable_fee_balance);
lxp_result lxp_fee_schedule(
    const lxp_param_table *parameters, uint64_t batch_epoch,
    const uint8_t cohort_id[32], lxp_fee_params *schedule,
    uint32_t *parameter_version);
lxp_result lxp_fee_treasury_account(lx_account_registry *registry,
                                    lx_account **treasury);
lxp_result lxp_fee_charge(
    lx_account *actor_main, lx_account *treasury, const uint8_t asset_id[32],
    lxp_u128 fee, lxp_u128 fee_limit, lxp_transfer_context *context,
    lxp_receipt *receipt, lxp_transfer_result *transfer_result);
lxp_result lxp_meter_init(
    lxp_meter_ctx *meter, uint64_t execution_ceiling,
    uint64_t storage_ceiling, lxp_u128 storage_rate, lxp_u128 fee_limit,
    uint32_t parameter_version, bool single_writer_bound);
lxp_result lxp_meter_charge_exec(lxp_meter_ctx *meter, uint64_t units);
lxp_result lxp_meter_charge_storage(lxp_meter_ctx *meter,
                                    int64_t net_byte_delta);
lxp_result lxp_meter_exhausted(const lxp_meter_ctx *meter);
lxp_result lxp_meter_fee_usage(const lxp_meter_ctx *meter,
                               uint64_t canonical_encoded_bytes,
                               lxp_fee_meter *usage);
lxp_result lxp_meter_admission_check(bool fee_limit_present,
                                     bool canonical_nonnegative_integer,
                                     lxp_u128 fee_limit,
                                     lxp_u128 actor_spendable_fee_balance);
lxp_result lxp_fee_admission_check(
    lxp_admission_result admission, lxp_u128 fee_limit,
    lxp_u128 actor_spendable_fee_balance,
    lxp_fee_policy_decision *decision);
lxp_result lxp_fee_rejection_policy(
    const lxp_fee_policy_decision *admission, lxp_result execution_result,
    lxp_u128 computed_fee, lxp_u128 fee_limit,
    lxp_fee_policy_decision *decision);
lxp_result lxp_fee_replay_check(
    const lxp_fee_replay_entry *committed,
    const lxp_fee_replay_entry *replayed, size_t count,
    lxp_u128 initial_treasury_balance);

#endif
