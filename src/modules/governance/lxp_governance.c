#include "layerx/lxp_module_ctx.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/programs.h"
#include <string.h>

enum { STATE_BYTES = 223, DID = 5, PRIMARY = 37, REVOCATION = 69,
       ROOT = 77, THRESHOLD = 109, PENDING = 111, BEGIN = 143,
       END = 151, EFFECTIVE = 159, ROTATION_REV = 167,
       RECOVERY_REV = 175, DELAY = 183, MAX_DELAY = 191,
       ROTATION_DELAY = 199, ROTATION_MAX = 207, SEQUENCE = 215 };

typedef struct governance_payload {
    uint16_t ordinal;
    uint16_t fields;
    size_t length;
    uint8_t bytes[1024];
} governance_payload;

static uint64_t read64(const uint8_t *p)
{
    uint64_t v = 0U;
    for (size_t i = 0U; i < 8U; ++i) v = (v << 8U) | p[i];
    return v;
}

static void write64(uint8_t *p, uint64_t v)
{
    for (size_t i = 0U; i < 8U; ++i) p[7U - i] = (uint8_t)(v >> (i * 8U));
}

bool lxp_governance_activity(uint32_t type)
{
    return type == 0x00070001U || type == 0x00070002U ||
           type == 0x00070003U || type == 0x00070005U || type == 0x00070006U ||
           type == 0x00070008U;
}

lxp_result lxp_governance_identity_refresh(const lxp_kernel *kernel,
                                           lxp_identity *identity)
{
    if (kernel == NULL || identity == NULL) return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != 32U ||
            memcmp(entry->key, identity->did_id, 32U) != 0) continue;
        const uint8_t *s = entry->value;
        if (entry->value_length != STATE_BYTES || memcmp(s, "LXGI1", 5U) != 0 ||
            memcmp(s + DID, identity->did_id, 32U) != 0)
            return LXP_FATAL_INVARIANT;
        (void)memcpy(identity->primary_key, s + PRIMARY, 32U);
        identity->revocation_sequence = read64(s + REVOCATION);
        (void)memcpy(identity->recovery_root, s + ROOT, 32U);
        identity->recovery_threshold = (uint16_t)((uint16_t)s[THRESHOLD] << 8U | s[THRESHOLD + 1]);
        (void)memcpy(identity->pending_key, s + PENDING, 32U);
        identity->has_pending_key = !lxp_ct_is_zero(s + PENDING, 32U);
        identity->rotation_announced_at = read64(s + BEGIN);
        identity->rotation_effective_at = read64(s + BEGIN);
        identity->rotation_lapse_at = read64(s + END);
        identity->rotation_effective_sequence = read64(s + EFFECTIVE);
        return LXP_OK;
    }
    return LXP_OK;
}

lxp_result lxp_governance_identities_restore(const lxp_kernel *kernel,
                                             lxp_identity_store *identities)
{
    if (kernel == NULL || identities == NULL || identities->count > LXP_IDENTITY_STORE_CAPACITY)
        return LXP_ERR_NON_CANONICAL;
    for (size_t i = 0U; i < kernel->module_kv_count; ++i) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[i];
        if (entry->module_id != LXP_MODULE_GOVERNANCE || entry->key_length != 32U ||
            entry->value_length < 5U || memcmp(entry->value, "LXGI1", 5U) != 0) continue;
        if (entry->value_length != STATE_BYTES || memcmp(entry->key, entry->value + DID, 32U) != 0 ||
            !lxp_ed25519_pubkey_is_canonical(entry->value + PRIMARY))
            return LXP_FATAL_INVARIANT;
        size_t index = 0U;
        while (index < identities->count && memcmp(identities->identities[index].did_id, entry->key, 32U) != 0)
            ++index;
        if (index == identities->count) {
            if (index >= LXP_IDENTITY_STORE_CAPACITY) return LXP_ERR_ARENA_EXHAUSTED;
            lxp_identity *identity = &identities->identities[identities->count++];
            (void)memset(identity, 0, sizeof(*identity));
            (void)memcpy(identity->did_id, entry->key, 32U);
            identity->status = LXP_IDENTITY_ACTIVE;
        } else continue;
        lxp_result status = lxp_governance_identity_refresh(kernel, &identities->identities[index]);
        if (status != LXP_OK) return status;
    }
    return LXP_OK;
}

static lxp_result decode(lxp_module_ctx *ctx, uint16_t ordinal,
                          const uint8_t *bytes, size_t length, void **decoded)
{
    governance_payload *p;
    void *memory = NULL;
    if (bytes == NULL || decoded == NULL || length < 4U || length > 1024U ||
        !lxp_governance_activity(0x00070000U | ordinal) || bytes[0] != 0x71U ||
        bytes[1] != ordinal || (ordinal == 1U ? (bytes[2] != 0U && bytes[2] != 2U) :
        ordinal == 5U ? (bytes[2] != 1U && bytes[2] != 2U) :
        bytes[2] != (ordinal == 8U ? 1U : 0U)))
        return LXP_ERR_NON_CANONICAL;
    uint16_t fields = bytes[3];
    if ((ordinal == 1U && (bytes[2] == 0U ? (fields != 2U || length != 68U) : (fields != 1U || length < 8U))) ||
        (ordinal == 2U && (fields != 4U || length != 92U)) ||
        (ordinal == 3U && !((fields == 3U && length == 70U) ||
                            (fields == 5U && length == 86U))) ||
        (ordinal == 5U && (fields != (bytes[2] == 2U ? 5U : 3U) || length < 52U)) ||
        (ordinal == 6U && (fields != 3U || length != 45U)) ||
        (ordinal == 8U && (fields != 1U || length < 9U)))
        return LXP_ERR_NON_CANONICAL;
    lxp_result result = lxp_ctx_arena_alloc(ctx, sizeof(*p), _Alignof(governance_payload), &memory);
    if (result != LXP_OK) return result;
    p = memory;
    p->ordinal = ordinal;
    p->fields = fields;
    p->length = length;
    (void)memcpy(p->bytes, bytes, length);
    *decoded = p;
    return LXP_OK;
}

static lxp_result validate(lxp_module_ctx *ctx, const lxp_activity *activity,
                            const lxp_authority_resolved *authority, const void *decoded)
{
    const governance_payload *p = decoded;
    uint8_t did[32];
    if (activity == NULL || authority == NULL || p == NULL || activity->protocol_version != 3U ||
        authority->kind != LXP_AUTHORITY_OWNER ||
        lxp_did_id_derive(activity->actor_did.bytes, activity->actor_did.length, did) != LXP_OK ||
        memcmp(did, authority->actor, 32U) != 0 ||
        (p->ordinal <= 3U && !(p->ordinal == 1U && p->bytes[2] == 2U) &&
         memcmp(did, p->bytes + 4U, 32U) != 0))
        return LXP_ERR_AUTH_SCOPE;
    return lxp_ctx_charge_gas(ctx, p->length);
}

static lxp_result grant_scope_validate(lxp_module_ctx *ctx,
                                        const lxp_authority_grant *grant);

static lxp_result session_inherit_fee(lxp_module_ctx *ctx,
    const lxp_authority_grant *successor, const uint8_t predecessor_id[32],
    const uint8_t expected_commitment[32])
{
    lxp_authority_grant predecessor, charged;
    uint8_t key[33], actual_commitment[32], counters[72];
    const uint8_t *prior;
    size_t prior_length;
    lxp_result status;
    if (successor->authentication_only || successor->revoked || !successor->fee_budget.present ||
        lxp_ct_is_zero(predecessor_id, 32U) || lxp_ct_is_zero(expected_commitment, 32U) ||
        memcmp(predecessor_id, successor->grant_id, 32U) == 0) return LXP_ERR_AUTH_SCOPE;
    status = lxp_authority_grant_load(ctx->kernel, predecessor_id, &predecessor);
    if (status != LXP_OK) return status;
    status = lxp_authority_session_charge_commitment(&predecessor, actual_commitment);
    if (status != LXP_OK) return status;
    const lxp_authority_fee_budget *a = &predecessor.fee_budget;
    const lxp_authority_fee_budget *b = &successor->fee_budget;
    if (memcmp(actual_commitment, expected_commitment, 32U) != 0 ||
        memcmp(predecessor.grantor, successor->grantor, 32U) != 0 ||
        memcmp(predecessor.grantee, successor->grantee, 32U) != 0 ||
        memcmp(predecessor.key, successor->key, 32U) == 0 ||
        predecessor.revoked_at_sequence >= lxp_ctx_global_sequence(ctx) ||
        predecessor.not_before != successor->not_before ||
        !lxp_authority_scope_equal(&predecessor.scope, &successor->scope) ||
        memcmp(a->asset_id, b->asset_id, 32U) != 0 ||
        lxp_u128_cmp(a->maximum_per_activity, b->maximum_per_activity) != 0 ||
        lxp_u128_cmp(a->maximum_total, b->maximum_total) != 0 ||
        lxp_u128_cmp(a->maximum_per_period, b->maximum_per_period) != 0 ||
        a->period_length != b->period_length ||
        (b->period_length != 0U && b->period_start != successor->not_before))
        return LXP_ERR_AUTH_SCOPE;
    lxp_authority_session_successor_key(predecessor_id, key);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &prior, &prior_length);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status == LXP_OK ? LXP_ERR_SEQUENCE_REUSED : status;
    status = lxp_ctx_kv_put(ctx, key, sizeof(key), successor->grant_id, 32U);
    if (status != LXP_OK) return status;
    charged = *successor;
    charged.fee_budget.spent_total = a->spent_total;
    charged.fee_budget.spent_this_period = a->spent_this_period;
    charged.fee_budget.period_start = a->period_start;
    status = lxp_authority_fee_record_encode(&charged, counters);
    lxp_authority_fee_record_key(successor->grant_id, key);
    if (status == LXP_OK) status = lxp_ctx_kv_put(ctx, key, sizeof(key), counters, sizeof(counters));
    return status;
}

static lxp_result session(lxp_module_ctx *ctx, const governance_payload *p,
                           const lxp_authority_resolved *authority, uint8_t state[STATE_BYTES])
{
    lxp_codec_reader reader;
    lxp_byte_span body;
    lxp_byte_span span;
    lxp_authority_grant grant;
    uint64_t expiry_sequence;
    uint8_t action_key[32], predecessor_id[32] = {0}, expected_commitment[32] = {0};
    uint8_t summary[209] = {0};
    uint8_t key[33];
    const uint8_t *prior;
    size_t length;
    lxp_result status;
    (void)memset(&grant, 0, sizeof(grant));
#define READ(call) do { status = (call); if (status != LXP_OK) return status; } while (0)
#define FIXED(dst, n) do { READ(lxp_codec_read_bytes(&reader, &span, n)); if (span.length != n) return LXP_ERR_NON_CANONICAL; (void)memcpy(dst, span.bytes, n); } while (0)
    READ(lxp_codec_reader_init(&reader, p->bytes + 4U, p->length - 4U));
    READ(lxp_codec_read_bytes(&reader, &body, 1024U));
    READ(lxp_codec_read_u64(&reader, &expiry_sequence));
    FIXED(action_key, 32U);
    if (p->bytes[2] == 2U) { FIXED(predecessor_id, 32U); FIXED(expected_commitment, 32U); }
    READ(lxp_codec_finish(&reader));
    if (expiry_sequence <= lxp_ctx_global_sequence(ctx) || lxp_ct_is_zero(action_key, 32U))
        return LXP_ERR_AUTH_SCOPE;
    READ(lxp_grant_decode(body.bytes, body.length, &grant));
    if (grant.kind != LXP_AUTHORITY_SESSION_KEY) return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    if (grant.revoked) return LXP_ERR_AUTH_REVOKED;
    if (memcmp(grant.grantor, authority->actor, 32U) != 0 ||
        memcmp(grant.grantee, authority->actor, 32U) != 0 ||
        !lxp_ed25519_pubkey_is_canonical(grant.key) ||
        grant.grantor_revocation_sequence != read64(state + REVOCATION) ||
        grant.not_after <= lxp_ctx_batch_timestamp_ms(ctx) ||
        (!grant.authentication_only && grant.scope.activity_ordinal_min == 0U) ||
        (grant.scope.module_mask & ~UINT64_C(0x3fe)) != 0U ||
        grant.revoked_at_sequence != 0U || !lxp_ct_is_zero(grant.grantor_signature, 64U))
        return LXP_ERR_AUTH_SCOPE;
    lxp_authority_grant canonical;
    if (grant.authentication_only) {
        READ(lxp_authentication_key_bind(&canonical, grant.grantor, grant.key,
            grant.not_before, grant.not_after, grant.grantor_revocation_sequence));
    } else {
        READ(lxp_session_key_bind(&canonical, grant.grantor, grant.key,
            grant.scope.module_mask, grant.scope.activity_ordinal_min, grant.scope.activity_ordinal_max,
            grant.not_before, grant.not_after, grant.grantor_revocation_sequence));
    }
    canonical.fee_budget = grant.fee_budget;
    if (canonical.fee_budget.present) READ(grant_scope_validate(ctx, &canonical));
    READ(lxp_grant_id_compute(&canonical, canonical.grant_id));
    lxp_byte_span encoded;
    READ(lxp_grant_encode(&canonical, ctx->arena, &encoded));
    if (encoded.length != body.length || memcmp(encoded.bytes, body.bytes, body.length) != 0)
        return LXP_ERR_NON_CANONICAL;
    key[0] = 5U;
    (void)memcpy(key + 1U, canonical.grant_id, 32U);
    status = lxp_ctx_kv_get(ctx, key, sizeof(key), &prior, &length);
    if (status != LXP_ERR_UNKNOWN_FIELD) return status == LXP_OK ? LXP_ERR_SEQUENCE_REUSED : status;
    if (p->bytes[2] == 2U) READ(session_inherit_fee(ctx, &canonical, predecessor_id, expected_commitment));
    READ(lxp_ctx_kv_put(ctx, key, sizeof(key), body.bytes, body.length));
    (void)memcpy(summary, "LXGS2", 5U);
    (void)memcpy(summary + 5U, canonical.grant_id, 32U);
    (void)memcpy(summary + 37U, canonical.grantor, 32U);
    (void)memcpy(summary + 69U, authority->verified_key, 32U);
    (void)memcpy(summary + 101U, action_key, 32U);
    (void)memcpy(summary + 133U, canonical.key, 32U);
    write64(summary + 165U, expiry_sequence);
    write64(summary + 173U, canonical.scope.module_mask);
    summary[181] = (uint8_t)(canonical.scope.activity_ordinal_min >> 8U);
    summary[182] = (uint8_t)canonical.scope.activity_ordinal_min;
    summary[183] = (uint8_t)(canonical.scope.activity_ordinal_max >> 8U);
    summary[184] = (uint8_t)canonical.scope.activity_ordinal_max;
    write64(summary + 185U, canonical.not_before);
    write64(summary + 193U, canonical.not_after);
    write64(summary + 201U, canonical.grantor_revocation_sequence);
    key[0] = 0x15U;
    READ(lxp_ctx_kv_put(ctx, key, sizeof(key), summary, sizeof(summary)));
    READ(lxp_ctx_emit_event(ctx, 0x7145U, summary, sizeof(summary)));
    for (size_t offset = 0U; offset < body.length; offset += 256U) {
        size_t remaining = body.length - offset;
        READ(lxp_ctx_emit_event(ctx, offset == 0U ? 0x7105U : 0x7125U,
            body.bytes + offset, remaining < 256U ? remaining : 256U));
    }
#undef FIXED
#undef READ
    return LXP_OK;
}

static lxp_result grant_scope_validate(lxp_module_ctx *ctx,
                                        const lxp_authority_grant *grant)
{
    lxp_authority_envelope envelope;
    const lxp_authority_scope *scope = &grant->scope;
    lxp_result status = lxp_authority_envelope_declare(ctx->kernel, ctx->epoch,
                                                       &envelope);
    if (status != LXP_OK) return status;
    if ((scope->module_mask & ~envelope.module_mask) != 0U ||
        scope->activity_ordinal_min < envelope.activity_ordinal_min ||
        scope->activity_ordinal_max > envelope.activity_ordinal_max ||
        !lxp_u128_is_zero(scope->spent_total) ||
        !lxp_u128_is_zero(scope->spent_this_period) ||
        (!lxp_u128_is_zero(scope->maximum_total) &&
         lxp_u128_cmp(scope->maximum_per_activity, scope->maximum_total) > 0) ||
        (scope->period_length == 0U &&
         (!lxp_u128_is_zero(scope->maximum_per_period) || scope->period_start != 0U)) ||
        (scope->period_length != 0U &&
         (scope->period_start != grant->not_before ||
          lxp_u128_cmp(scope->maximum_per_activity, scope->maximum_per_period) > 0)))
        return LXP_ERR_AUTH_SCOPE;
    if (grant->fee_budget.present) {
        bool enforced = false;
        status = lxp_authority_allowance_policy(ctx->kernel, &enforced);
        if (status != LXP_OK || !enforced) return status == LXP_OK ? LXP_ERR_AUTH_SCOPE : status;
        const lxp_authority_fee_budget *fee = &grant->fee_budget;
        const lx_programs_transfer_runtime *runtime = ctx->kernel->module_runtime[LXP_MODULE_PROGRAMS];
        if (runtime == NULL || memcmp(fee->asset_id, runtime->occupancy_asset_id, 32U) != 0 ||
            !lxp_u128_is_zero(fee->spent_total) || !lxp_u128_is_zero(fee->spent_this_period) ||
            (fee->period_length != 0U && fee->period_start != grant->not_before))
            return LXP_ERR_AUTH_SCOPE;
    }
    return LXP_OK;
}

static lxp_result issue_grant(lxp_module_ctx *ctx, const governance_payload *p,
                               const lxp_authority_resolved *authority,
                               const uint8_t state[STATE_BYTES])
{
    lxp_codec_reader reader;
    lxp_byte_span body, encoded;
    lxp_authority_grant grant, prior_grant;
    uint8_t key[33] = {5U};
    lxp_result status = lxp_codec_reader_init(&reader, p->bytes + 4U, p->length - 4U);
    if (status == LXP_OK) status = lxp_codec_read_bytes(&reader, &body, 1024U);
    if (status == LXP_OK) status = lxp_codec_finish(&reader);
    if (status == LXP_OK) status = lxp_grant_decode(body.bytes, body.length, &grant);
    if (status != LXP_OK) return status;
    if ((grant.kind != LXP_AUTHORITY_DELEGATED_CAPABILITY &&
         grant.kind != LXP_AUTHORITY_BUDGET_ALLOWANCE) ||
        memcmp(grant.grantor, authority->actor, 32U) != 0 ||
        memcmp(grant.grantee, authority->actor, 32U) != 0 ||
        !lxp_ed25519_pubkey_is_canonical(grant.key) ||
        memcmp(grant.key, authority->verified_key, 32U) == 0 ||
        grant.grantor_revocation_sequence != read64(state + REVOCATION) ||
        grant.not_after == UINT64_MAX ||
        grant.not_after <= lxp_ctx_batch_timestamp_ms(ctx) || grant.revoked ||
        grant.revoked_at_sequence != 0U ||
        !lxp_ct_is_zero(grant.grantor_signature, sizeof(grant.grantor_signature)))
        return LXP_ERR_AUTH_SCOPE;
    status = grant_scope_validate(ctx, &grant);
    if (status == LXP_OK) status = lxp_grant_encode(&grant, ctx->arena, &encoded);
    if (status != LXP_OK) return status;
    if (encoded.length != body.length || memcmp(encoded.bytes, body.bytes, body.length) != 0)
        return LXP_ERR_NON_CANONICAL;
    status = lxp_authority_grant_lookup(ctx->kernel, grant.grantor, grant.key, &prior_grant);
    if (status == LXP_OK) return LXP_ERR_SEQUENCE_REUSED;
    if (status != LXP_ERR_UNKNOWN_FIELD) return status;
    status = lxp_grant_id_compute(&grant, key + 1U);
    if (status == LXP_OK) status = lxp_ctx_kv_put(ctx, key, sizeof(key), body.bytes, body.length);
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7148U, key + 1U, 32U);
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7108U, body.bytes,
                                                    body.length < 256U ? body.length : 256U);
    for (size_t offset = 256U; status == LXP_OK && offset < body.length; offset += 256U) {
        size_t remaining = body.length - offset;
        status = lxp_ctx_emit_event(ctx, 0x7128U, body.bytes + offset,
                                    remaining < 256U ? remaining : 256U);
    }
    return status;
}

static lxp_result execute(lxp_module_ctx *ctx, const lxp_activity *activity,
                           const lxp_authority_resolved *authority, const void *decoded,
                           lxp_effect_buffer *effects)
{
    const governance_payload *p = decoded;
    const uint8_t *prior;
    size_t length;
    uint8_t state[STATE_BYTES] = {0};
    if (p->ordinal == 1U && p->bytes[2] == 2U)
        return lxp_governance_onboard(ctx, activity, authority);
    uint64_t sequence = lxp_ctx_global_sequence(ctx);
    lxp_result status = lxp_ctx_kv_get(ctx, authority->actor, 32U, &prior, &length);
    (void)activity;
    (void)effects;
    if (status == LXP_OK) {
        if (length != STATE_BYTES || memcmp(prior, "LXGI1", 5U) != 0) return LXP_FATAL_INVARIANT;
        if (p->ordinal == 1U) return LXP_ERR_SEQUENCE_REUSED;
        (void)memcpy(state, prior, length);
        if (memcmp(state + PRIMARY, authority->verified_key, 32U) != 0) return LXP_ERR_AUTH_SCOPE;
    } else if (status == LXP_ERR_UNKNOWN_FIELD && p->ordinal == 1U) {
        if (!lxp_ed25519_pubkey_is_canonical(p->bytes + 36U) ||
            memcmp(p->bytes + 36U, authority->verified_key, 32U) != 0 || sequence == 0U)
            return LXP_ERR_BAD_SIGNATURE;
        (void)memcpy(state, "LXGI1", 5U);
        (void)memcpy(state + DID, authority->actor, 32U);
        (void)memcpy(state + PRIMARY, authority->verified_key, 32U);
        write64(state + REVOCATION, sequence);
    } else return status;
    if (p->ordinal == 2U) {
        uint64_t begin = read64(p->bytes + 68U);
        uint64_t end = read64(p->bytes + 76U);
        uint64_t effective = read64(p->bytes + 84U);
        uint64_t now = lxp_ctx_batch_timestamp_ms(ctx);
        if (!lxp_ed25519_pubkey_is_canonical(p->bytes + 36U) ||
            memcmp(state + PRIMARY, p->bytes + 36U, 32U) == 0 ||
            !lxp_ct_is_zero(state + PENDING, 32U) || begin <= now || end <= begin ||
            effective <= sequence || read64(state + ROTATION_REV) == UINT64_MAX)
            return LXP_ERR_AUTH_SCOPE;
        (void)memcpy(state + PENDING, p->bytes + 36U, 32U);
        write64(state + BEGIN, begin);
        write64(state + END, end);
        write64(state + EFFECTIVE, effective);
        write64(state + ROTATION_REV, read64(state + ROTATION_REV) + 1U);
        write64(state + ROTATION_DELAY, begin - now);
        write64(state + ROTATION_MAX, end - now);
    } else if (p->ordinal == 3U) {
        if (lxp_ct_is_zero(p->bytes + 36U, 32U) || (p->bytes[68] == 0U && p->bytes[69] == 0U) ||
            read64(state + RECOVERY_REV) == UINT64_MAX) return LXP_ERR_AUTH_SCOPE;
        if (p->fields == 5U && (read64(p->bytes + 70U) == 0U ||
            read64(p->bytes + 78U) < read64(p->bytes + 70U))) return LXP_ERR_AUTH_SCOPE;
        (void)memcpy(state + ROOT, p->bytes + 36U, 34U);
        write64(state + RECOVERY_REV, read64(state + RECOVERY_REV) + 1U);
        write64(state + DELAY, p->fields == 5U ? read64(p->bytes + 70U) : 0U);
        write64(state + MAX_DELAY, p->fields == 5U ? read64(p->bytes + 78U) : 0U);
    } else if (p->ordinal == 5U) {
        status = session(ctx, p, authority, state);
        if (status != LXP_OK) return status;
    } else if (p->ordinal == 8U) {
        status = issue_grant(ctx, p, authority, state);
        if (status != LXP_OK) return status;
    } else if (p->ordinal == 6U) {
        uint8_t key[33];
        uint8_t revoked[41];
        key[0] = 5U;
        (void)memcpy(key + 1U, p->bytes + 4U, 32U);
        status = lxp_ctx_kv_get(ctx, key, sizeof(key), &prior, &length);
        if (status != LXP_OK) return status;
        lxp_authority_grant stored_grant;
        status = lxp_grant_decode(prior, length, &stored_grant);
        if (status != LXP_OK ||
            memcmp(stored_grant.grantor, authority->actor, 32U) != 0 ||
            read64(p->bytes + 37U) != sequence || p->bytes[36] == 0U || p->bytes[36] > 5U)
            return LXP_ERR_AUTH_SCOPE;
        (void)memcpy(revoked, p->bytes + 4U, sizeof(revoked));
        key[0] = 6U;
        status = lxp_ctx_kv_get(ctx, key, sizeof(key), &prior, &length);
        if (status != LXP_ERR_UNKNOWN_FIELD) return status == LXP_OK ? LXP_ERR_AUTH_REVOKED : status;
        status = lxp_ctx_kv_put(ctx, key, sizeof(key), revoked, sizeof(revoked));
        if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7106U, revoked, sizeof(revoked));
        if (status != LXP_OK) return status;
        write64(state + REVOCATION, sequence);
    }
    write64(state + SEQUENCE, sequence);
    status = lxp_ctx_kv_put(ctx, authority->actor, 32U, state, sizeof(state));
    if (status == LXP_OK) status = lxp_ctx_emit_event(ctx, 0x7110U, state, sizeof(state));
    return status;
}

static lxp_result genesis(lxp_module_ctx *ctx, const uint8_t *bytes, size_t length)
{
    return ctx == NULL || (bytes == NULL && length != 0U) ? LXP_ERR_NON_CANONICAL : LXP_OK;
}
static lxp_result epoch(lxp_module_ctx *ctx, uint64_t number, uint64_t timestamp)
{
    (void)number;
    (void)timestamp;
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : LXP_OK;
}
static lxp_result root(lxp_module_ctx *ctx, uint8_t digest[32])
{
    return ctx == NULL ? LXP_ERR_NON_CANONICAL : lxp_state_subtree_root(ctx->kernel, LXP_MODULE_GOVERNANCE, digest);
}
const lxp_module_iface *lxp_governance_module_iface(void)
{
    static const uint32_t types[] = {0x00070001U, 0x00070002U, 0x00070003U, 0x00070005U, 0x00070006U, 0x00070008U};
    static const lxp_module_iface iface = {LXP_MODULE_GOVERNANCE, 1U, "governance", types, 6U,
        genesis, decode, validate, execute, epoch, epoch, root, NULL};
    return &iface;
}
