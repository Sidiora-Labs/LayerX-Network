#include "layerx/lx_escrow.h"
#include "layerx/lxp_hash.h"
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
        set->context.source_authority_count == 0U)
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

static void write_u64(uint8_t out[8], uint64_t value)
{
    size_t i;
    for (i = 0U; i < 8U; ++i)
        out[i] = (uint8_t)(value >> ((7U - i) * 8U));
}

static void build_release_payload(const uint8_t escrow_id[32],
                                  const uint8_t idempotency_key[32],
                                  uint8_t payload[LX_ESCROW_RELEASE_PAYLOAD_BYTES])
{
    (void)memset(payload, 0, (size_t)LX_ESCROW_RELEASE_PAYLOAD_BYTES);
    (void)memcpy(payload, escrow_id, 32U);
    (void)memcpy(payload + 32U, idempotency_key, 32U);
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

static void asset_init(lx_asset_record *asset)
{
    (void)memset(asset, 0, sizeof(*asset));
    asset->asset_id[0] = 1U;
    asset->symbol_length = 3U;
    (void)memcpy(asset->symbol, "USD", 4U);
    asset->custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset->custody_reference[0] = 1U;
    asset->custody_reference_length = 1U;
}

static int explicit_release(void)
{
    static lxp_effect_buffer effects;
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *first_escrow;
    lx_account *second_escrow;
    lx_escrow_runtime runtime;
    lx_escrow_record record;
    lx_escrow_record stored;
    lx_escrow_release_request request;
    lxp_authority_resolved authority;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    lxp_receipt replayed;
    uint8_t arena_bytes[16384];
    uint8_t payload[LX_ESCROW_RELEASE_PAYLOAD_BYTES];
    uint8_t release_key[32];
    uint8_t timeout_key[32];
    uint64_t parameters = 1U;
    bool found;
    const char *owner_name = "agent:did:key:owner:main";
    const char *first_name = "agent:did:key:owner:escrow:release";
    const char *second_name = "agent:did:key:owner:escrow:timeout";

    asset_init(&asset);
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(release_key, 0, sizeof(release_key));
    (void)memset(timeout_key, 0, sizeof(timeout_key));
    release_key[0] = 5U;
    timeout_key[0] = 6U;
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U,
                          (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)owner_name, strlen(owner_name),
                              1U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &owner) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)first_name, strlen(first_name),
                              2U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &first_escrow) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)second_name,
                              strlen(second_name), 3U, LX_ACCOUNT_OPEN_CREDIT,
                              NULL, &second_escrow) != LXP_OK ||
        lxp_ledger_bootstrap_balance(first_escrow, asset.asset_id,
                                     (lxp_u128){ 0U, 20U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(second_escrow, asset.asset_id,
                                     (lxp_u128){ 0U, 7U }, 0U) != LXP_OK ||
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
    (void)memcpy(record.escrow_account, first_escrow->id, 32U);
    (void)memcpy(record.beneficiary, owner->id, 32U);
    record.arbiter[0] = 9U;
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 20U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    record.dispute_window = 800U;
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK) return 1;
    record.escrow_id[0] = 5U;
    (void)memcpy(record.escrow_account, second_escrow->id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 7U };
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return 1;
    record.escrow_id[0] = 4U;
    (void)memcpy(record.escrow_account, first_escrow->id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 20U };

    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, owner->id, 32U);
    (void)memcpy(authority.actor, owner->id, 32U);
    build_release_payload(record.escrow_id, release_key, payload);
    if (dispatch(&ctx, &effects, LX_ESCROW_RELEASE, 4U, payload,
                 sizeof(payload), &authority) != LXP_OK ||
        transfer_calls != 1U || last_leg_count != 1U ||
        last_reason != LXP_REASON_ESCROW_RELEASE ||
        first_escrow->balance.lo != 0U || owner->balance.lo != 20U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_RELEASED ||
        !lxp_u128_is_zero(stored.locked_amount) ||
        lx_escrow_invariant_check(&stored, first_escrow) != LXP_OK ||
        effects.count != 1U || effects.effects[0].event_type != 4U ||
        effects.effects[0].body[34] != (uint8_t)LX_ESCROW_STATE_RELEASED)
        return 1;
    if (lx_escrow_receipt_replay(&ctx, release_key, &receipt, &found) !=
            LXP_OK || !found || receipt.operation != 4U ||
        receipt.amount.lo != 20U)
        return 1;
    (void)memset(&request, 0, sizeof(request));
    request.escrow_id = record.escrow_id;
    request.escrow_account = first_escrow;
    request.owner_account = owner;
    request.asset = &asset;
    request.authority = &authority;
    (void)memcpy(request.idempotency_key, release_key, 32U);
    (void)memset(&replayed, 0xff, sizeof(replayed));
    if (lx_escrow_release_execute(&ctx, &request, &replayed) != LXP_OK ||
        transfer_calls != 1U ||
        memcmp(&receipt, &replayed, sizeof(receipt)) != 0 ||
        first_escrow->balance.lo != 0U || owner->balance.lo != 20U)
        return 1;

    record.escrow_id[0] = 5U;
    build_release_payload(record.escrow_id, timeout_key, payload);
    if (dispatch(&ctx, &effects, LX_ESCROW_TIMEOUT, 5U, payload,
                 sizeof(payload), &authority) != LXP_ERR_NOT_YET_VALID ||
        transfer_calls != 1U || second_escrow->balance.lo != 7U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_OPEN ||
        lx_escrow_receipt_replay(&ctx, timeout_key, &receipt, &found) !=
            LXP_OK || found || effects.count != 1U)
        return 1;
    if (lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 1200U, 0U, 2U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK ||
        dispatch(&ctx, &effects, LX_ESCROW_TIMEOUT, 5U, payload,
                 sizeof(payload), &authority) != LXP_OK ||
        transfer_calls != 2U || second_escrow->balance.lo != 0U ||
        owner->balance.lo != 27U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_TIMED_OUT ||
        effects.count != 1U || effects.effects[0].event_type != 5U ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

static int sweep_capacity(void)
{
    static lxp_effect_buffer effects;
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *escrow_account;
    lx_escrow_runtime runtime;
    lx_escrow_record record;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    uint8_t arena_bytes[8192];
    uint64_t parameters = 1U;
    const char *owner_name = "agent:did:key:sweep:main";
    char escrow_name[40];
    size_t i;
    const size_t holds = (size_t)LX_ESCROW_SWEEP_CAPACITY + 1U;

    asset_init(&asset);
    if (lx_asset_registry_init(&assets, 0U) != LXP_OK ||
        lx_asset_register(&assets, &asset, 0U,
                          (lxp_u128){ 0U, 0U }) != LXP_OK ||
        lx_account_registry_init(&accounts) != LXP_OK ||
        lx_asset_account_open(&assets, &accounts, asset.asset_id,
                              (const uint8_t *)owner_name, strlen(owner_name),
                              1U, LX_ACCOUNT_OPEN_CREDIT, NULL,
                              &owner) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ESCROW,
                                       &runtime) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 2000U, 0U, 1U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return 1;
    runtime.accounts = &accounts;
    runtime.assets = &assets;
    for (i = 0U; i < holds; ++i) {
        int written = 0;
        (void)memset(escrow_name, 0, sizeof(escrow_name));
        (void)memcpy(escrow_name, "agent:did:key:sweep:escrow:h", 28U);
        escrow_name[28] = (char)('a' + (int)(i / 10U));
        escrow_name[29] = (char)('0' + (int)(i % 10U));
        written = 30;
        if (lx_asset_account_open(&assets, &accounts, asset.asset_id,
                                  (const uint8_t *)escrow_name,
                                  (size_t)written, i + 2U,
                                  LX_ACCOUNT_OPEN_CREDIT, NULL,
                                  &escrow_account) != LXP_OK ||
            escrow_account->kind != LX_ACCOUNT_AGENT_ESCROW ||
            lxp_ledger_bootstrap_balance(escrow_account, asset.asset_id,
                                         (lxp_u128){ 0U, 2U }, 0U) != LXP_OK)
            return 1;
        (void)memset(&record, 0, sizeof(record));
        record.escrow_id[0] = (uint8_t)(i + 1U);
        (void)memcpy(record.owner, owner->id, 32U);
        (void)memcpy(record.escrow_account, escrow_account->id, 32U);
        (void)memcpy(record.beneficiary, owner->id, 32U);
        record.arbiter[0] = 9U;
        (void)memcpy(record.asset_id, asset.asset_id, 32U);
        record.locked_amount = (lxp_u128){ 0U, 2U };
        record.state = LX_ESCROW_STATE_OPEN;
        record.expiry = 1000U;
        if (lx_escrow_state_put(&ctx, &record) != LXP_OK) return 1;
        if ((i % 8U) == 7U && lxp_module_ctx_commit(&ctx) != LXP_OK) return 1;
    }
    if (lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 2000U) !=
            LXP_ERR_ARENA_EXHAUSTED ||
        owner->balance.lo != 0U || effects.count != 0U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &record) != LXP_OK ||
        record.state != LX_ESCROW_STATE_OPEN ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}

static int run_timeout(bool delayed, uint8_t root[32])
{
    static lxp_effect_buffer effects;
    lx_asset_registry assets;
    lx_asset_record asset;
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *escrow_account;
    lx_escrow_runtime runtime;
    lx_escrow_record record;
    lx_escrow_record stored;
    lx_escrow_capture_request capture;
    lxp_authority_resolved authority;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    lxp_transfer_set unauthorized;
    lxp_transfer_asset_state asset_state;
    lxp_transfer_source_authority source;
    uint8_t arena_bytes[8192];
    uint8_t root_input[33];
    uint8_t timeout_key[32];
    uint64_t parameters = 1U;
    volatile uint64_t elapsed_work = 0U;
    uint64_t i;
    bool found;
    const char *owner_name = "agent:did:key:owner:main";
    const char *escrow_name = "agent:did:key:owner:escrow:sweep";

    asset_init(&asset);
    (void)memset(&authority, 0, sizeof(authority));
    (void)memset(&receipt, 0, sizeof(receipt));
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
        lxp_ledger_bootstrap_balance(escrow_account, asset.asset_id,
                                     (lxp_u128){ 0U, 50U }, 0U) != LXP_OK ||
        lx_asset_transfer_state(&asset, &asset_state) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_escrow_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 1000U, 0U, 1U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return 1;

    (void)memset(&record, 0, sizeof(record));
    record.escrow_id[0] = 1U;
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    (void)memcpy(record.beneficiary, owner->id, 32U);
    record.arbiter[0] = 9U;
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 50U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 1000U;
    record.dispute_window = 900U;
    if (lx_escrow_state_put(&ctx, &record) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lx_escrow_timeout_key(&record, timeout_key) != LXP_OK)
        return 1;

    (void)memset(&unauthorized, 0, sizeof(unauthorized));
    (void)memset(&source, 0, sizeof(source));
    unauthorized.leg_count = 1U;
    unauthorized.legs[0].from = escrow_account;
    unauthorized.legs[0].to = owner;
    (void)memcpy(unauthorized.legs[0].asset_id, asset.asset_id, 32U);
    unauthorized.legs[0].amount = (lxp_u128){ 0U, 1U };
    unauthorized.legs[0].reason = LXP_REASON_PAYMENT;
    (void)memcpy(source.authorized_from, escrow_account->id, 32U);
    source.debit_authority_kind = LXP_AUTH_ESCROW;
    unauthorized.context.assets = &asset_state;
    unauthorized.context.asset_count = 1U;
    unauthorized.context.source_authorities = &source;
    unauthorized.context.source_authority_count = 1U;
    unauthorized.context.sequence_account = escrow_account;
    unauthorized.context.actor_sequence = escrow_account->next_sequence;
    (void)memcpy(unauthorized.context.authorized_from,
                 escrow_account->id, 32U);
    if (lxp_ctx_emit_transfer_set(&ctx, &unauthorized, &receipt) !=
            LXP_ERR_UNAUTHORIZED_ESCROW_SPEND ||
        escrow_account->balance.lo != 50U)
        return 1;
    unauthorized.legs[0].reason = LXP_REASON_ESCROW_RELEASE;
    source.debit_authority_kind = LXP_AUTH_OWNER;
    if (lxp_ctx_emit_transfer_set(&ctx, &unauthorized, &receipt) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        escrow_account->balance.lo != 50U)
        return 1;

    if (delayed)
        for (i = 0U; i < UINT64_C(1000000); ++i) elapsed_work += i;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 1U, 1000U) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 999U) !=
            LXP_ERR_TIMESTAMP_REGRESSION ||
        lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) != LXP_OK ||
        escrow_account->balance.lo != 50U || owner->balance.lo != 0U ||
        effects.count != 0U)
        return 1;
    if (lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ESCROW,
                                       &runtime) != LXP_OK)
        return 1;
    runtime.accounts = &accounts;
    runtime.assets = &assets;
    accounts.count = LX_ACCOUNT_REGISTRY_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    accounts.count = 2U;
    assets.count = LX_ASSET_REGISTRY_CAPACITY + 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) !=
        LXP_ERR_NON_CANONICAL)
        return 1;
    assets.count = 1U;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) != LXP_OK ||
        transfer_calls == 0U || last_leg_count != 1U ||
        last_reason != LXP_REASON_ESCROW_RELEASE ||
        escrow_account->balance.lo != 0U || owner->balance.lo != 50U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        stored.state != LX_ESCROW_STATE_TIMED_OUT ||
        !lxp_u128_is_zero(stored.locked_amount) ||
        lx_escrow_invariant_check(&stored, escrow_account) != LXP_OK ||
        effects.count != 1U || effects.effects[0].event_type != 5U ||
        effects.effects[0].body[34] != (uint8_t)LX_ESCROW_STATE_TIMED_OUT ||
        lx_escrow_receipt_replay(&ctx, timeout_key, &receipt, &found) !=
            LXP_OK || !found || receipt.operation != 5U ||
        receipt.amount.lo != 50U)
        return 1;
    if (lx_escrow_module_iface()->epoch_begin(&ctx, 0U, 1000U) != LXP_OK ||
        escrow_account->balance.lo != 0U || owner->balance.lo != 50U ||
        effects.count != 1U)
        return 1;

    (void)memset(&capture, 0, sizeof(capture));
    capture.escrow_id = record.escrow_id;
    capture.escrow_account = escrow_account;
    capture.beneficiary_account = owner;
    capture.owner_account = owner;
    capture.asset = &asset;
    capture.amount = (lxp_u128){ 0U, 1U };
    capture.authority = &authority;
    capture.idempotency_key[0] = 9U;
    (void)memcpy(authority.principal, owner->id, 32U);
    if (lx_escrow_partial_capture_execute(&ctx, &capture, &receipt) !=
        LXP_ERR_HOLD_EXPIRED)
        return 1;

    write_u64(root_input, owner->balance.hi);
    write_u64(root_input + 8U, owner->balance.lo);
    write_u64(root_input + 16U, escrow_account->balance.hi);
    write_u64(root_input + 24U, escrow_account->balance.lo);
    root_input[32] = (uint8_t)stored.state;
    if (lxp_hash_sha256(root_input, sizeof(root_input), root) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    (void)elapsed_work;
    return 0;
}

int main(void)
{
    uint8_t immediate[32];
    uint8_t delayed[32];
    if (explicit_release() != 0 || sweep_capacity() != 0 ||
        run_timeout(false, immediate) != 0 ||
        run_timeout(true, delayed) != 0 ||
        memcmp(immediate, delayed, 32U) != 0)
        return 1;
    return 0;
}
