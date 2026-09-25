import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  PERPS_ACTIVITY_TYPES,
  SPOT_ACTIVITY_TYPES,
  TradingPayloadError,
  buildPerpsActivity,
  buildSpotActivity,
  decodePerpsActivity,
  decodeSpotActivity,
  encodePerpsActivity,
  encodeSpotActivity,
  type PerpsActivity,
  type SpotActivity,
} from "../src/index.js";

interface TradingVector {
  readonly name: string;
  readonly activity_type: number;
  readonly fields: Record<string, string | boolean | readonly string[]>;
  readonly bytes: string;
}

const fixture = JSON.parse(readFileSync(
  new URL("../../../../../tests/fixtures/trading-payloads/vectors.json", import.meta.url),
  "utf8",
)) as { readonly source: string; readonly vectors: readonly TradingVector[] };
assert.equal(fixture.source, "tests/modules/dump_trading_vectors.c");

const SMALL = new Set(["side", "kind", "time_in_force"]);

function camel(name: string): string {
  return name.replace(/_([a-z])/gu, (_, letter: string) => letter.toUpperCase());
}

function activity(vector: TradingVector): Record<string, unknown> {
  const prefix = vector.name.startsWith("perps_") ? "perps_" : "spot_";
  let name = vector.name.slice(prefix.length);
  if (name.startsWith("order_place")) {
    name = "order_place";
  }
  const out: Record<string, unknown> = { activity: name };
  for (const [key, value] of Object.entries(vector.fields)) {
    if (typeof value === "boolean" || Array.isArray(value)) {
      out[camel(key)] = value;
    } else if (/^[0-9a-f]{64}$/u.test(value as string)) {
      out[camel(key)] = value;
    } else if (SMALL.has(key)) {
      out[camel(key)] = Number(value);
    } else {
      out[camel(key)] = BigInt(value as string);
    }
  }
  return out;
}

function bytes(value: string): Uint8Array {
  return Uint8Array.from(Buffer.from(value, "hex"));
}

function hex(value: Uint8Array): string {
  return Buffer.from(value).toString("hex");
}

function refuses(run: () => unknown, code: TradingPayloadError["code"]): void {
  assert.throws(run, (error: unknown) => error instanceof TradingPayloadError && error.code === code);
}

const perps = fixture.vectors.filter((vector) => vector.name.startsWith("perps_"));
const spot = fixture.vectors.filter((vector) => vector.name.startsWith("spot_"));
assert.equal(perps.length, Object.keys(PERPS_ACTIVITY_TYPES).length);
assert.equal(spot.length, 6);
assert.deepEqual(
  new Set(spot.map((vector) => vector.activity_type)),
  new Set(Object.values(SPOT_ACTIVITY_TYPES)),
);

for (const vector of perps) {
  const typed = activity(vector) as PerpsActivity;
  const built = buildPerpsActivity(typed);
  assert.equal(built.activityType, vector.activity_type, vector.name);
  assert.equal(hex(built.payload), vector.bytes, vector.name);
  assert.deepEqual(decodePerpsActivity(vector.activity_type, bytes(vector.bytes)), typed, vector.name);
}

for (const vector of spot) {
  const typed = activity(vector) as SpotActivity;
  const built = buildSpotActivity(typed);
  assert.equal(built.activityType, vector.activity_type, vector.name);
  assert.equal(hex(built.payload), vector.bytes, vector.name);
  assert.deepEqual(decodeSpotActivity(vector.activity_type, bytes(vector.bytes)), typed, vector.name);
}

const vector = (name: string): TradingVector => {
  const found = fixture.vectors.find((candidate) => candidate.name === name);
  assert.ok(found, name);
  return found;
};

const order = vector("perps_order_place");
const orderBytes = bytes(order.bytes);
refuses(() => decodePerpsActivity(order.activity_type, orderBytes.subarray(1)), "length");
refuses(() => decodePerpsActivity(0x0006000c, orderBytes), "unknown_activity");
refuses(() => decodePerpsActivity(0x000a0002, orderBytes), "unknown_activity");
const badSide = orderBytes.slice();
badSide[96] = 3;
refuses(() => decodePerpsActivity(order.activity_type, badSide), "non_canonical");
const zeroQuantity = orderBytes.slice();
zeroQuantity.fill(0, 113);
refuses(() => decodePerpsActivity(order.activity_type, zeroQuantity), "non_canonical");

const market = vector("perps_market_create");
const marketBytes = bytes(market.bytes);
const padding = marketBytes.slice();
padding[32 * 7 + 16 * 4 + 4 * 6 + 8 * 2 + 16 * 2 + 1 + 32 * 7] = 1;
refuses(() => decodePerpsActivity(market.activity_type, padding), "non_canonical");
const typedMarket = decodePerpsActivity(market.activity_type, marketBytes);
assert.equal(typedMarket.activity, "market_create");
refuses(
  () => encodePerpsActivity({ ...typedMarket, initialMarginRatioBps: typedMarket.maintenanceMarginRatioBps }),
  "parameter_bounds",
);
const halted = marketBytes.slice();
halted[621] = 2;
refuses(() => decodePerpsActivity(market.activity_type, halted), "non_canonical");

const adl = activity(vector("perps_adl")) as Extract<PerpsActivity, { activity: "adl" }>;
assert.ok(adl.positionIds.length > 1);
refuses(() => encodePerpsActivity({ ...adl, positionIds: [...adl.positionIds].reverse() }), "unsorted_sequence");

const open = activity(vector("perps_position_open")) as Extract<PerpsActivity, { activity: "position_open" }>;
refuses(() => encodePerpsActivity({ ...open, entryNotional: 1n }), "non_canonical");

const limit = activity(vector("spot_order_place_limit")) as Extract<SpotActivity, { activity: "order_place" }>;
refuses(() => encodeSpotActivity({ ...limit, price: 0n }), "non_canonical");
refuses(() => encodeSpotActivity({ ...limit, quoteAccountId: limit.baseAccountId }), "non_canonical");
refuses(() => encodeSpotActivity({ ...limit, kind: 2 }), "non_canonical");
const spotBytes = bytes(vector("spot_order_place_limit").bytes);
const badKind = spotBytes.slice();
badKind[129] = 3;
refuses(() => decodeSpotActivity(SPOT_ACTIVITY_TYPES.order_place, badKind), "non_canonical");
refuses(() => decodeSpotActivity(0x000a0006, spotBytes), "unknown_activity");
refuses(() => decodeSpotActivity(SPOT_ACTIVITY_TYPES.market_halt, new Uint8Array(32)), "non_canonical");
