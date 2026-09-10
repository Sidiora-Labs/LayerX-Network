#ifndef LAYERX_LXP_AUTHORITY_H
#define LAYERX_LXP_AUTHORITY_H

#include "layerx/lxp_activity.h"
#include "layerx/lxp_codec.h"
#include "layerx/lxp_u128.h"

#include <stdbool.h>
#include <stdint.h>

typedef enum lxp_authority_kind {
    LXP_AUTHORITY_OWNER = 1,
    LXP_AUTHORITY_SESSION_KEY = 2,
    LXP_AUTHORITY_DELEGATED_CAPABILITY = 3,
    LXP_AUTHORITY_BUDGET_ALLOWANCE = 4,
    LXP_AUTHORITY_ESCROW = 5,
    LXP_AUTHORITY_PROTOCOL_MODULE = 6
} lxp_authority_kind;

typedef struct lxp_authority_scope {
    uint64_t module_mask;
    uint16_t activity_ordinal_min;
    uint16_t activity_ordinal_max;
    uint8_t asset_id[32];
    lxp_u128 maximum_per_activity;
    lxp_u128 maximum_total;
    lxp_u128 spent_total;
    uint64_t period_length;
    lxp_u128 maximum_per_period;
    lxp_u128 spent_this_period;
    uint64_t period_start;
    uint8_t purpose_hash[32];
} lxp_authority_scope;
#define lxp_authority_scope lxp_authority_scope

typedef struct lxp_authority_grant {
    uint8_t grant_id[32];
    uint8_t grantor[32];
    uint8_t grantee[32];
    lxp_authority_kind kind;
    uint8_t key[32];
    lxp_authority_scope scope;
    uint64_t not_before;
    uint64_t not_after;
    uint64_t grantor_revocation_sequence;
    bool revoked;
    uint64_t revoked_at_sequence;
    uint8_t grantor_signature[64];
} lxp_authority_grant;
#define lxp_authority_grant lxp_authority_grant

lxp_result lxp_grant_encode(const lxp_authority_grant *grant,
                            lxp_arena *arena, lxp_byte_span *encoded);
lxp_result lxp_grant_id_compute(const lxp_authority_grant *grant,
                                uint8_t grant_id[32]);
lxp_result lxp_session_key_bind(lxp_authority_grant *grant,
                                const uint8_t grantor[32],
                                const uint8_t session_key[32],
                                uint64_t module_mask,
                                uint16_t ordinal_min,
                                uint16_t ordinal_max,
                                uint64_t not_before, uint64_t not_after,
                                uint64_t revocation_sequence);

typedef struct lxp_authority_resolved {
    uint8_t actor[32];
    uint8_t principal[32];
    lxp_authority_kind kind;
    uint8_t verified_key[32];
    const lxp_authority_scope *scope;
    uint8_t authority_hash[32];
} lxp_authority_resolved;
#define lxp_authority_resolved lxp_authority_resolved

/* The activity envelope an executing node declares from its registered module
 * set. Every resolved grant scope must fall inside it. */
typedef struct lxp_authority_envelope {
    uint64_t module_mask;
    uint16_t activity_ordinal_min;
    uint16_t activity_ordinal_max;
} lxp_authority_envelope;
#define lxp_authority_envelope lxp_authority_envelope

struct lxp_kernel;
struct lxp_identity;

lxp_result lxp_grant_decode(const uint8_t *bytes, size_t length,
                            lxp_authority_grant *grant);
lxp_result lxp_authority_envelope_declare(const struct lxp_kernel *kernel,
                                          uint64_t epoch,
                                          lxp_authority_envelope *envelope);
/* Owner authority is bounded by the account balance and by the signed activity
 * window, never by an allowance: an owner scope carries no spend caps. */
lxp_result lxp_authority_owner_grant(const struct lxp_identity *identity,
                                     const uint8_t verified_key[32],
                                     const lxp_authority_envelope *envelope,
                                     uint64_t not_before, uint64_t not_after,
                                     lxp_authority_grant *grant);
/* Loads the persisted grant the governance module wrote for verified_key,
 * applying its persisted revocation record. */
lxp_result lxp_authority_grant_lookup(const struct lxp_kernel *kernel,
                                      const uint8_t grantor[32],
                                      const uint8_t verified_key[32],
                                      lxp_authority_grant *grant);
/* The single authority entry every executing node uses. The resolved scope
 * points into grant, which the caller must keep alive for the execution. */
lxp_result lxp_authority_resolve_activity(const struct lxp_kernel *kernel,
                                          const struct lxp_identity *identity,
                                          const lxp_activity *activity,
                                          bool owner_key_valid,
                                          bool signature_valid,
                                          uint64_t batch_timestamp,
                                          uint64_t maximum_timestamp_window,
                                          uint64_t global_sequence,
                                          lxp_authority_grant *grant,
                                          lxp_authority_resolved *resolved);

lxp_result lxp_authority_hash(lxp_authority_kind kind,
                              const uint8_t grant_id[32],
                              const uint8_t verified_key[32],
                              uint8_t authority_hash[32]);
lxp_result lxp_authority_check_scope(const lxp_authority_scope *scope,
                                     uint32_t activity_type,
                                     uint64_t declared_module_mask,
                                     uint16_t declared_ordinal_min,
                                     uint16_t declared_ordinal_max);
lxp_result lxp_authority_resolve(const lxp_authority_grant *grant,
                                 const uint8_t actor[32],
                                 uint32_t activity_type,
                                 uint64_t declared_module_mask,
                                 uint16_t declared_ordinal_min,
                                 uint16_t declared_ordinal_max,
                                 bool signature_valid,
                                 lxp_authority_resolved *resolved);
lxp_result lxp_authority_period_roll(lxp_authority_scope *scope,
                                     uint64_t batch_timestamp);
lxp_result lxp_authority_spend_check(const lxp_authority_scope *scope,
                                     lxp_u128 amount);
lxp_result lxp_authority_charge_allowance(lxp_authority_scope *scope,
                                          lxp_u128 amount,
                                          uint64_t batch_timestamp);
lxp_result lxp_authority_revoke(lxp_authority_grant *grant,
                                uint64_t revocation_sequence,
                                uint64_t global_sequence);
lxp_result lxp_authority_amend(lxp_authority_grant *grant,
                               const lxp_authority_grant *narrower);
lxp_result lxp_authority_is_live(const lxp_authority_grant *grant,
                                 uint64_t identity_revocation_sequence,
                                 uint64_t batch_timestamp,
                                 uint64_t global_sequence);

lxp_result lxp_identity_bump_revocation_sequence(struct lxp_identity *identity,
                                                 uint64_t new_sequence);

#endif
