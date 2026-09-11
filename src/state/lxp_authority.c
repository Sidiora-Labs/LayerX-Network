#include "layerx/lxp_authority.h"

#include "layerx/lxp_admission.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_hash.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_protocol.h"

#include <stdlib.h>
#include <string.h>

static lxp_result write_amount(lxp_codec_writer *writer, lxp_u128 amount)
{
    return lxp_codec_write_u128(writer, amount);
}

static lxp_result validate_grant(const lxp_authority_grant *grant)
{
    if (grant->kind < LXP_AUTHORITY_OWNER ||
        grant->kind > LXP_AUTHORITY_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    if (grant->not_after == 0U || grant->not_after <= grant->not_before ||
        lxp_ct_is_zero(grant->grantee, 32U) || lxp_ct_is_zero(grant->key, 32U) ||
        grant->scope.module_mask == 0U ||
        grant->scope.activity_ordinal_min > grant->scope.activity_ordinal_max)
        return LXP_ERR_MALFORMED_GRANT;
    if (grant->kind == LXP_AUTHORITY_DELEGATED_CAPABILITY ||
        grant->kind == LXP_AUTHORITY_BUDGET_ALLOWANCE) {
        if (lxp_ct_is_zero(grant->scope.asset_id, 32U) ||
            lxp_u128_is_zero(grant->scope.maximum_per_activity) ||
            lxp_ct_is_zero(grant->scope.purpose_hash, 32U) ||
            grant->grantor_revocation_sequence == 0U ||
            (lxp_u128_is_zero(grant->scope.maximum_total) &&
             (grant->scope.period_length == 0U ||
              lxp_u128_is_zero(grant->scope.maximum_per_period))))
            return LXP_ERR_MALFORMED_GRANT;
    }
    return LXP_OK;
}

lxp_result lxp_grant_encode(const lxp_authority_grant *grant,
                            lxp_arena *arena, lxp_byte_span *encoded)
{
    lxp_codec_writer writer;
    lxp_result status;
    if (grant == NULL || arena == NULL || encoded == NULL)
        return LXP_ERR_MALFORMED_GRANT;
    status = validate_grant(grant);
    if (status != LXP_OK) return status;
    status = lxp_codec_writer_init(&writer, arena, 1024U);
    if (status != LXP_OK) return status;
#define WRITE(expression) do { status = (expression); if (status != LXP_OK) return status; } while (0)
    WRITE(lxp_codec_write_struct_header(&writer, 0x2001U));
    WRITE(lxp_codec_write_u8(&writer, 1U));
    WRITE(lxp_codec_write_bytes(&writer, grant->grantor, 32U, 32U));
    WRITE(lxp_codec_write_bytes(&writer, grant->grantee, 32U, 32U));
    WRITE(lxp_codec_write_u8(&writer, (uint8_t)grant->kind));
    WRITE(lxp_codec_write_bytes(&writer, grant->key, 32U, 32U));
    WRITE(lxp_codec_write_u64(&writer, grant->scope.module_mask));
    WRITE(lxp_codec_write_u16(&writer, grant->scope.activity_ordinal_min));
    WRITE(lxp_codec_write_u16(&writer, grant->scope.activity_ordinal_max));
    WRITE(lxp_codec_write_bytes(&writer, grant->scope.asset_id, 32U, 32U));
    WRITE(write_amount(&writer, grant->scope.maximum_per_activity));
    WRITE(write_amount(&writer, grant->scope.maximum_total));
    WRITE(write_amount(&writer, grant->scope.spent_total));
    WRITE(lxp_codec_write_u64(&writer, grant->scope.period_length));
    WRITE(write_amount(&writer, grant->scope.maximum_per_period));
    WRITE(write_amount(&writer, grant->scope.spent_this_period));
    WRITE(lxp_codec_write_u64(&writer, grant->scope.period_start));
    WRITE(lxp_codec_write_bytes(&writer, grant->scope.purpose_hash, 32U, 32U));
    WRITE(lxp_codec_write_u64(&writer, grant->not_before));
    WRITE(lxp_codec_write_u64(&writer, grant->not_after));
    WRITE(lxp_codec_write_u64(&writer, grant->grantor_revocation_sequence));
    WRITE(lxp_codec_write_u8(&writer, grant->revoked ? 1U : 0U));
    WRITE(lxp_codec_write_u64(&writer, grant->revoked_at_sequence));
    WRITE(lxp_codec_write_bytes(&writer, grant->grantor_signature, 64U, 64U));
#undef WRITE
    encoded->bytes = writer.bytes;
    encoded->length = writer.length;
    return LXP_OK;
}

lxp_result lxp_grant_id_compute(const lxp_authority_grant *grant,
                                uint8_t grant_id[32])
{
    uint8_t *storage;
    lxp_arena arena;
    lxp_byte_span encoded;
    lxp_result status;
    if (grant_id == NULL) return LXP_ERR_MALFORMED_GRANT;
    storage = malloc(1024U);
    if (storage == NULL) return LXP_ERR_IO;
    status = lxp_arena_init(&arena, storage, 1024U);
    if (status == LXP_OK) status = lxp_grant_encode(grant, &arena, &encoded);
    if (status == LXP_OK)
        status = lxp_hash_authority(encoded.bytes, encoded.length, grant_id);
    lxp_secure_zero(storage, 1024U);
    free(storage);
    return status;
}

lxp_result lxp_session_key_bind(lxp_authority_grant *grant,
                                const uint8_t grantor[32],
                                const uint8_t session_key[32],
                                uint64_t module_mask,
                                uint16_t ordinal_min,
                                uint16_t ordinal_max,
                                uint64_t not_before, uint64_t not_after,
                                uint64_t revocation_sequence)
{
    if (grant == NULL || grantor == NULL || session_key == NULL ||
        module_mask == 0U || ordinal_min > ordinal_max || not_after == 0U ||
        not_after <= not_before) return LXP_ERR_MALFORMED_GRANT;
    (void)memset(grant, 0, sizeof(*grant));
    (void)memcpy(grant->grantor, grantor, 32U);
    (void)memcpy(grant->grantee, grantor, 32U);
    (void)memcpy(grant->key, session_key, 32U);
    grant->kind = LXP_AUTHORITY_SESSION_KEY;
    grant->scope.module_mask = module_mask;
    grant->scope.activity_ordinal_min = ordinal_min;
    grant->scope.activity_ordinal_max = ordinal_max;
    grant->not_before = not_before;
    grant->not_after = not_after;
    grant->grantor_revocation_sequence = revocation_sequence;
    return lxp_grant_id_compute(grant, grant->grant_id);
}

lxp_result lxp_authority_hash(lxp_authority_kind kind,
                              const uint8_t grant_id[32],
                              const uint8_t verified_key[32],
                              uint8_t authority_hash[32])
{
    uint8_t preimage[65];
    if (kind < LXP_AUTHORITY_OWNER || kind > LXP_AUTHORITY_PROTOCOL_MODULE ||
        grant_id == NULL || verified_key == NULL || authority_hash == NULL)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    preimage[0] = (uint8_t)kind;
    (void)memcpy(preimage + 1U, grant_id, 32U);
    (void)memcpy(preimage + 33U, verified_key, 32U);
    return lxp_hash_authority(preimage, sizeof(preimage), authority_hash);
}

lxp_result lxp_authority_check_scope(const lxp_authority_scope *scope,
                                     uint32_t activity_type,
                                     uint64_t declared_module_mask,
                                     uint16_t declared_ordinal_min,
                                     uint16_t declared_ordinal_max)
{
    uint16_t module = (uint16_t)(activity_type >> 16U);
    uint16_t ordinal = (uint16_t)activity_type;
    uint64_t module_bit;
    if (scope == NULL || module >= 64U) return LXP_ERR_AUTH_SCOPE;
    module_bit = UINT64_C(1) << module;
    if ((scope->module_mask & module_bit) == 0U ||
        ordinal < scope->activity_ordinal_min ||
        ordinal > scope->activity_ordinal_max ||
        (scope->module_mask & ~declared_module_mask) != 0U ||
        scope->activity_ordinal_min < declared_ordinal_min ||
        scope->activity_ordinal_max > declared_ordinal_max)
        return LXP_ERR_AUTH_SCOPE;
    return LXP_OK;
}

lxp_result lxp_authority_resolve(const lxp_authority_grant *grant,
                                 const uint8_t actor[32],
                                 uint32_t activity_type,
                                 uint64_t declared_module_mask,
                                 uint16_t declared_ordinal_min,
                                 uint16_t declared_ordinal_max,
                                 bool signature_valid,
                                 lxp_authority_resolved *resolved)
{
    lxp_result status;
    if (grant == NULL || actor == NULL || resolved == NULL)
        return LXP_ERR_MALFORMED_GRANT;
    if (grant->kind < LXP_AUTHORITY_OWNER ||
        grant->kind > LXP_AUTHORITY_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    if (!signature_valid) return LXP_ERR_BAD_SIGNATURE;
    if (grant->revoked) return LXP_ERR_AUTH_REVOKED;
    if (lxp_ct_memcmp(actor, grant->grantee, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_authority_check_scope(&grant->scope, activity_type,
                                       declared_module_mask,
                                       declared_ordinal_min,
                                       declared_ordinal_max);
    if (status != LXP_OK) return status;
    (void)memcpy(resolved->actor, actor, 32U);
    (void)memcpy(resolved->principal, grant->grantor, 32U);
    (void)memcpy(resolved->verified_key, grant->key, 32U);
    resolved->kind = grant->kind;
    resolved->scope = &grant->scope;
    return lxp_authority_hash(grant->kind, grant->grant_id, grant->key,
                              resolved->authority_hash);
}

enum {
    AUTHORITY_GRANT_RECORD_TAG = 5,
    AUTHORITY_REVOCATION_RECORD_TAG = 6,
    AUTHORITY_RECORD_KEY_BYTES = 33,
    AUTHORITY_REVOCATION_RECORD_BYTES = 41
};

lxp_result lxp_grant_decode(const uint8_t *bytes, size_t length,
                            lxp_authority_grant *grant)
{
    lxp_codec_reader reader;
    lxp_byte_span span;
    uint8_t version;
    uint8_t kind;
    uint8_t revoked;
    lxp_result status;
    if (bytes == NULL || grant == NULL) return LXP_ERR_MALFORMED_GRANT;
    (void)memset(grant, 0, sizeof(*grant));
#define READ(expression) do { status = (expression); if (status != LXP_OK) return status; } while (0)
#define FIXED(destination, count) do { \
        READ(lxp_codec_read_bytes(&reader, &span, (count))); \
        if (span.length != (count)) return LXP_ERR_MALFORMED_GRANT; \
        (void)memcpy((destination), span.bytes, (count)); } while (0)
    READ(lxp_codec_reader_init(&reader, bytes, length));
    READ(lxp_codec_read_struct_header(&reader, 0x2001U));
    READ(lxp_codec_read_u8(&reader, &version));
    if (version != 1U) return LXP_ERR_VERSION_UNSUPPORTED;
    FIXED(grant->grantor, 32U);
    FIXED(grant->grantee, 32U);
    READ(lxp_codec_read_u8(&reader, &kind));
    if (kind < (uint8_t)LXP_AUTHORITY_OWNER ||
        kind > (uint8_t)LXP_AUTHORITY_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    grant->kind = (lxp_authority_kind)kind;
    FIXED(grant->key, 32U);
    READ(lxp_codec_read_u64(&reader, &grant->scope.module_mask));
    READ(lxp_codec_read_u16(&reader, &grant->scope.activity_ordinal_min));
    READ(lxp_codec_read_u16(&reader, &grant->scope.activity_ordinal_max));
    FIXED(grant->scope.asset_id, 32U);
    READ(lxp_codec_read_u128(&reader, &grant->scope.maximum_per_activity));
    READ(lxp_codec_read_u128(&reader, &grant->scope.maximum_total));
    READ(lxp_codec_read_u128(&reader, &grant->scope.spent_total));
    READ(lxp_codec_read_u64(&reader, &grant->scope.period_length));
    READ(lxp_codec_read_u128(&reader, &grant->scope.maximum_per_period));
    READ(lxp_codec_read_u128(&reader, &grant->scope.spent_this_period));
    READ(lxp_codec_read_u64(&reader, &grant->scope.period_start));
    FIXED(grant->scope.purpose_hash, 32U);
    READ(lxp_codec_read_u64(&reader, &grant->not_before));
    READ(lxp_codec_read_u64(&reader, &grant->not_after));
    READ(lxp_codec_read_u64(&reader, &grant->grantor_revocation_sequence));
    READ(lxp_codec_read_u8(&reader, &revoked));
    if (revoked > 1U) return LXP_ERR_MALFORMED_GRANT;
    grant->revoked = revoked != 0U;
    READ(lxp_codec_read_u64(&reader, &grant->revoked_at_sequence));
    FIXED(grant->grantor_signature, 64U);
    READ(lxp_codec_finish(&reader));
#undef FIXED
#undef READ
    return validate_grant(grant);
}

lxp_result lxp_authority_envelope_declare(const lxp_kernel *kernel,
                                          uint64_t epoch,
                                          lxp_authority_envelope *envelope)
{
    size_t registration_index;
    size_t type_index;
    bool declared = false;
    if (kernel == NULL || envelope == NULL) return LXP_ERR_NON_CANONICAL;
    envelope->module_mask = 0U;
    envelope->activity_ordinal_min = UINT16_MAX;
    envelope->activity_ordinal_max = 0U;
    for (registration_index = 0U; registration_index < kernel->module_count;
         ++registration_index) {
        const lxp_module_registration *registration =
            &kernel->modules[registration_index];
        if (!registration->enabled || epoch < registration->enabled_epoch ||
            epoch >= registration->disabled_epoch) continue;
        for (type_index = 0U; type_index < registration->activity_type_count;
             ++type_index) {
            uint32_t activity_type = registration->activity_types[type_index];
            uint16_t module = (uint16_t)(activity_type >> 16U);
            uint16_t ordinal = (uint16_t)activity_type;
            if (module >= 64U) return LXP_ERR_UNKNOWN_MODULE;
            envelope->module_mask |= UINT64_C(1) << module;
            if (ordinal < envelope->activity_ordinal_min)
                envelope->activity_ordinal_min = ordinal;
            if (ordinal > envelope->activity_ordinal_max)
                envelope->activity_ordinal_max = ordinal;
            declared = true;
        }
    }
    if (!declared) return LXP_ERR_MODULE_DISABLED;
    return LXP_OK;
}

lxp_result lxp_authority_owner_grant(const lxp_identity *identity,
                                     const uint8_t verified_key[32],
                                     const lxp_authority_envelope *envelope,
                                     uint64_t not_before, uint64_t not_after,
                                     lxp_authority_grant *grant)
{
    if (identity == NULL || verified_key == NULL || envelope == NULL ||
        grant == NULL) return LXP_ERR_MALFORMED_GRANT;
    if (not_after == UINT64_MAX || not_after < not_before)
        return LXP_ERR_MALFORMED_ENVELOPE;
    (void)memset(grant, 0, sizeof(*grant));
    (void)memcpy(grant->grantor, identity->did_id, 32U);
    (void)memcpy(grant->grantee, identity->did_id, 32U);
    grant->kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(grant->key, verified_key, 32U);
    grant->scope.module_mask = envelope->module_mask;
    grant->scope.activity_ordinal_min = envelope->activity_ordinal_min;
    grant->scope.activity_ordinal_max = envelope->activity_ordinal_max;
    grant->not_before = not_before;
    grant->not_after = not_after + 1U;
    grant->grantor_revocation_sequence = identity->revocation_sequence;
    return lxp_grant_id_compute(grant, grant->grant_id);
}

static bool authority_record(const lxp_module_kv_entry *entry, uint8_t tag)
{
    return entry->module_id == LXP_MODULE_GOVERNANCE &&
           entry->key_length == AUTHORITY_RECORD_KEY_BYTES &&
           entry->key[0] == tag;
}

static uint64_t authority_read_u64(const uint8_t *bytes)
{
    uint64_t value = 0U;
    size_t index;
    for (index = 0U; index < 8U; ++index)
        value = (value << 8U) | bytes[index];
    return value;
}

lxp_result lxp_authority_grant_lookup(const lxp_kernel *kernel,
                                      const uint8_t grantor[32],
                                      const uint8_t verified_key[32],
                                      lxp_authority_grant *grant)
{
    size_t index;
    bool found = false;
    lxp_result status;
    if (kernel == NULL || grantor == NULL || verified_key == NULL ||
        grant == NULL) return LXP_ERR_MALFORMED_GRANT;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        lxp_authority_grant candidate;
        uint8_t grant_id[32];
        if (!authority_record(entry, (uint8_t)AUTHORITY_GRANT_RECORD_TAG))
            continue;
        status = lxp_grant_decode(entry->value, entry->value_length, &candidate);
        if (status != LXP_OK) return status;
        if (lxp_ct_memcmp(candidate.key, verified_key, 32U) != 0 ||
            lxp_ct_memcmp(candidate.grantor, grantor, 32U) != 0) continue;
        status = lxp_grant_id_compute(&candidate, grant_id);
        if (status != LXP_OK) return status;
        if (lxp_ct_memcmp(grant_id, entry->key + 1U, 32U) != 0)
            return LXP_FATAL_INVARIANT;
        if (found) return LXP_ERR_SEQUENCE_REUSED;
        (void)memcpy(candidate.grant_id, grant_id, 32U);
        *grant = candidate;
        found = true;
    }
    if (!found) return LXP_ERR_UNKNOWN_FIELD;
    for (index = 0U; index < kernel->module_kv_count; ++index) {
        const lxp_module_kv_entry *entry = &kernel->module_kv[index];
        if (!authority_record(entry, (uint8_t)AUTHORITY_REVOCATION_RECORD_TAG) ||
            lxp_ct_memcmp(entry->key + 1U, grant->grant_id, 32U) != 0) continue;
        if (entry->value_length != AUTHORITY_REVOCATION_RECORD_BYTES ||
            lxp_ct_memcmp(entry->value, grant->grant_id, 32U) != 0)
            return LXP_FATAL_INVARIANT;
        grant->revoked = true;
        grant->revoked_at_sequence = authority_read_u64(entry->value + 33U);
    }
    return LXP_OK;
}

lxp_result lxp_authority_resolve_activity(const lxp_kernel *kernel,
                                          const lxp_identity *identity,
                                          const lxp_activity *activity,
                                          bool owner_key_valid,
                                          bool signature_valid,
                                          uint64_t batch_timestamp,
                                          uint64_t maximum_timestamp_window,
                                          uint64_t global_sequence,
                                          lxp_authority_grant *grant,
                                          lxp_authority_resolved *resolved)
{
    lxp_authority_envelope envelope;
    lxp_result status;
    if (kernel == NULL || identity == NULL || activity == NULL ||
        grant == NULL || resolved == NULL) return LXP_ERR_NON_CANONICAL;
    if (activity->authority.bytes == NULL ||
        activity->authority.length != 32U || !signature_valid)
        return LXP_ERR_BAD_SIGNATURE;
    status = lxp_authority_envelope_declare(kernel, kernel->epoch, &envelope);
    if (status != LXP_OK) return status;
    status = lxp_activity_check_timestamp_bound(activity->timestamp_bound,
                                                batch_timestamp,
                                                maximum_timestamp_window);
    if (status != LXP_OK) return status;
    if (owner_key_valid)
        status = lxp_authority_owner_grant(identity, activity->authority.bytes,
                                           &envelope,
                                           activity->timestamp_bound.not_before,
                                           activity->timestamp_bound.not_after,
                                           grant);
    else {
        status = lxp_authority_grant_lookup(kernel, identity->did_id,
                                            activity->authority.bytes, grant);
        if (status == LXP_ERR_UNKNOWN_FIELD) return LXP_ERR_BAD_SIGNATURE;
    }
    if (status != LXP_OK) return status;
    if (lxp_ct_memcmp(grant->grantor, identity->did_id, 32U) != 0 ||
        lxp_ct_memcmp(grant->grantee, identity->did_id, 32U) != 0)
        return LXP_ERR_UNAUTHORIZED_DEBIT;
    status = lxp_authority_is_live(grant, identity->revocation_sequence,
                                   batch_timestamp, global_sequence);
    if (status != LXP_OK) return status;
    return lxp_authority_resolve(grant, identity->did_id,
                                 activity->activity_type, envelope.module_mask,
                                 envelope.activity_ordinal_min,
                                 envelope.activity_ordinal_max, signature_valid,
                                 resolved);
}

lxp_result lxp_authority_period_roll(lxp_authority_scope *scope,
                                     uint64_t batch_timestamp)
{
    uint64_t elapsed;
    uint64_t periods;
    if (scope == NULL) return LXP_ERR_NON_CANONICAL;
    if (scope->period_length == 0U) return LXP_OK;
    if (batch_timestamp < scope->period_start) return LXP_ERR_NOT_YET_VALID;
    elapsed = batch_timestamp - scope->period_start;
    periods = elapsed / scope->period_length;
    if (periods != 0U) {
        scope->period_start += periods * scope->period_length;
        scope->spent_this_period = (lxp_u128){ 0U, 0U };
    }
    return LXP_OK;
}

lxp_result lxp_authority_spend_check(const lxp_authority_scope *scope,
                                     lxp_u128 amount)
{
    lxp_u128 total;
    lxp_u128 period;
    lxp_result status;
    if (scope == NULL) return LXP_ERR_NON_CANONICAL;
    if (lxp_u128_cmp(amount, scope->maximum_per_activity) > 0)
        return LXP_ERR_GRANT_EXHAUSTED;
    status = lxp_u128_add(scope->spent_total, amount, &total);
    if (status != LXP_OK) return status;
    if (!lxp_u128_is_zero(scope->maximum_total) &&
        lxp_u128_cmp(total, scope->maximum_total) > 0)
        return LXP_ERR_GRANT_EXHAUSTED;
    status = lxp_u128_add(scope->spent_this_period, amount, &period);
    if (status != LXP_OK) return status;
    if (scope->period_length != 0U &&
        lxp_u128_cmp(period, scope->maximum_per_period) > 0)
        return LXP_ERR_GRANT_EXHAUSTED;
    return LXP_OK;
}

lxp_result lxp_authority_charge_allowance(lxp_authority_scope *scope,
                                          lxp_u128 amount,
                                          uint64_t batch_timestamp)
{
    lxp_authority_scope updated;
    lxp_result status;
    if (scope == NULL) return LXP_ERR_NON_CANONICAL;
    updated = *scope;
    status = lxp_authority_period_roll(&updated, batch_timestamp);
    if (status != LXP_OK) return status;
    status = lxp_authority_spend_check(&updated, amount);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(updated.spent_total, amount, &updated.spent_total);
    if (status != LXP_OK) return status;
    status = lxp_u128_add(updated.spent_this_period, amount,
                          &updated.spent_this_period);
    if (status != LXP_OK) return status;
    *scope = updated;
    return LXP_OK;
}

static bool authority_kind_metered(lxp_authority_kind kind)
{
    return kind == LXP_AUTHORITY_DELEGATED_CAPABILITY ||
           kind == LXP_AUTHORITY_BUDGET_ALLOWANCE;
}

static lxp_result debit_scope_binding(const lxp_authority_scope *scope,
                                      lxp_authority_kind kind,
                                      const uint8_t asset_id[32],
                                      uint16_t module_id)
{
    if (scope == NULL || asset_id == NULL) return LXP_ERR_NON_CANONICAL;
    if (kind < LXP_AUTHORITY_OWNER || kind > LXP_AUTHORITY_PROTOCOL_MODULE)
        return LXP_ERR_UNKNOWN_AUTHORITY_KIND;
    if (module_id >= 64U ||
        (scope->module_mask & (UINT64_C(1) << module_id)) == 0U)
        return LXP_ERR_AUTH_SCOPE;
    if (!authority_kind_metered(kind)) {
        if (!lxp_u128_is_zero(scope->maximum_per_activity) ||
            !lxp_u128_is_zero(scope->maximum_total) ||
            !lxp_u128_is_zero(scope->maximum_per_period) ||
            !lxp_u128_is_zero(scope->spent_total) ||
            !lxp_u128_is_zero(scope->spent_this_period) ||
            scope->period_length != 0U)
            return LXP_ERR_AUTH_SCOPE;
        return LXP_OK;
    }
    if (lxp_ct_memcmp(scope->asset_id, asset_id, 32U) != 0)
        return LXP_ERR_ASSET_MISMATCH;
    return LXP_OK;
}

lxp_result lxp_authority_check_debit(const lxp_authority_scope *scope,
                                     lxp_authority_kind kind,
                                     const uint8_t asset_id[32],
                                     uint16_t module_id, lxp_u128 amount,
                                     uint64_t batch_timestamp)
{
    lxp_authority_scope rolled;
    lxp_result status = debit_scope_binding(scope, kind, asset_id, module_id);
    if (status != LXP_OK || !authority_kind_metered(kind)) return status;
    rolled = *scope;
    status = lxp_authority_period_roll(&rolled, batch_timestamp);
    if (status != LXP_OK) return status;
    return lxp_authority_spend_check(&rolled, amount);
}

lxp_result lxp_authority_charge_debit(lxp_authority_scope *scope,
                                      lxp_authority_kind kind,
                                      const uint8_t asset_id[32],
                                      uint16_t module_id, lxp_u128 amount,
                                      uint64_t batch_timestamp)
{
    lxp_result status = debit_scope_binding(scope, kind, asset_id, module_id);
    if (status != LXP_OK || !authority_kind_metered(kind)) return status;
    return lxp_authority_charge_allowance(scope, amount, batch_timestamp);
}

lxp_result lxp_authority_revoke(lxp_authority_grant *grant,
                                uint64_t revocation_sequence,
                                uint64_t global_sequence)
{
    if (grant == NULL) return LXP_ERR_MALFORMED_GRANT;
    if (revocation_sequence <= grant->grantor_revocation_sequence)
        return LXP_ERR_STALE_REVOCATION;
    grant->grantor_revocation_sequence = revocation_sequence;
    grant->revoked = true;
    grant->revoked_at_sequence = global_sequence;
    return LXP_OK;
}

static int cap_narrows(lxp_u128 old_cap, lxp_u128 new_cap)
{
    if (lxp_u128_is_zero(old_cap)) return 1;
    return !lxp_u128_is_zero(new_cap) && lxp_u128_cmp(new_cap, old_cap) <= 0;
}

lxp_result lxp_authority_amend(lxp_authority_grant *grant,
                               const lxp_authority_grant *narrower)
{
    if (grant == NULL || narrower == NULL) return LXP_ERR_MALFORMED_GRANT;
    if (narrower->grantor_revocation_sequence <=
        grant->grantor_revocation_sequence) return LXP_ERR_STALE_REVOCATION;
    if (narrower->kind != grant->kind ||
        lxp_ct_memcmp(narrower->grantor, grant->grantor, 32U) != 0 ||
        lxp_ct_memcmp(narrower->grantee, grant->grantee, 32U) != 0 ||
        lxp_ct_memcmp(narrower->key, grant->key, 32U) != 0 ||
        lxp_ct_memcmp(narrower->scope.asset_id, grant->scope.asset_id, 32U) != 0 ||
        lxp_ct_memcmp(narrower->scope.purpose_hash,
                      grant->scope.purpose_hash, 32U) != 0 ||
        (narrower->scope.module_mask & ~grant->scope.module_mask) != 0U ||
        narrower->scope.activity_ordinal_min <
            grant->scope.activity_ordinal_min ||
        narrower->scope.activity_ordinal_max >
            grant->scope.activity_ordinal_max ||
        narrower->scope.activity_ordinal_min >
            narrower->scope.activity_ordinal_max ||
        narrower->not_before < grant->not_before ||
        narrower->not_after > grant->not_after ||
        narrower->not_after <= narrower->not_before ||
        !cap_narrows(grant->scope.maximum_per_activity,
                     narrower->scope.maximum_per_activity) ||
        !cap_narrows(grant->scope.maximum_total,
                     narrower->scope.maximum_total) ||
        !cap_narrows(grant->scope.maximum_per_period,
                     narrower->scope.maximum_per_period) ||
        lxp_u128_cmp(narrower->scope.spent_total,
                     grant->scope.spent_total) != 0 ||
        lxp_u128_cmp(narrower->scope.spent_this_period,
                     grant->scope.spent_this_period) != 0 ||
        narrower->scope.period_length != grant->scope.period_length ||
        narrower->scope.period_start != grant->scope.period_start)
        return LXP_ERR_AUTH_SCOPE;
    *grant = *narrower;
    return lxp_grant_id_compute(grant, grant->grant_id);
}

lxp_result lxp_authority_is_live(const lxp_authority_grant *grant,
                                 uint64_t identity_revocation_sequence,
                                 uint64_t batch_timestamp,
                                 uint64_t global_sequence)
{
    if (grant == NULL) return LXP_ERR_MALFORMED_GRANT;
    if (batch_timestamp < grant->not_before) return LXP_ERR_NOT_YET_VALID;
    if (batch_timestamp >= grant->not_after) return LXP_ERR_AUTH_EXPIRED;
    if (grant->grantor_revocation_sequence != identity_revocation_sequence)
        return LXP_ERR_AUTH_REVOKED;
    if (grant->revoked && global_sequence >= grant->revoked_at_sequence)
        return LXP_ERR_AUTH_REVOKED;
    return LXP_OK;
}

lxp_result lxp_identity_bump_revocation_sequence(lxp_identity *identity,
                                                 uint64_t new_sequence)
{
    if (identity == NULL) return LXP_ERR_UNKNOWN_DID;
    if (new_sequence <= identity->revocation_sequence)
        return LXP_ERR_STALE_REVOCATION;
    identity->revocation_sequence = new_sequence;
    return LXP_OK;
}
