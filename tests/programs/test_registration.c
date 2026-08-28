#include "layerx/programs.h"

#include "layerx/lxp_kernel.h"

#include <string.h>

static int registration_contract(void)
{
    static const uint32_t expected_types[] = {
        LX_PROGRAMS_DEPLOY,
        LX_PROGRAMS_UPGRADE,
        LX_PROGRAMS_CALL,
        LX_PROGRAMS_REGISTRY,
        LX_PROGRAMS_TRANSFER
    };
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    const lxp_module_iface *current = programs_module_registration();
    const lxp_module_iface *next = programs_module_registration_v2();
    const lxp_module_registration *resolved;
    uint64_t parameters = 1U;
    size_t i;
    if (current == NULL || current != lx_programs_module_iface() ||
        current->module_id != LXP_MODULE_PROGRAMS ||
        current->abi_version != LX_PROGRAMS_ABI_VERSION ||
        strcmp(current->name, "programs") != 0 ||
        current->activity_type_count !=
            sizeof(expected_types) / sizeof(expected_types[0]) ||
        current->genesis == NULL || current->decode == NULL ||
        current->validate == NULL || current->execute == NULL ||
        current->epoch_begin == NULL || current->epoch_end == NULL ||
        current->state_root == NULL)
        return 1;
    if (next == NULL || next->module_id != LXP_MODULE_PROGRAMS ||
        next->abi_version != LX_PROGRAMS_ACCOUNT_ABI_VERSION ||
        strcmp(next->name, "programs") != 0 ||
        next->activity_type_count !=
            sizeof(expected_types) / sizeof(expected_types[0]) + 2U)
        return 1;
    for (i = 0U; i < current->activity_type_count; ++i)
        if (current->activity_types[i] != expected_types[i] ||
            next->activity_types[i] != expected_types[i])
            return 1;
    if (next->activity_types[next->activity_type_count - 2U] !=
            LX_PROGRAMS_ACCOUNT ||
        next->activity_types[next->activity_type_count - 1U] !=
            LX_PROGRAMS_WIND_DOWN)
        return 1;
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, current) != LXP_OK)
        return 1;
    for (i = 0U; i < current->activity_type_count; ++i)
        if (lxp_kernel_module_for_activity(&kernel, expected_types[i], 0U,
                                           &resolved) != LXP_OK ||
            resolved->iface != current ||
            resolved->abi_version != LX_PROGRAMS_ABI_VERSION)
            return 1;
    if (lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_ACCOUNT, 0U,
                                       &resolved) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        lxp_kernel_set_epoch(&kernel, 1U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, next) != LXP_OK ||
        lxp_module_version_for_epoch(&kernel, LXP_MODULE_PROGRAMS, 0U,
                                     LX_PROGRAMS_ABI_VERSION, &resolved) !=
            LXP_OK ||
        resolved->iface != current ||
        lxp_module_version_for_epoch(&kernel, LXP_MODULE_PROGRAMS, 1U,
                                     LX_PROGRAMS_ACCOUNT_ABI_VERSION,
                                     &resolved) != LXP_OK ||
        resolved->iface != next ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_ACCOUNT, 1U,
                                       &resolved) != LXP_OK ||
        resolved->iface != next ||
        lxp_kernel_module_for_activity(&kernel, LX_PROGRAMS_WIND_DOWN, 1U,
                                       &resolved) != LXP_OK ||
        resolved->iface != next ||
        lxp_module_version_for_epoch(&kernel, LXP_MODULE_PROGRAMS, 1U,
                                     LX_PROGRAMS_ABI_VERSION, &resolved) !=
            LXP_ERR_VERSION_UNSUPPORTED)
        return 1;
    return lxp_state_store_destroy(&store) == LXP_OK ? 0 : 1;
}

static int exercise(uint16_t ordinal, size_t payload_length,
                    lxp_result expected, size_t expected_writes)
{
    uint8_t arena_bytes[4096];
    uint8_t payload[40];
    lxp_arena arena;
    lxp_state_store store;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_effect_buffer effects;
    lxp_activity activity;
    lxp_authority_resolved authority;
    const lxp_module_registration *registration;
    lxp_result module_result = LXP_OK;
    uint64_t parameters = 1U;
    (void)memset(payload, 0x31, sizeof(payload));
    (void)memset(&activity, 0, sizeof(activity));
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(authority.principal, 0x42, sizeof(authority.principal));
    activity.activity_type = ((uint32_t)LXP_MODULE_PROGRAMS << 16U) | ordinal;
    activity.payload = (lxp_byte_span){ payload, payload_length };
    if (lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters, 0U) !=
            LXP_OK ||
        lxp_kernel_register_module(&kernel, programs_module_registration()) !=
            LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, activity.activity_type, 0U,
                                       &registration) != LXP_OK ||
        registration->abi_version != LX_PROGRAMS_ABI_VERSION ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PROGRAMS, 1U, 0U, 9U,
                            1000U, &arena, false) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK ||
        lxp_kernel_dispatch(registration, &ctx, &activity, &authority,
                            &effects, &module_result) != LXP_OK)
        return 1;
    if (module_result != expected) return 1;
    if (module_result == LXP_OK) {
        if (lxp_module_ctx_commit(&ctx) != LXP_OK || effects.count != 1U ||
            effects.effects[0].module_id != LXP_MODULE_PROGRAMS ||
            effects.effects[0].event_type != ordinal ||
            kernel.module_kv_count != expected_writes)
            return 1;
    } else {
        lxp_module_ctx_rollback(&ctx);
        if (kernel.module_kv_count != 0U || effects.count != 0U) return 1;
    }
    return lxp_state_store_destroy(&store) == LXP_OK ? 0 : 1;
}

int main(void)
{
    if (lxp_programs_abi_transition_validate(0U, LX_PROGRAMS_ABI_VERSION) != LXP_OK ||
        lxp_programs_abi_transition_validate(0U, LX_PROGRAMS_ACCOUNT_ABI_VERSION) != LXP_OK ||
        lxp_programs_abi_transition_validate(LX_PROGRAMS_ABI_VERSION,
                                             LX_PROGRAMS_ACCOUNT_ABI_VERSION) != LXP_OK ||
        lxp_programs_abi_transition_validate(LX_PROGRAMS_ACCOUNT_ABI_VERSION,
                                             LX_PROGRAMS_ABI_VERSION) !=
            LXP_ERR_VERSION_UNSUPPORTED ||
        lxp_programs_abi_transition_validate(0U,
                                             LX_PROGRAMS_ACCOUNT_ABI_VERSION + 1U) !=
            LXP_ERR_VERSION_UNSUPPORTED)
        return 1;
    if (registration_contract() != 0) return 1;
    if (exercise(lxp_activity_type_ordinal(LX_PROGRAMS_CALL), 40U,
                 LXP_ERR_MODULE_DISABLED, 0U) != 0) return 1;
    if (exercise(lxp_activity_type_ordinal(LX_PROGRAMS_REGISTRY), 32U,
                 LXP_OK, 0U) != 0) return 1;
    if (exercise(lxp_activity_type_ordinal(LX_PROGRAMS_CALL), 31U,
                 LXP_ERR_MODULE_DISABLED, 0U) != 0) return 1;
    return 0;
}
