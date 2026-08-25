#ifndef LAYERX_LXP_GOVERNANCE_H
#define LAYERX_LXP_GOVERNANCE_H

#include "layerx/lxp_receipt.h"

#include <stdbool.h>
#include <stddef.h>
#include <stdint.h>

enum {
    LXP_MAX_PARAMETERS = 64,
    LXP_MAX_PARAMETER_KEY_BYTES = 64,
    LXP_MAX_PARAMETER_HISTORY = 32,
    LXP_MAX_PARAMETER_VERSIONS = 256,
    LXP_MAX_GOV_PROPOSALS = 64,
    LXP_MAX_GOV_COHORT_MEMBERS = 32,
    LXP_MAX_GOV_PAUSES = 64,
    LXP_MAX_GOV_EMERGENCY_EVENTS = 128,
    LXP_MAX_GOV_MODULE_ID = 255
};

typedef enum lxp_gov_rollout_scope {
    LXP_GOV_ROLLOUT_ALL = 1,
    LXP_GOV_ROLLOUT_MODULE = 2,
    LXP_GOV_ROLLOUT_MARKET = 3,
    LXP_GOV_ROLLOUT_ACCOUNT_SET = 4
} lxp_gov_rollout_scope;

typedef enum lxp_pause_scope {
    LXP_PAUSE_MODULE = 1,
    LXP_PAUSE_MARKET = 2,
    LXP_PAUSE_NETWORK = 3
} lxp_pause_scope;

enum {
    LXP_GOV_EFFECT_BALANCE_WRITE = 1U << 0,
    LXP_GOV_EFFECT_MINT = 1U << 1,
    LXP_GOV_EFFECT_BURN = 1U << 2,
    LXP_GOV_EFFECT_RECEIPT_REWRITE = 1U << 3,
    LXP_GOV_EFFECT_BATCH_REWRITE = 1U << 4,
    LXP_GOV_EFFECT_STATE_ROOT_SUBSTITUTE = 1U << 5,
    LXP_GOV_EFFECT_FINALIZED_HISTORY_REASSIGN = 1U << 6
};

typedef struct lxp_param_value_record {
    uint64_t value;
    uint64_t activation_epoch;
    uint32_t parameter_version;
    uint8_t proposal_id[32];
} lxp_param_value_record;

typedef struct lxp_param_entry {
    uint8_t key[LXP_MAX_PARAMETER_KEY_BYTES];
    size_t key_length;
    uint16_t target_module;
    uint64_t minimum_value;
    uint64_t maximum_value;
    uint64_t proposed_value;
    uint8_t proposal_id[32];
    lxp_param_value_record history[LXP_MAX_PARAMETER_HISTORY];
    size_t history_count;
} lxp_param_entry;

typedef struct lxp_param_version_record {
    uint32_t parameter_version;
    uint64_t activation_epoch;
} lxp_param_version_record;

typedef struct lxp_gov_param_proposal {
    uint8_t proposal_id[32];
    uint16_t target_module;
    uint8_t parameter_key[LXP_MAX_PARAMETER_KEY_BYTES];
    size_t parameter_key_length;
    uint64_t proposed_value;
    uint64_t activation_epoch;
    uint64_t ordered_sequence;
    lxp_gov_rollout_scope rollout_scope;
    uint8_t cohort[LXP_MAX_GOV_COHORT_MEMBERS][32];
    size_t cohort_count;
    uint32_t parameter_version;
    bool enacted;
} lxp_gov_param_proposal;

typedef struct lxp_param_table {
    lxp_param_entry entries[LXP_MAX_PARAMETERS];
    size_t count;
    lxp_param_version_record versions[LXP_MAX_PARAMETER_VERSIONS];
    size_t version_count;
    uint32_t current_version;
    uint64_t last_sealed_epoch;
    lxp_gov_param_proposal proposals[LXP_MAX_GOV_PROPOSALS];
    size_t proposal_count;
} lxp_param_table;
#define lxp_param_table lxp_param_table

typedef struct lxp_gov_pause_record {
    lxp_pause_scope scope;
    uint16_t module_id;
    uint8_t market_id[32];
    uint8_t trigger[32];
    uint8_t exit_conditions[32];
    uint64_t entry_epoch;
    bool active;
} lxp_gov_pause_record;

typedef struct lxp_gov_emergency_event {
    uint64_t ordered_sequence;
    lxp_gov_pause_record pause;
    bool entered;
} lxp_gov_emergency_event;

typedef struct lxp_gov_emergency_state {
    lxp_gov_pause_record pauses[LXP_MAX_GOV_PAUSES];
    size_t pause_count;
    lxp_gov_emergency_event events[LXP_MAX_GOV_EMERGENCY_EVENTS];
    size_t event_count;
    uint64_t last_ordered_sequence;
    bool module_enabled[LXP_MAX_GOV_MODULE_ID + 1U];
    bool ordering_running;
    bool sealing_running;
    bool distribution_running;
    bool checkpointing_running;
    bool receipts_servable;
    bool inclusion_proofs_servable;
    bool balance_proofs_servable;
} lxp_gov_emergency_state;

lxp_result lxp_param_table_init(lxp_param_table *table);
lxp_result lxp_param_table_validate(const lxp_param_table *table);
lxp_result lxp_param_set_bounds(
    lxp_param_table *table, lxp_byte_span key, uint16_t target_module,
    uint64_t minimum_value, uint64_t maximum_value, uint64_t initial_value,
    uint64_t activation_epoch);
lxp_result lxp_param_apply_ordered(
    lxp_param_table *table, lxp_byte_span key, uint64_t value,
    uint64_t activation_epoch, const uint8_t proposal_id[32],
    bool ordered_governance_activity);
lxp_result lxp_param_get(const lxp_param_table *table, lxp_byte_span key,
                         uint64_t execution_epoch, uint64_t *value,
                         uint32_t *parameter_version);
lxp_result lxp_param_version(const lxp_param_table *table,
                             uint64_t execution_epoch,
                             uint32_t *parameter_version);
lxp_result lxp_param_at(const lxp_param_table *table, size_t index,
                        const lxp_param_entry **entry);
lxp_result lxp_param_mark_sealed(lxp_param_table *table, uint64_t epoch);
lxp_result lxp_gov_stage_cohort(
    lxp_gov_param_proposal *proposal, lxp_gov_rollout_scope rollout_scope,
    const uint8_t (*cohort)[32], size_t cohort_count);
lxp_result lxp_gov_param_propose(
    lxp_param_table *table, const lxp_gov_param_proposal *proposal,
    uint64_t current_epoch, uint64_t minimum_activation_delay,
    bool governance_authorized, bool ordered_governance_activity);
lxp_result lxp_gov_activation_apply(lxp_param_table *table,
                                    uint64_t batch_epoch,
                                    bool first_batch_of_epoch);
lxp_result lxp_gov_param_enact(
    const lxp_param_table *table, lxp_byte_span key, uint64_t execution_epoch,
    const uint8_t cohort_id[32], uint64_t *value,
    uint32_t *parameter_version);
lxp_result lxp_gov_parameter_state_root(
    const lxp_param_table *table, uint64_t execution_epoch,
    const uint8_t cohort_id[32], uint8_t root[32]);
lxp_result lxp_gov_emergency_state_init(lxp_gov_emergency_state *state);
lxp_result lxp_gov_emergency_halt(
    lxp_gov_emergency_state *state, lxp_pause_scope scope,
    uint16_t module_id, const uint8_t market_id[32],
    const uint8_t trigger[32], const uint8_t exit_conditions[32],
    uint64_t entry_epoch, uint64_t ordered_sequence,
    bool governance_authorized, bool ordered_governance_activity);
lxp_result lxp_gov_emergency_resume(
    lxp_gov_emergency_state *state, lxp_pause_scope scope,
    uint16_t module_id, const uint8_t market_id[32],
    uint64_t ordered_sequence, bool governance_authorized,
    bool ordered_governance_activity);
lxp_result lxp_pause_scope_check(
    const lxp_gov_emergency_state *state, uint16_t module_id,
    const uint8_t market_id[32], bool cancellation_or_exit_path);
lxp_result lxp_gov_module_enable(
    lxp_gov_emergency_state *state, uint16_t module_id, bool enabled,
    uint32_t attempted_effect_mask, uint64_t ordered_sequence,
    bool governance_authorized, bool ordered_governance_activity);

#endif
