#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_crypto.h"
#include "layerx/programs.h"

#include <string.h>

enum { IDENTITY_BYTES = 223, CONSENT_BYTES = 140 };

static uint64_t read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void write_u64(uint8_t *bytes, uint64_t value)
{
    for (size_t i = 0U; i < 8U; ++i) bytes[7U - i] = (uint8_t)(value >> (8U * i));
}

static lxp_result consent_decode(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    lxp_activity *consent, lx_account *account, uint8_t did[32])
{
    lxp_codec_reader reader;
    lxp_byte_span encoded, canonical;
    uint8_t sponsor[32], activity_id[32];
    const lx_programs_transfer_runtime *runtime;
    lxp_result status;
    size_t arena_mark = lxp_arena_mark(ctx->arena);
    bool canonical_match = false;
    if (activity->activity_type != 0x00070001U || activity->protocol_version != 3U ||
        activity->payload.length < 8U ||
        memcmp(activity->payload.bytes, "\x71\x01\x02\x01", 4U) != 0 ||
        activity->authority.length != 32U || authority->kind != LXP_AUTHORITY_OWNER ||
        memcmp(activity->authority.bytes, authority->verified_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK) status = lxp_activity_encode(activity, ctx->arena, &canonical);
    if (status == LXP_OK) status = lxp_activity_id(canonical.bytes, canonical.length, activity_id);
    if (lxp_arena_reset(ctx->arena, arena_mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    if (memcmp(activity_id, ctx->activity_id, 32U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_codec_reader_init(&reader, activity->payload.bytes + 4U, activity->payload.length - 4U);
    if (status == LXP_OK) status = lxp_codec_read_bytes(&reader, &encoded, 1016U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK) status = lxp_activity_decode(encoded.bytes, encoded.length, consent);
    if (status == LXP_OK) status = lxp_activity_encode(consent, ctx->arena, &canonical);
    if (status == LXP_OK) canonical_match = canonical.length == encoded.length &&
        memcmp(canonical.bytes, encoded.bytes, encoded.length) == 0;
    if (lxp_arena_reset(ctx->arena, arena_mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    if (!canonical_match ||
        consent->protocol_version != 3U || consent->network_id != activity->network_id ||
        consent->activity_type != 0x00070001U || consent->authority.length != 32U ||
        consent->signature.length != 64U || !lxp_ed25519_pubkey_is_canonical(consent->authority.bytes) ||
        consent->account_sequence != 0U || !lxp_u128_is_zero(consent->fee_limit) ||
        consent->payload.length != CONSENT_BYTES ||
        memcmp(consent->payload.bytes, "\x71\x01\x03\x05", 4U) != 0 ||
        consent->timestamp_bound.not_before > activity->timestamp_bound.not_before ||
        consent->timestamp_bound.not_after < activity->timestamp_bound.not_after ||
        consent->timestamp_bound.not_before > lxp_ctx_batch_timestamp_ms(ctx) ||
        consent->timestamp_bound.not_after < lxp_ctx_batch_timestamp_ms(ctx) ||
        consent->timestamp_bound.not_before >= consent->timestamp_bound.not_after ||
        read_u64(consent->payload.bytes + 132U) != consent->timestamp_bound.not_after ||
        lxp_ct_is_zero(activity->idempotency_key, 32U) ||
        memcmp(consent->payload.bytes + 100U, activity->idempotency_key, 32U) != 0 ||
        memcmp(consent->idempotency_key, activity->idempotency_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_activity_verify_payload_hash(consent);
    if (status == LXP_OK) status = lxp_activity_verify_signature(consent);
    if (status == LXP_OK) status = lxp_did_id_derive(activity->actor_did.bytes, activity->actor_did.length, sponsor);
    if (status == LXP_OK) status = lxp_did_id_derive(consent->actor_did.bytes, consent->actor_did.length, did);
    if (status != LXP_OK) return status;
    runtime = ctx->kernel->module_runtime[LXP_MODULE_PROGRAMS];
    if (runtime == NULL || memcmp(sponsor, authority->actor, 32U) != 0 ||
        memcmp(sponsor, consent->payload.bytes + 4U, 32U) != 0 || memcmp(sponsor, did, 32U) == 0 ||
        memcmp(runtime->occupancy_asset_id, consent->payload.bytes + 36U, 32U) != 0 ||
        consent->actor_did.length == 0U || consent->actor_did.length > LX_ACCOUNT_NAME_MAX - 11U)
        return LXP_ERR_AUTH_SCOPE;
    (void)memset(account, 0, sizeof(*account));
    (void)memcpy(account->name, "agent:", 6U);
    (void)memcpy(account->name + 6U, consent->actor_did.bytes, consent->actor_did.length);
    (void)memcpy(account->name + 6U + consent->actor_did.length, ":main", 5U);
    account->name_length = (uint16_t)(consent->actor_did.length + 11U);
    account->kind = LX_ACCOUNT_AGENT_MAIN;
    account->has_asset = true;
    (void)memcpy(account->asset_id, runtime->occupancy_asset_id, 32U);
    account->has_authority_key = true;
    (void)memcpy(account->authority_key, consent->authority.bytes, 32U);
    account->created_at_sequence = ctx->global_sequence;
    status = lx_account_id_from_string(account->name, account->name_length, account->id);
    if (status == LXP_OK) status = lx_account_validate_canonical(account);
    if (status == LXP_OK && memcmp(account->id, consent->payload.bytes + 68U, 32U) != 0)
        status = LXP_ERR_AUTH_SCOPE;
    return status;
}

lxp_result lxp_governance_onboarding_prepared(const lxp_module_ctx *ctx)
{
    const lx_account *account;
    uint8_t did[32];
    bool state_present = false;
    if (ctx == NULL || ctx->module_id != LXP_MODULE_GOVERNANCE || !ctx->identity_staged ||
        ctx->identities == NULL || ctx->identities->count >= LXP_IDENTITY_STORE_CAPACITY ||
        ctx->staged_account_count != 1U || ctx->staged_identity.status != LXP_IDENTITY_ACTIVE ||
        ctx->staged_identity.next_sequence != 0U || ctx->staged_identity.revocation_sequence != ctx->global_sequence)
        return LXP_ERR_AUTH_SCOPE;
    account = &ctx->staged_accounts[0].account;
    if (account->kind != LX_ACCOUNT_AGENT_MAIN || !account->has_authority_key ||
        !account->has_asset || !lxp_u128_is_zero(account->balance) || account->next_sequence != 0U ||
        account->created_at_sequence != ctx->global_sequence || account->name_length <= 11U ||
        lx_account_validate_canonical(account) != LXP_OK ||
        lxp_did_id_derive(account->name + 6U, account->name_length - 11U, did) != LXP_OK ||
        memcmp(did, ctx->staged_identity.did_id, 32U) != 0 ||
        memcmp(account->authority_key, ctx->staged_identity.primary_key, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    for (size_t i = 0U; i < ctx->identities->count; ++i)
        if (memcmp(ctx->identities->identities[i].did_id, did, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
    for (size_t i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *entry = &ctx->staged[i];
        if (entry->key_length != 32U || memcmp(entry->key, did, 32U) != 0) continue;
        if (state_present || entry->deleted || entry->value_length != IDENTITY_BYTES ||
            memcmp(entry->value, "LXGI1", 5U) != 0 || memcmp(entry->value + 5U, did, 32U) != 0 ||
            memcmp(entry->value + 37U, account->authority_key, 32U) != 0 ||
            read_u64(entry->value + 69U) != ctx->global_sequence ||
            read_u64(entry->value + 215U) != ctx->global_sequence)
            return LXP_ERR_AUTH_SCOPE;
        state_present = true;
    }
    return state_present ? LXP_OK : LXP_ERR_AUTH_SCOPE;
}

lxp_result lxp_governance_onboard(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority)
{
    lxp_activity consent;
    lx_account_registration registration;
    lx_account *existing;
    const uint8_t *prior;
    size_t length;
    uint8_t did[32], state[IDENTITY_BYTES] = {0};
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || !ctx->mutable ||
        ctx->module_id != LXP_MODULE_GOVERNANCE || ctx->kernel == NULL ||
        ctx->kernel->state == NULL || ctx->kernel->state->accounts == NULL ||
        ctx->kernel->journal == NULL || !ctx->kernel->journal->open ||
        ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence || ctx->global_sequence == 0U ||
        ctx->identities == NULL || ctx->identity_staged || ctx->staged_account_count != 0U)
        return LXP_ERR_AUTH_SCOPE;
    if (ctx->identities->count >= LXP_IDENTITY_STORE_CAPACITY) return LXP_ERR_ARENA_EXHAUSTED;
    (void)memset(&registration, 0, sizeof(registration));
    status = consent_decode(ctx, activity, authority, &consent, &registration.account, did);
    if (status != LXP_OK) return status;
    for (size_t i = 0U; i < ctx->identities->count; ++i)
        if (memcmp(ctx->identities->identities[i].did_id, did, 32U) == 0)
            return LXP_ERR_SEQUENCE_REUSED;
    status = lxp_ctx_kv_get(ctx, did, 32U, &prior, &length);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status == LXP_OK ? LXP_ERR_SEQUENCE_REUSED : status;
    status = lxp_ctx_account_find(ctx, registration.account.id, &existing);
    if (status != LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE) return status == LXP_OK ? LXP_ERR_SEQUENCE_REUSED : status;
    (void)memcpy(state, "LXGI1", 5U);
    (void)memcpy(state + 5U, did, 32U);
    (void)memcpy(state + 37U, consent.authority.bytes, 32U);
    write_u64(state + 69U, ctx->global_sequence);
    write_u64(state + 215U, ctx->global_sequence);
    status = lxp_ctx_kv_put(ctx, did, 32U, state, sizeof(state));
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7110U, state, sizeof(state));
    if (status == LXP_OK) status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    if (status != LXP_OK) return status;
    registration.expected_count = ctx->kernel->state->accounts->count;
    ctx->staged_accounts[0] = registration;
    ctx->staged_account_count = 1U;
    (void)memset(&ctx->staged_identity, 0, sizeof(ctx->staged_identity));
    (void)memcpy(ctx->staged_identity.did_id, did, 32U);
    (void)memcpy(ctx->staged_identity.primary_key, consent.authority.bytes, 32U);
    ctx->staged_identity.status = LXP_IDENTITY_ACTIVE;
    ctx->staged_identity.revocation_sequence = ctx->global_sequence;
    ctx->identity_staged = true;
    return lxp_governance_onboarding_prepared(ctx);
}
