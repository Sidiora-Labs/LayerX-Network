#include "layerx/lxp_governance.h"

#include <stdint.h>
#include <string.h>

int main(void)
{
    uint8_t trigger[32] = {1U};
    uint8_t exit_conditions[32] = {2U};
    uint8_t market_a[32] = {3U};
    uint8_t market_b[32] = {4U};
    lxp_gov_emergency_state state;
    lxp_gov_emergency_state unchanged;
    uint32_t forbidden = LXP_GOV_EFFECT_BALANCE_WRITE |
        LXP_GOV_EFFECT_MINT | LXP_GOV_EFFECT_BURN |
        LXP_GOV_EFFECT_RECEIPT_REWRITE | LXP_GOV_EFFECT_BATCH_REWRITE |
        LXP_GOV_EFFECT_STATE_ROOT_SUBSTITUTE |
        LXP_GOV_EFFECT_FINALIZED_HISTORY_REASSIGN;

    if (lxp_gov_emergency_state_init(&state) != LXP_OK ||
        lxp_gov_emergency_halt(&state, LXP_PAUSE_MODULE, 2U, NULL,
                               trigger, exit_conditions, 7U, 1U,
                               true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 2U, NULL, false) !=
            LXP_ERR_PAUSED_SCOPE ||
        lxp_pause_scope_check(&state, 3U, NULL, false) != LXP_OK ||
        !state.ordering_running || !state.sealing_running ||
        !state.distribution_running || !state.checkpointing_running ||
        !state.receipts_servable || !state.inclusion_proofs_servable ||
        !state.balance_proofs_servable)
        return 1;
    unchanged = state;
    if (lxp_gov_emergency_resume(&state, LXP_PAUSE_MODULE, 2U, NULL, 2U,
                                 false, true) != LXP_ERR_AUTH_SCOPE ||
        memcmp(&state, &unchanged, sizeof(state)) != 0 ||
        lxp_gov_emergency_resume(&state, LXP_PAUSE_MODULE, 2U, NULL, 2U,
                                 true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 2U, NULL, false) != LXP_OK)
        return 1;

    if (lxp_gov_emergency_halt(&state, LXP_PAUSE_MARKET, 7U, market_a,
                               trigger, exit_conditions, 8U, 3U,
                               true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 7U, market_a, false) !=
            LXP_ERR_PAUSED_SCOPE ||
        lxp_pause_scope_check(&state, 7U, market_b, false) != LXP_OK ||
        lxp_pause_scope_check(&state, 7U, market_a, true) != LXP_OK)
        return 1;
    if (lxp_gov_emergency_halt(&state, LXP_PAUSE_NETWORK, 0U, NULL,
                               trigger, exit_conditions, 9U, 4U,
                               true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 9U, market_b, false) !=
            LXP_ERR_PAUSED_SCOPE ||
        lxp_pause_scope_check(&state, 9U, market_b, true) != LXP_OK ||
        lxp_gov_emergency_resume(&state, LXP_PAUSE_NETWORK, 0U, NULL, 5U,
                                 true, true) != LXP_OK)
        return 1;

    unchanged = state;
    if (lxp_gov_module_enable(&state, 4U, false, forbidden, 6U,
                              true, true) != LXP_ERR_AUTH_SCOPE ||
        memcmp(&state, &unchanged, sizeof(state)) != 0 ||
        lxp_gov_module_enable(&state, 4U, false, 0U, 6U,
                              true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 4U, NULL, false) !=
            LXP_ERR_PAUSED_SCOPE ||
        lxp_gov_module_enable(&state, 4U, true, 0U, 7U,
                              true, true) != LXP_OK ||
        lxp_pause_scope_check(&state, 4U, NULL, false) != LXP_OK)
        return 1;
    if (state.event_count != 7U || state.events[0].ordered_sequence != 1U ||
        !state.events[0].entered || state.events[1].entered ||
        memcmp(state.events[0].pause.trigger, trigger, 32U) != 0 ||
        memcmp(state.events[0].pause.exit_conditions,
               exit_conditions, 32U) != 0)
        return 1;
    unchanged = state;
    state.pause_count = LXP_MAX_GOV_PAUSES + 1U;
    if (lxp_pause_scope_check(&state, 2U, NULL, false) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_gov_emergency_resume(&state, LXP_PAUSE_MODULE, 2U, NULL, 8U,
                                 true, true) != LXP_ERR_NON_CANONICAL)
        return 1;
    state = unchanged;
    state.event_count = LXP_MAX_GOV_EMERGENCY_EVENTS + 1U;
    if (lxp_gov_module_enable(&state, 4U, false, 0U, 8U,
                              true, true) != LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}
