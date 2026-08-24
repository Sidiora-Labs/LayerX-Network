#include "layerx/programs.h"

#include "layerx/lxp_crypto.h"
#include "layerx/lxp_kernel.h"

static bool state_event(uint16_t event_type)
{
    return event_type >= LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED &&
           event_type <= LX_PROGRAMS_EVENT_VALUE_EXITED;
}

static lxp_result event_program(const lxp_effect *effect,
                                const uint8_t **program)
{
    static const uint8_t account_magic[5] = {'L', 'X', 'P', 'A', '1'};
    size_t offset;
    if (effect == NULL || program == NULL) return LXP_ERR_NON_CANONICAL;
    if (effect->event_type == LX_PROGRAMS_EVENT_ACCOUNT_REGISTERED) {
        if (effect->body_length != 143U ||
            lxp_ct_memcmp(effect->body, account_magic,
                          sizeof(account_magic)) != 0)
            return LXP_FATAL_INVARIANT;
        offset = sizeof(account_magic);
    } else if (effect->event_type == LX_PROGRAMS_EVENT_DEPRECATED ||
               effect->event_type == LX_PROGRAMS_EVENT_TOMBSTONED) {
        if (effect->body_length != 119U) return LXP_FATAL_INVARIANT;
        offset = 35U;
    } else if (effect->event_type == LX_PROGRAMS_EVENT_EXIT_ROUTE) {
        if (effect->body_length != 128U) return LXP_FATAL_INVARIANT;
        offset = 0U;
    } else if (effect->event_type == LX_PROGRAMS_EVENT_VALUE_EXITED) {
        if (effect->body_length != 96U) return LXP_FATAL_INVARIANT;
        offset = 0U;
    } else {
        return LXP_ERR_UNKNOWN_FIELD;
    }
    if (lxp_ct_is_zero(effect->body + offset, 32U))
        return LXP_FATAL_INVARIANT;
    *program = effect->body + offset;
    return LXP_OK;
}

lxp_result lxp_programs_state_feed_observe(
    const lx_programs_state_feed *feed, const lxp_activity *activity,
    const lxp_receipt *receipt)
{
    const uint8_t *activity_program;
    uint32_t ordinal = 0U;
    size_t index;
    lxp_result status;
    if (feed == NULL || feed->begin == NULL || feed->append == NULL ||
        feed->advance == NULL || feed->lock == NULL || feed->unlock == NULL ||
        feed->context == NULL ||
        activity == NULL || receipt == NULL)
        return LXP_FATAL_INVARIANT;
    status = feed->lock(feed->context);
    if (status != LXP_OK) return status;
    status = feed->begin(feed->context, activity, receipt);
    if (status != LXP_OK) goto finish;
    if (receipt->result_code != LXP_OK ||
        (activity->activity_type != LX_PROGRAMS_ACCOUNT &&
         activity->activity_type != LX_PROGRAMS_WIND_DOWN))
        goto advance;
    if (receipt->module_id != LXP_MODULE_PROGRAMS ||
        receipt->global_sequence == 0U || activity->payload.bytes == NULL ||
        activity->payload.length < 32U ||
        lxp_ct_is_zero(activity->payload.bytes, 32U))
        { status = LXP_FATAL_INVARIANT; goto finish; }
    activity_program = activity->payload.bytes;
    for (index = 0U; index < receipt->effects.count; ++index) {
        const lxp_effect *effect = &receipt->effects.effects[index];
        const uint8_t *event_program_id;
        lxp_result event_status;
        if (effect->kind != LXP_EFFECT_EVENT ||
            effect->module_id != LXP_MODULE_PROGRAMS ||
            !state_event(effect->event_type))
            continue;
        event_status = event_program(effect, &event_program_id);
        if (event_status != LXP_OK ||
            lxp_ct_memcmp(event_program_id, activity_program, 32U) != 0)
            { status = event_status != LXP_OK ? event_status :
                                                LXP_FATAL_INVARIANT;
              goto finish; }
        event_status = feed->append(feed->context, receipt->global_sequence,
                                    ordinal, activity_program,
                                    activity->activity_type,
                                    effect->event_type, receipt);
        if (event_status != LXP_OK) {
            status = event_status;
            goto finish;
        }
        ++ordinal;
    }
    if (ordinal == 0U && activity->activity_type != LX_PROGRAMS_ACCOUNT)
        { status = LXP_FATAL_INVARIANT; goto finish; }
advance:
    status = feed->advance(feed->context, activity, receipt);
finish:
    {
        lxp_result unlock_status = feed->unlock(feed->context);
        return status == LXP_OK ? unlock_status : status;
    }
}

static lxp_result observe_programs_commit(
    void *context, const lxp_kernel *kernel, const lxp_activity *activity,
    const lxp_receipt *receipt)
{
    (void)kernel;
    return lxp_programs_state_feed_observe(
        (const lx_programs_state_feed *)context, activity, receipt);
}

lxp_result lxp_programs_bind_state_feed(
    lxp_kernel *kernel, const lx_programs_state_feed *feed)
{
    if (kernel == NULL || feed == NULL || feed->begin == NULL ||
        feed->append == NULL || feed->advance == NULL ||
        feed->lock == NULL || feed->unlock == NULL ||
        feed->context == NULL)
        return LXP_ERR_NON_CANONICAL;
    return lxp_kernel_set_commit_observer(kernel, observe_programs_commit,
                                          (void *)feed);
}
