#ifndef LAYERX_LX_STREAM_EXECUTION_H
#define LAYERX_LX_STREAM_EXECUTION_H

static void stream_put_u64(uint8_t bytes[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - 8U * i));
}

static lxp_result stream_asset_state(lxp_module_ctx *ctx,
                                     const uint8_t asset_id[32],
                                     lxp_transfer_asset_state *state)
{
    const lx_stream_runtime *runtime =
        (const lx_stream_runtime *)lxp_ctx_module_runtime(ctx);
    size_t i;
    if (runtime == NULL || runtime->assets == NULL ||
        runtime->asset_count == 0U)
        return LXP_ERR_ASSET_MISMATCH;
    for (i = 0U; i < runtime->asset_count; ++i) {
        if (memcmp(runtime->assets[i].asset_id, asset_id, 32U) != 0) continue;
        *state = runtime->assets[i];
        if (!state->registered) return LXP_ERR_ASSET_MISMATCH;
        return state->paused ? LXP_ERR_ASSET_PAUSED : LXP_OK;
    }
    return LXP_ERR_ASSET_MISMATCH;
}

static lxp_result stream_parties(lxp_module_ctx *ctx,
                                 const lx_stream_record *record,
                                 lx_account **payer,
                                 lx_account **stream_account,
                                 lx_account **recipient)
{
    lxp_result status = lxp_ctx_account_find(ctx, record->payer, payer);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, record->stream_account,
                                      stream_account);
    if (status == LXP_OK)
        status = lxp_ctx_account_find(ctx, record->recipient, recipient);
    if (status != LXP_OK) return status;
    if ((*payer)->kind != LX_ACCOUNT_AGENT_MAIN ||
        (*stream_account)->kind != LX_ACCOUNT_AGENT_STREAM)
        return LXP_ERR_NON_CANONICAL;
    return LXP_OK;
}

static lxp_result stream_event_open(lxp_module_ctx *ctx,
                                    const lx_stream_record *record,
                                    lxp_u128 funding)
{
    uint8_t body[177];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    (void)memcpy(body + 32U, record->payer, 32U);
    (void)memcpy(body + 64U, record->stream_account, 32U);
    (void)memcpy(body + 96U, record->recipient, 32U);
    (void)memcpy(body + 128U, record->asset_id, 32U);
    body[160] = (uint8_t)record->mode;
    status = lxp_u128_to_be(funding, body + 161U);
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, 1U, body, sizeof(body));
}

static lxp_result stream_event_top_up(lxp_module_ctx *ctx,
                                      const lx_stream_record *record,
                                      lxp_u128 amount, bool refunded_state)
{
    uint8_t body[49];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    status = lxp_u128_to_be(amount, body + 32U);
    if (status != LXP_OK) return status;
    body[48] = refunded_state ? 1U : 0U;
    return lxp_ctx_emit_event(ctx, 2U, body, sizeof(body));
}

static lxp_result stream_event_meter(lxp_module_ctx *ctx,
                                     const lx_stream_record *record,
                                     lxp_u128 accrued)
{
    uint8_t body[72];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    stream_put_u64(body + 32U, record->cumulative_meter);
    status = lxp_u128_to_be(accrued, body + 40U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->accrued_total, body + 56U);
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, 3U, body, sizeof(body));
}

static lxp_result stream_event_settle(lxp_module_ctx *ctx,
                                      const lx_stream_record *record,
                                      lxp_u128 paid)
{
    uint8_t body[65];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    status = lxp_u128_to_be(paid, body + 32U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(record->settled_total, body + 48U);
    if (status != LXP_OK) return status;
    body[64] = record->underfunded ? 1U : 0U;
    return lxp_ctx_emit_event(ctx, 4U, body, sizeof(body));
}

static lxp_result stream_event_lifecycle(lxp_module_ctx *ctx,
                                         uint16_t event_type,
                                         const lx_stream_record *record)
{
    uint8_t body[56];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    stream_put_u64(body + 32U, record->last_accrual_timestamp);
    status = lxp_u128_to_be(record->accrued_total, body + 40U);
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, event_type, body, sizeof(body));
}

static lxp_result stream_event_close(lxp_module_ctx *ctx,
                                     const lx_stream_record *record,
                                     const lx_stream_economic_result *result)
{
    uint8_t body[64];
    lxp_result status;
    (void)memcpy(body, record->stream_id, 32U);
    status = lxp_u128_to_be(result->paid, body + 32U);
    if (status == LXP_OK)
        status = lxp_u128_to_be(result->refunded, body + 48U);
    if (status != LXP_OK) return status;
    return lxp_ctx_emit_event(ctx, 7U, body, sizeof(body));
}

static lxp_result stream_execute_open(lxp_module_ctx *ctx,
                                      const lxp_authority_resolved *authority,
                                      const lx_stream_open_payload *payload)
{
    lx_stream_fund_request request;
    lxp_transfer_asset_state asset;
    lxp_receipt receipt;
    lx_account *payer;
    lx_account *stream_account;
    lx_account *recipient;
    lxp_result status;
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&receipt, 0, sizeof(receipt));
    request.record = payload->record;
    (void)memcpy(request.record.payer, authority->principal, 32U);
    status = stream_parties(ctx, &request.record, &payer, &stream_account,
                            &recipient);
    if (status != LXP_OK) return status;
    status = stream_asset_state(ctx, request.record.asset_id, &asset);
    if (status != LXP_OK) return status;
    request.payer = payer;
    request.stream_account = stream_account;
    (void)memcpy(request.asset_id, request.record.asset_id, 32U);
    request.amount = payload->initial_funding;
    request.context.assets = &asset;
    request.context.asset_count = 1U;
    status = lx_stream_open_execute(ctx, &request, &receipt);
    if (status != LXP_OK) return status;
    return stream_event_open(ctx, &request.record, payload->initial_funding);
}

static lxp_result stream_execute_top_up(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_stream_amount_payload *payload)
{
    lx_stream_fund_request request;
    lx_stream_record record;
    lxp_transfer_asset_state asset;
    lxp_receipt receipt;
    lx_account *payer;
    lx_account *stream_account;
    lx_account *recipient;
    bool was_underfunded;
    lxp_result status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_STREAM_CLOSED;
    if (memcmp(authority->principal, record.payer, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = stream_parties(ctx, &record, &payer, &stream_account, &recipient);
    if (status != LXP_OK) return status;
    status = stream_asset_state(ctx, record.asset_id, &asset);
    if (status != LXP_OK) return status;
    was_underfunded = record.underfunded;
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&receipt, 0, sizeof(receipt));
    request.payer = payer;
    request.stream_account = stream_account;
    (void)memcpy(request.asset_id, record.asset_id, 32U);
    request.amount = payload->amount;
    request.record = record;
    request.context.assets = &asset;
    request.context.asset_count = 1U;
    status = lx_stream_top_up_execute(ctx, &request, &receipt);
    if (status != LXP_OK) return status;
    return stream_event_top_up(ctx, &record, payload->amount,
                               was_underfunded);
}

static lxp_result stream_execute_meter(
    lxp_module_ctx *ctx, const lx_stream_meter_attestation *attestation)
{
    lx_stream_record record;
    lxp_u128 accrued;
    lxp_result status = lx_stream_load(ctx, attestation->stream_id, &record);
    if (status != LXP_OK) return status;
    if (record.closed) return LXP_ERR_STREAM_CLOSED;
    status = lx_stream_meter_execute(&record, attestation, &accrued);
    if (status != LXP_OK) return status;
    status = lx_stream_save(ctx, &record);
    if (status != LXP_OK) return status;
    return stream_event_meter(ctx, &record, accrued);
}

static lxp_result stream_execute_settle(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_stream_keyed_payload *payload)
{
    lx_stream_settle_request request;
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_transfer_asset_state asset;
    lxp_receipt receipt;
    lx_account *payer;
    lx_account *stream_account;
    lx_account *recipient;
    bool found;
    lxp_result status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    if (memcmp(authority->principal, record.payer, 32U) != 0 &&
        memcmp(authority->principal, record.recipient, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = stream_parties(ctx, &record, &payer, &stream_account, &recipient);
    if (status != LXP_OK) return status;
    status = stream_asset_state(ctx, record.asset_id, &asset);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&receipt, 0, sizeof(receipt));
    request.stream_id = record.stream_id;
    request.stream_account = stream_account;
    request.recipient = recipient;
    (void)memcpy(request.asset_id, record.asset_id, 32U);
    (void)memcpy(request.idempotency_key, payload->idempotency_key, 32U);
    request.context.assets = &asset;
    request.context.asset_count = 1U;
    status = lx_stream_settle_execute(ctx, &request, &receipt);
    if (status != LXP_OK) return status;
    status = lx_stream_result_load(ctx, payload->idempotency_key, &result,
                                   &found);
    if (status != LXP_OK) return status;
    if (!found) return LXP_FATAL_INVARIANT;
    status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    return stream_event_settle(ctx, &record, result.paid);
}

static lxp_result stream_execute_lifecycle(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    uint16_t ordinal, const lx_stream_id_payload *payload)
{
    lx_stream_lifecycle_request request;
    lx_stream_record record;
    lx_account *payer;
    lx_account *stream_account;
    lx_account *recipient;
    lxp_result status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    status = stream_parties(ctx, &record, &payer, &stream_account, &recipient);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    request.stream_id = record.stream_id;
    request.stream_account = stream_account;
    request.payer = payer;
    request.recipient = recipient;
    (void)memcpy(request.asset_id, record.asset_id, 32U);
    request.authority = authority;
    status = ordinal == 5U ? lx_stream_pause_execute(ctx, &request) :
                             lx_stream_resume_execute(ctx, &request);
    if (status != LXP_OK) return status;
    status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    return stream_event_lifecycle(ctx, ordinal, &record);
}

static lxp_result stream_execute_close(
    lxp_module_ctx *ctx, const lxp_authority_resolved *authority,
    const lx_stream_keyed_payload *payload)
{
    lx_stream_lifecycle_request request;
    lx_stream_record record;
    lx_stream_economic_result result;
    lxp_transfer_asset_state asset;
    lxp_receipt receipt;
    lx_account *payer;
    lx_account *stream_account;
    lx_account *recipient;
    bool found;
    lxp_result status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    status = stream_parties(ctx, &record, &payer, &stream_account, &recipient);
    if (status != LXP_OK) return status;
    status = stream_asset_state(ctx, record.asset_id, &asset);
    if (status != LXP_OK) return status;
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&receipt, 0, sizeof(receipt));
    request.stream_id = record.stream_id;
    request.stream_account = stream_account;
    request.payer = payer;
    request.recipient = recipient;
    (void)memcpy(request.asset_id, record.asset_id, 32U);
    request.authority = authority;
    (void)memcpy(request.idempotency_key, payload->idempotency_key, 32U);
    request.context.assets = &asset;
    request.context.asset_count = 1U;
    status = lx_stream_close_execute(ctx, &request, &receipt);
    if (status != LXP_OK) return status;
    status = lx_stream_result_load(ctx, payload->idempotency_key, &result,
                                   &found);
    if (status != LXP_OK) return status;
    if (!found) return LXP_FATAL_INVARIANT;
    status = lx_stream_load(ctx, payload->stream_id, &record);
    if (status != LXP_OK) return status;
    return stream_event_close(ctx, &record, &result);
}

static lxp_result stream_execute_typed(lxp_module_ctx *ctx,
                                       const lxp_activity *activity,
                                       const lxp_authority_resolved *authority,
                                       const stream_decoded *value)
{
    if (ctx == NULL || activity == NULL || authority == NULL ||
        value == NULL || value->typed == NULL)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (lxp_activity_module_id(activity->activity_type) != LXP_MODULE_STREAM ||
        lxp_activity_type_ordinal(activity->activity_type) != value->ordinal)
        return LXP_ERR_UNKNOWN_ACTIVITY;
    if (authority->kind < LXP_AUTHORITY_OWNER ||
        authority->kind > LXP_AUTHORITY_DELEGATED_CAPABILITY ||
        lxp_ct_is_zero(authority->principal, 32U))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    switch (value->ordinal) {
    case 1U: return stream_execute_open(ctx, authority, &value->typed->open);
    case 2U: return stream_execute_top_up(ctx, authority,
                                          &value->typed->amount);
    case 3U: return stream_execute_meter(ctx, &value->typed->meter);
    case 4U: return stream_execute_settle(ctx, authority,
                                          &value->typed->keyed);
    case 5U:
    case 6U: return stream_execute_lifecycle(ctx, authority, value->ordinal,
                                             &value->typed->id);
    default: return stream_execute_close(ctx, authority,
                                         &value->typed->keyed);
    }
}

#endif
