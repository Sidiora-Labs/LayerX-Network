/**
 * Typed builders for every LayerX perps module activity. Each encoder emits
 * the exact bytes the kernel codec (`src/modules/perps`) accepts and each
 * decoder refuses what the kernel refuses.
 */

const ID = /^[0-9a-f]{64}$/u;
const MAX_ORACLE_KEYS = 8;
const ADL_CAPACITY = 128;
const MARKET_BYTES = 622;
const BASIS_POINTS_ONE = 10000n;

export const PERPS_MODULE_ID = 6;

export const PERPS_ACTIVITY_TYPES = {
  market_create: 0x00060001,
  market_halt: 0x00060002,
  oracle_push: 0x00060003,
  order_place: 0x00060004,
  order_cancel: 0x00060005,
  position_open: 0x00060006,
  position_increase: 0x00060007,
  position_close: 0x00060008,
  funding_tick: 0x00060009,
  liquidate: 0x0006000a,
  adl: 0x0006000b,
} as const;

export type PerpsActivityName = keyof typeof PERPS_ACTIVITY_TYPES;

export type TradingPayloadErrorCode =
  | "unknown_activity"
  | "length"
  | "non_canonical"
  | "parameter_bounds"
  | "unsorted_sequence";

export class TradingPayloadError extends Error {
  readonly code: TradingPayloadErrorCode;

  constructor(code: TradingPayloadErrorCode, detail?: string) {
    super(detail === undefined ? code : `${code}: ${detail}`);
    this.name = "TradingPayloadError";
    this.code = code;
  }
}

/** 1 buys (perps) or bids (spot); 2 sells or asks. */
export type TradeSide = 1 | 2;

/** A 32-byte identifier as 64 lowercase hex digits. */
export type TradingId = string;

export interface PerpsMarket {
  readonly marketId: TradingId;
  readonly quoteAsset: TradingId;
  readonly administrator: TradingId;
  readonly liquidityAccountId: TradingId;
  readonly longFundingAccountId: TradingId;
  readonly shortFundingAccountId: TradingId;
  readonly insuranceAccountId: TradingId;
  readonly contractSize: bigint;
  readonly tickSize: bigint;
  readonly lotSize: bigint;
  readonly priceScale: bigint;
  readonly initialMarginRatioBps: bigint;
  readonly maintenanceMarginRatioBps: bigint;
  readonly liquidationFeeBps: bigint;
  readonly liquidatorShareBps: bigint;
  readonly maximumFundingRateBps: bigint;
  readonly maximumDeviationBasisPoints: bigint;
  readonly fundingIntervalMs: bigint;
  readonly maximumOracleStalenessMs: bigint;
  readonly minimumPrice: bigint;
  readonly maximumPrice: bigint;
  readonly permittedOracleKeys: readonly TradingId[];
  readonly parameterVersion: bigint;
  readonly halted: boolean;
}

export type PerpsActivity =
  | ({ readonly activity: "market_create" } & PerpsMarket)
  | { readonly activity: "market_halt"; readonly marketId: TradingId; readonly halted: boolean }
  | {
      readonly activity: "oracle_push";
      readonly marketId: TradingId;
      readonly observationSequence: bigint;
      readonly price: bigint;
      readonly observedAt: bigint;
      readonly sourceIdentifier: bigint;
    }
  | {
      readonly activity: "order_place";
      readonly marketId: TradingId;
      readonly orderId: TradingId;
      readonly ownerAccountId: TradingId;
      readonly side: TradeSide;
      readonly price: bigint;
      readonly quantity: bigint;
    }
  | { readonly activity: "order_cancel"; readonly marketId: TradingId; readonly orderId: TradingId }
  | {
      readonly activity: "position_open";
      readonly marketId: TradingId;
      readonly positionId: TradingId;
      readonly marginAccountId: TradingId;
      readonly side: TradeSide;
      readonly size: bigint;
      readonly entryNotional: bigint;
      readonly marginAmount: bigint;
    }
  | {
      readonly activity: "position_increase";
      readonly marketId: TradingId;
      readonly positionId: TradingId;
      readonly sizeDelta: bigint;
      readonly notionalDelta: bigint;
      readonly marginAmount: bigint;
    }
  | { readonly activity: "position_close"; readonly marketId: TradingId; readonly positionId: TradingId }
  | { readonly activity: "funding_tick"; readonly marketId: TradingId }
  | {
      readonly activity: "liquidate";
      readonly marketId: TradingId;
      readonly positionId: TradingId;
      readonly liquidatorAccountId: TradingId;
    }
  | { readonly activity: "adl"; readonly marketId: TradingId; readonly positionIds: readonly TradingId[] };

export interface EncodedActivity {
  readonly activityType: number;
  readonly payload: Uint8Array;
}

/** Big-endian writer shared by the perps and spot codecs. */
export class TradingWriter {
  private readonly parts: number[] = [];

  id(value: TradingId, nonzero = true): this {
    if (!ID.test(value)) {
      throw new TradingPayloadError("non_canonical", "identifier");
    }
    if (nonzero && /^0+$/u.test(value)) {
      throw new TradingPayloadError("non_canonical", "zero identifier");
    }
    for (let index = 0; index < 64; index += 2) {
      this.parts.push(Number.parseInt(value.slice(index, index + 2), 16));
    }
    return this;
  }

  uint(value: bigint, bytes: number): this {
    const limit = (1n << BigInt(bytes * 8)) - 1n;
    if (value < 0n || value > limit) {
      throw new TradingPayloadError("non_canonical", "integer width");
    }
    for (let index = bytes - 1; index >= 0; index -= 1) {
      this.parts.push(Number((value >> BigInt(index * 8)) & 0xffn));
    }
    return this;
  }

  byte(value: number): this {
    this.parts.push(value);
    return this;
  }

  zeros(count: number): this {
    for (let index = 0; index < count; index += 1) {
      this.parts.push(0);
    }
    return this;
  }

  bytes(): Uint8Array {
    return Uint8Array.from(this.parts);
  }
}

/** Big-endian reader shared by the perps and spot codecs. */
export class TradingReader {
  private offset = 0;

  constructor(private readonly source: Uint8Array) {}

  private take(count: number): Uint8Array {
    if (this.offset + count > this.source.length) {
      throw new TradingPayloadError("length");
    }
    const out = this.source.subarray(this.offset, this.offset + count);
    this.offset += count;
    return out;
  }

  id(): TradingId {
    return Array.from(this.take(32), (byte) => byte.toString(16).padStart(2, "0")).join("");
  }

  uint(bytes: number): bigint {
    return this.take(bytes).reduce((value, byte) => (value << 8n) | BigInt(byte), 0n);
  }

  byte(): number {
    return this.take(1)[0] ?? 0;
  }

  flag(): boolean {
    const value = this.byte();
    if (value > 1) {
      throw new TradingPayloadError("non_canonical", "flag");
    }
    return value === 1;
  }
}

function nonzero(...values: bigint[]): void {
  if (values.some((value) => value === 0n)) {
    throw new TradingPayloadError("non_canonical", "zero value");
  }
}

function side(value: number): TradeSide {
  if (value !== 1 && value !== 2) {
    throw new TradingPayloadError("non_canonical", "side");
  }
  return value;
}

function isZeroId(value: TradingId): boolean {
  return /^0+$/u.test(value);
}

function validateMarket(market: PerpsMarket): void {
  const accounts = [
    market.liquidityAccountId,
    market.longFundingAccountId,
    market.shortFundingAccountId,
    market.insuranceAccountId,
  ];
  const keys = market.permittedOracleKeys;
  const ok =
    !isZeroId(market.marketId) &&
    !isZeroId(market.quoteAsset) &&
    !isZeroId(market.administrator) &&
    accounts.every((account, index) => !isZeroId(account) && !accounts.slice(0, index).includes(account)) &&
    market.contractSize !== 0n &&
    market.tickSize !== 0n &&
    market.lotSize !== 0n &&
    market.priceScale !== 0n &&
    market.maintenanceMarginRatioBps !== 0n &&
    market.initialMarginRatioBps > market.maintenanceMarginRatioBps &&
    market.initialMarginRatioBps <= BASIS_POINTS_ONE &&
    market.liquidationFeeBps <= BASIS_POINTS_ONE &&
    market.liquidatorShareBps <= BASIS_POINTS_ONE &&
    market.maximumFundingRateBps !== 0n &&
    market.maximumFundingRateBps <= BASIS_POINTS_ONE &&
    market.maximumDeviationBasisPoints !== 0n &&
    market.maximumDeviationBasisPoints <= BASIS_POINTS_ONE &&
    market.fundingIntervalMs !== 0n &&
    market.maximumOracleStalenessMs !== 0n &&
    market.minimumPrice !== 0n &&
    market.minimumPrice < market.maximumPrice &&
    market.parameterVersion !== 0n &&
    keys.length > 0 &&
    keys.length <= MAX_ORACLE_KEYS &&
    keys.every((key, index) => ID.test(key) && !isZeroId(key) && (index === 0 || (keys[index - 1] ?? "") < key));
  if (!ok) {
    throw new TradingPayloadError("parameter_bounds");
  }
}

function encodeMarket(writer: TradingWriter, market: PerpsMarket): void {
  validateMarket(market);
  writer
    .id(market.marketId)
    .id(market.quoteAsset)
    .id(market.administrator)
    .id(market.liquidityAccountId)
    .id(market.longFundingAccountId)
    .id(market.shortFundingAccountId)
    .id(market.insuranceAccountId)
    .uint(market.contractSize, 16)
    .uint(market.tickSize, 16)
    .uint(market.lotSize, 16)
    .uint(market.priceScale, 16)
    .uint(market.initialMarginRatioBps, 4)
    .uint(market.maintenanceMarginRatioBps, 4)
    .uint(market.liquidationFeeBps, 4)
    .uint(market.liquidatorShareBps, 4)
    .uint(market.maximumFundingRateBps, 4)
    .uint(market.maximumDeviationBasisPoints, 4)
    .uint(market.fundingIntervalMs, 8)
    .uint(market.maximumOracleStalenessMs, 8)
    .uint(market.minimumPrice, 16)
    .uint(market.maximumPrice, 16)
    .byte(market.permittedOracleKeys.length);
  for (const key of market.permittedOracleKeys) {
    writer.id(key);
  }
  writer
    .zeros((MAX_ORACLE_KEYS - market.permittedOracleKeys.length) * 32)
    .uint(market.parameterVersion, 4)
    .byte(market.halted ? 1 : 0);
}

function decodeMarket(reader: TradingReader): PerpsActivity {
  const head = {
    marketId: reader.id(),
    quoteAsset: reader.id(),
    administrator: reader.id(),
    liquidityAccountId: reader.id(),
    longFundingAccountId: reader.id(),
    shortFundingAccountId: reader.id(),
    insuranceAccountId: reader.id(),
    contractSize: reader.uint(16),
    tickSize: reader.uint(16),
    lotSize: reader.uint(16),
    priceScale: reader.uint(16),
    initialMarginRatioBps: reader.uint(4),
    maintenanceMarginRatioBps: reader.uint(4),
    liquidationFeeBps: reader.uint(4),
    liquidatorShareBps: reader.uint(4),
    maximumFundingRateBps: reader.uint(4),
    maximumDeviationBasisPoints: reader.uint(4),
    fundingIntervalMs: reader.uint(8),
    maximumOracleStalenessMs: reader.uint(8),
    minimumPrice: reader.uint(16),
    maximumPrice: reader.uint(16),
  };
  const count = reader.byte();
  if (count > MAX_ORACLE_KEYS) {
    throw new TradingPayloadError("non_canonical", "oracle key count");
  }
  const permittedOracleKeys: TradingId[] = [];
  for (let index = 0; index < MAX_ORACLE_KEYS; index += 1) {
    const key = reader.id();
    if (index < count) {
      permittedOracleKeys.push(key);
    } else if (!isZeroId(key)) {
      throw new TradingPayloadError("non_canonical", "oracle key padding");
    }
  }
  const market = {
    ...head,
    permittedOracleKeys,
    parameterVersion: reader.uint(4),
    halted: reader.flag(),
  };
  validateMarket(market);
  return { activity: "market_create", ...market };
}

function encodeAdl(writer: TradingWriter, marketId: TradingId, positionIds: readonly TradingId[]): void {
  if (positionIds.length === 0 || positionIds.length > ADL_CAPACITY) {
    throw new TradingPayloadError("non_canonical", "position count");
  }
  if (positionIds.some((id) => !ID.test(id) || isZeroId(id))) {
    throw new TradingPayloadError("non_canonical", "position identifier");
  }
  if (positionIds.some((id, index) => index > 0 && (positionIds[index - 1] ?? "") >= id)) {
    throw new TradingPayloadError("unsorted_sequence");
  }
  writer.id(marketId).byte(positionIds.length);
  for (const id of positionIds) {
    writer.id(id);
  }
}

/** Returns the packed activity type of a perps activity. */
export function perpsActivityType(activity: PerpsActivity): number {
  return PERPS_ACTIVITY_TYPES[activity.activity];
}

/** Encodes the exact kernel payload bytes of a perps activity. */
export function encodePerpsActivity(activity: PerpsActivity): Uint8Array {
  const writer = new TradingWriter();
  switch (activity.activity) {
    case "market_create":
      encodeMarket(writer, activity);
      break;
    case "market_halt":
      writer.id(activity.marketId).byte(activity.halted ? 1 : 0);
      break;
    case "oracle_push":
      nonzero(activity.observationSequence, activity.price, activity.observedAt, activity.sourceIdentifier);
      writer
        .id(activity.marketId)
        .uint(activity.observationSequence, 8)
        .uint(activity.price, 16)
        .uint(activity.observedAt, 8)
        .uint(activity.sourceIdentifier, 8);
      break;
    case "order_place":
      nonzero(activity.price, activity.quantity);
      writer
        .id(activity.marketId)
        .id(activity.orderId)
        .id(activity.ownerAccountId)
        .byte(side(activity.side))
        .uint(activity.price, 16)
        .uint(activity.quantity, 16);
      break;
    case "order_cancel":
      writer.id(activity.marketId).id(activity.orderId);
      break;
    case "position_open":
      nonzero(activity.size, activity.marginAmount);
      if (activity.entryNotional !== 0n) {
        throw new TradingPayloadError("non_canonical", "entry notional must be zero");
      }
      writer
        .id(activity.marketId)
        .id(activity.positionId)
        .id(activity.marginAccountId)
        .byte(side(activity.side))
        .uint(activity.size, 16)
        .uint(activity.entryNotional, 16)
        .uint(activity.marginAmount, 16);
      break;
    case "position_increase":
      nonzero(activity.sizeDelta, activity.marginAmount);
      if (activity.notionalDelta !== 0n) {
        throw new TradingPayloadError("non_canonical", "notional delta must be zero");
      }
      writer
        .id(activity.marketId)
        .id(activity.positionId)
        .uint(activity.sizeDelta, 16)
        .uint(activity.notionalDelta, 16)
        .uint(activity.marginAmount, 16);
      break;
    case "position_close":
      writer.id(activity.marketId).id(activity.positionId);
      break;
    case "funding_tick":
      writer.id(activity.marketId);
      break;
    case "liquidate":
      writer.id(activity.marketId).id(activity.positionId).id(activity.liquidatorAccountId);
      break;
    case "adl":
      encodeAdl(writer, activity.marketId, activity.positionIds);
      break;
  }
  return writer.bytes();
}

/** Builds the activity type and payload a perps activity submits. */
export function buildPerpsActivity(activity: PerpsActivity): EncodedActivity {
  return { activityType: perpsActivityType(activity), payload: encodePerpsActivity(activity) };
}

const PERPS_LENGTHS: Readonly<Record<number, number>> = {
  1: MARKET_BYTES,
  2: 33,
  3: 72,
  4: 129,
  5: 64,
  6: 145,
  7: 112,
  8: 64,
  9: 32,
  10: 96,
};

function sameBytes(left: Uint8Array, right: Uint8Array): boolean {
  return left.length === right.length && left.every((byte, index) => byte === right[index]);
}

/** Decodes and validates kernel payload bytes of a perps activity. */
export function decodePerpsActivity(activityType: number, payload: Uint8Array): PerpsActivity {
  if (activityType >>> 16 !== PERPS_MODULE_ID) {
    throw new TradingPayloadError("unknown_activity", activityType.toString(16));
  }
  const ordinal = activityType & 0xffff;
  if (ordinal === 11) {
    if (payload.length < 65 || payload.length > 33 + 32 * ADL_CAPACITY) {
      throw new TradingPayloadError("length");
    }
    const reader = new TradingReader(payload);
    const marketId = reader.id();
    const count = reader.byte();
    if (count === 0 || payload.length !== 33 + 32 * count || isZeroId(marketId)) {
      throw new TradingPayloadError("non_canonical", "adl shape");
    }
    const positionIds = Array.from({ length: count }, () => reader.id());
    const decoded: PerpsActivity = { activity: "adl", marketId, positionIds };
    encodePerpsActivity(decoded);
    return decoded;
  }
  const expected = PERPS_LENGTHS[ordinal];
  if (expected === undefined) {
    throw new TradingPayloadError("unknown_activity", activityType.toString(16));
  }
  if (payload.length !== expected) {
    throw new TradingPayloadError("length");
  }
  const reader = new TradingReader(payload);
  let decoded: PerpsActivity;
  switch (ordinal) {
    case 1:
      decoded = decodeMarket(reader);
      break;
    case 2:
      decoded = { activity: "market_halt", marketId: reader.id(), halted: reader.flag() };
      break;
    case 3:
      decoded = {
        activity: "oracle_push",
        marketId: reader.id(),
        observationSequence: reader.uint(8),
        price: reader.uint(16),
        observedAt: reader.uint(8),
        sourceIdentifier: reader.uint(8),
      };
      break;
    case 4:
      decoded = {
        activity: "order_place",
        marketId: reader.id(),
        orderId: reader.id(),
        ownerAccountId: reader.id(),
        side: side(reader.byte()),
        price: reader.uint(16),
        quantity: reader.uint(16),
      };
      break;
    case 5:
      decoded = { activity: "order_cancel", marketId: reader.id(), orderId: reader.id() };
      break;
    case 6:
      decoded = {
        activity: "position_open",
        marketId: reader.id(),
        positionId: reader.id(),
        marginAccountId: reader.id(),
        side: side(reader.byte()),
        size: reader.uint(16),
        entryNotional: reader.uint(16),
        marginAmount: reader.uint(16),
      };
      break;
    case 7:
      decoded = {
        activity: "position_increase",
        marketId: reader.id(),
        positionId: reader.id(),
        sizeDelta: reader.uint(16),
        notionalDelta: reader.uint(16),
        marginAmount: reader.uint(16),
      };
      break;
    case 8:
      decoded = { activity: "position_close", marketId: reader.id(), positionId: reader.id() };
      break;
    case 9:
      decoded = { activity: "funding_tick", marketId: reader.id() };
      break;
    default:
      decoded = {
        activity: "liquidate",
        marketId: reader.id(),
        positionId: reader.id(),
        liquidatorAccountId: reader.id(),
      };
  }
  if (!sameBytes(encodePerpsActivity(decoded), payload)) {
    throw new TradingPayloadError("non_canonical");
  }
  return decoded;
}
