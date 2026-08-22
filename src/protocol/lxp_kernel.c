#include "layerx/lxp_kernel.h"

#include "layerx/lxp_admission.h"
#include "layerx/lxp_hash.h"
#include "layerx/programs.h"

#include "../modules/programs/event.h"

#include <limits.h>
#include <string.h>

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

lxp_result lxp_kernel_bind_module_runtime(lxp_kernel *kernel,
                                          uint16_t module_id,
                                          void *runtime)
{
    if (kernel == NULL || runtime == NULL || module_id == 0U ||
        module_id > LXP_MODULE_RESERVED_COUNT)
        return LXP_ERR_NON_CANONICAL;
    kernel->module_runtime[module_id] = runtime;
    if (module_id == LXP_MODULE_PROGRAMS &&
        kernel->fee_transaction.prepare == NULL)
        return lxp_programs_bind_fee_transaction(kernel);
    return LXP_OK;
}

static lxp_result validate_iface(const lxp_module_iface *iface)
{
    size_t i;
    if (iface == NULL || iface->module_id == 0U ||
        iface->module_id > LXP_MODULE_RESERVED_COUNT ||
        iface->abi_version == 0U || iface->name == NULL ||
        strcmp(iface->name, module_names[iface->module_id - 1U]) != 0 ||
        iface->activity_types == NULL || iface->activity_type_count == 0U ||
        iface->activity_type_count > LXP_MODULE_MAX_ACTIVITY_TYPES ||
        iface->genesis == NULL || iface->decode == NULL ||
        iface->validate == NULL || iface->execute == NULL ||
        iface->epoch_begin == NULL || iface->epoch_end == NULL ||
        iface->state_root == NULL) return LXP_ERR_UNKNOWN_MODULE;
    if (strlen(iface->name) > LXP_MODULE_MAX_NAME)
        return LXP_ERR_LENGTH_LIMIT;
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
    lxp_program_outcome program_outcome;
} compact_receipt;

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

static lxp_result receipt_restore_compact(const uint8_t *bytes, size_t length,
                                          lxp_receipt *receipt)
{
    const compact_receipt *compact;
    if (bytes == NULL || receipt == NULL)
        return LXP_FATAL_REPLAY_DIVERGENCE;
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
    receipt->program_outcome = compact->program_outcome;
    return LXP_OK;
}

static lxp_result receipt_store(lxp_state_journal *journal,
                                const lxp_activity *activity,
                                const lxp_receipt *receipt)
{
    compact_receipt compact;
    (void)memset(&compact, 0, sizeof(compact));
    compact.protocol_version = receipt->protocol_version;
    (void)memcpy(compact.activity_id, receipt->activity_id, 32U);
    compact.global_sequence = receipt->global_sequence;
    (void)memcpy(compact.previous_state_root, receipt->previous_state_root, 32U);
    (void)memcpy(compact.resulting_state_root, receipt->resulting_state_root,
                 32U);
    (void)memcpy(compact.activity_root, receipt->activity_root, 32U);
    compact.result_code = receipt->result_code;
    compact.fee_charged = receipt->fee_charged;
    (void)memcpy(compact.batch_id, receipt->batch_id, 32U);
    compact.module_id = receipt->module_id;
    compact.module_version = receipt->module_version;
    compact.parameter_version = receipt->parameter_version;
    compact.program_outcome = receipt->program_outcome;
    return lxp_idempotency_record(journal, activity->actor_did.bytes,
                                  activity->actor_did.length,
                                  activity->idempotency_key,
                                  (const uint8_t *)&compact,
                                  sizeof(compact));
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
    lxp_u128 pre_runtime_fee, lxp_program_outcome *outcome)
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
    outcome->fee_schedule_version = execution->fee_parameters->version;
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
    if (execution != NULL && execution->canonical_events_out != NULL)
        *execution->canonical_events_out = (lxp_byte_span){NULL, 0U};
    if (kernel == NULL || activity == NULL || execution == NULL ||
        receipt == NULL || execution->identities == NULL ||
        execution->authority == NULL || execution->fee_parameters == NULL ||
        execution->arena == NULL) return LXP_ERR_NON_CANONICAL;
    arena_mark = lxp_arena_mark(execution->arena);
    programs_call = lxp_activity_module_id(activity->activity_type) ==
                        LXP_MODULE_PROGRAMS &&
                    lxp_activity_type_ordinal(activity->activity_type) == 3U;
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
                execution->fee_parameters->version;
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
                    &synthetic_program_outcome);
                if (status == LXP_OK)
                    program_outcome = &synthetic_program_outcome;
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
            kernel->fee_transaction.rollback(kernel, fee_transaction);
        (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    if (fee_policy.apply_module_effects)
        status = lxp_module_ctx_prepare_commit(&module_ctx);
    if (status != LXP_OK) {
        lxp_module_ctx_rollback(&module_ctx);
        if (fee_transaction_open)
            kernel->fee_transaction.rollback(kernel, fee_transaction);
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
    if (status == LXP_OK) status = lxp_state_journal_commit(kernel->journal);
    if (status == LXP_OK && fee_policy.apply_module_effects)
        status = lxp_module_ctx_commit(&module_ctx);
    else if (module_ctx_initialized)
        lxp_module_ctx_rollback(&module_ctx);
    if (status != LXP_OK) {
        if (identity_sequence_consumed)
            identity->next_sequence = identity_sequence_before;
        if (fee_transaction_open)
            kernel->fee_transaction.rollback(kernel, fee_transaction);
        if (kernel->journal->open)
            (void)lxp_state_journal_rollback(kernel->journal);
        return status;
    }
    if (fee_transaction_open)
        kernel->fee_transaction.commit(kernel, fee_transaction);
    (void)memcpy(kernel->current_state_root, receipt->resulting_state_root, 32U);
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
