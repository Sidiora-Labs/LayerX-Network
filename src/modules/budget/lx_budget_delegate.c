#include "layerx/lx_budget.h"

#include "layerx/lxp_crypto.h"

#include <string.h>

static size_t delegate_position(const lx_budget_record *record,
                                const uint8_t delegate[32], bool *found)
{
    size_t position = 0U;
    while (position < record->delegate_count &&
           memcmp(record->delegates[position], delegate, 32U) < 0)
        ++position;
    *found = position < record->delegate_count &&
             memcmp(record->delegates[position], delegate, 32U) == 0;
    return position;
}

lxp_result lx_budget_delegate_add_execute(lx_budget_record *record,
                                          const uint8_t delegate[32])
{
    bool found;
    size_t position;
    if (record == NULL || delegate == NULL ||
        record->delegate_count > LX_BUDGET_MAX_DELEGATES ||
        lxp_ct_is_zero(delegate, 32U))
        return LXP_ERR_NON_CANONICAL;
    position = delegate_position(record, delegate, &found);
    if (found) return LXP_ERR_SEQUENCE_REUSED;
    if (record->delegate_count == LX_BUDGET_MAX_DELEGATES)
        return LXP_ERR_ARENA_EXHAUSTED;
    if (position != record->delegate_count)
        (void)memmove(&record->delegates[position + 1U],
                      &record->delegates[position],
                      (record->delegate_count - position) * 32U);
    (void)memcpy(record->delegates[position], delegate, 32U);
    ++record->delegate_count;
    return LXP_OK;
}

lxp_result lx_budget_delegate_remove_execute(lx_budget_record *record,
                                             const uint8_t delegate[32])
{
    bool found;
    size_t position;
    if (record == NULL || delegate == NULL ||
        record->delegate_count > LX_BUDGET_MAX_DELEGATES)
        return LXP_ERR_NON_CANONICAL;
    position = delegate_position(record, delegate, &found);
    if (!found) return LXP_ERR_UNAUTHORIZED_DELEGATE;
    if (position + 1U != record->delegate_count)
        (void)memmove(&record->delegates[position],
                      &record->delegates[position + 1U],
                      (record->delegate_count - position - 1U) * 32U);
    --record->delegate_count;
    (void)memset(record->delegates[record->delegate_count], 0, 32U);
    return LXP_OK;
}

lxp_result lx_budget_authorize_delegate(
    const lx_budget_record *record, const uint8_t submitter[32],
    lx_budget_delegate_capability *capability,
    const uint8_t recipient[32], lxp_u128 amount,
    uint64_t batch_timestamp)
{
    bool found;
    lxp_u128 consumed;
    if (record == NULL || submitter == NULL || capability == NULL ||
        recipient == NULL ||
        record->delegate_count > LX_BUDGET_MAX_DELEGATES ||
        lxp_u128_is_zero(amount))
        return LXP_ERR_UNAUTHORIZED_DELEGATE;
    (void)delegate_position(record, submitter, &found);
    if (!found || capability->revoked ||
        memcmp(capability->holder, submitter, 32U) != 0 ||
        memcmp(capability->asset_id, record->asset_id, 32U) != 0 ||
        memcmp(capability->recipient, recipient, 32U) != 0 ||
        memcmp(capability->purpose_hash, record->purpose_hash, 32U) != 0 ||
        capability->expiry < batch_timestamp ||
        capability->revocation_sequence != record->revocation_sequence ||
        lxp_u128_cmp(amount, capability->maximum_per_spend) > 0)
        return LXP_ERR_UNAUTHORIZED_DELEGATE;
    if (lxp_u128_add(capability->consumed, amount, &consumed) != LXP_OK ||
        lxp_u128_cmp(consumed, capability->maximum_total) > 0)
        return LXP_ERR_UNAUTHORIZED_DELEGATE;
    capability->consumed = consumed;
    return LXP_OK;
}

lxp_result lx_budget_delegate_spend_execute(
    lxp_module_ctx *ctx, const lx_budget_delegate_spend_request *request,
    lxp_receipt *receipt)
{
    lx_budget_record *record;
    lxp_u128 original_consumed;
    lxp_result status;
    if (request == NULL || request->submitter == NULL ||
        request->capability == NULL || request->spend.store == NULL ||
        request->spend.budget_id == NULL || request->spend.recipient == NULL)
        return LXP_ERR_UNAUTHORIZED_DELEGATE;
    status = lx_budget_lookup(request->spend.store,
                              request->spend.budget_id, &record);
    if (status != LXP_OK) return status;
    original_consumed = request->capability->consumed;
    status = lx_budget_authorize_delegate(
        record, request->submitter, request->capability,
        request->spend.recipient->id, request->spend.amount,
        lxp_ctx_batch_timestamp_ms(ctx));
    if (status != LXP_OK) return status;
    status = lx_budget_spend_execute(ctx, &request->spend, receipt);
    if (status != LXP_OK) request->capability->consumed = original_consumed;
    return status;
}

static lxp_result pull_authorize(const lx_budget_pull_request *request,
                                 const lx_budget_record *record,
                                 uint64_t batch_timestamp)
{
    const lxp_payer_grant *grant = request->grant;
    lxp_result status = lxp_verify_payer_grant(grant, request->grantor);
    if (status != LXP_OK) return status;
    if (memcmp(grant->from, record->owner, 32U) != 0 ||
        memcmp(grant->recipient, request->spend.recipient->id, 32U) != 0 ||
        memcmp(grant->asset, record->asset_id, 32U) != 0 ||
        memcmp(grant->purpose_hash, record->purpose_hash, 32U) != 0 ||
        lxp_u128_cmp(request->spend.amount, grant->per_draw_maximum) > 0 ||
        lxp_u128_cmp(request->spend.amount, grant->allowance) > 0 ||
        grant->expiration < batch_timestamp ||
        grant->revocation_sequence != record->revocation_sequence ||
        (grant->recurring && grant->window_length == 0U))
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    return LXP_OK;
}

lxp_result lx_budget_pull_execute(lxp_module_ctx *ctx,
                                  const lx_budget_pull_request *request,
                                  lxp_receipt *receipt)
{
    lx_budget_record *record;
    lxp_result status;
    if (ctx == NULL || request == NULL || request->grant == NULL ||
        request->grantor == NULL || request->spend.store == NULL ||
        request->spend.budget_id == NULL || request->spend.recipient == NULL)
        return LXP_ERR_GRANT_SCOPE_VIOLATION;
    status = lx_budget_lookup(request->spend.store,
                              request->spend.budget_id, &record);
    if (status == LXP_OK)
        status = pull_authorize(request, record,
                                lxp_ctx_batch_timestamp_ms(ctx));
    if (status != LXP_OK) return status;
    return lx_budget_spend_execute(ctx, &request->spend, receipt);
}
