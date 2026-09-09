#ifndef LAYERX_LX_ASSET_EXECUTION_H
#define LAYERX_LX_ASSET_EXECUTION_H

static void asset_key(uint8_t key[38], const uint8_t id[32])
{
    (void)memcpy(key, "asset:", 6U);
    (void)memcpy(key + 6U, id, 32U);
}

static lxp_result asset_load(lxp_module_ctx *ctx, const uint8_t id[32],
                              lx_asset_record *record)
{
    uint8_t key[38];
    const uint8_t *bytes;
    size_t length;
    const lx_asset_record *base;
    lxp_result status;
    asset_key(key, id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status == LXP_OK) return lx_asset_record_decode(bytes, length, record);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    base = runtime_asset((lx_asset_runtime *)lxp_ctx_module_runtime(ctx), id);
    if (base == NULL) return LXP_ERR_ASSET_MISMATCH;
    *record = *base;
    return LXP_OK;
}

static lxp_result asset_save(lxp_module_ctx *ctx, const lx_asset_record *record)
{
    uint8_t key[38];
    uint8_t bytes[384];
    size_t length;
    lxp_result status = lx_asset_record_encode(record, bytes, sizeof(bytes), &length);
    asset_key(key, record->asset_id);
    return status == LXP_OK ? lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length) : status;
}

static lxp_result issuance_find(lxp_module_ctx *ctx, const uint8_t id[32], lx_account **account)
{
    uint8_t name[LX_ASSET_ISSUANCE_NAME_BYTES];
    uint8_t account_id[32];
    lxp_result status = lx_asset_issuance_name(id, name, account_id);
    return status == LXP_OK ? lxp_ctx_account_find(ctx, account_id, account) : status;
}

static void grant_key(uint8_t key[38], const uint8_t id[32])
{
    (void)memcpy(key, "grant:", 6U);
    (void)memcpy(key + 6U, id, 32U);
}

static void asset_u64_write(uint8_t *bytes, uint64_t value)
{
    for (size_t i = 0U; i < 8U; ++i) bytes[i] = (uint8_t)(value >> (56U - i * 8U));
}

static uint64_t asset_u64_read(const uint8_t *bytes)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static lxp_result grant_save(lxp_module_ctx *ctx, const lxp_grant_state *state)
{
    uint8_t key[38];
    uint8_t bytes[512];
    size_t length;
    lxp_result status = lxp_payer_grant_encode(&state->grant, bytes, sizeof(bytes) - 50U, &length);
    if (status != LXP_OK) return status;
    (void)lxp_u128_to_be(state->drawn_total, bytes + length);
    (void)lxp_u128_to_be(state->drawn_this_period, bytes + length + 16U);
    asset_u64_write(bytes + length + 32U, state->window_start);
    asset_u64_write(bytes + length + 40U, state->revoked_at_sequence);
    bytes[length + 48U] = state->revoked ? 1U : 0U;
    bytes[length + 49U] = state->invoice_settled ? 1U : 0U;
    grant_key(key, state->grant.grant_id);
    return lxp_ctx_kv_put(ctx, key, sizeof(key), bytes, length + 50U);
}

static lxp_result grant_load(lxp_module_ctx *ctx, const uint8_t id[32], lxp_grant_state *state)
{
    uint8_t key[38];
    const uint8_t *bytes;
    size_t length;
    lxp_result status;
    grant_key(key, id);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &bytes, &length);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_NO_PAYER_GRANT;
    if (status != LXP_OK) return status;
    if (length < 50U) return LXP_ERR_NON_CANONICAL;
    length -= 50U;
    (void)memset(state, 0, sizeof(*state));
    status = lxp_payer_grant_decode(bytes, length, &state->grant);
    if (status != LXP_OK || bytes[length + 48U] > 1U || bytes[length + 49U] > 1U)
        return LXP_ERR_NON_CANONICAL;
    (void)lxp_u128_from_be(bytes + length, &state->drawn_total);
    (void)lxp_u128_from_be(bytes + length + 16U, &state->drawn_this_period);
    state->window_start = asset_u64_read(bytes + length + 32U);
    state->revoked_at_sequence = asset_u64_read(bytes + length + 40U);
    state->revoked = bytes[length + 48U] != 0U;
    state->invoice_settled = bytes[length + 49U] != 0U;
    return memcmp(id, state->grant.grant_id, 32U) == 0 ? LXP_OK : LXP_ERR_NON_CANONICAL;
}

static lxp_result asset_transfer(lxp_module_ctx *ctx, const lxp_activity *activity,
    const lx_asset_record *asset, lx_account *from, lx_account *to,
    lx_account *sequence_account, lxp_u128 amount, const uint8_t context_hash[32],
    const uint8_t authorization_hash[32])
{
    lxp_transfer_set set;
    lxp_transfer_source_authority source;
    lxp_transfer_asset_state transfer_asset;
    lxp_receipt receipt;
    lxp_ledger_receipt_input input;
    lxp_result status;
    (void)memset(&set, 0, sizeof(set));
    (void)memset(&source, 0, sizeof(source));
    (void)memset(&receipt, 0, sizeof(receipt));
    (void)memset(&input, 0, sizeof(input));
    if (sequence_account == NULL || sequence_account->next_sequence == UINT64_MAX)
        return LXP_ERR_OVERFLOW;
    status = lx_asset_transfer_state(asset, &transfer_asset);
    if (status != LXP_OK) return status;
    set.leg_count = 1U;
    set.legs[0].from = from;
    set.legs[0].to = to;
    set.legs[0].amount = amount;
    set.legs[0].reason = LXP_REASON_PAYMENT;
    (void)memcpy(set.legs[0].asset_id, asset->asset_id, 32U);
    set.context.assets = &transfer_asset;
    set.context.asset_count = 1U;
    set.context.sequence_account = sequence_account;
    set.context.actor_sequence = sequence_account->next_sequence;
    set.context.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    set.context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(set.context.authorized_from, from->id, 32U);
    (void)memcpy(source.authorized_from, from->id, 32U);
    source.debit_authority_kind = LXP_AUTH_OWNER;
    set.context.source_authorities = &source;
    set.context.source_authority_count = 1U;
    input.from_balance_before = from->balance;
    input.to_balance_before = to->balance;
    input.from_sequence = set.context.actor_sequence;
    status = lxp_ctx_emit_monetary_transfer_set(ctx, &set, &receipt);
    if (status != LXP_OK) return status;
    (void)memcpy(input.transaction_id, lxp_ctx_activity_id(ctx), 32U);
    input.operation = (uint8_t)lxp_activity_type_ordinal(activity->activity_type);
    input.global_sequence = lxp_ctx_global_sequence(ctx);
    (void)memcpy(input.asset, asset->asset_id, 32U);
    input.amount = amount;
    (void)memcpy(input.from, from->id, 32U);
    (void)memcpy(input.to, to->id, 32U);
    input.from_balance_after = from->balance;
    input.to_balance_after = to->balance;
    (void)memcpy(input.transfer_set_root, receipt.transfer_set_root, 32U);
    (void)memcpy(input.context_hash, context_hash, 32U);
    (void)memcpy(input.authorization_hash, authorization_hash, 32U);
    input.timestamp = lxp_ctx_batch_timestamp_ms(ctx);
    input.leg_count = 1U;
    return lxp_ctx_bind_ledger_receipt(ctx, &input);
}

static lxp_result asset_execute_typed(lxp_module_ctx *ctx, const lxp_activity *activity,
    const lxp_authority_resolved *authority, const asset_decoded *value)
{
    lx_asset_record record;
    lx_account *account;
    lx_account *issuance;
    lxp_grant_state grant;
    uint8_t context[32];
    uint8_t actor[32];
    uint8_t authorization[32];
    lxp_result status;
    if (authority == NULL || activity == NULL || value->typed == NULL ||
        authority->kind != LXP_AUTHORITY_OWNER || activity->authority.length != 32U ||
        memcmp(authority->verified_key, activity->authority.bytes, 32U) != 0 ||
        activity->activity_type != ((uint32_t)LXP_MODULE_ASSET << 16U | value->ordinal))
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_did_id_derive(activity->actor_did.bytes, activity->actor_did.length, actor);
    if (status == LXP_OK && memcmp(actor, authority->actor, 32U) != 0)
        status = LXP_ERR_UNAUTHORIZED_DEBIT;
    if (status == LXP_OK) status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK) status = lxp_hash_context_value(value->payload, value->payload_length, context);
    if (status == LXP_OK) status = lxp_hash_authority(activity->signature.bytes,
                                                     activity->signature.length, authorization);
    if (status != LXP_OK) return status;
    if (value->ordinal == 1U) {
        const lx_asset_register_payload *p = &value->typed->registration;
        status = asset_load(ctx, p->asset_id, &record);
        if (status == LXP_OK) return LXP_ERR_ASSET_ALREADY_REGISTERED;
        if (status != LXP_ERR_ASSET_MISMATCH) return status;
        {
            const lx_asset_runtime *runtime = lxp_ctx_module_runtime(ctx);
            size_t count = runtime == NULL ? 0U : runtime->asset_count;
            if (count > LX_ASSET_REGISTRY_CAPACITY) return LXP_FATAL_INVARIANT;
            for (size_t i = 0U; i < ctx->kernel->module_kv_count; ++i) {
                const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
                if (entry->module_id == LXP_MODULE_ASSET && entry->key_length == 38U &&
                    memcmp(entry->key, "asset:", 6U) == 0 &&
                    runtime_asset(runtime, entry->key + 6U) == NULL) ++count;
            }
            if (count >= LX_ASSET_REGISTRY_CAPACITY) return LXP_ERR_ARENA_EXHAUSTED;
        }
        (void)memset(&record, 0, sizeof(record));
        (void)memcpy(record.asset_id, p->asset_id, 32U);
        (void)memcpy(record.symbol, p->symbol, p->symbol_length);
        record.symbol_length = p->symbol_length;
        (void)memcpy(record.name, p->name, p->name_length);
        record.name_length = p->name_length;
        record.decimals = p->decimals;
        record.supply_cap = p->supply_cap;
        record.issuer_kind = p->issuer_kind;
        (void)memcpy(record.salt, p->salt, 32U);
        (void)memcpy(record.issuer_did32, authority->actor, 32U);
        record.custody_kind = LX_ASSET_CUSTODY_PAXEER;
        record.custody_reference_length = p->custody_reference_length;
        (void)memcpy(record.custody_reference, p->custody_reference, p->custody_reference_length);
        if (p->issuer_kind == 1U) {
            lxp_hash_context hash;
            uint8_t expected[32];
            lxp_hash_init(&hash);
            status = lxp_hash_update(&hash, (const uint8_t *)"LX:ASSET:v1", 11U);
            if (status == LXP_OK) status = lxp_hash_update(&hash, authority->actor, 32U);
            if (status == LXP_OK) status = lxp_hash_update(&hash, p->salt, 32U);
            if (status == LXP_OK) status = lxp_hash_final(&hash, expected);
            if (status != LXP_OK) return status;
            if (memcmp(expected, p->asset_id, 32U) != 0) return LXP_ERR_ASSET_MISMATCH;
        }
        status = asset_save(ctx, &record);
        if (status == LXP_OK) status = lxp_ctx_asset_issuance_stage(ctx, activity, authority, &issuance);
        return status == LXP_OK ? lxp_ctx_emit_event(ctx, 1U, p->asset_id, 32U) : status;
    }
    if (value->ordinal == 4U) {
        status = asset_load(ctx, value->typed->account_open.asset_id, &record);
        if (status != LXP_OK) return status;
        if (record.paused) return LXP_ERR_ASSET_PAUSED;
        status = lxp_ctx_asset_account_stage(ctx, activity, authority, &account);
        if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 4U, account->id, 32U);
        if (status == LXP_OK) {
            lxp_ledger_receipt_input input;
            lx_account *owner_account = NULL;
            (void)memset(&input, 0, sizeof(input));
            (void)memcpy(input.transaction_id, ctx->activity_id, 32U);
            input.operation = 4U;
            input.global_sequence = ctx->global_sequence;
            input.timestamp = lxp_ctx_batch_timestamp_ms(ctx);
            (void)memcpy(input.asset, record.asset_id, 32U);
            if (ctx->ledger_admission.account_present) {
                status = lxp_ctx_account_find(
                    ctx, authority->principal, &owner_account);
                if (status != LXP_OK ||
                    owner_account->next_sequence !=
                        ctx->ledger_admission.next_sequence)
                    return status != LXP_OK ? status :
                                              LXP_ERR_CONTEXT_MISMATCH;
                input.from_balance_before = owner_account->balance;
                input.from_balance_after = owner_account->balance;
            }
            input.from_sequence = ctx->ledger_admission.next_sequence;
            (void)memcpy(input.from, authority->principal, 32U);
            (void)memcpy(input.to, account->id, 32U);
            (void)memcpy(input.context_hash, context, 32U);
            (void)memcpy(input.authorization_hash, authorization, 32U);
            (void)memcpy(input.transfer_set_root, context, 32U);
            input.leg_count = 1U;
            status = lxp_ctx_bind_ledger_receipt(ctx, &input);
        }
        return status;
    }
    if (value->ordinal == 10U || value->ordinal == 11U) {
        const lx_asset_supply_payload *p = &value->typed->supply;
        lxp_u128 initial;
        lxp_u128 total;
        status = asset_load(ctx, p->asset_id, &record);
        if (status == LXP_OK) status = lxp_ctx_account_find(ctx, p->account_id, &account);
        if (status == LXP_OK) status = issuance_find(ctx, p->asset_id, &issuance);
        if (status != LXP_OK) return status;
        if (record.paused) return LXP_ERR_ASSET_PAUSED;
        if (account->kind != LX_ACCOUNT_AGENT_MAIN || !account->has_asset ||
            memcmp(account->asset_id, p->asset_id, 32U) != 0) return LXP_ERR_ASSET_MISMATCH;
        if ((value->ordinal == 10U && memcmp(record.issuer_did32, authority->actor, 32U) != 0) ||
            (value->ordinal == 11U && (!source_matches_actor(account, activity) ||
             !account->has_authority_key || memcmp(account->authority_key, authority->verified_key, 32U) != 0)))
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        initial = lxp_u128_is_zero(record.supply_cap) ? (lxp_u128){UINT64_MAX, UINT64_MAX} : record.supply_cap;
        if (lxp_u128_sub(initial, issuance->balance, &total) != LXP_OK ||
            lxp_u128_cmp(total, record.total_units) != 0) return LXP_FATAL_SUPPLY_MISMATCH;
        status = value->ordinal == 10U ? lxp_u128_add(total, p->amount, &total) :
                                        lxp_u128_sub(total, p->amount, &total);
        if (status != LXP_OK) return status;
        if (lxp_u128_cmp(total, initial) > 0) return LXP_ERR_INSUFFICIENT_BALANCE;
        status = asset_transfer(ctx, activity, &record,
            value->ordinal == 10U ? issuance : account,
            value->ordinal == 10U ? account : issuance,
            value->ordinal == 10U ? issuance : account, p->amount, context, authorization);
        if (status != LXP_OK) return status;
        record.total_units = total;
        return asset_save(ctx, &record);
    }
    if (value->ordinal == 6U) {
        const lxp_receive *p = &value->typed->receive;
        lx_account_registry *preview;
        lxp_grant_store *store;
        lxp_send_store *idempotency;
        lxp_receive_environment environment;
        lxp_send_receipt_projection projection;
        lxp_transfer_asset_state transfer_asset;
        lx_account *recipient;
        uint8_t message[512];
        size_t message_length;
        status = asset_load(ctx, p->asset, &record);
        if (status == LXP_OK) status = grant_load(ctx, p->grant_id, &grant);
        if (status == LXP_OK) status = lxp_ctx_account_find(ctx, p->from, &account);
        if (status == LXP_OK) status = lxp_ctx_account_find(ctx, p->to, &recipient);
        if (status != LXP_OK) return status;
        if (!source_matches_actor(recipient, activity) ||
            p->receiver_authorization.kind != LXP_AUTH_OWNER ||
            memcmp(p->receiver_authorization.public_key, authority->verified_key, 32U) != 0 ||
            memcmp(p->idempotency_key, activity->idempotency_key, 32U) != 0 ||
            memcmp(p->from, p->to, 32U) == 0) return LXP_ERR_UNAUTHORIZED_DEBIT;
        preview = (lx_account_registry *)malloc(sizeof(*preview));
        store = (lxp_grant_store *)calloc(1U, sizeof(*store));
        idempotency = (lxp_send_store *)calloc(1U, sizeof(*idempotency));
        if (preview == NULL || store == NULL || idempotency == NULL) {
            free(preview); free(store); free(idempotency);
            return LXP_ERR_ARENA_EXHAUSTED;
        }
        status = lx_account_registry_init(preview);
        if (status == LXP_OK) {
            preview->count = ctx->kernel->state->accounts->count;
            (void)memcpy(preview->accounts, ctx->kernel->state->accounts->accounts,
                         preview->count * sizeof(preview->accounts[0]));
            store->count = 1U;
            store->grants[0] = grant;
            (void)memset(&environment, 0, sizeof(environment));
            (void)lx_asset_transfer_state(&record, &transfer_asset);
            environment.accounts = preview;
            environment.assets = &transfer_asset;
            environment.asset_count = 1U;
            environment.grants = store;
            environment.idempotency = idempotency;
            environment.batch_timestamp = lxp_ctx_batch_timestamp_ms(ctx);
            environment.global_sequence = ctx->global_sequence;
            environment.network_id = activity->network_id;
            environment.protocol_version = activity->protocol_version;
            status = lxp_receive_execute(p, &environment, &projection);
            if (status == LXP_OK) grant = store->grants[0];
        }
        free(preview); free(store); free(idempotency);
        if (status != LXP_OK) return status;
        status = lxp_receive_authorization_message(p, message, sizeof(message), &message_length);
        if (status == LXP_OK) status = lxp_hash_domain(LXP_DOMAIN_SIGNATURE_PREIMAGE,
            message, message_length, authorization);
        if (status == LXP_OK) status = asset_transfer(ctx, activity, &record, account,
            recipient, recipient, p->amount, p->context_hash, authorization);
        if (status == LXP_OK) status = grant_save(ctx, &grant);
        return status;
    }
    if (value->ordinal == 7U) {
        const lxp_payer_grant *p = &value->typed->grant;
        status = grant_load(ctx, p->grant_id, &grant);
        if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
        if (status != LXP_ERR_NO_PAYER_GRANT) return status;
        status = asset_load(ctx, p->asset, &record);
        if (status == LXP_OK) status = lxp_ctx_account_find(ctx, p->from, &account);
        if (status != LXP_OK) return status;
        if (record.paused) return LXP_ERR_ASSET_PAUSED;
        if (!source_matches_actor(account, activity) || !account->has_asset ||
            memcmp(account->asset_id, p->asset, 32U) != 0 ||
            memcmp(p->public_key, authority->verified_key, 32U) != 0)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        status = lxp_verify_payer_grant(p, account);
        if (status != LXP_OK) return status;
        (void)memset(&grant, 0, sizeof(grant));
        grant.grant = *p;
        status = grant_save(ctx, &grant);
        return status == LXP_OK ? lxp_ctx_emit_event(ctx, 7U, p->grant_id, 32U) : status;
    }
    if (value->ordinal == 8U) {
        const lx_asset_grant_revoke_payload *p = &value->typed->revocation;
        status = grant_load(ctx, p->grant_id, &grant);
        if (status == LXP_OK) status = lxp_ctx_account_find(ctx, grant.grant.from, &account);
        if (status != LXP_OK) return status;
        if (!source_matches_actor(account, activity) || !account->has_authority_key ||
            memcmp(account->authority_key, authority->verified_key, 32U) != 0)
            return LXP_ERR_UNAUTHORIZED_DEBIT;
        if (grant.revoked || p->revocation_sequence < grant.grant.revocation_sequence ||
            p->revocation_sequence != activity->account_sequence) return LXP_ERR_STALE_REVOCATION;
        grant.revoked = true;
        grant.revoked_at_sequence = ctx->global_sequence;
        status = grant_save(ctx, &grant);
        return status == LXP_OK ? lxp_ctx_emit_event(ctx, 8U, p->grant_id, 32U) : status;
    }
    return LXP_ERR_UNKNOWN_ACTIVITY;
}
#endif
