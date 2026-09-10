#include "layerx/lxp_authority.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_crypto.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define TEST_ACTIVITY UINT32_C(0x00070005)
#define TEST_ASSET_ACTIVITY UINT32_C(0x00010002)
#define TEST_UNDECLARED_ACTIVITY UINT32_C(0x00030001)

enum {
    TEST_BATCH_TIMESTAMP = 1000,
    TEST_NOT_BEFORE = 900,
    TEST_NOT_AFTER = 1100,
    TEST_WINDOW = 300000,
    TEST_SEQUENCE = 12,
    TEST_REVOCATION_SEQUENCE = 7,
    TEST_REVOKED_AT = 20
};

static const uint64_t declared_mask = (UINT64_C(1) << LXP_MODULE_ASSET) |
                                      (UINT64_C(1) << LXP_MODULE_GOVERNANCE);

static void initialize(lxp_authority_grant *grant)
{
    (void)memset(grant, 0, sizeof(*grant));
    grant->kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    grant->grantor[0] = 1U;
    grant->grantee[0] = 2U;
    grant->key[0] = 3U;
    grant->grant_id[0] = 4U;
    grant->scope.module_mask = UINT64_C(1) << 5U;
    grant->scope.activity_ordinal_min = 2U;
    grant->scope.activity_ordinal_max = 4U;
}

static void declare_module(lxp_kernel *kernel, uint16_t module_id,
                           const uint32_t *activity_types, size_t count)
{
    lxp_module_registration *registration = &kernel->modules[kernel->module_count];
    (void)memset(registration, 0, sizeof(*registration));
    registration->module_id = module_id;
    registration->abi_version = 1U;
    registration->activity_type_count = count;
    (void)memcpy(registration->activity_types, activity_types,
                 count * sizeof(activity_types[0]));
    registration->enabled_epoch = kernel->epoch;
    registration->disabled_epoch = UINT64_MAX;
    registration->enabled = true;
    ++kernel->module_count;
}

static void put_record(lxp_kernel *kernel, uint8_t tag, const uint8_t grant_id[32],
                       const uint8_t *value, size_t value_length)
{
    lxp_module_kv_entry *entry = &kernel->module_kv[kernel->module_kv_count];
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_GOVERNANCE;
    entry->key_length = 33U;
    entry->key[0] = tag;
    (void)memcpy(entry->key + 1U, grant_id, 32U);
    entry->value_length = (uint32_t)value_length;
    (void)memcpy(entry->value, value, value_length);
    ++kernel->module_kv_count;
}

static void build_identity(lxp_identity *identity)
{
    size_t index;
    (void)memset(identity, 0, sizeof(*identity));
    for (index = 0U; index < 32U; ++index) {
        identity->did_id[index] = (uint8_t)(0x40U + index);
        identity->primary_key[index] = (uint8_t)(0x80U + index);
    }
    identity->revocation_sequence = TEST_REVOCATION_SEQUENCE;
}

static void build_activity(lxp_activity *activity, const lxp_identity *identity,
                           const uint8_t key[32], uint32_t activity_type)
{
    (void)memset(activity, 0, sizeof(*activity));
    activity->protocol_version = 1U;
    activity->activity_type = activity_type;
    activity->actor_did.bytes = identity->did_id;
    activity->actor_did.length = 32U;
    activity->authority.bytes = key;
    activity->authority.length = 32U;
    activity->timestamp_bound.not_before = TEST_NOT_BEFORE;
    activity->timestamp_bound.not_after = TEST_NOT_AFTER;
}

static lxp_result resolve(const lxp_kernel *kernel, const lxp_identity *identity,
                          const lxp_activity *activity, bool owner_key_valid,
                          uint64_t sequence, lxp_authority_grant *grant,
                          lxp_authority_resolved *resolved)
{
    return lxp_authority_resolve_activity(kernel, identity, activity,
                                          owner_key_valid, true,
                                          TEST_BATCH_TIMESTAMP, TEST_WINDOW,
                                          sequence, grant, resolved);
}

static int scope_checks(void)
{
    lxp_authority_grant grant;
    lxp_authority_resolved first;
    lxp_authority_resolved second;
    uint8_t actor[32] = { 2U };
    initialize(&grant);
    if (lxp_authority_resolve(&grant, actor, UINT32_C(0x00050003),
                              UINT64_C(1) << 5U, 1U, 5U, true, &first) !=
            LXP_OK ||
        lxp_authority_resolve(&grant, actor, UINT32_C(0x00050003),
                              UINT64_C(1) << 5U, 1U, 5U, true, &second) !=
            LXP_OK || memcmp(first.authority_hash, second.authority_hash, 32U) != 0 ||
        memcmp(first.principal, grant.grantor, 32U) != 0) return 1;
    if (lxp_authority_resolve(&grant, actor, UINT32_C(0x00050006),
                              UINT64_C(1) << 5U, 1U, 6U, true, &first) !=
        LXP_ERR_AUTH_SCOPE) return 1;
    grant.scope.module_mask |= UINT64_C(1) << 6U;
    if (lxp_authority_resolve(&grant, actor, UINT32_C(0x00050003),
                              UINT64_C(1) << 5U, 1U, 5U, true, &first) !=
        LXP_ERR_AUTH_SCOPE) return 1;
    grant.kind = (lxp_authority_kind)7;
    if (lxp_authority_resolve(&grant, actor, UINT32_C(0x00050003),
                              UINT64_MAX, 0U, UINT16_MAX, true, &first) !=
        LXP_ERR_UNKNOWN_AUTHORITY_KIND) return 1;
    return 0;
}

static int envelope_checks(const lxp_kernel *kernel)
{
    lxp_authority_envelope envelope;
    lxp_kernel *empty;
    lxp_result status;
    if (lxp_authority_envelope_declare(kernel, kernel->epoch, &envelope) != LXP_OK ||
        envelope.module_mask != declared_mask ||
        envelope.activity_ordinal_min != 1U || envelope.activity_ordinal_max != 6U)
        return 1;
    if (lxp_authority_envelope_declare(kernel, kernel->epoch + 1U, &envelope) != LXP_OK ||
        envelope.module_mask != declared_mask) return 1;
    empty = calloc(1U, sizeof(*empty));
    if (empty == NULL) return 1;
    status = lxp_authority_envelope_declare(empty, 0U, &envelope);
    free(empty);
    return status == LXP_ERR_MODULE_DISABLED ? 0 : 1;
}

static int owner_checks(const lxp_kernel *kernel, const lxp_identity *identity)
{
    lxp_authority_grant grant;
    lxp_authority_grant repeated;
    lxp_authority_resolved resolved;
    lxp_authority_resolved again;
    lxp_activity activity;
    uint8_t expected[32];
    uint8_t synthesized[32];
    uint8_t zero_grant_id[32] = {0};
    build_activity(&activity, identity, identity->primary_key, TEST_ACTIVITY);
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_OK) return 1;
    /* The grant the receipt hash commits to is a real, derived grant identifier,
     * never the zero identifier the daemon used to synthesize. */
    if (lxp_ct_is_zero(grant.grant_id, 32U)) return 1;
    if (lxp_authority_hash(LXP_AUTHORITY_OWNER, grant.grant_id, identity->primary_key,
                           expected) != LXP_OK ||
        memcmp(resolved.authority_hash, expected, 32U) != 0) return 1;
    if (lxp_authority_hash(LXP_AUTHORITY_OWNER, zero_grant_id, identity->primary_key,
                           synthesized) != LXP_OK ||
        memcmp(resolved.authority_hash, synthesized, 32U) == 0) return 1;
    if (resolved.kind != LXP_AUTHORITY_OWNER ||
        memcmp(resolved.actor, identity->did_id, 32U) != 0 ||
        memcmp(resolved.principal, identity->did_id, 32U) != 0 ||
        memcmp(resolved.verified_key, identity->primary_key, 32U) != 0) return 1;
    /* The owner scope is the node's declared envelope, never an unlimited one,
     * and carries no spend allowance. */
    if (grant.scope.module_mask != declared_mask ||
        grant.scope.activity_ordinal_min != 1U ||
        grant.scope.activity_ordinal_max != 6U ||
        !lxp_u128_is_zero(grant.scope.maximum_per_activity) ||
        !lxp_u128_is_zero(grant.scope.maximum_total) ||
        !lxp_u128_is_zero(grant.scope.maximum_per_period) ||
        grant.not_before != TEST_NOT_BEFORE || grant.not_after != TEST_NOT_AFTER + 1U ||
        grant.grantor_revocation_sequence != identity->revocation_sequence) return 1;
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &repeated,
                &again) != LXP_OK ||
        memcmp(repeated.grant_id, grant.grant_id, 32U) != 0 ||
        memcmp(again.authority_hash, resolved.authority_hash, 32U) != 0) return 1;
    build_activity(&activity, identity, identity->primary_key,
                   TEST_UNDECLARED_ACTIVITY);
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_AUTH_SCOPE) return 1;
    build_activity(&activity, identity, identity->primary_key, TEST_ACTIVITY);
    activity.timestamp_bound.not_after = UINT64_MAX;
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_MALFORMED_ENVELOPE) return 1;
    build_activity(&activity, identity, identity->primary_key, TEST_ACTIVITY);
    activity.timestamp_bound.not_before = TEST_BATCH_TIMESTAMP + 1U;
    activity.timestamp_bound.not_after = TEST_BATCH_TIMESTAMP + 2U;
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_NOT_YET_VALID) return 1;
    activity.timestamp_bound.not_before = TEST_BATCH_TIMESTAMP - 2U;
    activity.timestamp_bound.not_after = TEST_BATCH_TIMESTAMP - 1U;
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_EXPIRED) return 1;
    build_activity(&activity, identity, identity->primary_key, TEST_ACTIVITY);
    if (lxp_authority_resolve_activity(kernel, identity, &activity, true, false,
                                       TEST_BATCH_TIMESTAMP, TEST_WINDOW,
                                       TEST_SEQUENCE, &grant, &resolved) !=
        LXP_ERR_BAD_SIGNATURE) return 1;
    activity.authority.length = 31U;
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_BAD_SIGNATURE) return 1;
    return 0;
}

static int delegated_checks(lxp_kernel *kernel, const lxp_identity *identity,
                            const uint8_t session_key[32])
{
    lxp_authority_grant persisted;
    lxp_authority_grant grant;
    lxp_authority_resolved resolved;
    lxp_authority_resolved unused;
    lxp_activity activity;
    lxp_identity rotated;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[1024];
    uint8_t expected[32];
    uint8_t revocation[41];
    uint8_t stranger[32];
    size_t index;
    if (lxp_session_key_bind(&persisted, identity->did_id, session_key,
                             declared_mask, 1U, 6U, TEST_NOT_BEFORE - 400U,
                             TEST_NOT_AFTER + 900U,
                             identity->revocation_sequence) != LXP_OK) return 1;
    if (lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_grant_encode(&persisted, &arena, &encoded) != LXP_OK) return 1;
    if (lxp_grant_decode(encoded.bytes, encoded.length, &grant) != LXP_OK ||
        grant.kind != LXP_AUTHORITY_SESSION_KEY ||
        memcmp(grant.key, session_key, 32U) != 0 ||
        grant.scope.module_mask != persisted.scope.module_mask ||
        grant.not_after != persisted.not_after ||
        grant.grantor_revocation_sequence != persisted.grantor_revocation_sequence)
        return 1;
    put_record(kernel, 5U, persisted.grant_id, encoded.bytes, encoded.length);
    build_activity(&activity, identity, session_key, TEST_ACTIVITY);
    if (resolve(kernel, identity, &activity, false, TEST_SEQUENCE, &grant,
                &resolved) != LXP_OK) return 1;
    if (memcmp(grant.grant_id, persisted.grant_id, 32U) != 0 ||
        resolved.kind != LXP_AUTHORITY_SESSION_KEY ||
        memcmp(resolved.verified_key, session_key, 32U) != 0 ||
        memcmp(resolved.principal, identity->did_id, 32U) != 0) return 1;
    if (lxp_authority_hash(LXP_AUTHORITY_SESSION_KEY, persisted.grant_id,
                           session_key, expected) != LXP_OK ||
        memcmp(resolved.authority_hash, expected, 32U) != 0) return 1;
    if (lxp_authority_hash(LXP_AUTHORITY_OWNER, persisted.grant_id, session_key,
                           expected) != LXP_OK ||
        memcmp(resolved.authority_hash, expected, 32U) == 0) return 1;
    /* A key with no persisted grant is refused; it is never promoted to owner. */
    for (index = 0U; index < 32U; ++index) stranger[index] = (uint8_t)(0xC0U + index);
    build_activity(&activity, identity, stranger, TEST_ACTIVITY);
    if (resolve(kernel, identity, &activity, false, TEST_SEQUENCE, &grant,
                &unused) != LXP_ERR_BAD_SIGNATURE) return 1;
    /* An identity revocation that advances past the grant retires it. */
    rotated = *identity;
    rotated.revocation_sequence = identity->revocation_sequence + 1U;
    build_activity(&activity, identity, session_key, TEST_ACTIVITY);
    if (resolve(kernel, &rotated, &activity, false, TEST_SEQUENCE, &grant,
                &unused) != LXP_ERR_AUTH_REVOKED) return 1;
    /* The persisted revocation record retires the grant on its own. */
    (void)memset(revocation, 0, sizeof(revocation));
    (void)memcpy(revocation, persisted.grant_id, 32U);
    revocation[32] = 1U;
    for (index = 0U; index < 8U; ++index)
        revocation[33U + index] =
            (uint8_t)((uint64_t)TEST_REVOKED_AT >> (56U - 8U * index));
    put_record(kernel, 6U, persisted.grant_id, revocation, sizeof(revocation));
    if (resolve(kernel, identity, &activity, false, TEST_SEQUENCE, &grant,
                &unused) != LXP_ERR_AUTH_REVOKED) return 1;
    if (!grant.revoked || grant.revoked_at_sequence != TEST_REVOKED_AT) return 1;
    if (resolve(kernel, identity, &activity, false, TEST_REVOKED_AT + 5U, &grant,
                &unused) != LXP_ERR_AUTH_REVOKED) return 1;
    /* The owner key of a revoked-session identity still resolves on its own
     * owner grant. */
    build_activity(&activity, identity, identity->primary_key, TEST_ACTIVITY);
    if (resolve(kernel, identity, &activity, true, TEST_SEQUENCE, &grant,
                &resolved) != LXP_OK ||
        resolved.kind != LXP_AUTHORITY_OWNER) return 1;
    kernel->module_kv_count -= 2U;
    return 0;
}

static int narrow_grant_checks(lxp_kernel *kernel, const lxp_identity *identity,
                               const uint8_t session_key[32])
{
    lxp_authority_grant persisted;
    lxp_authority_grant grant;
    lxp_authority_resolved resolved;
    lxp_activity activity;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[1024];
    /* A session grant confined to the asset module cannot authorize a
     * governance activity even though both are inside the node envelope. */
    if (lxp_session_key_bind(&persisted, identity->did_id, session_key,
                             UINT64_C(1) << LXP_MODULE_ASSET, 1U, 2U,
                             TEST_NOT_BEFORE - 400U, TEST_NOT_AFTER + 900U,
                             identity->revocation_sequence) != LXP_OK) return 1;
    if (lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_grant_encode(&persisted, &arena, &encoded) != LXP_OK) return 1;
    put_record(kernel, 5U, persisted.grant_id, encoded.bytes, encoded.length);
    build_activity(&activity, identity, session_key, TEST_ACTIVITY);
    if (resolve(kernel, identity, &activity, false, TEST_SEQUENCE, &grant,
                &resolved) != LXP_ERR_AUTH_SCOPE) return 1;
    build_activity(&activity, identity, session_key, TEST_ASSET_ACTIVITY);
    if (resolve(kernel, identity, &activity, false, TEST_SEQUENCE, &grant,
                &resolved) != LXP_OK ||
        resolved.kind != LXP_AUTHORITY_SESSION_KEY ||
        grant.scope.module_mask != (UINT64_C(1) << LXP_MODULE_ASSET)) return 1;
    kernel->module_kv_count -= 1U;
    return 0;
}

int main(void)
{
    static const uint32_t asset_types[] = { UINT32_C(0x00010001), UINT32_C(0x00010002) };
    static const uint32_t governance_types[] = {
        UINT32_C(0x00070001), UINT32_C(0x00070005), UINT32_C(0x00070006) };
    lxp_kernel *kernel;
    lxp_identity identity;
    uint8_t session_key[32];
    size_t index;
    int failed;
    if (scope_checks() != 0) return 1;
    kernel = calloc(1U, sizeof(*kernel));
    if (kernel == NULL) return 1;
    kernel->epoch = 4U;
    declare_module(kernel, LXP_MODULE_ASSET, asset_types,
                   sizeof(asset_types) / sizeof(asset_types[0]));
    declare_module(kernel, LXP_MODULE_GOVERNANCE, governance_types,
                   sizeof(governance_types) / sizeof(governance_types[0]));
    build_identity(&identity);
    for (index = 0U; index < 32U; ++index)
        session_key[index] = (uint8_t)(0x11U + index);
    failed = envelope_checks(kernel);
    if (failed == 0) failed = owner_checks(kernel, &identity);
    if (failed == 0) failed = delegated_checks(kernel, &identity, session_key);
    if (failed == 0) failed = narrow_grant_checks(kernel, &identity, session_key);
    free(kernel);
    return failed;
}
