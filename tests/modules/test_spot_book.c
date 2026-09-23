#include "layerx/lx_asset.h"
#include "layerx/lx_spot.h"
#include "layerx/lxp_identity.h"
#include "layerx/lxp_kernel.h"

#include <stddef.h>
#include <stdio.h>
#include <string.h>

#define REQUIRE(condition) do { if (!(condition)) { \
    fprintf(stderr, "spot book line %d: %s\n", __LINE__, #condition); \
    return 1; } } while (0)

enum {
    FIXTURE_ARENA_BYTES = 1U << 21,
    BASE_SUPPLY = 3000U,
    QUOTE_SUPPLY = 300000U
};

static const uint8_t base_asset_id[32] = { 0x0bU };
static const uint8_t quote_asset_id[32] = { 7U };
static const char admin_did[] = "did:key:admin";

static lx_account_registry accounts;
static lx_asset_record asset_records[2];
static lxp_transfer_asset_state asset_states[2];
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
static uint8_t activity_marker = 1U;
static uint8_t administrator[32];

typedef struct trader {
    const char *did;
    uint8_t main_id[32];
    uint8_t base_id[32];
} trader;

static trader alice = { "did:key:alice", { 0U }, { 0U } };
static trader bob = { "did:key:bob", { 0U }, { 0U } };
static trader carol = { "did:key:carol", { 0U }, { 0U } };

static int account_add(const char *name, const uint8_t asset_id[32],
                       uint64_t balance, uint8_t id[32])
{
    lx_account *opened = NULL;
    size_t length = strlen(name);
    if (lx_account_id_from_string((const uint8_t *)name, length, id) !=
            LXP_OK ||
        lx_account_open(&accounts, (const uint8_t *)name, length, id, 1U,
                        LX_ACCOUNT_OPEN_CREDIT, NULL, &opened) != LXP_OK ||
        lxp_ledger_bootstrap_balance(opened, asset_id,
                                     (lxp_u128){ 0U, balance },
                                     0U) != LXP_OK)
        return 1;
    return 0;
}

static int trader_open(trader *party)
{
    static const char hex[] = "0123456789abcdef";
    char name[160];
    size_t length;
    size_t i;
    (void)snprintf(name, sizeof(name), "agent:%s:main", party->did);
    if (account_add(name, quote_asset_id, QUOTE_SUPPLY / 3U,
                    party->main_id) != 0)
        return 1;
    length = (size_t)snprintf(name, sizeof(name), "agent:%s:asset:",
                              party->did);
    for (i = 0U; i < 32U; ++i) {
        name[length + i * 2U] = hex[base_asset_id[i] >> 4U];
        name[length + i * 2U + 1U] = hex[base_asset_id[i] & 15U];
    }
    name[length + 64U] = '\0';
    return account_add(name, base_asset_id, BASE_SUPPLY / 3U,
                       party->base_id);
}

static int asset_record_init(size_t index, const uint8_t asset_id[32],
                             const char *symbol)
{
    lx_asset_record *record = &asset_records[index];
    (void)memset(record, 0, sizeof(*record));
    (void)memcpy(record->asset_id, asset_id, 32U);
    record->symbol_length = 1U;
    (void)memcpy(record->symbol, symbol, 2U);
    record->custody_kind = LX_ASSET_CUSTODY_PAXEER;
    record->custody_reference[0] = (uint8_t)(index + 1U);
    record->custody_reference_length = 1U;
    return lx_asset_transfer_state(record, &asset_states[index]) != LXP_OK;
}

static int fixture_init(void)
{
    return asset_record_init(0U, base_asset_id, "B") != 0 ||
           asset_record_init(1U, quote_asset_id, "Q") != 0 ||
           lx_account_registry_init(&accounts) != LXP_OK ||
           lxp_did_id_derive((const uint8_t *)admin_did,
                             sizeof(admin_did) - 1U, administrator) !=
               LXP_OK ||
           trader_open(&alice) != 0 || trader_open(&bob) != 0 ||
           trader_open(&carol) != 0;
}

static int kernel_start(void)
{
    if (lxp_state_store_init(&store, 1U) != LXP_OK ||
        lxp_state_store_bind_accounts(&store, &accounts) != LXP_OK ||
        lxp_state_store_require_account_root(&store) != LXP_OK ||
        lxp_kernel_create(&kernel, &store, &journal, &parameters,
                          0U) != LXP_OK ||
        lxp_kernel_register_module(&kernel, lx_spot_module_iface()) !=
            LXP_OK ||
        lxp_kernel_set_capabilities(
            &kernel, NULL, lxp_kernel_canonical_ledger_apply) != LXP_OK)
        return 1;
    asset_runtime = (lx_asset_runtime){ &accounts, asset_records, 2U,
                                        asset_states, 2U, 7U,
                                        LXP_PROTOCOL_VERSION_OCCUPANCY };
    return lxp_kernel_bind_module_runtime(&kernel, LXP_MODULE_ASSET,
                                          &asset_runtime) != LXP_OK;
}

static const lx_account *account_by_id(const uint8_t id[32])
{
    size_t i;
    for (i = 0U; i < accounts.count; ++i)
        if (memcmp(accounts.accounts[i].id, id, 32U) == 0)
            return &accounts.accounts[i];
    return NULL;
}

static uint64_t main_sequence(const char *did)
{
    char name[160];
    uint8_t id[32];
    const lx_account *account;
    size_t length = (size_t)snprintf(name, sizeof(name), "agent:%s:main", did);
    if (lx_account_id_from_string((const uint8_t *)name, length, id) !=
        LXP_OK)
        return 0U;
    account = account_by_id(id);
    return account == NULL ? 0U : account->next_sequence;
}

static int ctx_open(void)
{
    if (lxp_arena_init(&arena, arena_storage.bytes,
                       sizeof(arena_storage.bytes)) != LXP_OK ||
        lxp_module_ctx_init(&ctx, &kernel, LXP_MODULE_SPOT, batch_timestamp,
                            0U, store.next_sequence, 1000000U, &arena,
                            true) != LXP_OK)
        return 1;
    ctx.protocol_version = LXP_PROTOCOL_VERSION_OCCUPANCY;
    ctx.batch_number = 1U;
    ctx.activity_id[0] = activity_marker++;
    return 0;
}

static lxp_result run(uint32_t activity_type, const char *did,
                      const uint8_t *payload, size_t payload_length)
{
    const lxp_module_registration *registration = NULL;
    lxp_activity activity;
    lxp_authority_resolved authority;
    lxp_result module_result = LXP_OK;
    lxp_result status;
    size_t did_length = strlen(did);
    if (lxp_kernel_module_for_activity(&kernel, activity_type, 0U,
                                       &registration) != LXP_OK ||
        lxp_effect_buffer_init(&effects) != LXP_OK ||
        lxp_state_journal_open(&store, store.next_sequence, &journal) !=
            LXP_OK)
        return LXP_FATAL_INVARIANT;
    if (ctx_open() != 0 ||
        lxp_module_ctx_bind_effects(&ctx, &effects) != LXP_OK) {
        (void)lxp_state_journal_rollback(&journal);
        return LXP_FATAL_INVARIANT;
    }
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
                          authority.actor) != LXP_OK) {
        lxp_module_ctx_rollback(&ctx);
        (void)lxp_state_journal_rollback(&journal);
        return LXP_FATAL_INVARIANT;
    }
    (void)memcpy(authority.principal, authority.actor, 32U);
    status = lxp_kernel_dispatch(registration, &ctx, &activity, &authority,
                                 &effects, &module_result);
    if (status != LXP_OK || module_result != LXP_OK) {
        lxp_module_ctx_rollback(&ctx);
        if (lxp_state_journal_rollback(&journal) != LXP_OK)
            return LXP_FATAL_INVARIANT;
        return status != LXP_OK ? status : module_result;
    }
    if (lxp_module_ctx_prepare_commit(&ctx) != LXP_OK ||
        lxp_state_journal_commit(&journal) != LXP_OK ||
        lxp_module_ctx_commit(&ctx) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return LXP_OK;
}

static void identifier(uint8_t value, uint8_t out[32])
{
    (void)memset(out, 0, 32U);
    out[0] = value;
}

static void market_defaults(uint8_t id, lx_spot_market *market)
{
    (void)memset(market, 0, sizeof(*market));
    market->market_id[0] = id;
    (void)memcpy(market->base_asset, base_asset_id, 32U);
    (void)memcpy(market->quote_asset, quote_asset_id, 32U);
    (void)memcpy(market->administrator, administrator, 32U);
    market->tick_size = (lxp_u128){ 0U, 2U };
    market->lot_size = (lxp_u128){ 0U, 1U };
}

static lxp_result market_create(const lx_spot_market *market,
                                const char *did)
{
    uint8_t payload[LX_SPOT_MARKET_PAYLOAD_BYTES];
    if (lx_spot_market_encode(market, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_SPOT_MARKET_CREATE, did, payload, sizeof(payload));
}

static lxp_result market_state(uint32_t activity_type, const char *did)
{
    lx_spot_market_command command;
    uint8_t payload[LX_SPOT_MARKET_ID_PAYLOAD_BYTES];
    identifier(1U, command.market_id);
    if (lx_spot_market_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(activity_type, did, payload, sizeof(payload));
}

static lxp_result place_as(uint8_t order_id, const trader *owner,
                           const char *did, lx_spot_side side,
                           lx_spot_order_kind kind,
                           lx_spot_time_in_force tif, uint64_t price,
                           uint64_t quantity)
{
    lx_spot_order_command command;
    uint8_t payload[LX_SPOT_ORDER_PAYLOAD_BYTES];
    (void)memset(&command, 0, sizeof(command));
    identifier(1U, command.market_id);
    identifier(order_id, command.order_id);
    (void)memcpy(command.base_account_id, owner->base_id, 32U);
    (void)memcpy(command.quote_account_id, owner->main_id, 32U);
    command.side = side;
    command.kind = kind;
    command.time_in_force = tif;
    command.price = (lxp_u128){ 0U, price };
    command.quantity = (lxp_u128){ 0U, quantity };
    if (lx_spot_order_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_SPOT_ORDER_PLACE, did, payload, sizeof(payload));
}

static lxp_result limit(uint8_t order_id, const trader *owner,
                        lx_spot_side side, uint64_t price, uint64_t quantity)
{
    return place_as(order_id, owner, owner->did, side, LX_SPOT_ORDER_LIMIT,
                    LX_SPOT_TIF_GTC, price, quantity);
}

static lxp_result cancel(uint8_t order_id, const char *did)
{
    lx_spot_cancel_command command;
    uint8_t payload[LX_SPOT_CANCEL_PAYLOAD_BYTES];
    identifier(1U, command.market_id);
    identifier(order_id, command.order_id);
    if (lx_spot_cancel_command_encode(&command, payload) != LXP_OK)
        return LXP_FATAL_INVARIANT;
    return run(LX_SPOT_ORDER_CANCEL, did, payload, sizeof(payload));
}

static uint64_t balance_of(const uint8_t id[32])
{
    const lx_account *account = account_by_id(id);
    if (account == NULL || account->balance.hi != 0U) return UINT64_MAX;
    return account->balance.lo;
}

static uint64_t escrow_balance(const uint8_t asset_id[32])
{
    uint8_t market_id[32];
    uint8_t escrow_id[32];
    identifier(1U, market_id);
    if (lx_spot_escrow_id(market_id, asset_id, escrow_id) != LXP_OK)
        return UINT64_MAX;
    return balance_of(escrow_id);
}

static int supply_conserved(void)
{
    uint64_t base_total = 0U;
    uint64_t quote_total = 0U;
    size_t i;
    for (i = 0U; i < accounts.count; ++i) {
        const lx_account *account = &accounts.accounts[i];
        if (!account->has_asset || account->balance.hi != 0U) continue;
        if (memcmp(account->asset_id, base_asset_id, 32U) == 0)
            base_total += account->balance.lo;
        else if (memcmp(account->asset_id, quote_asset_id, 32U) == 0)
            quote_total += account->balance.lo;
    }
    return base_total == BASE_SUPPLY && quote_total == QUOTE_SUPPLY;
}

static int order_is(uint8_t order_id, uint64_t remaining, uint64_t escrowed)
{
    lx_spot_order order;
    uint8_t market_id[32];
    uint8_t id[32];
    identifier(1U, market_id);
    identifier(order_id, id);
    if (ctx_open() != 0) return 0;
    if (remaining == 0U)
        return lx_spot_order_lookup(&ctx, market_id, id, &order) ==
               LXP_ERR_UNKNOWN_FIELD;
    return lx_spot_order_lookup(&ctx, market_id, id, &order) == LXP_OK &&
           order.remaining.hi == 0U && order.remaining.lo == remaining &&
           order.escrowed.hi == 0U && order.escrowed.lo == escrowed;
}

static int balances_are(const trader *party, uint64_t quote, uint64_t base)
{
    return balance_of(party->main_id) == quote &&
           balance_of(party->base_id) == base;
}

static int market_create_case(void)
{
    lx_spot_market market;
    lx_spot_market stored;
    uint8_t market_id[32];
    const lx_account *base_escrow;
    const lx_account *quote_escrow;
    market_defaults(1U, &market);
    REQUIRE(market_create(&market, "did:key:bob") ==
            LXP_ERR_UNAUTHORIZED_DEBIT);
    REQUIRE(market_create(&market, admin_did) == LXP_OK);
    REQUIRE(market_create(&market, admin_did) ==
            LXP_ERR_MARKET_ALREADY_EXISTS);
    identifier(1U, market_id);
    REQUIRE(ctx_open() == 0);
    REQUIRE(lx_spot_market_lookup(&ctx, market_id, &stored) == LXP_OK);
    REQUIRE(!stored.halted && stored.tick_size.lo == 2U &&
            stored.lot_size.lo == 1U &&
            memcmp(stored.base_asset, base_asset_id, 32U) == 0 &&
            memcmp(stored.quote_asset, quote_asset_id, 32U) == 0 &&
            memcmp(stored.administrator, administrator, 32U) == 0);
    base_escrow = account_by_id(stored.base_escrow_id);
    quote_escrow = account_by_id(stored.quote_escrow_id);
    REQUIRE(base_escrow != NULL && quote_escrow != NULL);
    REQUIRE(base_escrow->kind == LX_ACCOUNT_MODULE_VALUE &&
            quote_escrow->kind == LX_ACCOUNT_MODULE_VALUE);
    REQUIRE(base_escrow->has_asset &&
            memcmp(base_escrow->asset_id, base_asset_id, 32U) == 0 &&
            quote_escrow->has_asset &&
            memcmp(quote_escrow->asset_id, quote_asset_id, 32U) == 0);
    REQUIRE(escrow_balance(base_asset_id) == 0U &&
            escrow_balance(quote_asset_id) == 0U);
    REQUIRE(supply_conserved());
    return 0;
}

/* Two asks at the best price rest in arrival order behind a worse one; a
 * crossing bid fills the earlier best-priced ask first, then the later one,
 * and never touches the worse price. Every fill settles base to the buyer
 * and quote to the seller in the same activity. */
static int priority_and_cross_case(void)
{
    REQUIRE(limit(1U, &alice, LX_SPOT_SIDE_ASK, 102U, 5U) == LXP_OK);
    REQUIRE(limit(2U, &carol, LX_SPOT_SIDE_ASK, 100U, 3U) == LXP_OK);
    REQUIRE(limit(3U, &alice, LX_SPOT_SIDE_ASK, 100U, 4U) == LXP_OK);
    REQUIRE(balances_are(&alice, 100000U, 991U));
    REQUIRE(balances_are(&carol, 100000U, 997U));
    REQUIRE(escrow_balance(base_asset_id) == 12U);
    REQUIRE(supply_conserved());
    REQUIRE(limit(4U, &bob, LX_SPOT_SIDE_BID, 102U, 6U) == LXP_OK);
    REQUIRE(order_is(2U, 0U, 0U));
    REQUIRE(order_is(3U, 1U, 1U));
    REQUIRE(order_is(1U, 5U, 5U));
    REQUIRE(order_is(4U, 0U, 0U));
    REQUIRE(balances_are(&bob, 99400U, 1006U));
    REQUIRE(balances_are(&carol, 100300U, 997U));
    REQUIRE(balances_are(&alice, 100300U, 991U));
    REQUIRE(escrow_balance(base_asset_id) == 6U);
    REQUIRE(escrow_balance(quote_asset_id) == 0U);
    REQUIRE(supply_conserved());
    return 0;
}

/* A bid larger than the book fills what it can and rests the rest with its
 * quote still in escrow; a later ask then trades against that escrow. */
static int partial_fill_case(void)
{
    REQUIRE(limit(5U, &bob, LX_SPOT_SIDE_BID, 102U, 10U) == LXP_OK);
    REQUIRE(order_is(3U, 0U, 0U));
    REQUIRE(order_is(1U, 0U, 0U));
    REQUIRE(order_is(5U, 4U, 408U));
    REQUIRE(balances_are(&bob, 98382U, 1012U));
    REQUIRE(balances_are(&alice, 100910U, 991U));
    REQUIRE(escrow_balance(base_asset_id) == 0U);
    REQUIRE(escrow_balance(quote_asset_id) == 408U);
    REQUIRE(supply_conserved());
    REQUIRE(place_as(6U, &carol, carol.did, LX_SPOT_SIDE_ASK,
                     LX_SPOT_ORDER_LIMIT, LX_SPOT_TIF_IOC, 100U, 1U) ==
            LXP_OK);
    REQUIRE(order_is(6U, 0U, 0U));
    REQUIRE(order_is(5U, 3U, 306U));
    REQUIRE(balances_are(&carol, 100402U, 996U));
    REQUIRE(balances_are(&bob, 98382U, 1013U));
    REQUIRE(escrow_balance(base_asset_id) == 0U);
    REQUIRE(escrow_balance(quote_asset_id) == 306U);
    REQUIRE(supply_conserved());
    return 0;
}

static int cancel_case(void)
{
    REQUIRE(cancel(5U, alice.did) == LXP_ERR_UNAUTHORIZED_DEBIT);
    REQUIRE(order_is(5U, 3U, 306U));
    REQUIRE(cancel(5U, bob.did) == LXP_OK);
    REQUIRE(order_is(5U, 0U, 0U));
    REQUIRE(balances_are(&bob, 98688U, 1013U));
    REQUIRE(escrow_balance(quote_asset_id) == 0U);
    REQUIRE(cancel(5U, bob.did) == LXP_ERR_UNKNOWN_FIELD);
    REQUIRE(limit(7U, &alice, LX_SPOT_SIDE_ASK, 110U, 4U) == LXP_OK);
    REQUIRE(balances_are(&alice, 100910U, 987U));
    REQUIRE(escrow_balance(base_asset_id) == 4U);
    REQUIRE(cancel(7U, alice.did) == LXP_OK);
    REQUIRE(balances_are(&alice, 100910U, 991U));
    REQUIRE(escrow_balance(base_asset_id) == 0U);
    REQUIRE(supply_conserved());
    return 0;
}

static int remainder_and_market_case(void)
{
    REQUIRE(place_as(8U, &alice, alice.did, LX_SPOT_SIDE_ASK,
                     LX_SPOT_ORDER_LIMIT, LX_SPOT_TIF_IOC, 200U, 2U) ==
            LXP_OK);
    REQUIRE(order_is(8U, 0U, 0U));
    REQUIRE(balances_are(&alice, 100910U, 991U));
    REQUIRE(place_as(9U, &alice, alice.did, LX_SPOT_SIDE_ASK,
                     LX_SPOT_ORDER_MARKET, LX_SPOT_TIF_IOC, 0U, 2U) ==
            LXP_ERR_AGREEMENT_STATE);
    REQUIRE(limit(10U, &carol, LX_SPOT_SIDE_ASK, 104U, 2U) == LXP_OK);
    REQUIRE(place_as(11U, &bob, bob.did, LX_SPOT_SIDE_BID,
                     LX_SPOT_ORDER_MARKET, LX_SPOT_TIF_IOC, 0U, 3U) ==
            LXP_OK);
    REQUIRE(order_is(10U, 0U, 0U));
    REQUIRE(order_is(11U, 0U, 0U));
    REQUIRE(balances_are(&bob, 98480U, 1015U));
    REQUIRE(balances_are(&carol, 100610U, 994U));
    REQUIRE(escrow_balance(base_asset_id) == 0U);
    REQUIRE(escrow_balance(quote_asset_id) == 0U);
    REQUIRE(supply_conserved());
    return 0;
}

static int refusal_case(void)
{
    REQUIRE(limit(12U, &alice, LX_SPOT_SIDE_ASK, 101U, 1U) ==
            LXP_ERR_NON_CANONICAL);
    REQUIRE(place_as(12U, &alice, bob.did, LX_SPOT_SIDE_ASK,
                     LX_SPOT_ORDER_LIMIT, LX_SPOT_TIF_GTC, 120U, 1U) ==
            LXP_ERR_UNAUTHORIZED_DEBIT);
    REQUIRE(limit(12U, &bob, LX_SPOT_SIDE_BID, 100U, 5000U) ==
            LXP_ERR_INSUFFICIENT_BALANCE);
    REQUIRE(limit(12U, &alice, LX_SPOT_SIDE_ASK, 120U, 1U) == LXP_OK);
    REQUIRE(limit(12U, &alice, LX_SPOT_SIDE_ASK, 120U, 1U) ==
            LXP_ERR_SEQUENCE_REUSED);
    REQUIRE(order_is(12U, 1U, 1U));
    REQUIRE(supply_conserved());
    return 0;
}

static int halt_case(void)
{
    REQUIRE(market_state(LX_SPOT_MARKET_HALT, bob.did) ==
            LXP_ERR_UNAUTHORIZED_DEBIT);
    REQUIRE(market_state(LX_SPOT_MARKET_RESUME, admin_did) ==
            LXP_ERR_AGREEMENT_STATE);
    REQUIRE(market_state(LX_SPOT_MARKET_HALT, admin_did) == LXP_OK);
    REQUIRE(market_state(LX_SPOT_MARKET_HALT, admin_did) ==
            LXP_ERR_AGREEMENT_STATE);
    REQUIRE(limit(13U, &bob, LX_SPOT_SIDE_BID, 120U, 1U) ==
            LXP_ERR_MARKET_HALTED);
    REQUIRE(order_is(12U, 1U, 1U));
    REQUIRE(order_is(13U, 0U, 0U));
    REQUIRE(balances_are(&bob, 98480U, 1015U));
    REQUIRE(cancel(12U, alice.did) == LXP_OK);
    REQUIRE(balances_are(&alice, 100910U, 991U));
    REQUIRE(market_state(LX_SPOT_MARKET_RESUME, admin_did) == LXP_OK);
    REQUIRE(limit(13U, &bob, LX_SPOT_SIDE_BID, 120U, 1U) == LXP_OK);
    REQUIRE(order_is(13U, 1U, 120U));
    REQUIRE(balances_are(&bob, 98360U, 1015U));
    REQUIRE(escrow_balance(quote_asset_id) == 120U);
    REQUIRE(escrow_balance(base_asset_id) == 0U);
    REQUIRE(supply_conserved());
    return 0;
}

int main(void)
{
    REQUIRE(fixture_init() == 0);
    REQUIRE(kernel_start() == 0);
    REQUIRE(supply_conserved());
    REQUIRE(market_create_case() == 0);
    REQUIRE(priority_and_cross_case() == 0);
    REQUIRE(partial_fill_case() == 0);
    REQUIRE(cancel_case() == 0);
    REQUIRE(remainder_and_market_case() == 0);
    REQUIRE(refusal_case() == 0);
    REQUIRE(halt_case() == 0);
    REQUIRE(lxp_state_store_destroy(&store) == LXP_OK);
    return 0;
}
