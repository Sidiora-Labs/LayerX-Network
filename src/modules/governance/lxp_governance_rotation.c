#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"

#include <string.h>

enum { IDENTITY_BYTES = 223, CONSENT_BYTES = 140, ROTATION_BYTES = 141 };

static uint64_t read64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    for (size_t i = 0U; i < 8U; ++i) value = (value << 8U) | bytes[i];
    return value;
}

static void write64(uint8_t *bytes, uint64_t value)
{
    for (size_t i = 0U; i < 8U; ++i) bytes[7U - i] = (uint8_t)(value >> (8U * i));
}

static lxp_result account_selected(const lxp_module_ctx *ctx,
    const lx_account *account, bool *selected)
{
    uint8_t did[32];
    lxp_result status;
    *selected = false;
    status = lx_account_owner_did(account, did);
    if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_OK;
    if (status != LXP_OK) return status;
    if (memcmp(did, ctx->owner_rotation_did, 32U) != 0) return LXP_OK;
    if (!account->has_authority_key) {
        return account->kind == LX_ACCOUNT_AGENT_MAIN || account->kind == LX_ACCOUNT_AGENT_ASSET ?
            LXP_ERR_AUTH_SCOPE : LXP_OK;
    }
    if (memcmp(account->authority_key, ctx->owner_rotation_from, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    *selected = true;
    return LXP_OK;
}

static lxp_result account_count(const lxp_module_ctx *ctx,
    const lx_account_registry *accounts, size_t *count)
{
    bool main = false;
    lxp_result status;
    if (accounts == NULL || accounts->count > LX_ACCOUNT_REGISTRY_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    status = lx_account_registry_index_validate(accounts);
    if (status != LXP_OK) return status;
    *count = 0U;
    for (size_t i = 0U; i < accounts->count; ++i) {
        bool selected;
        status = account_selected(ctx, &accounts->accounts[i], &selected);
        if (status != LXP_OK) return status;
        if (!selected) continue;
        ++*count;
        if (accounts->accounts[i].kind == LX_ACCOUNT_AGENT_MAIN) {
            if (main) return LXP_ERR_AUTH_SCOPE;
            main = true;
        }
    }
    return main && *count != 0U ? LXP_OK : LXP_ERR_AUTH_SCOPE;
}

lxp_result lxp_governance_rotation_accounts(const lxp_module_ctx *ctx,
    lx_account_registry *accounts, bool apply)
{
    size_t count;
    lxp_result status;
    if (ctx == NULL || !ctx->owner_rotation_staged ||
        ctx->module_id != LXP_MODULE_GOVERNANCE || ctx->owner_rotation_account_count == 0U)
        return LXP_ERR_AUTH_SCOPE;
    status = account_count(ctx, accounts, &count);
    if (status != LXP_OK) return status;
    if (count != ctx->owner_rotation_account_count) return LXP_ERR_CONTEXT_MISMATCH;
    if (!apply) return LXP_OK;
    for (size_t i = 0U; i < accounts->count; ++i) {
        bool selected;
        status = account_selected(ctx, &accounts->accounts[i], &selected);
        if (status != LXP_OK) return LXP_FATAL_INVARIANT;
        if (selected) (void)memcpy(accounts->accounts[i].authority_key,
                                  ctx->owner_rotation_to, 32U);
    }
    return LXP_OK;
}

lxp_result lxp_governance_rotation_refresh(const lxp_kernel *kernel,
    lxp_identity *identity)
{
    uint8_t key[33] = {0x0aU};
    if (kernel == NULL || identity == NULL) return LXP_ERR_NON_CANONICAL;
    (void)memcpy(key + 1U, identity->did_id, 32U);
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != sizeof(key) ||
            memcmp(entry->key, key, sizeof(key)) != 0) continue;
        const uint8_t *record = entry->value;
        if (entry->value_length != ROTATION_BYTES || memcmp(record, "LXOR1", 5U) != 0 ||
            memcmp(record + 5U, identity->did_id, 32U) != 0 ||
            memcmp(record + 69U, identity->primary_key, 32U) != 0 ||
            !lxp_ed25519_pubkey_is_canonical(record + 37U) ||
            memcmp(record + 37U, record + 69U, 32U) == 0 ||
            read64(record + 101U) == 0U || read64(record + 101U) > identity->revocation_sequence ||
            lxp_ct_is_zero(record + 109U, 32U)) return LXP_FATAL_INVARIANT;
        (void)memcpy(identity->superseded_key, record + 37U, 32U);
        identity->has_superseded_key = true;
        identity->rotation_effective_sequence = read64(record + 101U);
        return LXP_OK;
    }
    return LXP_OK;
}

static lxp_result consent_verify(lxp_module_ctx *ctx, const lxp_activity *activity,
    const uint8_t previous[IDENTITY_BYTES], lxp_activity *consent, uint8_t commitment[32])
{
    lxp_codec_reader reader;
    lxp_byte_span bytes, canonical;
    uint8_t digest[32], did[32];
    size_t mark = lxp_arena_mark(ctx->arena);
    bool match = false;
    lxp_result status = lxp_activity_verify_payload_hash(activity);
    if (status == LXP_OK) status = lxp_activity_verify_signature(activity);
    if (status == LXP_OK) status = lxp_activity_encode(activity, ctx->arena, &canonical);
    if (status == LXP_OK) status = lxp_activity_id(canonical.bytes, canonical.length, digest);
    if (lxp_arena_reset(ctx->arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    if (memcmp(digest, ctx->activity_id, 32U) != 0) return LXP_ERR_CONTEXT_MISMATCH;
    status = lxp_codec_reader_init(&reader, activity->payload.bytes + 4U, activity->payload.length - 4U);
    if (status == LXP_OK) status = lxp_codec_read_bytes(&reader, &bytes, 1016U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK) status = lxp_activity_decode(bytes.bytes, bytes.length, consent);
    if (status == LXP_OK) status = lxp_activity_encode(consent, ctx->arena, &canonical);
    if (status == LXP_OK) match = canonical.length == bytes.length &&
        memcmp(canonical.bytes, bytes.bytes, bytes.length) == 0;
    if (lxp_arena_reset(ctx->arena, mark) != LXP_OK) return LXP_FATAL_INVARIANT;
    if (status != LXP_OK) return status;
    if (!match || consent->protocol_version != 3U || consent->network_id != activity->network_id ||
        consent->activity_type != 0x00070002U || consent->authority.length != 32U ||
        memcmp(consent->authority.bytes, previous + 111U, 32U) != 0 ||
        !lxp_ed25519_pubkey_is_canonical(consent->authority.bytes) ||
        consent->account_sequence != 0U || !lxp_u128_is_zero(consent->fee_limit) ||
        consent->actor_did.length != activity->actor_did.length ||
        memcmp(consent->actor_did.bytes, activity->actor_did.bytes, activity->actor_did.length) != 0 ||
        consent->payload.length != CONSENT_BYTES || memcmp(consent->payload.bytes, "\x71\x02\x02\x05", 4U) != 0 ||
        consent->timestamp_bound.not_before >= consent->timestamp_bound.not_after ||
        consent->timestamp_bound.not_before > activity->timestamp_bound.not_before ||
        consent->timestamp_bound.not_after < activity->timestamp_bound.not_after ||
        read64(consent->payload.bytes + 132U) != consent->timestamp_bound.not_after ||
        memcmp(consent->idempotency_key, activity->idempotency_key, 32U) != 0 ||
        lxp_ct_is_zero(activity->idempotency_key, 32U) ||
        memcmp(consent->payload.bytes + 100U, activity->idempotency_key, 32U) != 0 ||
        memcmp(consent->payload.bytes + 36U, previous + 37U, 32U) != 0)
        return LXP_ERR_AUTH_SCOPE;
    status = lxp_activity_verify_payload_hash(consent);
    if (status == LXP_OK) status = lxp_activity_verify_signature(consent);
    if (status == LXP_OK) status = lxp_did_id_derive(activity->actor_did.bytes, activity->actor_did.length, did);
    if (status == LXP_OK) status = lxp_hash_context_value(previous, IDENTITY_BYTES, commitment);
    if (status != LXP_OK) return status;
    if (memcmp(consent->payload.bytes + 4U, did, 32U) != 0 ||
        memcmp(previous + 5U, did, 32U) != 0 ||
        memcmp(consent->payload.bytes + 68U, commitment, 32U) != 0) return LXP_ERR_AUTH_SCOPE;
    return LXP_OK;
}

lxp_result lxp_governance_rotation(lxp_module_ctx *ctx,
    const lxp_activity *activity, const lxp_authority_resolved *authority,
    const uint8_t previous[IDENTITY_BYTES], uint8_t next[IDENTITY_BYTES])
{
    lxp_activity consent;
    uint8_t commitment[32], key[33] = {0x0aU}, record[ROTATION_BYTES] = {0};
    lxp_result status;
    if (ctx == NULL || activity == NULL || authority == NULL || previous == NULL || next == NULL ||
        ctx->kernel == NULL || ctx->kernel->state == NULL || ctx->identities == NULL ||
        ctx->arena == NULL || ctx->kernel->journal == NULL || !ctx->kernel->journal->open ||
        ctx->kernel->journal->store != ctx->kernel->state ||
        ctx->kernel->journal->global_sequence != ctx->global_sequence ||
        !ctx->mutable || ctx->identity_staged || ctx->owner_rotation_staged ||
        ctx->staged_account_count != 0U || ctx->module_id != LXP_MODULE_GOVERNANCE ||
        activity->activity_type != 0x00070002U || activity->protocol_version != 3U ||
        activity->payload.length < 8U || memcmp(activity->payload.bytes, "\x71\x02\x01\x01", 4U) != 0 ||
        authority->kind != LXP_AUTHORITY_OWNER || activity->authority.length != 32U ||
        memcmp(activity->authority.bytes, authority->verified_key, 32U) != 0 ||
        memcmp(previous, "LXGI1", 5U) != 0 ||
        memcmp(previous + 37U, authority->verified_key, 32U) != 0 ||
        memcmp(previous + 5U, authority->actor, 32U) != 0 ||
        lxp_ct_is_zero(previous + 111U, 32U) ||
        read64(previous + 143U) == 0U || read64(previous + 151U) <= read64(previous + 143U) ||
        read64(previous + 159U) == 0U || read64(previous + 167U) == 0U ||
        ctx->global_sequence <= read64(previous + 69U) ||
        ctx->global_sequence < read64(previous + 159U)) return LXP_ERR_AUTH_SCOPE;
    uint64_t now = lxp_ctx_batch_timestamp_ms(ctx);
    if (now < read64(previous + 143U)) return LXP_ERR_NOT_YET_VALID;
    if (now > read64(previous + 151U)) return LXP_ERR_EXPIRED;
    status = consent_verify(ctx, activity, previous, &consent, commitment);
    if (status != LXP_OK) return status;
    (void)memcpy(ctx->owner_rotation_did, authority->actor, 32U);
    (void)memcpy(ctx->owner_rotation_from, previous + 37U, 32U);
    (void)memcpy(ctx->owner_rotation_to, consent.authority.bytes, 32U);
    status = account_count(ctx, ctx->kernel->state->accounts, &ctx->owner_rotation_account_count);
    if (status != LXP_OK) return status;
    (void)memcpy(next, previous, IDENTITY_BYTES);
    (void)memcpy(next + 37U, consent.authority.bytes, 32U);
    (void)memset(next + 111U, 0, 32U);
    write64(next + 69U, ctx->global_sequence);
    write64(next + 159U, ctx->global_sequence);
    write64(next + 215U, ctx->global_sequence);
    (void)memcpy(record, "LXOR1", 5U);
    (void)memcpy(record + 5U, authority->actor, 32U);
    (void)memcpy(record + 37U, previous + 37U, 32U);
    (void)memcpy(record + 69U, consent.authority.bytes, 32U);
    write64(record + 101U, ctx->global_sequence);
    (void)memcpy(record + 109U, commitment, 32U);
    (void)memcpy(key + 1U, authority->actor, 32U);
    status = lxp_ctx_kv_put(ctx, key, sizeof(key), record, sizeof(record));
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7142U, record, sizeof(record));
    if (status == LXP_OK) status = lxp_state_journal_require_account_root(ctx->kernel->journal);
    if (status == LXP_OK) ctx->owner_rotation_staged = true;
    return status;
}

lxp_result lxp_governance_rotation_prepared(const lxp_module_ctx *ctx)
{
    bool identity = false, state = false, history = false;
    bool committed = false;
    uint8_t expected[IDENTITY_BYTES], commitment[32];
    if (ctx == NULL || !ctx->owner_rotation_staged || ctx->identity_staged ||
        ctx->module_id != LXP_MODULE_GOVERNANCE || ctx->identities == NULL ||
        ctx->kernel == NULL || ctx->kernel->state == NULL ||
        ctx->staged_count > LXP_MODULE_MAX_STAGED_WRITES ||
        ctx->staged_account_count != 0U || ctx->identities->count > LXP_IDENTITY_STORE_CAPACITY ||
        !lxp_ed25519_pubkey_is_canonical(ctx->owner_rotation_from) ||
        !lxp_ed25519_pubkey_is_canonical(ctx->owner_rotation_to) ||
        memcmp(ctx->owner_rotation_from, ctx->owner_rotation_to, 32U) == 0)
        return LXP_ERR_AUTH_SCOPE;
    for (size_t i = 0U; i < ctx->kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &ctx->kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != 32U ||
            memcmp(entry->key, ctx->owner_rotation_did, 32U) != 0) continue;
        if (committed || entry->value_length != IDENTITY_BYTES ||
            memcmp(entry->value, "LXGI1", 5U) != 0 ||
            memcmp(entry->value + 37U, ctx->owner_rotation_from, 32U) != 0 ||
            memcmp(entry->value + 111U, ctx->owner_rotation_to, 32U) != 0)
            return LXP_ERR_AUTH_SCOPE;
        lxp_result status = lxp_hash_context_value(entry->value, IDENTITY_BYTES, commitment);
        if (status != LXP_OK) return status;
        (void)memcpy(expected, entry->value, IDENTITY_BYTES);
        (void)memcpy(expected + 37U, ctx->owner_rotation_to, 32U);
        (void)memset(expected + 111U, 0, 32U);
        write64(expected + 69U, ctx->global_sequence);
        write64(expected + 159U, ctx->global_sequence);
        write64(expected + 215U, ctx->global_sequence);
        committed = true;
    }
    if (!committed) return LXP_ERR_AUTH_SCOPE;
    for (size_t i = 0U; i < ctx->identities->count; ++i) {
        const lxp_identity *current = &ctx->identities->identities[i];
        if (memcmp(current->did_id, ctx->owner_rotation_did, 32U) != 0) continue;
        if (identity || memcmp(current->primary_key, ctx->owner_rotation_from, 32U) != 0)
            return LXP_ERR_AUTH_SCOPE;
        identity = true;
    }
    for (size_t i = 0U; i < ctx->staged_count; ++i) {
        const lxp_module_kv_change *entry = &ctx->staged[i];
        if (entry->key_length == 33U && entry->key[0] == 0x0aU &&
            memcmp(entry->key + 1U, ctx->owner_rotation_did, 32U) == 0) {
            if (history || entry->deleted || entry->value_length != ROTATION_BYTES ||
                memcmp(entry->value, "LXOR1", 5U) != 0 ||
                memcmp(entry->value + 5U, ctx->owner_rotation_did, 32U) != 0 ||
                memcmp(entry->value + 37U, ctx->owner_rotation_from, 32U) != 0 ||
                memcmp(entry->value + 69U, ctx->owner_rotation_to, 32U) != 0 ||
                read64(entry->value + 101U) != ctx->global_sequence ||
                memcmp(entry->value + 109U, commitment, 32U) != 0) return LXP_ERR_AUTH_SCOPE;
            history = true;
            continue;
        }
        if (entry->key_length != 32U || memcmp(entry->key, ctx->owner_rotation_did, 32U) != 0) continue;
        if (state || entry->deleted || entry->value_length != IDENTITY_BYTES ||
            memcmp(entry->value, "LXGI1", 5U) != 0 ||
            memcmp(entry->value + 5U, ctx->owner_rotation_did, 32U) != 0 ||
            memcmp(entry->value + 37U, ctx->owner_rotation_to, 32U) != 0 ||
            !lxp_ct_is_zero(entry->value + 111U, 32U) ||
            read64(entry->value + 69U) != ctx->global_sequence ||
            read64(entry->value + 159U) != ctx->global_sequence ||
            read64(entry->value + 215U) != ctx->global_sequence) return LXP_ERR_AUTH_SCOPE;
        if (memcmp(entry->value, expected, IDENTITY_BYTES) != 0) return LXP_ERR_AUTH_SCOPE;
        state = true;
    }
    if (!identity || !state || !history) return LXP_ERR_AUTH_SCOPE;
    return lxp_governance_rotation_accounts(ctx, ctx->kernel->state->accounts, false);
}
