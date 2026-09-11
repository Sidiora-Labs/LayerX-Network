#include "layerx/lxp_authority.h"

#include "layerx/lxp_activity.h"
#include "layerx/lxp_arena.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_module.h"

#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#define ASSET_ACTIVITY UINT32_C(0x00010002)

enum {
    GRANT_NOT_BEFORE = 900,
    GRANT_NOT_AFTER = 1100,
    BATCH_TIMESTAMP = 1000,
    TIMESTAMP_WINDOW = 300000,
    GLOBAL_SEQUENCE = 12,
    REVOCATION_SEQUENCE = 5
};

static void metered_scope(lxp_authority_scope *scope)
{
    (void)memset(scope, 0, sizeof(*scope));
    scope->module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    scope->activity_ordinal_min = 1U;
    scope->activity_ordinal_max = 2U;
    scope->asset_id[0] = 0x11U;
    scope->maximum_per_activity = (lxp_u128){ 0U, 40U };
    scope->maximum_total = (lxp_u128){ 0U, 100U };
    scope->period_length = 10U;
    scope->maximum_per_period = (lxp_u128){ 0U, 60U };
    scope->period_start = 100U;
    scope->purpose_hash[0] = 0x77U;
}

static int charge_checks(void)
{
    lxp_authority_scope scope;
    lxp_authority_scope unchanged;
    (void)memset(&scope, 0, sizeof(scope));
    scope.maximum_per_activity = (lxp_u128){ 0U, 40U };
    scope.maximum_total = (lxp_u128){ 0U, 100U };
    scope.period_length = 10U;
    scope.maximum_per_period = (lxp_u128){ 0U, 60U };
    scope.period_start = 100U;
    if (lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 40U }, 100U) !=
            LXP_OK ||
        lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 20U }, 109U) !=
            LXP_OK || scope.spent_total.lo != 60U ||
        scope.spent_this_period.lo != 60U) return 1;
    unchanged = scope;
    if (lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 1U }, 109U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 40U }, 130U) !=
            LXP_OK || scope.period_start != 130U ||
        scope.spent_this_period.lo != 40U || scope.spent_total.lo != 100U)
        return 1;
    unchanged = scope;
    if (lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 1U }, 130U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    scope.spent_total = (lxp_u128){ UINT64_MAX, UINT64_MAX };
    scope.maximum_total = (lxp_u128){ 0U, 0U };
    if (lxp_authority_charge_allowance(&scope, (lxp_u128){ 0U, 1U }, 130U) !=
        LXP_ERR_OVERFLOW) return 1;
    return 0;
}

/* The debit entries bind one leg to the grant it draws against: the grant
 * module, the grant asset and the grant caps. A refused debit never moves the
 * scope, and only the charging entry moves it at all. */
static int binding_checks(void)
{
    lxp_authority_scope scope;
    lxp_authority_scope unchanged;
    uint8_t asset[32] = { 0x11U };
    uint8_t other[32] = { 0x22U };
    metered_scope(&scope);
    unchanged = scope;
    if (lxp_authority_check_debit(NULL, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  asset, LXP_MODULE_ASSET,
                                  (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_authority_check_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  NULL, LXP_MODULE_ASSET,
                                  (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_NON_CANONICAL ||
        lxp_authority_charge_debit(NULL, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_NON_CANONICAL) return 1;
    if (lxp_authority_check_debit(&scope, (lxp_authority_kind)0, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_UNKNOWN_AUTHORITY_KIND ||
        lxp_authority_charge_debit(&scope, (lxp_authority_kind)7, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                   100U) != LXP_ERR_UNKNOWN_AUTHORITY_KIND)
        return 1;
    /* A module the grant scope does not cover, and a module index no mask can
     * hold, are both refused before any cap is consulted. */
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  asset, LXP_MODULE_PERPS,
                                  (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_AUTH_SCOPE ||
        lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, 64U, (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_AUTH_SCOPE ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  other, LXP_MODULE_ASSET,
                                  (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_ASSET_MISMATCH ||
        lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   other, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 1U }, 100U) !=
            LXP_ERR_ASSET_MISMATCH ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    /* Above the per-activity cap both entries refuse with the same typed
     * exhaustion and leave the scope byte for byte where it was. */
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  asset, LXP_MODULE_ASSET,
                                  (lxp_u128){ 0U, 41U }, 100U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 41U }, 100U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    /* A permitted debit is recorded only once it is charged. */
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                  asset, LXP_MODULE_ASSET,
                                  (lxp_u128){ 0U, 40U }, 100U) != LXP_OK ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 40U }, 100U) != LXP_OK ||
        scope.spent_total.lo != 40U || scope.spent_this_period.lo != 40U)
        return 1;
    /* The period cap binds inside the window and releases when the window
     * rolls; checking the roll never performs it. */
    unchanged = scope;
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_BUDGET_ALLOWANCE, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 40U },
                                  105U) != LXP_ERR_GRANT_EXHAUSTED ||
        lxp_authority_check_debit(&scope, LXP_AUTHORITY_BUDGET_ALLOWANCE, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 40U },
                                  120U) != LXP_OK ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_charge_debit(&scope, LXP_AUTHORITY_BUDGET_ALLOWANCE,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 40U }, 120U) != LXP_OK ||
        scope.period_start != 120U || scope.spent_this_period.lo != 40U ||
        scope.spent_total.lo != 80U) return 1;
    /* The lifetime cap still binds after the period window has rolled. */
    unchanged = scope;
    if (lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 21U }, 140U) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_charge_debit(&scope, LXP_AUTHORITY_DELEGATED_CAPABILITY,
                                   asset, LXP_MODULE_ASSET,
                                   (lxp_u128){ 0U, 20U }, 140U) != LXP_OK ||
        scope.spent_total.lo != 100U) return 1;
    return 0;
}

/* Kinds that carry no allowance must present a scope with no caps and no
 * recorded spend, so a metered grant can never be drawn as an unmetered one. */
static int unmetered_checks(void)
{
    lxp_authority_scope scope;
    lxp_authority_scope unchanged;
    uint8_t asset[32] = { 0x11U };
    (void)memset(&scope, 0, sizeof(scope));
    scope.module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    scope.activity_ordinal_max = 2U;
    unchanged = scope;
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_OWNER, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_OK ||
        lxp_authority_charge_debit(&scope, LXP_AUTHORITY_OWNER, asset,
                                   LXP_MODULE_ASSET,
                                   (lxp_u128){ UINT64_MAX, UINT64_MAX },
                                   100U) != LXP_OK ||
        memcmp(&scope, &unchanged, sizeof(scope)) != 0) return 1;
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_OWNER, asset,
                                  LXP_MODULE_PERPS, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE) return 1;
    scope.maximum_per_activity = (lxp_u128){ 0U, 5U };
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_OWNER, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE ||
        lxp_authority_charge_debit(&scope, LXP_AUTHORITY_SESSION_KEY, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                   100U) != LXP_ERR_AUTH_SCOPE) return 1;
    scope.maximum_per_activity = (lxp_u128){ 0U, 0U };
    scope.maximum_total = (lxp_u128){ 0U, 5U };
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_ESCROW, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE) return 1;
    scope.maximum_total = (lxp_u128){ 0U, 0U };
    scope.spent_total = (lxp_u128){ 0U, 1U };
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_PROTOCOL_MODULE, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE) return 1;
    scope.spent_total = (lxp_u128){ 0U, 0U };
    scope.spent_this_period = (lxp_u128){ 0U, 1U };
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_SESSION_KEY, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE) return 1;
    scope.spent_this_period = (lxp_u128){ 0U, 0U };
    scope.period_length = 4U;
    scope.maximum_per_period = (lxp_u128){ 0U, 3U };
    if (lxp_authority_check_debit(&scope, LXP_AUTHORITY_ESCROW, asset,
                                  LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                  100U) != LXP_ERR_AUTH_SCOPE) return 1;
    return 0;
}

static void declare_asset_module(lxp_kernel *kernel)
{
    static const uint32_t types[2] = { UINT32_C(0x00010001), ASSET_ACTIVITY };
    lxp_module_registration *registration =
        &kernel->modules[kernel->module_count];
    (void)memset(registration, 0, sizeof(*registration));
    registration->module_id = LXP_MODULE_ASSET;
    registration->abi_version = 1U;
    registration->activity_type_count = 2U;
    (void)memcpy(registration->activity_types, types, sizeof(types));
    registration->enabled_epoch = kernel->epoch;
    registration->disabled_epoch = UINT64_MAX;
    registration->enabled = true;
    ++kernel->module_count;
}

static lxp_result build_capability(lxp_authority_grant *grant,
                                   const lxp_identity *identity,
                                   const uint8_t key[32])
{
    (void)memset(grant, 0, sizeof(*grant));
    (void)memcpy(grant->grantor, identity->did_id, 32U);
    (void)memcpy(grant->grantee, identity->did_id, 32U);
    (void)memcpy(grant->key, key, 32U);
    grant->kind = LXP_AUTHORITY_DELEGATED_CAPABILITY;
    grant->scope.module_mask = UINT64_C(1) << LXP_MODULE_ASSET;
    grant->scope.activity_ordinal_min = 1U;
    grant->scope.activity_ordinal_max = 2U;
    grant->scope.asset_id[0] = 0x11U;
    grant->scope.maximum_per_activity = (lxp_u128){ 0U, 40U };
    grant->scope.maximum_total = (lxp_u128){ 0U, 60U };
    grant->scope.purpose_hash[0] = 0x77U;
    grant->not_before = GRANT_NOT_BEFORE;
    grant->not_after = GRANT_NOT_AFTER;
    grant->grantor_revocation_sequence = identity->revocation_sequence;
    return lxp_grant_id_compute(grant, grant->grant_id);
}

/* A capability grant persisted in governance state resolves through the same
 * entry every executing node uses, and the scope it hands back is the one the
 * ledger charges: the real caps the grantor wrote, never an unlimited scope. */
static int resolved_grant_checks(void)
{
    lxp_kernel *kernel;
    lxp_module_kv_entry *entry;
    lxp_identity identity;
    lxp_activity activity;
    lxp_authority_grant persisted;
    lxp_authority_grant resolved_grant;
    lxp_authority_resolved resolved;
    lxp_arena arena;
    lxp_byte_span encoded;
    uint8_t storage[1024];
    uint8_t key[32];
    uint8_t asset[32] = { 0x11U };
    size_t index;
    int failed = 1;
    kernel = calloc(1U, sizeof(*kernel));
    if (kernel == NULL) return 1;
    kernel->epoch = 4U;
    declare_asset_module(kernel);
    (void)memset(&identity, 0, sizeof(identity));
    for (index = 0U; index < 32U; ++index) {
        identity.did_id[index] = (uint8_t)(0x40U + index);
        identity.primary_key[index] = (uint8_t)(0x80U + index);
        key[index] = (uint8_t)(0xA0U + index);
    }
    identity.revocation_sequence = REVOCATION_SEQUENCE;
    if (build_capability(&persisted, &identity, key) != LXP_OK) goto done;
    if (lxp_arena_init(&arena, storage, sizeof(storage)) != LXP_OK ||
        lxp_grant_encode(&persisted, &arena, &encoded) != LXP_OK) goto done;
    entry = &kernel->module_kv[kernel->module_kv_count];
    (void)memset(entry, 0, sizeof(*entry));
    entry->module_id = LXP_MODULE_GOVERNANCE;
    entry->key_length = 33U;
    entry->key[0] = 5U;
    (void)memcpy(entry->key + 1U, persisted.grant_id, 32U);
    entry->value_length = (uint32_t)encoded.length;
    (void)memcpy(entry->value, encoded.bytes, encoded.length);
    ++kernel->module_kv_count;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = 1U;
    activity.activity_type = ASSET_ACTIVITY;
    activity.actor_did.bytes = identity.did_id;
    activity.actor_did.length = 32U;
    activity.authority.bytes = key;
    activity.authority.length = 32U;
    activity.timestamp_bound.not_before = GRANT_NOT_BEFORE;
    activity.timestamp_bound.not_after = GRANT_NOT_AFTER;
    if (lxp_authority_resolve_activity(kernel, &identity, &activity, false,
                                       true, BATCH_TIMESTAMP, TIMESTAMP_WINDOW,
                                       GLOBAL_SEQUENCE, &resolved_grant,
                                       &resolved) != LXP_OK) goto done;
    if (resolved.kind != LXP_AUTHORITY_DELEGATED_CAPABILITY ||
        resolved.scope != &resolved_grant.scope ||
        memcmp(resolved_grant.grant_id, persisted.grant_id, 32U) != 0 ||
        resolved_grant.scope.module_mask !=
            (UINT64_C(1) << LXP_MODULE_ASSET) ||
        resolved_grant.scope.maximum_per_activity.lo != 40U ||
        resolved_grant.scope.maximum_total.lo != 60U ||
        !lxp_u128_is_zero(resolved_grant.scope.spent_total)) goto done;
    /* The resolved scope carries a real bound: it charges twice and then
     * refuses, instead of admitting every draw. */
    if (lxp_authority_charge_debit(&resolved_grant.scope, resolved.kind, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 40U },
                                   BATCH_TIMESTAMP) != LXP_OK ||
        resolved_grant.scope.spent_total.lo != 40U) goto done;
    if (lxp_authority_charge_debit(&resolved_grant.scope, resolved.kind, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 21U },
                                   BATCH_TIMESTAMP) !=
            LXP_ERR_GRANT_EXHAUSTED ||
        resolved_grant.scope.spent_total.lo != 40U) goto done;
    if (lxp_authority_charge_debit(&resolved_grant.scope, resolved.kind, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 20U },
                                   BATCH_TIMESTAMP) != LXP_OK ||
        resolved_grant.scope.spent_total.lo != 60U) goto done;
    if (lxp_authority_charge_debit(&resolved_grant.scope, resolved.kind, asset,
                                   LXP_MODULE_ASSET, (lxp_u128){ 0U, 1U },
                                   BATCH_TIMESTAMP) != LXP_ERR_GRANT_EXHAUSTED)
        goto done;
    failed = 0;
done:
    free(kernel);
    return failed;
}

int main(void)
{
    if (charge_checks() != 0) return 1;
    if (binding_checks() != 0) return 1;
    if (unmetered_checks() != 0) return 1;
    if (resolved_grant_checks() != 0) return 1;
    return 0;
}
