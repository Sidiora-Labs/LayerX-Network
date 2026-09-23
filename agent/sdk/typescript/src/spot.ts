/**
 * Typed builders for every LayerX spot module activity (module 10). Each
 * encoder emits the exact bytes the kernel codec (`src/modules/spot`) accepts
 * and each decoder refuses what the kernel refuses.
 */

import {
  TradingPayloadError,
  TradingReader,
  TradingWriter,
  type EncodedActivity,
  type TradeSide,
  type TradingId,
} from "./perps.js";

export const SPOT_MODULE_ID = 10;

export const SPOT_ACTIVITY_TYPES = {
  market_create: 0x000a0001,
  order_place: 0x000a0002,
  order_cancel: 0x000a0003,
  market_halt: 0x000a0004,
  market_resume: 0x000a0005,
} as const;

/** 1 is a limit order, 2 a market order. */
export type SpotOrderKind = 1 | 2;

/** 1 is good-til-cancelled, 2 immediate-or-cancel. */
export type SpotTimeInForce = 1 | 2;

export type SpotActivity =
  | {
      readonly activity: "market_create";
      readonly marketId: TradingId;
      readonly baseAsset: TradingId;
      readonly quoteAsset: TradingId;
      readonly tickSize: bigint;
      readonly lotSize: bigint;
      readonly administrator: TradingId;
    }
  | {
      readonly activity: "order_place";
      readonly marketId: TradingId;
      readonly orderId: TradingId;
      readonly baseAccountId: TradingId;
      readonly quoteAccountId: TradingId;
      readonly side: TradeSide;
      readonly kind: SpotOrderKind;
      readonly timeInForce: SpotTimeInForce;
      readonly price: bigint;
      readonly quantity: bigint;
    }
  | { readonly activity: "order_cancel"; readonly marketId: TradingId; readonly orderId: TradingId }
  | { readonly activity: "market_halt"; readonly marketId: TradingId }
  | { readonly activity: "market_resume"; readonly marketId: TradingId };

function oneOrTwo(value: number, label: string): 1 | 2 {
  if (value !== 1 && value !== 2) {
    throw new TradingPayloadError("non_canonical", label);
  }
  return value;
}

/** Returns the packed activity type of a spot activity. */
export function spotActivityType(activity: SpotActivity): number {
  return SPOT_ACTIVITY_TYPES[activity.activity];
}

/** Encodes the exact kernel payload bytes of a spot activity. */
export function encodeSpotActivity(activity: SpotActivity): Uint8Array {
  const writer = new TradingWriter();
  switch (activity.activity) {
    case "market_create":
      if (
        activity.tickSize === 0n ||
        activity.lotSize === 0n ||
        activity.baseAsset === activity.quoteAsset ||
        /^0+$/u.test(activity.administrator)
      ) {
        throw new TradingPayloadError("non_canonical", "market");
      }
      writer
        .id(activity.marketId)
        .id(activity.baseAsset)
        .id(activity.quoteAsset)
        .uint(activity.tickSize, 16)
        .uint(activity.lotSize, 16)
        .id(activity.administrator);
      break;
    case "order_place": {
      const priced =
        activity.kind === 1 ? activity.price !== 0n : activity.price === 0n && activity.timeInForce === 2;
      if (!priced || activity.quantity === 0n || activity.baseAccountId === activity.quoteAccountId) {
        throw new TradingPayloadError("non_canonical", "order");
      }
      writer
        .id(activity.marketId)
        .id(activity.orderId)
        .id(activity.baseAccountId)
        .id(activity.quoteAccountId)
        .byte(oneOrTwo(activity.side, "side"))
        .byte(oneOrTwo(activity.kind, "kind"))
        .byte(oneOrTwo(activity.timeInForce, "time in force"))
        .uint(activity.price, 16)
        .uint(activity.quantity, 16);
      break;
    }
    case "order_cancel":
      writer.id(activity.marketId).id(activity.orderId);
      break;
    case "market_halt":
    case "market_resume":
      writer.id(activity.marketId);
      break;
  }
  return writer.bytes();
}

/** Builds the activity type and payload a spot activity submits. */
export function buildSpotActivity(activity: SpotActivity): EncodedActivity {
  return { activityType: spotActivityType(activity), payload: encodeSpotActivity(activity) };
}

const SPOT_LENGTHS: Readonly<Record<number, number>> = { 1: 160, 2: 163, 3: 64, 4: 32, 5: 32 };

/** Decodes and validates kernel payload bytes of a spot activity. */
export function decodeSpotActivity(activityType: number, payload: Uint8Array): SpotActivity {
  const ordinal = activityType & 0xffff;
  const expected = activityType >>> 16 === SPOT_MODULE_ID ? SPOT_LENGTHS[ordinal] : undefined;
  if (expected === undefined) {
    throw new TradingPayloadError("unknown_activity", activityType.toString(16));
  }
  if (payload.length !== expected) {
    throw new TradingPayloadError("length");
  }
  const reader = new TradingReader(payload);
  let decoded: SpotActivity;
  switch (ordinal) {
    case 1:
      decoded = {
        activity: "market_create",
        marketId: reader.id(),
        baseAsset: reader.id(),
        quoteAsset: reader.id(),
        tickSize: reader.uint(16),
        lotSize: reader.uint(16),
        administrator: reader.id(),
      };
      break;
    case 2:
      decoded = {
        activity: "order_place",
        marketId: reader.id(),
        orderId: reader.id(),
        baseAccountId: reader.id(),
        quoteAccountId: reader.id(),
        side: oneOrTwo(reader.byte(), "side"),
        kind: oneOrTwo(reader.byte(), "kind"),
        timeInForce: oneOrTwo(reader.byte(), "time in force"),
        price: reader.uint(16),
        quantity: reader.uint(16),
      };
      break;
    case 3:
      decoded = { activity: "order_cancel", marketId: reader.id(), orderId: reader.id() };
      break;
    case 4:
      decoded = { activity: "market_halt", marketId: reader.id() };
      break;
    default:
      decoded = { activity: "market_resume", marketId: reader.id() };
  }
  const encoded = encodeSpotActivity(decoded);
  if (encoded.length !== payload.length || encoded.some((byte, index) => byte !== payload[index])) {
    throw new TradingPayloadError("non_canonical");
  }
  return decoded;
}
