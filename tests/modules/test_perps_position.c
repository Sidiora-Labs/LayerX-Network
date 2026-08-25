#include "layerx/lx_asset.h"
#include "layerx/lx_perps.h"
#include "layerx/lxp_kernel.h"

#include <string.h>

static size_t last_legs;
static uint16_t last_reason;

static lxp_result apply_capability(lxp_kernel *kernel,
                                   const lxp_transfer_set *set,
                                   lxp_receipt *receipt)
{
    lxp_transfer_set_result result;
    lxp_transfer_context context = set->context;
    lxp_result status;
    (void)kernel;
    last_legs = set->leg_count;
    last_reason = set->legs[0].reason;
    status = lxp_apply_transfer_set((lxp_transfer_leg *)set->legs,
                                    set->leg_count, &context, &result);
    if (status == LXP_OK)
        (void)memcpy(receipt->transfer_set_root, result.transfer_set_root, 32U);
    return status;
}

int main(void)
{
    const char *owner_name = "agent:did:key:alice:main";
    const char *margin_name = "agent:did:key:alice:margin:btc-usd";
    lx_account_registry accounts;
    lx_account *owner;
    lx_account *margin;
    uint8_t owner_id[32];
    uint8_t margin_id[32];
    uint8_t asset_id[32] = { 7U };
    lxp_transfer_asset_state asset = { { 0U }, true, false };
    lx_perps_position_store positions;
    lx_perps_position_request request;
    lx_asset_custody_attestation attestation;
    lx_asset_reserve_report_record report;
    lxp_state_store state;
    lxp_state_journal journal;
    lxp_kernel kernel;
    lxp_module_ctx ctx;
    lxp_arena arena;
    lxp_receipt receipt;
    uint8_t arena_bytes[4096];
    uint64_t parameters = 1U;
    lxp_transfer_context unauthorized;
    lxp_transfer_leg leg;
    lxp_transfer_result leg_result;

    (void)memcpy(asset.asset_id, asset_id, 32U);
    (void)memset(&positions, 0, sizeof(positions));
    (void)memset(&request, 0, sizeof(request));
    (void)memset(&attestation, 0, sizeof(attestation));
    if (lx_account_registry_init(&accounts) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)owner_name,
                                  strlen(owner_name), owner_id) != LXP_OK ||
        lx_account_id_from_string((const uint8_t *)margin_name,
                                  strlen(margin_name), margin_id) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)owner_name,
                        strlen(owner_name), owner_id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &owner) != LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)margin_name,
                        strlen(margin_name), margin_id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &margin) != LXP_OK ||
        lxp_ledger_bootstrap_balance(owner, asset_id,
                                     (lxp_u128){ 0U, 100U }, 0U) != LXP_OK ||
        lxp_ledger_bootstrap_balance(margin, asset_id,
                                     (lxp_u128){ 0U, 0U }, 0U) != LXP_OK ||
        lxp_state_store_init(&state, 0U) != LXP_OK ||
        lxp_kernel_create(&kernel, &state, &journal, &parameters, 0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_perps_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(&kernel, NULL, apply_capability) != LXP_OK ||
        lxp_arena_init(&arena, arena_bytes, sizeof(arena_bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PERPS, 100U, 0U, 1U,
                            1000U, &arena, true) != LXP_OK)
        return 1;
    request.store = &positions;
    request.owner_main = owner;
    request.margin_account = margin;
    request.asset = &asset;
    request.margin_amount = (lxp_u128){ 0U, 40U };
    request.position.position_id[0] = 1U;
    request.position.market_id[0] = 2U;
    (void)memcpy(request.position.owner_main_account_id, owner->id, 32U);
    (void)memcpy(request.position.margin_account_id, margin->id, 32U);
    (void)memcpy(request.position.asset_id, asset_id, 32U);
    request.position.side = LX_PERPS_SIDE_BUY;
    request.position.size = (lxp_u128){ 0U, 2U };
    request.position.entry_notional = (lxp_u128){ 0U, 200U };
    request.context.assets = &asset;
    request.context.asset_count = 1U;
    request.context.sequence_account = owner;
    request.context.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(request.context.authorized_from, owner->id, 32U);
    positions.count = LX_PERPS_POSITION_CAPACITY + 1U;
    if (lx_perps_position_open_execute(&ctx, &request, &receipt) !=
            LXP_ERR_NON_CANONICAL ||
        owner->balance.lo != 100U || !lxp_u128_is_zero(margin->balance))
        return 1;
    positions.count = 0U;
    if (lx_perps_position_open_execute(&ctx, &request, &receipt) != LXP_OK ||
        last_legs != 1U || last_reason != LXP_REASON_MARGIN_POST ||
        owner->balance.lo != 60U || margin->balance.lo != 40U ||
        positions.count != 1U || !positions.positions[0].open)
        return 1;
    (void)memcpy(attestation.asset_id, asset_id, 32U);
    attestation.custody_amount = (lxp_u128){ 0U, 100U };
    attestation.finalized = true;
    if (lx_asset_reserve_report(&accounts, &attestation, &report) != LXP_OK ||
        report.agent_main.lo != 60U || report.margin.lo != 40U)
        return 1;
    (void)memset(&unauthorized, 0, sizeof(unauthorized));
    unauthorized.assets = &asset;
    unauthorized.asset_count = 1U;
    unauthorized.debit_authority_kind = LXP_AUTH_OWNER;
    (void)memcpy(unauthorized.authorized_from, margin->id, 32U);
    (void)memset(&leg, 0, sizeof(leg));
    leg.from = margin;
    leg.to = owner;
    (void)memcpy(leg.asset_id, asset_id, 32U);
    leg.amount = (lxp_u128){ 0U, 1U };
    leg.reason = LXP_REASON_MARGIN_RELEASE;
    if (lxp_apply_transfer(&leg, &unauthorized, &leg_result) !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        lx_perps_position_close_execute(
            &ctx, &positions, request.position.position_id, margin, owner,
            &asset, request.context, &receipt) != LXP_OK ||
        last_reason != LXP_REASON_MARGIN_RELEASE || owner->balance.lo != 100U ||
        margin->balance.lo != 0U || positions.positions[0].open ||
        lxp_state_store_destroy(&state) != LXP_OK)
        return 1;
    return 0;
}
