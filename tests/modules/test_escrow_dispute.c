#include "layerx/lx_escrow.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <string.h>

static size_t transfer_calls;
static size_t last_leg_count;
static uint16_t last_reason;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    if (set->context.source_authorities == NULL ||
        set->context.source_authority_count != 1U)
        return LXP_ERR_NON_CANONICAL;
    ++transfer_calls;
    last_leg_count = set->leg_count;
    last_reason = set->legs[0].reason;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

static void build_resolve_payload(const uint8_t escrow_id[32],
                                  uint32_t basis_points,
                                  const uint8_t idempotency_key[32],
                                  uint8_t payload[LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES])
{
    (void)memset(payload, 0,
                 (size_t)LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES);
    (void)memcpy(payload, escrow_id, 32U);
    payload[32] = (uint8_t)(basis_points >> 24U);
    payload[33] = (uint8_t)(basis_points >> 16U);
    payload[34] = (uint8_t)(basis_points >> 8U);
    payload[35] = (uint8_t)basis_points;
    (void)memcpy(payload + 36U, idempotency_key, 32U);
}

static lxp_result dispatch(lxp_module_ctx *ctx, lxp_effect_buffer *effects,
                           uint32_t activity_type, uint16_t ordinal,
                           const uint8_t *payload, size_t payload_length,
                           const lxp_authority_resolved *authority)
{
    const lxp_module_iface *iface = lx_escrow_module_iface();
    lxp_activity activity;
    void *decoded = NULL;
    lxp_result status;
    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = activity_type;
    activity.payload.bytes = payload;
    activity.payload.length = payload_length;
    status = iface->decode(ctx, ordinal, payload, payload_length, &decoded);
    if (status == LXP_OK)
        status = iface->validate(ctx, &activity, authority, decoded);
    if (status != LXP_OK) return status;
    return iface->execute(ctx, &activity, authority, decoded, effects);
}

int main(void)
{
    static lxp_effect_buffer effects;
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *escrow_account;
    lx_account *second_escrow;
    lx_account *beneficiary;
    lx_escrow_runtime runtime;
    lx_escrow_record record;
    lx_escrow_record stored;
    lx_escrow_capture_request capture;
    lx_escrow_release_request release;
    lx_escrow_dispute_request dispute;
    lxp_authority_resolved authority;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[16384];
    uint8_t open_payload[LX_ESCROW_DISPUTE_OPEN_PAYLOAD_BYTES];
    uint8_t resolve_payload[LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES];
    uint8_t resolve_key[32];
    uint8_t rollback_key[32];
    uint8_t arbiter[32];
    uint64_t parameters = 1U;
    lxp_u128 beneficiary_share;
    lxp_u128 owner_share;
    bool found;
    const char *owner_name = "agent:did:key:owner:main";
    const char *escrow_name = "agent:did:key:owner:escrow:first";
    const char *second_name = "agent:did:key:owner:escrow:second";
    const char *beneficiary_name = "agent:did:key:provider:main";

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 5U;
    asset.symbol_length = 3U;
    (void)memcpy(asset.symbol, "USD", 4U);
    asset.name[0] = (uint8_t)'A';
    asset.name_length = 1U;
    asset.issuer_kind = 2U;
    asset.issuer_did32[0] = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(resolve_key, 0, sizeof(resolve_key));
    (void)memset(rollback_key, 0, sizeof(rollback_key));
    (void)memset(arbiter, 0, sizeof(arbiter));
    resolve_key[0] = 1U;
    rollback_key[0] = 2U;
    arbiter[0] = 9U;
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U,
                          (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)owner_name, strlen(owner_name),
                              1U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &owner) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)escrow_name,
                              strlen(escrow_name), 2U, LX_ACCOUNT_OPEN_CREDIT,
                              NULL, &escrow_account) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)second_name,
                              strlen(second_name), 3U, LX_ACCOUNT_OPEN_CREDIT,
                              NULL, &second_escrow) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)beneficiary_name,
                              strlen(beneficiary_name), 4U,
                              LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &beneficiary) != LXP_OK ||
        lxp_ledger_bootstrap_balance(escrow_account, asset.asset_id,
                                     (lxp_u128){ 0U, 101U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(second_escrow, asset.asset_id,
                                     (lxp_u128){ 0U, 101U }, 0U) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ESCROW,
                                       &runtime) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 500U, 0U, 1U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return 1;
    runtime.accounts = &accounts;
    runtime.assets = &assets;

    (void)memset(&record, 0, sizeof(record));
    record.escrow_id[0] = 4U;
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    (void)memcpy(record.beneficiary, beneficiary->id, 32U);
    (void)memcpy(record.arbiter, arbiter, 32U);
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 101U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    record.dispute_window = 600U;
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK) return 1;
    record.escrow_id[0] = 6U;
    (void)memcpy(record.escrow_account, second_escrow->id, 32U);
    record.state = LX_ESCROW_STATE_DISPUTED;
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    record.escrow_id[0] = 4U;
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    record.state = LX_ESCROW_STATE_OPEN;

    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, beneficiary->id, 32U);
    (void)memcpy(authority.actor, beneficiary->id, 32U);
    (void)memset(open_payload, 0, sizeof(open_payload));
    (void)memcpy(open_payload, record.escrow_id, 32U);
    if (dispatch(&ctx, &effects, LX_ESCROW_DISPUTE_OPEN, 6U, open_payload,
                 sizeof(open_payload), &authority) != LXP_OK ||
        transfer_calls != 0U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_DISPUTED ||
        effects.count != 1U || effects.effects[0].event_type != 6U ||
        effects.effects[0].body[34] != (uint8_t)LX_ESCROW_STATE_DISPUTED)
        return 1;

    (void)memset(&capture, 0, sizeof(capture));
    capture.escrow_id = record.escrow_id;
    capture.escrow_account = escrow_account;
    capture.beneficiary_account = beneficiary;
    capture.owner_account = owner;
    capture.asset = &asset;
    capture.amount = (lxp_u128){ 0U, 1U };
    capture.authority = &authority;
    capture.idempotency_key[0] = 7U;
    (void)memset(&release, 0, sizeof(release));
    release.escrow_id = record.escrow_id;
    release.escrow_account = escrow_account;
    release.owner_account = owner;
    release.asset = &asset;
    release.authority = &authority;
    release.idempotency_key[0] = 8U;
    if (lx_escrow_partial_capture_execute(&ctx, &capture, &receipt) !=
            LXP_ERR_HOLD_DISPUTED ||
        lx_escrow_release_execute(&ctx, &release, &receipt) !=
            LXP_ERR_HOLD_DISPUTED ||
        lx_escrow_timeout_execute(&ctx, &release, &receipt) !=
            LXP_ERR_HOLD_DISPUTED ||
        transfer_calls != 0U || escrow_account->balance.lo != 101U)
        return 1;

    build_resolve_payload(record.escrow_id, 3333U, resolve_key,
                          resolve_payload);
    if (lx_escrow_split_bps((lxp_u128){ 0U, 101U }, 3333U,
                            &beneficiary_share, &owner_share) != LXP_OK ||
        beneficiary_share.lo != 33U || owner_share.lo != 68U ||
        lx_escrow_split_bps((lxp_u128){ 0U, 101U }, 10001U,
                            &beneficiary_share, &owner_share) !=
            LXP_ERR_NON_CANONICAL ||
        dispatch(&ctx, &effects, LX_ESCROW_DISPUTE_RESOLVE, 7U,
                 resolve_payload, sizeof(resolve_payload), &authority) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        transfer_calls != 0U)
        return 1;
    (void)memcpy(authority.principal, arbiter, 32U);
    if (dispatch(&ctx, &effects, LX_ESCROW_DISPUTE_RESOLVE, 7U,
                 resolve_payload, sizeof(resolve_payload), &authority) !=
            LXP_OK ||
        transfer_calls != 1U || last_leg_count != 2U ||
        last_reason != LXP_REASON_ESCROW_RESOLVE ||
        escrow_account->balance.lo != 0U || beneficiary->balance.lo != 33U ||
        owner->balance.lo != 68U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_RESOLVED ||
        stored.captured_amount.lo != 33U ||
        !lxp_u128_is_zero(stored.locked_amount) ||
        lx_escrow_invariant_check(&stored, escrow_account) != LXP_OK ||
        effects.count != 2U || effects.effects[1].event_type != 7U ||
        lx_escrow_receipt_replay(&ctx, resolve_key, &receipt, &found) !=
            LXP_OK || !found || receipt.operation != 7U ||
        receipt.amount.lo != 33U)
        return 1;
    {
        uint8_t bad_payload[LX_ESCROW_DISPUTE_RESOLVE_PAYLOAD_BYTES];
        const lxp_module_iface *iface = lx_escrow_module_iface();
        void *decoded = NULL;
        build_resolve_payload(record.escrow_id, 10001U, resolve_key,
                              bad_payload);
        if (iface->decode(&ctx, 7U, bad_payload, sizeof(bad_payload),
                          &decoded) != LXP_ERR_NON_CANONICAL ||
            iface->decode(&ctx, 6U, open_payload, sizeof(open_payload) + 1U,
                          &decoded) != LXP_ERR_LENGTH_LIMIT)
            return 1;
    }

    if (lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 500U, 0U, 2U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return 1;
    (void)memset(&dispute, 0, sizeof(dispute));
    record.escrow_id[0] = 6U;
    dispute.escrow_id = record.escrow_id;
    dispute.escrow_account = second_escrow;
    dispute.beneficiary_account = beneficiary;
    dispute.owner_account = owner;
    dispute.asset = &asset;
    dispute.authority = &authority;
    dispute.beneficiary_basis_points = 3333U;
    (void)memcpy(dispute.idempotency_key, rollback_key, 32U);
#ifdef LXP_TESTING
    dispute.context.inject_failure = true;
    dispute.context.failure_after_leg = 0U;
    if (lx_escrow_dispute_resolve_execute(&ctx, &dispute, &receipt) !=
            LXP_ERR_IO ||
        second_escrow->balance.lo != 101U || beneficiary->balance.lo != 33U ||
        owner->balance.lo != 68U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_DISPUTED ||
        lx_escrow_receipt_replay(&ctx, rollback_key, &receipt, &found) !=
            LXP_OK || found)
        return 1;
    dispute.context.inject_failure = false;
#endif
    if (lx_escrow_dispute_resolve_execute(&ctx, &dispute, &receipt) !=
            LXP_OK ||
        second_escrow->balance.lo != 0U || beneficiary->balance.lo != 66U ||
        owner->balance.lo != 136U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_RESOLVED)
        return 1;

    stored.state = LX_ESCROW_STATE_OPEN;
    stored.locked_amount = (lxp_u128){ 0U, 0U };
    stored.dispute_window = 499U;
    (void)memcpy(authority.principal, beneficiary->id, 32U);
    if (lx_escrow_state_update(&ctx, &stored) != LXP_OK) return 1;
    (void)memset(&dispute, 0, sizeof(dispute));
    dispute.escrow_id = stored.escrow_id;
    dispute.escrow_account = second_escrow;
    dispute.beneficiary_account = beneficiary;
    dispute.owner_account = owner;
    dispute.asset = &asset;
    dispute.authority = &authority;
    if (lx_escrow_dispute_open_execute(&ctx, &dispute) !=
            LXP_ERR_DISPUTE_WINDOW_CLOSED)
        return 1;
    (void)memset(authority.principal, 0x3cU, 32U);
    stored.dispute_window = 600U;
    if (lx_escrow_state_update(&ctx, &stored) != LXP_OK ||
        lx_escrow_dispute_open_execute(&ctx, &dispute) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        lx_escrow_lookup(&ctx, stored.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_OPEN ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
