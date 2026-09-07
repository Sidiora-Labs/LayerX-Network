#ifndef LXP_DAEMON_LNI_ACCOUNT_H
#define LXP_DAEMON_LNI_ACCOUNT_H

#include "layerx/lxp_daemon.h"
#include "layerx/lxp_crypto.h"
#include "layerx/programs.h"

static lxp_result latest_receipt_evidence(
    lxp_daemon_protocol_owner *owner, lxp_arena *arena,
    lxp_daemon_receipt_evidence *evidence)
{
    uint64_t offset = 0U;
    uint64_t target;
    size_t mark;
    bool present = true;
    lxp_result status = LXP_OK;
    if (owner == NULL || owner->receipt_authority == NULL || arena == NULL ||
        evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    target = owner->receipt_authority->last_global_sequence;
    if (target == 0U) return LXP_ERR_MODULE_DISABLED;
    mark = lxp_arena_mark(arena);
    while (status == LXP_OK && present) {
        status = lxp_daemon_receipt_authority_scan(
            owner->receipt_authority, &offset, arena, evidence, &present);
        if (status != LXP_OK || !present) break;
        if (evidence->global_sequence == target) return LXP_OK;
        if (evidence->global_sequence > target)
            return LXP_ERR_LOG_CORRUPT;
        status = lxp_arena_reset(arena, mark);
    }
    return status == LXP_OK ? LXP_ERR_PROJECTION_STALE : status;
}

static lxp_result latest_account_evidence(
    lxp_daemon_protocol_owner *owner, const uint8_t account_id[32],
    const uint8_t *asset_id, const uint8_t *target_activity_id,
    lxp_arena *arena, lxp_daemon_account_evidence *evidence)
{
    lxp_daemon_receipt_evidence head;
    lxp_receipt receipt;
    const lx_account_registry *accounts;
    uint8_t receipt_digest[32];
    size_t index;
    bool found = false;
    lxp_result status;
    if (owner == NULL || owner->kernel == NULL || owner->kernel->state == NULL ||
        owner->kernel->state->accounts == NULL || account_id == NULL ||
        arena == NULL || evidence == NULL)
        return LXP_ERR_NON_CANONICAL;
    accounts = owner->kernel->state->accounts;
    for (index = 0U; index < accounts->count; ++index) {
        const lx_account *account = &accounts->accounts[index];
        if (lxp_ct_memcmp(account->id, account_id, 32U) != 0) continue;
        if (found) return LXP_FATAL_INVARIANT;
        found = true;
        if (asset_id != NULL &&
            (!account->has_asset ||
             lxp_ct_memcmp(account->asset_id, asset_id, 32U) != 0))
            return LXP_ERR_ASSET_MISMATCH;
    }
    if (!found) return LXP_ERR_UNKNOWN_ACCOUNT_NAMESPACE;
    status = latest_receipt_evidence(owner, arena, &head);
    if (status == LXP_OK && head.format_version == 3U) {
        lxp_programs_occupancy_receipt maintenance;
        status = lxp_programs_occupancy_receipt_decode(
            head.canonical_receipt.bytes, head.canonical_receipt.length,
            &maintenance);
        if (status == LXP_OK && target_activity_id != NULL) {
            lxp_daemon_activity_evidence activity;
            status = lxp_daemon_activity_evidence_lookup(
                owner->evidence_store, target_activity_id, arena, &activity);
            if (status == LXP_OK)
                status = lxp_receipt_decode(activity.canonical_receipt.bytes,
                    activity.canonical_receipt.length, true, &receipt);
            if (status == LXP_OK &&
                (receipt.global_sequence == UINT64_MAX ||
                 receipt.global_sequence + 1U != maintenance.global_sequence ||
                 lxp_ct_memcmp(receipt.activity_id, target_activity_id, 32U) != 0 ||
                 lxp_ct_memcmp(receipt.resulting_state_root,
                     maintenance.previous_state_root, 32U) != 0 ||
                 activity.signed_header.canonical_header.length != head.canonical_header.length ||
                 lxp_ct_memcmp(activity.signed_header.canonical_header.bytes,
                     head.canonical_header.bytes, head.canonical_header.length) != 0 ||
                 lxp_ct_memcmp(activity.signed_header.signature, head.header_signature, 64U) != 0))
                status = LXP_ERR_CONTEXT_MISMATCH;
        }
        if (status == LXP_OK)
            status = lxp_daemon_account_evidence_lookup(
                owner->evidence_store, account_id, maintenance.resulting_state_root,
                arena, evidence);
        if (status == LXP_OK &&
            (evidence->format_version != 2U ||
             evidence->observed_sequence != maintenance.global_sequence ||
             evidence->canonical_receipt.length != head.canonical_receipt.length ||
             lxp_ct_memcmp(evidence->canonical_receipt.bytes,
                 head.canonical_receipt.bytes, head.canonical_receipt.length) != 0))
            status = LXP_ERR_PROJECTION_STALE;
        if (status == LXP_OK)
            status = lxp_state_root(owner->kernel, receipt_digest);
        if (status == LXP_OK &&
            (owner->kernel->state->next_sequence == 0U ||
             owner->kernel->state->next_sequence - 1U != maintenance.global_sequence ||
             lxp_ct_memcmp(receipt_digest, owner->kernel->current_state_root, 32U) != 0 ||
             lxp_ct_memcmp(receipt_digest, maintenance.resulting_state_root, 32U) != 0 ||
             evidence->signed_header.canonical_header.length != head.canonical_header.length ||
             lxp_ct_memcmp(evidence->signed_header.canonical_header.bytes,
                 head.canonical_header.bytes, head.canonical_header.length) != 0 ||
             lxp_ct_memcmp(evidence->signed_header.signature, head.header_signature, 64U) != 0))
            status = LXP_ERR_PROJECTION_STALE;
        return status;
    }
    if (status == LXP_OK)
        status = lxp_receipt_decode(head.canonical_receipt.bytes,
                                    head.canonical_receipt.length,
                                    true, &receipt);
    if (status == LXP_OK && target_activity_id != NULL &&
        lxp_ct_memcmp(receipt.activity_id, target_activity_id, 32U) != 0)
        status = LXP_ERR_CONTEXT_MISMATCH;
    if (status == LXP_OK)
        status = lxp_receipt_digest(&receipt, arena, receipt_digest);
    if (status == LXP_OK)
        status = lxp_daemon_account_evidence_build(
            owner->kernel, owner->network_id, account_id, receipt_digest,
            receipt.timestamp, head.canonical_receipt, &head.receipt_proof,
            &owner->receipt_authority->authorization,
            head.canonical_header, head.header_signature, arena, evidence);
    return status;
}

#endif
