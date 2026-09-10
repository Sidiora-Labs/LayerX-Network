#include "layerx/lx_asset.h"
#include "layerx/lx_perps.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"

#include <stddef.h>
#include <string.h>

enum { FIXTURE_ARENA_BYTES = 1U << 20 };

static const uint8_t quote_asset_id[32] = { 7U };
static const uint8_t oracle_seed[32] = {
    0x9dU, 0x61U, 0xb1U, 0x9dU, 0xefU, 0xfdU, 0x5aU, 0x60U,
    0xbaU, 0x84U, 0x4aU, 0xf4U, 0x92U, 0xecU, 0x2cU, 0xc4U,
    0x44U, 0x49U, 0xc5U, 0x69U, 0x7bU, 0x32U, 0x69U, 0x19U,
    0x70U, 0x3bU, 0xacU, 0x03U, 0x1cU, 0xaeU, 0x7fU, 0x60U
};
static const char admin_did[] = "did:key:admin";

static lx_account_registry accounts;
static lx_asset_record asset_record;
static lxp_transfer_asset_state asset_state;
static lx_asset_runtime asset_runtime;
static lxp_state_store store;
static lxp_state_journal journal;
static lxp_kernel kernel;
static lxp_module_ctx ctx;
static lxp_arena arena;
static lxp_effect_buffer effects;
static uint64_t parameters = 1U;
static union {
    max_align_t alignment;
    uint8_t bytes[FIXTURE_ARENA_BYTES];
} arena_storage;
static uint64_t batch_timestamp = 1000U;
static uint64_t global_sequence = 1U;
static uint8_t activity_marker = 1U;
static uint8_t oracle_key[32];
static uint8_t administrator[32];
static lx_account *liquidity_account;
static lx_account *long_pool_account;
static lx_account *short_pool_account;
static lx_account *insurance_account;

static int account_add(const char *name, lx_account_open_authority mode,
                       uint64_t balance, lx_account **account)
{
    uint8_t id[32];
    lx_account *opened = NULL;
    size_t length = strlen(name);
    if (lx_account_id_from_string((const uint8_t *)name, length, id) !=
            LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)name, length, id, 1U,
                        mode, NULL, &opened) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened, quote_asset_id,
                                     (lxp_u128){ 0U, balance },
                                     0U) != LXP_OK)
        return 1;
    if (account != NULL) *account = opened;
    return 0;
}

static int oracle_key_init(void)
{
    lx_perps_oracle_command command;
    (void)memset(&command, 0, sizeof(command));
    command.market_id[0] = 1U;
    command.observation_sequence = 1U;
    command.price = (lxp_u128){ 0U, 1U };
    command.observed_at = 1U;
    command.source_identifier = 1U;
    if (lx_perps_oracle_command_sign(&command, oracle_seed) != LXP_OK)
        return 1;
    (void)memcpy(oracle_key, command.oracle_public_key, 32U);
    return lxp_did_id_derive((const uint8_t *)admin_did,
                             sizeof(admin_did) - 1U, administrator) != LXP_OK;
}

static int venue_accounts_open(void)
{
    return account_add("system:liquidity:perp", LX_ACCOUNT_OPEN_GENESIS, 0U,
                       &liquidity_account) != 0 ||
           account_add("system:funding:perp:long", LX_ACCOUNT_OPEN_GENESIS,
                       100000U, &long_pool_account) != 0 ||
           account_add("system:funding:perp:short", LX_ACCOUNT_OPEN_GENESIS,
                       100000U, &short_pool_account) != 0 ||
           account_add("system:insurance", LX_ACCOUNT_OPEN_GENESIS, 0U,
                       &insurance_account) != 0;
}

static int fixture_init(void)
{
    (void)memset(&asset_record, 0, sizeof(asset_record));
    (void)memcpy(asset_record.asset_id, quote_asset_id, 32U);
    asset_record.symbol_length = 1U;
    (void)memcpy(asset_record.symbol, "Q", 2U);
    asset_record.custody_kind = LX_ASSET_CUSTODY_PAXEER;
    asset_record.custody_reference[0] = 1U;
    asset_record.custody_reference_length = 1U;
    return lx_asset_transfer_state(&asset_record, &asset_state) != LXP_OK ||
           lx_account_registry_init(&accounts) != LXP_OK ||
           oracle_key_init() != 0 || venue_accounts_open() != 0;
}

static int kernel_start(void)
{
    if (lxp_state_store_init(&store, 0U) != LXP_OK ||
        lxp_state_store_bind_accounts(&store, &accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&store) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters,
                          0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel,
                                   lx_perps_module_iface()) != LXP_OK ||
        lxp_kernel_set_capabilities(
            &kernel, NULL, lxp_kernel_canonical_ledger_apply) != LXP_OK)
        return 1;
    asset_runtime = (lx_asset_runtime){ &accounts, &asset_record, 1U,
                                        &asset_state, 1U, 7U,
                                        LXP_PROTOCOL_VERSION_OCCUPANCY };
    return lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ASSET,
                                          &asset_runtime) != LXP_OK;
}

static uint64_t main_sequence(const char *did)
{
    uint8_t name[LX_ACCOUNT_NAME_MAX];
    uint8_t id[32];
    size_t did_length = strlen(did);
    size_t i;
    if (did_length + 11U > sizeof(name)) return 0U;
    (void)memcpy(name, "agent:", 6U);
    (void)memcpy(name + 6U, did, did_length);
    (void)memcpy(name + 6U + did_length, ":main", 5U);
    if (lx_account_id_from_string(name, did_length + 11U, id) != LXP_OK)
        return 0U;
    for (i = 0U; i < accounts.count; ++i)
        if (memcmp(accounts.accounts[i].id, id, 32U) == 0)
            return accounts.accounts[i].next_sequence;
    return 0U;
}

static int ctx_open(void)
{
    if (lxp_arena_init(&arena, arena_storage.bytes,
                       sizeof(arena_storage.bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_PERPS, batch_timestamp,
                            0U, global_sequence, 1000000U, &arena,
                            true) != LXP_OK)
        return 1;
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    ctx.batch_number = 1U;
    ctx.activity_id[0] = activity_marker++;
    return 0;
}

static lxp_result run(uint32_t activity_type, const char *did,
                      const uint8_t *payload, size_t payload_length,
                      const uint8_t *authority_key,
                      const uint8_t *authority_signature)
{
    const lxp_module_registration *registration = NULL;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_result module_result = LXP_OK;
    lxp_result status;
    size_t did_length = strlen(did);
    if (lxp_kernel_module_for_activity(&kernel, activity_type, 0U,
                                       &registration) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK || ctx_open() != 0 ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    (void)memset(&activity, 0, sizeof(activity));
    activity.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    activity.network_id = 7U;
    activity.activity_type = activity_type;
    activity.actor_did = (lxp_byte_span){ (const uint8_t *)did, did_length };
    activity.account_sequence = main_sequence(did);
    activity.timestamp_bound.not_after = batch_timestamp + 1000U;
    activity.idempotency_key[0] = ctx.activity_id[0];
    activity.payload = (lxp_byte_span){ payload, payload_length };
    (void)memset(&authority, 0, sizeof(authority));
    authority.kind = LXP_AUTHORITY_OWNER;
    if (lxp_did_id_derive((const uint8_t *)did, did_length,
                          authority.actor) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    (void)memcpy(authority.principal, authority.actor, 32U);
    if (authority_key != NULL) {
        activity.authority = (lxp_byte_span){ authority_key, 32U };
        activity.signature = (lxp_byte_span){ authority_signature, 64U };
        (void)memcpy(authority.verified_key, authority_key, 32U);
    }
    status = lxp_kernel_dispatch(registration, &ctx, &activity, &authority,
                                 &effects, &module_result);
    if (status != LXP_OK || module_result != LXP_OK) {
        lxp_module_ctx_rollback(&ctx);
        return status != LXP_OK ? status : module_result;
    }
    if (lxp_module_ctx_commit(&ctx) != LXP_OK) return LXP_FATAL_INVARIANT;
    ++global_sequence;
    return LXP_OK;
}

static void market_defaults(uint8_t id, lx_perps_market *market)
{
    (void)memset(market, 0, sizeof(*market));
    market->market_id[0] = id;
    (void)memcpy(market->quote_asset, quote_asset_id, 32U);
    (void)memcpy(market->administrator, administrator, 32U);
    (void)memcpy(market->liquidity_account_id, liquidity_account->id, 32U);
    (void)memcpy(market->long_funding_account_id, long_pool_account->id, 32U);
    (void)memcpy(market->short_funding_account_id, short_pool_account->id,
                 32U);
    (void)memcpy(market->insurance_account_id, insurance_account->id, 32U);
    market->contract_size = (lxp_u128){ 0U, 1U };
    market->tick_size = (lxp_u128){ 0U, 1U };
    market->lot_size = (lxp_u128){ 0U, 1U };
    market->price_scale = (lxp_u128){ 0U, 1U };
    market->initial_margin_ratio_bps = 1000U;
    market->maintenance_margin_ratio_bps = 500U;
    market->liquidation_fee_bps = 10U;
    market->liquidator_share_bps = 6000U;
    market->maximum_funding_rate_bps = 1000U;
    market->maximum_deviation_basis_points = 10000U;
    market->funding_interval_ms = 100U;
    market->maximum_oracle_staleness_ms = 100000U;
    market->minimum_price = (lxp_u128){ 0U, 1U };
    market->maximum_price = (lxp_u128){ 0U, 1000000U };
    market->permitted_oracle_key_count = 1U;
    (void)memcpy(market->permitted_oracle_keys[0], oracle_key, 32U);
    market->parameter_version = 1U;
}

static lxp_result market_create(const lx_perps_market *market,
                                const char *did)
{
    uint8_t payload[LX_PERPS_MARKET_BYTES];
    if (lx_perps_market_encode(market, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_PERPS_MARKET_CREATE, did, payload, sizeof(payload), NULL,
               NULL);
}

static lxp_result oracle_push(const uint8_t seed[32], uint8_t market_id,
                              uint64_t sequence, uint64_t price,
                              uint64_t observed_at)
{
    lx_perps_oracle_command command;
    uint8_t payload[LX_PERPS_ORACLE_PAYLOAD_BYTES];
    (void)memset(&command, 0, sizeof(command));
    command.market_id[0] = market_id;
    command.observation_sequence = sequence;
    command.price = (lxp_u128){ 0U, price };
    command.observed_at = observed_at;
    command.source_identifier = 5U;
    if (lx_perps_oracle_command_sign(&command, seed) != LXP_OK ||
        lx_perps_oracle_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_PERPS_ORACLE_PUSH, admin_did, payload, sizeof(payload),
               command.oracle_public_key, command.signature);
}

static int balance_is(const lx_account *account, uint64_t value)
{
    return account != NULL && account->balance.hi == 0U &&
           account->balance.lo == value;
}

static lx_account *alice_main;
static lx_account *alice_margin;
static lx_account *bob_main;

static void identifier(uint8_t value, uint8_t out[32])
{
    (void)memset(out, 0, 32U);
    out[0] = value;
}

static lxp_result position_open(uint8_t position_id,
                                const lx_account *margin_account,
                                lx_perps_side side, uint64_t size,
                                uint64_t notional, uint64_t margin_amount,
                                const char *did)
{
    lx_perps_open_command command;
    uint8_t payload[LX_PERPS_OPEN_PAYLOAD_BYTES];
    (void)memset(&command, 0, sizeof(command));
    command.market_id[0] = 1U;
    command.position_id[0] = position_id;
    (void)memcpy(command.margin_account_id, margin_account->id, 32U);
    command.side = side;
    command.size = (lxp_u128){ 0U, size };
    command.entry_notional = (lxp_u128){ 0U, notional };
    command.margin_amount = (lxp_u128){ 0U, margin_amount };
    if (lx_perps_open_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_PERPS_POSITION_OPEN, did, payload, sizeof(payload), NULL,
               NULL);
}

static lxp_result position_increase(uint8_t position_id, uint64_t size,
                                    uint64_t notional, uint64_t margin_amount,
                                    const char *did)
{
    lx_perps_increase_command command;
    uint8_t payload[LX_PERPS_INCREASE_PAYLOAD_BYTES];
    (void)memset(&command, 0, sizeof(command));
    command.market_id[0] = 1U;
    command.position_id[0] = position_id;
    command.size_delta = (lxp_u128){ 0U, size };
    command.notional_delta = (lxp_u128){ 0U, notional };
    command.margin_amount = (lxp_u128){ 0U, margin_amount };
    if (lx_perps_increase_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_PERPS_POSITION_INCREASE, did, payload, sizeof(payload),
               NULL, NULL);
}

static lxp_result position_close(uint8_t position_id, const char *did)
{
    lx_perps_close_command command;
    uint8_t payload[LX_PERPS_CLOSE_PAYLOAD_BYTES];
    (void)memset(&command, 0, sizeof(command));
    command.market_id[0] = 1U;
    command.position_id[0] = position_id;
    if (lx_perps_close_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_PERPS_POSITION_CLOSE, did, payload, sizeof(payload), NULL,
               NULL);
}

static lxp_result position_state(uint8_t position_id,
                                 lx_perps_position *position)
{
    uint8_t market_id[32];
    uint8_t id[32];
    identifier(1U, market_id);
    identifier(position_id, id);
    return lx_perps_position_get(&ctx, market_id, id, position);
}

int main(void)
{
    lx_perps_market market;
    lx_perps_position position;
    lx_perps_position_store positions;
    lx_perps_position *found = NULL;
    lx_perps_funding_state funding;
    uint8_t market_id[32];
    uint8_t absent_id[32];

    identifier(1U, market_id);
    identifier(9U, absent_id);
    if (fixture_init() != 0 || kernel_start() != 0 ||
        account_add("agent:did:key:alice:main", LX_ACCOUNT_OPEN_CREDIT, 1000U,
                    &alice_main) != 0 ||
        account_add("agent:did:key:alice:margin:a", LX_ACCOUNT_OPEN_CREDIT,
                    0U, &alice_margin) != 0 ||
        account_add("agent:did:key:bob:main", LX_ACCOUNT_OPEN_CREDIT, 0U,
                    &bob_main) != 0)
        return 1;
    market_defaults(1U, &market);
    if (market_create(&market, admin_did) != LXP_OK ||
        oracle_push(oracle_seed, 1U, 1U, 100U, 1000U) != LXP_OK)
        return 1;
    if (position_open(1U, alice_margin, LX_PERPS_SIDE_BUY, 10U, 1001U, 100U,
                      "did:key:alice") != LXP_ERR_PARAMETER_BOUNDS ||
        position_open(1U, alice_margin, LX_PERPS_SIDE_BUY, 10U, 1000U, 50U,
                      "did:key:alice") != LXP_ERR_MARGIN_INSUFFICIENT ||
        position_open(2U, alice_margin, LX_PERPS_SIDE_BUY, 10U, 1000U, 100U,
                      "did:key:bob") != LXP_ERR_UNAUTHORIZED_DEBIT ||
        position_open(1U, alice_margin, LX_PERPS_SIDE_BUY, 10U, 1000U, 100U,
                      "did:key:alice") != LXP_OK ||
        position_open(1U, alice_margin, LX_PERPS_SIDE_BUY, 10U, 1000U, 100U,
                      "did:key:alice") != LXP_ERR_SEQUENCE_REUSED)
        return 1;
    if (ctx_open() != 0 || !balance_is(alice_main, 900U) ||
        !balance_is(alice_margin, 100U) || !alice_margin->has_open_reference ||
        position_state(1U, &position) != LXP_OK || !position.open ||
        position.size.lo != 10U || position.entry_notional.lo != 1000U ||
        position.side != LX_PERPS_SIDE_BUY ||
        memcmp(position.owner_main_account_id, alice_main->id, 32U) != 0 ||
        memcmp(position.margin_account_id, alice_margin->id, 32U) != 0 ||
        memcmp(position.asset_id, quote_asset_id, 32U) != 0 ||
        lx_perps_funding_state_lookup(&ctx, market_id, &funding) != LXP_OK ||
        funding.long_open_notional.lo != 1000U ||
        !lxp_u128_is_zero(funding.short_open_notional))
        return 1;
    if (position_increase(1U, 10U, 1000U, 100U, "did:key:bob") !=
            LXP_ERR_UNAUTHORIZED_DEBIT ||
        position_increase(2U, 10U, 1000U, 100U, "did:key:alice") !=
            LXP_ERR_UNKNOWN_FIELD ||
        position_increase(1U, 10U, 1000U, 10U, "did:key:alice") !=
            LXP_ERR_MARGIN_INSUFFICIENT ||
        position_increase(1U, 10U, 1000U, 100U, "did:key:alice") != LXP_OK)
        return 1;
    if (ctx_open() != 0 || !balance_is(alice_main, 800U) ||
        !balance_is(alice_margin, 200U) ||
        position_state(1U, &position) != LXP_OK ||
        position.size.lo != 20U || position.entry_notional.lo != 2000U ||
        lx_perps_funding_state_lookup(&ctx, market_id, &funding) != LXP_OK ||
        funding.long_open_notional.lo != 2000U)
        return 1;
    (void)memset(&positions, 0, sizeof(positions));
    positions.positions[0] = position;
    positions.count = 1U;
    if (lx_perps_position_lookup(&positions, position.position_id,
                                 &found) != LXP_OK || found == NULL ||
        found->size.lo != 20U ||
        lx_perps_position_lookup(&positions, absent_id, &found) !=
            LXP_ERR_UNKNOWN_FIELD)
        return 1;
    if (position_close(1U, "did:key:bob") != LXP_ERR_UNAUTHORIZED_DEBIT ||
        position_close(1U, "did:key:alice") != LXP_OK ||
        position_close(1U, "did:key:alice") != LXP_ERR_AGREEMENT_STATE)
        return 1;
    if (ctx_open() != 0 || !balance_is(alice_main, 1000U) ||
        !balance_is(alice_margin, 0U) || !balance_is(bob_main, 0U) ||
        alice_margin->has_open_reference ||
        position_state(1U, &position) != LXP_OK || position.open ||
        lx_perps_funding_state_lookup(&ctx, market_id, &funding) != LXP_OK ||
        !lxp_u128_is_zero(funding.long_open_notional) ||
        lxp_state_store_destroy(&store) != LXP_OK)
        return 1;
    return 0;
}
