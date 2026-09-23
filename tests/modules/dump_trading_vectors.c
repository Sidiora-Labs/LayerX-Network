/* Prints the canonical perps and spot activity payload vectors as JSON.
 *
 * Every payload is produced by the kernel's own codec from the fields printed
 * beside it, so the SDK and layerx-types codecs are checked against the C
 * encoding byte for byte. Not part of any Makefile target; regenerate with
 *   make build/liblayerx.a programs-build
 *   cc -std=c17 -Iinclude tests/modules/dump_trading_vectors.c \
 *      build/liblayerx.a programs/target/debug/liblayerx_programs_sandbox.a \
 *      build/liblayerx.a -lcrypto -pthread -ldl -lm -o build/dump_trading_vectors
 *   build/dump_trading_vectors > tests/fixtures/trading-payloads/vectors.json
 */
#include "layerx/lx_perps.h"
#include "layerx/lx_spot.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static int first_vector = 1;

static void fail(const char *what)
{
    fprintf(stderr, "vector %s refused by the codec\n", what);
    exit(1);
}

static void fill(uint8_t id[32], uint8_t value)
{
    (void)memset(id, value, 32U);
}

static lxp_u128 u128(uint64_t hi, uint64_t lo)
{
    lxp_u128 value;
    value.hi = hi;
    value.lo = lo;
    return value;
}

static void print_hex(const uint8_t *bytes, size_t length)
{
    size_t i;
    putchar('"');
    for (i = 0U; i < length; ++i) printf("%02x", bytes[i]);
    putchar('"');
}

static void print_u128(lxp_u128 value)
{
    uint32_t limbs[4];
    char digits[48];
    size_t count = 0U;
    size_t i;
    limbs[0] = (uint32_t)(value.hi >> 32U);
    limbs[1] = (uint32_t)value.hi;
    limbs[2] = (uint32_t)(value.lo >> 32U);
    limbs[3] = (uint32_t)value.lo;
    do {
        uint64_t remainder = 0U;
        int nonzero = 0;
        for (i = 0U; i < 4U; ++i) {
            uint64_t current = (remainder << 32U) | limbs[i];
            limbs[i] = (uint32_t)(current / 10U);
            remainder = current % 10U;
            if (limbs[i] != 0U) nonzero = 1;
        }
        digits[count++] = (char)('0' + (int)remainder);
        if (!nonzero) break;
    } while (1);
    putchar('"');
    while (count > 0U) putchar(digits[--count]);
    putchar('"');
}

static void begin(const char *name, uint32_t activity_type)
{
    printf("%s\n    {\"name\": \"%s\", \"activity_type\": %u, \"fields\": {",
           first_vector ? "" : ",", name, activity_type);
    first_vector = 0;
}

static void field_id(const char *name, const uint8_t id[32], int last)
{
    printf("\"%s\": ", name);
    print_hex(id, 32U);
    if (!last) printf(", ");
}

static void field_u128(const char *name, lxp_u128 value, int last)
{
    printf("\"%s\": ", name);
    print_u128(value);
    if (!last) printf(", ");
}

static void field_uint(const char *name, uint64_t value, int last)
{
    printf("\"%s\": \"%llu\"%s", name, (unsigned long long)value,
           last ? "" : ", ");
}

static void field_bool(const char *name, bool value, int last)
{
    printf("\"%s\": %s%s", name, value ? "true" : "false", last ? "" : ", ");
}

static void end(const uint8_t *bytes, size_t length)
{
    printf("}, \"bytes\": ");
    print_hex(bytes, length);
    printf("}");
}

static void perps(void)
{
    lx_perps_market market;
    lx_perps_halt_command halt;
    lx_perps_oracle_command oracle;
    lx_perps_order_command order;
    lx_perps_cancel_command cancel;
    lx_perps_open_command open;
    lx_perps_increase_command increase;
    lx_perps_close_command close;
    lx_perps_tick_command tick;
    lx_perps_liquidate_command liquidate;
    lx_perps_adl_command adl;
    uint8_t bytes[LX_PERPS_ADL_PAYLOAD_MAX_BYTES];
    size_t length = 0U;
    size_t i;

    (void)memset(&market, 0, sizeof(market));
    fill(market.market_id, 0x11);
    fill(market.quote_asset, 0x12);
    fill(market.administrator, 0x13);
    fill(market.liquidity_account_id, 0x14);
    fill(market.long_funding_account_id, 0x15);
    fill(market.short_funding_account_id, 0x16);
    fill(market.insurance_account_id, 0x17);
    market.contract_size = u128(0U, 1U);
    market.tick_size = u128(0U, 10U);
    market.lot_size = u128(0U, 1000U);
    market.price_scale = u128(0U, 100000000U);
    market.initial_margin_ratio_bps = 1000U;
    market.maintenance_margin_ratio_bps = 500U;
    market.liquidation_fee_bps = 100U;
    market.liquidator_share_bps = 5000U;
    market.maximum_funding_rate_bps = 75U;
    market.maximum_deviation_basis_points = 250U;
    market.funding_interval_ms = 3600000U;
    market.maximum_oracle_staleness_ms = 60000U;
    market.minimum_price = u128(0U, 1U);
    market.maximum_price = u128(0x0102030405060708U, 0x090a0b0c0d0e0f10U);
    fill(market.permitted_oracle_keys[0], 0x21);
    fill(market.permitted_oracle_keys[1], 0x22);
    market.permitted_oracle_key_count = 2U;
    market.parameter_version = 1U;
    market.halted = false;
    if (lx_perps_market_encode(&market, bytes) != LXP_OK) fail("market");
    begin("perps_market_create", LX_PERPS_MARKET_CREATE);
    field_id("market_id", market.market_id, 0);
    field_id("quote_asset", market.quote_asset, 0);
    field_id("administrator", market.administrator, 0);
    field_id("liquidity_account_id", market.liquidity_account_id, 0);
    field_id("long_funding_account_id", market.long_funding_account_id, 0);
    field_id("short_funding_account_id", market.short_funding_account_id, 0);
    field_id("insurance_account_id", market.insurance_account_id, 0);
    field_u128("contract_size", market.contract_size, 0);
    field_u128("tick_size", market.tick_size, 0);
    field_u128("lot_size", market.lot_size, 0);
    field_u128("price_scale", market.price_scale, 0);
    field_uint("initial_margin_ratio_bps", market.initial_margin_ratio_bps, 0);
    field_uint("maintenance_margin_ratio_bps",
               market.maintenance_margin_ratio_bps, 0);
    field_uint("liquidation_fee_bps", market.liquidation_fee_bps, 0);
    field_uint("liquidator_share_bps", market.liquidator_share_bps, 0);
    field_uint("maximum_funding_rate_bps", market.maximum_funding_rate_bps, 0);
    field_uint("maximum_deviation_basis_points",
               market.maximum_deviation_basis_points, 0);
    field_uint("funding_interval_ms", market.funding_interval_ms, 0);
    field_uint("maximum_oracle_staleness_ms",
               market.maximum_oracle_staleness_ms, 0);
    field_u128("minimum_price", market.minimum_price, 0);
    field_u128("maximum_price", market.maximum_price, 0);
    printf("\"permitted_oracle_keys\": [");
    for (i = 0U; i < market.permitted_oracle_key_count; ++i) {
        if (i != 0U) printf(", ");
        print_hex(market.permitted_oracle_keys[i], 32U);
    }
    printf("], ");
    field_uint("parameter_version", market.parameter_version, 0);
    field_bool("halted", market.halted, 1);
    end(bytes, LX_PERPS_MARKET_BYTES);

    (void)memset(&halt, 0, sizeof(halt));
    fill(halt.market_id, 0x11);
    halt.halted = true;
    if (lx_perps_halt_command_encode(&halt, bytes) != LXP_OK) fail("halt");
    begin("perps_market_halt", LX_PERPS_MARKET_HALT);
    field_id("market_id", halt.market_id, 0);
    field_bool("halted", halt.halted, 1);
    end(bytes, LX_PERPS_HALT_PAYLOAD_BYTES);

    (void)memset(&oracle, 0, sizeof(oracle));
    fill(oracle.market_id, 0x11);
    oracle.observation_sequence = 7U;
    oracle.price = u128(0U, 123456789U);
    oracle.observed_at = 1700000000000U;
    oracle.source_identifier = 42U;
    if (lx_perps_oracle_command_encode(&oracle, bytes) != LXP_OK)
        fail("oracle");
    begin("perps_oracle_push", LX_PERPS_ORACLE_PUSH);
    field_id("market_id", oracle.market_id, 0);
    field_uint("observation_sequence", oracle.observation_sequence, 0);
    field_u128("price", oracle.price, 0);
    field_uint("observed_at", oracle.observed_at, 0);
    field_uint("source_identifier", oracle.source_identifier, 1);
    end(bytes, LX_PERPS_ORACLE_PAYLOAD_BYTES);

    (void)memset(&order, 0, sizeof(order));
    fill(order.market_id, 0x11);
    fill(order.order_id, 0x31);
    fill(order.owner_account_id, 0x32);
    order.side = LX_PERPS_SIDE_SELL;
    order.price = u128(1U, 2U);
    order.quantity = u128(0U, 5000U);
    if (lx_perps_order_command_encode(&order, bytes) != LXP_OK) fail("order");
    begin("perps_order_place", LX_PERPS_ORDER_PLACE);
    field_id("market_id", order.market_id, 0);
    field_id("order_id", order.order_id, 0);
    field_id("owner_account_id", order.owner_account_id, 0);
    field_uint("side", (uint64_t)order.side, 0);
    field_u128("price", order.price, 0);
    field_u128("quantity", order.quantity, 1);
    end(bytes, LX_PERPS_ORDER_PAYLOAD_BYTES);

    (void)memset(&cancel, 0, sizeof(cancel));
    fill(cancel.market_id, 0x11);
    fill(cancel.order_id, 0x31);
    if (lx_perps_cancel_command_encode(&cancel, bytes) != LXP_OK)
        fail("cancel");
    begin("perps_order_cancel", LX_PERPS_ORDER_CANCEL);
    field_id("market_id", cancel.market_id, 0);
    field_id("order_id", cancel.order_id, 1);
    end(bytes, LX_PERPS_CANCEL_PAYLOAD_BYTES);

    (void)memset(&open, 0, sizeof(open));
    fill(open.market_id, 0x11);
    fill(open.position_id, 0x41);
    fill(open.margin_account_id, 0x42);
    open.side = LX_PERPS_SIDE_BUY;
    open.size = u128(0U, 1000U);
    open.entry_notional = u128(0U, 0U);
    open.margin_amount = u128(0U, 250000U);
    if (lx_perps_open_command_encode(&open, bytes) != LXP_OK) fail("open");
    begin("perps_position_open", LX_PERPS_POSITION_OPEN);
    field_id("market_id", open.market_id, 0);
    field_id("position_id", open.position_id, 0);
    field_id("margin_account_id", open.margin_account_id, 0);
    field_uint("side", (uint64_t)open.side, 0);
    field_u128("size", open.size, 0);
    field_u128("entry_notional", open.entry_notional, 0);
    field_u128("margin_amount", open.margin_amount, 1);
    end(bytes, LX_PERPS_OPEN_PAYLOAD_BYTES);

    (void)memset(&increase, 0, sizeof(increase));
    fill(increase.market_id, 0x11);
    fill(increase.position_id, 0x41);
    increase.size_delta = u128(0U, 500U);
    increase.notional_delta = u128(0U, 0U);
    increase.margin_amount = u128(0U, 1000U);
    if (lx_perps_increase_command_encode(&increase, bytes) != LXP_OK)
        fail("increase");
    begin("perps_position_increase", LX_PERPS_POSITION_INCREASE);
    field_id("market_id", increase.market_id, 0);
    field_id("position_id", increase.position_id, 0);
    field_u128("size_delta", increase.size_delta, 0);
    field_u128("notional_delta", increase.notional_delta, 0);
    field_u128("margin_amount", increase.margin_amount, 1);
    end(bytes, LX_PERPS_INCREASE_PAYLOAD_BYTES);

    (void)memset(&close, 0, sizeof(close));
    fill(close.market_id, 0x11);
    fill(close.position_id, 0x41);
    if (lx_perps_close_command_encode(&close, bytes) != LXP_OK) fail("close");
    begin("perps_position_close", LX_PERPS_POSITION_CLOSE);
    field_id("market_id", close.market_id, 0);
    field_id("position_id", close.position_id, 1);
    end(bytes, LX_PERPS_CLOSE_PAYLOAD_BYTES);

    (void)memset(&tick, 0, sizeof(tick));
    fill(tick.market_id, 0x11);
    if (lx_perps_tick_command_encode(&tick, bytes) != LXP_OK) fail("tick");
    begin("perps_funding_tick", LX_PERPS_FUNDING_TICK);
    field_id("market_id", tick.market_id, 1);
    end(bytes, LX_PERPS_TICK_PAYLOAD_BYTES);

    (void)memset(&liquidate, 0, sizeof(liquidate));
    fill(liquidate.market_id, 0x11);
    fill(liquidate.position_id, 0x41);
    fill(liquidate.liquidator_account_id, 0x51);
    if (lx_perps_liquidate_command_encode(&liquidate, bytes) != LXP_OK)
        fail("liquidate");
    begin("perps_liquidate", LX_PERPS_LIQUIDATE);
    field_id("market_id", liquidate.market_id, 0);
    field_id("position_id", liquidate.position_id, 0);
    field_id("liquidator_account_id", liquidate.liquidator_account_id, 1);
    end(bytes, LX_PERPS_LIQUIDATE_PAYLOAD_BYTES);

    (void)memset(&adl, 0, sizeof(adl));
    fill(adl.market_id, 0x11);
    fill(adl.position_ids[0], 0x61);
    fill(adl.position_ids[1], 0x62);
    fill(adl.position_ids[2], 0x63);
    adl.position_count = 3U;
    if (lx_perps_adl_command_encode(&adl, bytes, sizeof(bytes), &length) !=
        LXP_OK)
        fail("adl");
    begin("perps_adl", LX_PERPS_ADL);
    field_id("market_id", adl.market_id, 0);
    printf("\"position_ids\": [");
    for (i = 0U; i < adl.position_count; ++i) {
        if (i != 0U) printf(", ");
        print_hex(adl.position_ids[i], 32U);
    }
    printf("]");
    end(bytes, length);
}

static void spot_order(const char *name, const lx_spot_order_command *order)
{
    uint8_t bytes[LX_SPOT_ORDER_PAYLOAD_BYTES];
    if (lx_spot_order_command_encode(order, bytes) != LXP_OK) fail(name);
    begin(name, LX_SPOT_ORDER_PLACE);
    field_id("market_id", order->market_id, 0);
    field_id("order_id", order->order_id, 0);
    field_id("base_account_id", order->base_account_id, 0);
    field_id("quote_account_id", order->quote_account_id, 0);
    field_uint("side", (uint64_t)order->side, 0);
    field_uint("kind", (uint64_t)order->kind, 0);
    field_uint("time_in_force", (uint64_t)order->time_in_force, 0);
    field_u128("price", order->price, 0);
    field_u128("quantity", order->quantity, 1);
    end(bytes, sizeof(bytes));
}

static void spot(void)
{
    lx_spot_market market;
    lx_spot_order_command order;
    lx_spot_cancel_command cancel;
    lx_spot_market_command command;
    uint8_t bytes[LX_SPOT_ORDER_PAYLOAD_BYTES];

    (void)memset(&market, 0, sizeof(market));
    fill(market.market_id, 0x71);
    fill(market.base_asset, 0x72);
    fill(market.quote_asset, 0x73);
    fill(market.administrator, 0x74);
    market.tick_size = u128(0U, 5U);
    market.lot_size = u128(0U, 100U);
    if (lx_spot_market_encode(&market, bytes) != LXP_OK) fail("spot market");
    begin("spot_market_create", LX_SPOT_MARKET_CREATE);
    field_id("market_id", market.market_id, 0);
    field_id("base_asset", market.base_asset, 0);
    field_id("quote_asset", market.quote_asset, 0);
    field_u128("tick_size", market.tick_size, 0);
    field_u128("lot_size", market.lot_size, 0);
    field_id("administrator", market.administrator, 1);
    end(bytes, LX_SPOT_MARKET_PAYLOAD_BYTES);

    (void)memset(&order, 0, sizeof(order));
    fill(order.market_id, 0x71);
    fill(order.order_id, 0x81);
    fill(order.base_account_id, 0x82);
    fill(order.quote_account_id, 0x83);
    order.side = LX_SPOT_SIDE_BID;
    order.kind = LX_SPOT_ORDER_LIMIT;
    order.time_in_force = LX_SPOT_TIF_GTC;
    order.price = u128(0U, 250U);
    order.quantity = u128(0x0102030405060708U, 0x090a0b0c0d0e0f10U);
    spot_order("spot_order_place_limit", &order);

    fill(order.order_id, 0x84);
    order.side = LX_SPOT_SIDE_ASK;
    order.kind = LX_SPOT_ORDER_MARKET;
    order.time_in_force = LX_SPOT_TIF_IOC;
    order.price = u128(0U, 0U);
    order.quantity = u128(0U, 400U);
    spot_order("spot_order_place_market", &order);

    (void)memset(&cancel, 0, sizeof(cancel));
    fill(cancel.market_id, 0x71);
    fill(cancel.order_id, 0x81);
    if (lx_spot_cancel_command_encode(&cancel, bytes) != LXP_OK)
        fail("spot cancel");
    begin("spot_order_cancel", LX_SPOT_ORDER_CANCEL);
    field_id("market_id", cancel.market_id, 0);
    field_id("order_id", cancel.order_id, 1);
    end(bytes, LX_SPOT_CANCEL_PAYLOAD_BYTES);

    (void)memset(&command, 0, sizeof(command));
    fill(command.market_id, 0x71);
    if (lx_spot_market_command_encode(&command, bytes) != LXP_OK)
        fail("spot halt");
    begin("spot_market_halt", LX_SPOT_MARKET_HALT);
    field_id("market_id", command.market_id, 1);
    end(bytes, LX_SPOT_MARKET_ID_PAYLOAD_BYTES);
    begin("spot_market_resume", LX_SPOT_MARKET_RESUME);
    field_id("market_id", command.market_id, 1);
    end(bytes, LX_SPOT_MARKET_ID_PAYLOAD_BYTES);
}

int main(void)
{
    printf("{\n  \"source\": \"tests/modules/dump_trading_vectors.c\",\n"
           "  \"vectors\": [");
    perps();
    spot();
    printf("\n  ]\n}\n");
    return 0;
}
