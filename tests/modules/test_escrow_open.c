#include "layerx/lx_escrow.h"
#include "layerx/lxp_kernel.h"
#include "layerx/lxp_receipt.h"

#include <string.h>

static size_t emitted_legs;
static uint16_t emitted_reason;
static uint16_t emitted_origin;
static lxp_authorization_kind emitted_authority;

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
    emitted_legs = set->leg_count;
    emitted_reason = set->legs[0].reason;
    emitted_origin = set->context.origin_module_id;
    emitted_authority =
        set->context.source_authorities[0].debit_authority_kind;
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

static lxp_result build_open_payload(const lx_escrow_record *record,
                                     uint8_t payload[LX_ESCROW_OPEN_PAYLOAD_BYTES])
{
    (void)memset(payload, 0, (size_t)LX_ESCROW_OPEN_PAYLOAD_BYTES);
    (void)memcpy(payload, record->escrow_id, 32U);
    (void)memcpy(payload + 32U, record->owner, 32U);
    (void)memcpy(payload + 64U, record->escrow_account, 32U);
    (void)memcpy(payload + 96U, record->beneficiary, 32U);
    (void)memcpy(payload + 128U, record->arbiter, 32U);
    (void)memcpy(payload + 160U, record->asset_id, 32U);
    write_u64(payload + 208U, record->expiry);
    write_u64(payload + 216U, record->dispute_window);
    (void)memcpy(payload + 224U, record->terms_hash, 32U);
    (void)memcpy(payload + 256U, record->agreement_reference, 32U);
    return lxp_u128_to_be(record->locked_amount, payload + 192U);
}

static int codec_round_trip(const lx_escrow_record *record)
{
    uint8_t bytes[LX_ESCROW_RECORD_BYTES];
    lx_escrow_record decoded;
    lx_escrow_record zero_id;
    lx_escrow_economic_result result;
    lx_escrow_economic_result decoded_result;
    uint8_t result_bytes[LX_ESCROW_RESULT_BYTES];
    if (lx_escrow_record_encode(record, bytes) != LXP_OK ||
        lx_escrow_record_decode(bytes, sizeof(bytes), &decoded) != LXP_OK ||
        memcmp(record, &decoded, sizeof(decoded)) != 0 ||
        lx_escrow_record_decode(bytes, sizeof(bytes) - 1U, &decoded) !=
            LXP_ERR_NON_CANONICAL)
        return 1;
    zero_id = *record;
    (void)memset(zero_id.escrow_id, 0, 32U);
    if (lx_escrow_record_encode(&zero_id, bytes) != LXP_ERR_NON_CANONICAL)
        return 1;
    (void)memset(&result, 0, sizeof(result));
    (void)memcpy(result.escrow_id, record->escrow_id, 32U);
    result.ordinal = 3U;
    result.state_after = LX_ESCROW_STATE_PARTIALLY_CAPTURED;
    result.captured_after = (lxp_u128){ 1U, 2U };
    result.locked_after = (lxp_u128){ 3U, 4U };
    (void)memcpy(result.asset_id, record->asset_id, 32U);
    (void)memcpy(result.from, record->escrow_account, 32U);
    (void)memcpy(result.to, record->beneficiary, 32U);
    result.amount = (lxp_u128){ 5U, 6U };
    result.secondary_amount = (lxp_u128){ 7U, 8U };
    result.transfer_set_root[31] = 9U;
    result.global_sequence = UINT64_C(0x0102030405060708);
    result.timestamp = UINT64_C(0x1112131415161718);
    if (lx_escrow_result_encode(&result, result_bytes) != LXP_OK ||
        lx_escrow_result_decode(result_bytes, sizeof(result_bytes),
                                &decoded_result) != LXP_OK ||
        memcmp(&result, &decoded_result, sizeof(result)) != 0 ||
        lx_escrow_result_decode(result_bytes, sizeof(result_bytes) - 1U,
                                &decoded_result) != LXP_ERR_NON_CANONICAL)
        return 1;
    return 0;
}

int main(void)
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
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_activity activity;
    lxp_authority_resolved authority;
    const lxp_module_iface *iface = lx_escrow_module_iface();
    const lxp_module_registration *registration;
    void *decoded = NULL;
    uint8_t payload[LX_ESCROW_OPEN_PAYLOAD_BYTES];
    uint8_t arena_bytes[8192];
    uint8_t root[32];
    uint64_t parameters = 1U;
    const char *owner_name = "agent:did:key:alice:main";
    const char *escrow_name = "agent:did:key:alice:escrow:order-7";

    (void)memset(&asset, 0, sizeof(asset));
    asset.asset_id[0] = 1U;
    asset.symbol_length = 3U;
    (void)memcpy(asset.symbol, "USD", 4U);
    asset.name[0] = (uint8_t)'A';
    asset.name_length = 1U;
    asset.issuer_kind = 2U;
    asset.issuer_did32[0] = 1U;
    asset.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset.custody_reference[0] = 1U;
    asset.custody_reference_length = 1U;
    if (iface == NULL || iface->module_id != LXP_MODULE_ESCROW ||
        iface->abi_version != 1U || iface->activity_type_count != 7U ||
        iface->decode == NULL || iface->validate == NULL ||
        iface->execute == NULL || iface->epoch_begin == NULL ||
        iface->epoch_end == NULL || iface->state_root == NULL ||
        strcmp(iface->name, "escrow") != 0)
        return 1;
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
        owner->kind != LX_ACCOUNT_AGENT_MAIN ||
        escrow_account->kind != LX_ACCOUNT_AGENT_ESCROW ||
        !lxp_u128_is_zero(owner->balance) ||
        !lxp_u128_is_zero(escrow_account->balance) ||
        lxp_ledger_bootstrap_balance(owner, asset.asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, iface) != LXP_OK ||
        lxp_kernel_module_for_activity(&kernel, LX_ESCROW_DISPUTE_RESOLVE, 0U,
                                       &registration) != LXP_OK ||
        registration->activity_type_count != 7U ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ESCROW,
                                       &runtime) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_ESCROW, 10U, 0U, 1U,
                            100000U, &arena, true) != LXP_OK ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return 1;
    runtime.accounts = &accounts;
    runtime.assets = &assets;

    (void)memset(&record, 0, sizeof(record));
    record.escrow_id[0] = 9U;
    (void)memcpy(record.owner, owner->id, 32U);
    (void)memcpy(record.escrow_account, escrow_account->id, 32U);
    record.beneficiary[0] = 7U;
    record.arbiter[0] = 8U;
    (void)memcpy(record.asset_id, asset.asset_id, 32U);
    record.locked_amount = (lxp_u128){ 0U, 40U };
    record.state = LX_ESCROW_STATE_OPEN;
    record.expiry = 5000U;
    record.dispute_window = 600U;
    record.terms_hash[0] = 10U;
    record.agreement_reference[0] = 11U;
    if (codec_round_trip(&record) != 0 ||
        build_open_payload(&record, payload) != LXP_OK)
        return 1;

    (void)memset(&activity, 0, sizeof(activity));
    activity.activity_type = LX_ESCROW_OPEN;
    activity.payload.bytes = payload;
    activity.payload.length = sizeof(payload);
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = LXP_AUTHORITY_OWNER;
    (void)memcpy(authority.principal, owner->id, 32U);
    (void)memcpy(authority.actor, owner->id, 32U);

    if (iface->genesis(&ctx, NULL, 0U) != LXP_OK ||
        iface->decode(&ctx, 8U, payload, sizeof(payload), &decoded) !=
            LXP_ERR_UNKNOWN_ACTIVITY ||
        iface->decode(&ctx, 1U, payload, sizeof(payload) - 1U, &decoded) !=
            LXP_ERR_LENGTH_LIMIT ||
        iface->decode(&ctx, 1U, payload, sizeof(payload), &decoded) != LXP_OK ||
        decoded == NULL ||
        iface->validate(&ctx, &activity, &authority, decoded) != LXP_OK)
        return 1;
    activity.activity_type = LX_ESCROW_CAPTURE;
    if (iface->validate(&ctx, &activity, &authority, decoded) !=
        LXP_ERR_UNKNOWN_ACTIVITY)
        return 1;
    activity.activity_type = LX_ESCROW_OPEN;

    if (iface->execute(&ctx, &activity, &authority, decoded, &effects) !=
            LXP_OK ||
        emitted_legs != 1U || emitted_reason != LXP_REASON_ESCROW_LOCK ||
        emitted_origin != LXP_MODULE_ESCROW ||
        emitted_authority != LXP_AUTH_OWNER ||
        owner->balance.hi != 0U || owner->balance.lo != 60U ||
        escrow_account->balance.hi != 0U || escrow_account->balance.lo != 40U ||
        effects.count != 1U ||
        effects.effects[0].kind != LXP_EFFECT_EVENT ||
        effects.effects[0].module_id != LXP_MODULE_ESCROW ||
        effects.effects[0].event_type != 1U ||
        effects.effects[0].body_length != (uint16_t)LX_ESCROW_EVENT_BYTES ||
        memcmp(effects.effects[0].body, record.escrow_id, 32U) != 0 ||
        effects.effects[0].body[33] != 1U ||
        effects.effects[0].body[34] != (uint8_t)LX_ESCROW_STATE_OPEN ||
        effects.effects[0].body[66] != 40U ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        memcmp(&stored, &record, sizeof(stored)) != 0 ||
        lx_escrow_invariant_check(&stored, escrow_account) != LXP_OK ||
        iface->state_root(&ctx, root) != LXP_OK)
        return 1;

    if (lxp_module_ctx_commit(&ctx) != LXP_OK ||
        lx_escrow_lookup(&ctx, record.escrow_id, &stored) != LXP_OK ||
        memcmp(&stored, &record, sizeof(stored)) != 0 ||
        iface->execute(&ctx, &activity, &authority, decoded, &effects) !=
            LXP_ERR_ESCROW_STATE ||
        owner->balance.lo != 60U || escrow_account->balance.lo != 40U ||
        effects.count != 1U)
        return 1;

    (void)memcpy(authority.principal, escrow_account->id, 32U);
    if (iface->execute(&ctx, &activity, &authority, decoded, &effects) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        effects.count != 1U ||
        iface->epoch_end(&ctx, 0U, 10U) != LXP_OK ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
