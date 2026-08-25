#include "layerx/lxp_governance.h"

#include <string.h>

static int state_counts_valid(const lxp_gov_emergency_state *state)
{
    return state != NULL && state->pause_count <= LXP_MAX_GOV_PAUSES &&
           state->event_count <= LXP_MAX_GOV_EMERGENCY_EVENTS;
}

static int zero_id(const uint8_t id[32])
{
    uint8_t combined = 0U;
    size_t i;
    if (id == NULL) return 1;
    for (i = 0U; i < 32U; ++i) combined |= id[i];
    return combined == 0U;
}

static int same_scope(const lxp_gov_pause_record *pause,
                      lxp_pause_scope scope, uint16_t module_id,
                      const uint8_t market_id[32])
{
    if (pause->scope != scope || pause->module_id != module_id) return 0;
    return scope != LXP_PAUSE_MARKET ||
        memcmp(pause->market_id, market_id, 32U) == 0;
}

static lxp_result ordered(lxp_gov_emergency_state *state,
                          uint64_t ordered_sequence,
                          bool governance_authorized,
                          bool ordered_governance_activity)
{
    if (!governance_authorized || !ordered_governance_activity)
        return LXP_ERR_AUTH_SCOPE;
    if (ordered_sequence == 0U ||
        ordered_sequence != state->last_ordered_sequence + 1U)
        return LXP_ERR_SEQUENCE_MISMATCH;
    return LXP_OK;
}

static lxp_result event_append(lxp_gov_emergency_state *state,
                               const lxp_gov_pause_record *pause,
                               uint64_t ordered_sequence, bool entered)
{
    lxp_gov_emergency_event *event;
    if (!state_counts_valid(state)) return LXP_ERR_NON_CANONICAL;
    if (state->event_count == LXP_MAX_GOV_EMERGENCY_EVENTS)
        return LXP_ERR_LENGTH_LIMIT;
    event = &state->events[state->event_count++];
    event->ordered_sequence = ordered_sequence;
    event->pause = *pause;
    event->entered = entered;
    state->last_ordered_sequence = ordered_sequence;
    return LXP_OK;
}

lxp_result lxp_gov_emergency_state_init(lxp_gov_emergency_state *state)
{
    if (state == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memset(state, 0, sizeof(*state));
    (void)memset(state->module_enabled, 1, sizeof(state->module_enabled));
    state->ordering_running = true;
    state->sealing_running = true;
    state->distribution_running = true;
    state->checkpointing_running = true;
    state->receipts_servable = true;
    state->inclusion_proofs_servable = true;
    state->balance_proofs_servable = true;
    return LXP_OK;
}

lxp_result lxp_gov_emergency_halt(
    lxp_gov_emergency_state *state, lxp_pause_scope scope,
    uint16_t module_id, const uint8_t market_id[32],
    const uint8_t trigger[32], const uint8_t exit_conditions[32],
    uint64_t entry_epoch, uint64_t ordered_sequence,
    bool governance_authorized, bool ordered_governance_activity)
{
    lxp_gov_pause_record pause;
    size_t i;
    lxp_result status;
    if (!state_counts_valid(state) || trigger == NULL ||
        exit_conditions == NULL ||
        entry_epoch == 0U || scope < LXP_PAUSE_MODULE ||
        scope > LXP_PAUSE_NETWORK ||
        (scope == LXP_PAUSE_NETWORK && module_id != 0U) ||
        (scope != LXP_PAUSE_NETWORK && module_id == 0U) ||
        (scope == LXP_PAUSE_MARKET && zero_id(market_id)) ||
        state->pause_count == LXP_MAX_GOV_PAUSES)
        return LXP_ERR_NON_CANONICAL;
    status = ordered(state, ordered_sequence, governance_authorized,
                     ordered_governance_activity);
    if (status != LXP_OK) return status;
    for (i = 0U; i < state->pause_count; ++i)
        if (state->pauses[i].active && same_scope(
                &state->pauses[i], scope, module_id, market_id))
            return LXP_ERR_PAUSED_SCOPE;
    (void)memset(&pause, 0, sizeof(pause));
    pause.scope = scope;
    pause.module_id = module_id;
    if (scope == LXP_PAUSE_MARKET)
        (void)memcpy(pause.market_id, market_id, 32U);
    (void)memcpy(pause.trigger, trigger, 32U);
    (void)memcpy(pause.exit_conditions, exit_conditions, 32U);
    pause.entry_epoch = entry_epoch;
    pause.active = true;
    status = event_append(state, &pause, ordered_sequence, true);
    if (status != LXP_OK) return status;
    state->pauses[state->pause_count++] = pause;
    return LXP_OK;
}

lxp_result lxp_gov_emergency_resume(
    lxp_gov_emergency_state *state, lxp_pause_scope scope,
    uint16_t module_id, const uint8_t market_id[32],
    uint64_t ordered_sequence, bool governance_authorized,
    bool ordered_governance_activity)
{
    size_t i;
    lxp_result status;
    if (!state_counts_valid(state) || scope < LXP_PAUSE_MODULE ||
        scope > LXP_PAUSE_NETWORK)
        return LXP_ERR_NON_CANONICAL;
    status = ordered(state, ordered_sequence, governance_authorized,
                     ordered_governance_activity);
    if (status != LXP_OK) return status;
    for (i = 0U; i < state->pause_count; ++i)
        if (state->pauses[i].active && same_scope(
                &state->pauses[i], scope, module_id, market_id)) {
            status = event_append(state, &state->pauses[i],
                                  ordered_sequence, false);
            if (status == LXP_OK) state->pauses[i].active = false;
            return status;
        }
    return LXP_ERR_NON_CANONICAL;
}

lxp_result lxp_pause_scope_check(
    const lxp_gov_emergency_state *state, uint16_t module_id,
    const uint8_t market_id[32], bool cancellation_or_exit_path)
{
    size_t i;
    if (!state_counts_valid(state) || module_id == 0U ||
        module_id > LXP_MAX_GOV_MODULE_ID)
        return LXP_ERR_NON_CANONICAL;
    if (cancellation_or_exit_path) return LXP_OK;
    if (!state->module_enabled[module_id]) return LXP_ERR_PAUSED_SCOPE;
    for (i = 0U; i < state->pause_count; ++i) {
        const lxp_gov_pause_record *pause = &state->pauses[i];
        if (!pause->active) continue;
        if (pause->scope == LXP_PAUSE_NETWORK ||
            (pause->scope == LXP_PAUSE_MODULE &&
             pause->module_id == module_id) ||
            (pause->scope == LXP_PAUSE_MARKET &&
             pause->module_id == module_id && market_id != NULL &&
             memcmp(pause->market_id, market_id, 32U) == 0))
            return LXP_ERR_PAUSED_SCOPE;
    }
    return LXP_OK;
}

lxp_result lxp_gov_module_enable(
    lxp_gov_emergency_state *state, uint16_t module_id, bool enabled,
    uint32_t attempted_effect_mask, uint64_t ordered_sequence,
    bool governance_authorized, bool ordered_governance_activity)
{
    lxp_gov_pause_record control;
    lxp_result status;
    if (!state_counts_valid(state) || module_id == 0U ||
        module_id > LXP_MAX_GOV_MODULE_ID)
        return LXP_ERR_NON_CANONICAL;
    if (attempted_effect_mask != 0U) return LXP_ERR_AUTH_SCOPE;
    status = ordered(state, ordered_sequence, governance_authorized,
                     ordered_governance_activity);
    if (status != LXP_OK) return status;
    (void)memset(&control, 0, sizeof(control));
    control.scope = LXP_PAUSE_MODULE;
    control.module_id = module_id;
    control.active = !enabled;
    status = event_append(state, &control, ordered_sequence, !enabled);
    if (status == LXP_OK) state->module_enabled[module_id] = enabled;
    return status;
}
